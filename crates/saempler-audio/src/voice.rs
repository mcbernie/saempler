use crate::command::CellSpec;
use crate::sample::SampleBuffer;

/// Stage of a voice's amplitude envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Not sounding; the voice slot is free.
    Idle,
    /// Fading in towards the sustain level.
    Attack,
    /// Holding the sustain level until the key is released.
    Sustain,
    /// Fading out; the voice becomes idle when the envelope reaches zero.
    Release,
}

/// A single sounding note.
///
/// Voices are preallocated in a fixed array and reused, so this type contains
/// no owned allocations and never needs to be dropped on the audio thread.
#[derive(Debug, Clone, Copy)]
pub struct Voice {
    stage: Stage,
    note: u8,
    /// Monotonic counter used to pick the oldest voice when stealing.
    age: u64,
    level: f32,
    /// Target level while the key is held, from velocity and cell gain.
    sustain_level: f32,
    attack_step: f32,
    release_step: f32,
    /// Fractional read position in the sample, in frames.
    ///
    /// Fractional because speed and pitch change how fast the slice is read;
    /// the sample values either side are interpolated.
    position: f64,
    /// Frames advanced per output frame. Always positive; direction is
    /// carried by `reverse`.
    rate: f64,
    /// Multiplier a tape stop applies to the rate, falling from 1 to 0.
    rate_scale: f64,
    /// How much `rate_scale` falls per output frame. Zero means no tape stop.
    rate_decay: f64,
    reverse: bool,
    /// Frame playback jumps back to while looping, and the length of that
    /// loop. Zero means the voice plays straight through.
    loop_start: u64,
    loop_frames: u64,
    spec: CellSpec,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            note: 0,
            age: 0,
            level: 0.0,
            sustain_level: 0.0,
            attack_step: 1.0,
            release_step: 1.0,
            position: 0.0,
            rate: 1.0,
            rate_scale: 1.0,
            rate_decay: 0.0,
            reverse: false,
            loop_start: 0,
            loop_frames: 0,
            spec: CellSpec::default(),
        }
    }
}

impl Voice {
    /// Whether this voice currently contributes to the output.
    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Whether this voice is sounding the given note and has not been released.
    pub fn is_playing_note(&self, note: u8) -> bool {
        self.note == note && matches!(self.stage, Stage::Attack | Stage::Sustain)
    }

    /// Age counter, used by the engine to find the oldest voice.
    pub fn age(&self) -> u64 {
        self.age
    }

    /// Frame this voice is reading, for the playhead display.
    pub fn position(&self) -> u64 {
        self.position.max(0.0) as u64
    }

    /// Where this voice is reading, or `None` while it is idle.
    pub fn active_position(&self) -> Option<u64> {
        self.is_active().then(|| self.position())
    }

    /// Start this voice, replacing whatever it was playing before.
    pub fn start(&mut self, note: u8, velocity: f32, age: u64, spec: CellSpec, sample_rate: f32) {
        self.stage = Stage::Attack;
        self.note = note;
        self.age = age;
        self.level = 0.0;
        self.sustain_level = (velocity.clamp(0.0, 1.0) * spec.gain).clamp(0.0, 4.0);
        self.rate = spec.rate.max(f32::MIN_POSITIVE) as f64;
        self.rate_scale = 1.0;
        self.rate_decay = if spec.tape_stop_frames > 0 {
            1.0 / spec.tape_stop_frames as f64
        } else {
            0.0
        };
        // A loop as long as the slice, or longer, is no loop at all: there is
        // nothing to come back to before the end arrives.
        self.loop_frames = if spec.loop_frames > 0 && spec.loop_frames < spec.bounds.len_frames() {
            spec.loop_frames
        } else {
            0
        };

        // The envelope has to fit inside the slice. A cell whose attack and
        // release together outlast the material gets both scaled down in
        // proportion, rather than a fade that starts before the level has
        // risen and silences the voice on its first frame.
        let available = (spec.bounds.len_frames() as f64 / self.rate) as f32;
        let attack = spec.attack_ms / 1000.0 * sample_rate;
        let release = spec.release_ms / 1000.0 * sample_rate;
        let wanted = attack + release;
        let scale = if wanted > available && wanted > 0.0 {
            available / wanted
        } else {
            1.0
        };
        self.attack_step = envelope_step(attack * scale);
        self.release_step = envelope_step(release * scale);
        self.reverse = spec.reverse;
        self.spec = spec;
        // Backwards playback starts at the last frame of the slice.
        self.position = if spec.reverse {
            spec.bounds.end_frame.saturating_sub(1) as f64
        } else {
            spec.bounds.start_frame as f64
        };
        self.loop_start = self.position as u64;
    }

    /// Move the voice into its release stage.
    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Silence the voice immediately without a release stage.
    pub fn kill(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
        self.position = 0.0;
    }

    /// Render the next frame, advancing both playback position and envelope.
    ///
    /// The fade out is started early enough to finish at the slice edge. A
    /// voice therefore never reads past its own slice: doing so would mix the
    /// neighbouring chop into the tail, and would show a playhead running on
    /// past the region it is playing.
    pub fn next_frame(&mut self, sample: &SampleBuffer) -> (f32, f32) {
        if self.stage == Stage::Idle {
            return (0.0, 0.0);
        }

        if self.rate_decay > 0.0 {
            self.rate_scale -= self.rate_decay;
            if self.rate_scale <= 0.0 {
                // The tape has come to rest; there is nothing left to read.
                self.kill();
                return (0.0, 0.0);
            }
        }

        if self.wrap_loop() {
            // A looping voice never approaches the slice edge, so the fade out
            // is left to the note off.
        } else {
            let remaining = self.frames_to_edge();
            if remaining <= 0.0 {
                self.kill();
                return (0.0, 0.0);
            }
            if self.stage != Stage::Release && remaining <= self.release_frames() {
                self.release();
            }
        }

        match self.stage {
            Stage::Attack => {
                self.level += self.attack_step * self.sustain_level;
                if self.level >= self.sustain_level {
                    self.level = self.sustain_level;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Release => {
                // Whichever is steeper: the release the cell asks for, or the
                // one the remaining audio allows. A release longer than the
                // slice simply fades across all of it.
                let wanted = self.release_step * self.sustain_level;
                let forced = if self.loop_frames > 0 {
                    0.0
                } else {
                    self.level / self.frames_to_edge().max(1.0) as f32
                };
                self.level -= wanted.max(forced);
                if self.level <= 0.0 {
                    self.kill();
                    return (0.0, 0.0);
                }
            }
            Stage::Idle | Stage::Sustain => {}
        }

        let (left, right) = interpolated(sample, self.position);

        let step = self.rate * self.rate_scale;
        if self.reverse {
            self.position -= step;
        } else {
            self.position += step;
        }

        (left * self.level, right * self.level)
    }

    /// Jump back to the loop start when the loop runs out.
    ///
    /// Returns whether this voice is looping at all, which decides whether the
    /// slice edge is something it can ever reach.
    fn wrap_loop(&mut self) -> bool {
        if self.loop_frames == 0 {
            return false;
        }

        let start = self.loop_start as f64;
        let travelled = if self.reverse {
            start - self.position
        } else {
            self.position - start
        };
        if travelled >= self.loop_frames as f64 {
            self.position = start;
        }

        true
    }

    /// Output frames left before the read position leaves the slice.
    fn frames_to_edge(&self) -> f64 {
        let remaining_source = if self.reverse {
            self.position - self.spec.bounds.start_frame as f64
        } else {
            self.spec.bounds.end_frame as f64 - self.position
        };

        // The scaled rate, so a braking voice is not told it has moments left
        // when in truth it has almost stopped moving.
        let rate = (self.rate * self.rate_scale).max(1e-9);
        (remaining_source / rate).max(0.0)
    }

    /// Output frames a full fade out would take at the configured release.
    fn release_frames(&self) -> f64 {
        if self.release_step <= 0.0 {
            return 0.0;
        }
        (1.0 / self.release_step) as f64
    }
}

/// Read a frame between two samples, weighting them by the fraction.
///
/// Linear interpolation. It adds some high-frequency dullness and a little
/// distortion at large transpositions, which is the accepted cost until a
/// proper interpolator exists.
fn interpolated(sample: &SampleBuffer, position: f64) -> (f32, f32) {
    if position < 0.0 {
        return (0.0, 0.0);
    }

    let index = position as usize;
    let fraction = (position - index as f64) as f32;
    let (left_a, right_a) = sample.frame(index);
    let (left_b, right_b) = sample.frame(index + 1);

    (
        left_a + (left_b - left_a) * fraction,
        right_a + (right_b - right_a) * fraction,
    )
}

/// Per-frame increment that traverses the full envelope range in `frames`.
fn envelope_step(frames: f32) -> f32 {
    if frames >= 1.0 {
        1.0 / frames
    } else {
        // A zero-length stage must not stall the envelope forever.
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::SliceBounds;

    const SAMPLE_RATE: f32 = 48_000.0;

    /// A buffer of constant full-scale samples, so the measured output is the
    /// envelope level alone.
    fn dc_buffer(frames: usize) -> SampleBuffer {
        SampleBuffer::new(vec![vec![1.0; frames], vec![1.0; frames]], 48_000)
    }

    /// A buffer whose sample value equals its frame index divided by `frames`,
    /// so the output says which frame was read.
    fn ramp_buffer(frames: usize) -> SampleBuffer {
        let data: Vec<f32> = (0..frames)
            .map(|index| index as f32 / frames as f32)
            .collect();
        SampleBuffer::new(vec![data], 48_000)
    }

    fn spec(start: u64, end: u64) -> CellSpec {
        CellSpec {
            bounds: SliceBounds {
                start_frame: start,
                end_frame: end,
            },
            ..CellSpec::default()
        }
    }

    #[test]
    fn idle_voice_is_silent() {
        let buffer = dc_buffer(100);
        let mut voice = Voice::default();

        assert!(!voice.is_active());
        assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
    }

    #[test]
    fn playback_starts_at_the_slice_start() {
        let mut data = vec![0.0f32; 1_000];
        data[500..].fill(1.0);
        let buffer = SampleBuffer::new(vec![data], 48_000);

        let mut inside = Voice::default();
        inside.start(60, 1.0, 0, spec(500, 700), SAMPLE_RATE);
        let heard = (0..100)
            .map(|_| inside.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        let mut before = Voice::default();
        before.start(60, 1.0, 0, spec(0, 200), SAMPLE_RATE);
        let silent = (0..100)
            .map(|_| before.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert!(heard > 0.0, "a slice over audio must sound");
        assert_eq!(silent, 0.0, "a slice over silence must stay silent");
    }

    #[test]
    fn reverse_reads_the_slice_from_the_other_end() {
        let buffer = ramp_buffer(1_000);
        let bounds = spec(100, 900);

        let mut forward = Voice::default();
        forward.start(60, 1.0, 0, bounds, SAMPLE_RATE);
        let mut backward = Voice::default();
        backward.start(
            60,
            1.0,
            0,
            CellSpec {
                reverse: true,
                ..bounds
            },
            SAMPLE_RATE,
        );

        // Skip the attack so the envelope is not what is being compared.
        for _ in 0..200 {
            forward.next_frame(&buffer);
            backward.next_frame(&buffer);
        }

        assert_eq!(forward.position(), 300);
        assert_eq!(backward.position(), 699);
    }

    #[test]
    fn a_reversed_voice_ends_at_the_slice_start() {
        let buffer = dc_buffer(10_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                reverse: true,
                ..spec(1_000, 1_500)
            },
            SAMPLE_RATE,
        );

        // 500 frames of slice plus a generous release.
        for _ in 0..5_000 {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active());
    }

    #[test]
    fn a_higher_rate_reads_further_in_the_same_time() {
        let buffer = ramp_buffer(10_000);

        let mut normal = Voice::default();
        normal.start(60, 1.0, 0, spec(0, 10_000), SAMPLE_RATE);
        let mut doubled = Voice::default();
        doubled.start(
            60,
            1.0,
            0,
            CellSpec {
                rate: 2.0,
                ..spec(0, 10_000)
            },
            SAMPLE_RATE,
        );

        for _ in 0..1_000 {
            normal.next_frame(&buffer);
            doubled.next_frame(&buffer);
        }

        assert_eq!(normal.position(), 1_000);
        assert_eq!(doubled.position(), 2_000);
    }

    #[test]
    fn a_half_rate_voice_lasts_twice_as_long() {
        let buffer = dc_buffer(100_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                rate: 0.5,
                release_ms: 0.0,
                ..spec(0, 1_000)
            },
            SAMPLE_RATE,
        );

        for _ in 0..1_500 {
            voice.next_frame(&buffer);
        }
        assert!(
            voice.is_active(),
            "half speed must still be inside 1000 frames"
        );

        for _ in 0..1_500 {
            voice.next_frame(&buffer);
        }
        assert!(!voice.is_active());
    }

    #[test]
    fn interpolation_lands_between_the_neighbouring_samples() {
        let buffer = SampleBuffer::new(vec![vec![0.0, 1.0]], 48_000);

        let (left, _) = interpolated(&buffer, 0.5);

        assert!((left - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_fractional_rate_produces_no_steps_or_spikes() {
        let buffer = ramp_buffer(10_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                rate: 1.0 / 3.0,
                attack_ms: 0.0,
                ..spec(0, 10_000)
            },
            SAMPLE_RATE,
        );

        let mut previous = voice.next_frame(&buffer).0;
        for _ in 0..2_000 {
            let current = voice.next_frame(&buffer).0;
            assert!(current.is_finite());
            // The ramp rises by 1/10000 per frame, so a third of that per step.
            assert!(
                (current - previous).abs() < 0.001,
                "{previous} -> {current} is not a smooth step"
            );
            previous = current;
        }
    }

    #[test]
    fn cell_gain_scales_the_output() {
        let buffer = dc_buffer(10_000);
        let mut quiet = Voice::default();
        quiet.start(
            60,
            1.0,
            0,
            CellSpec {
                gain: 0.25,
                ..spec(0, 10_000)
            },
            SAMPLE_RATE,
        );

        let peak = (0..4_800)
            .map(|_| quiet.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert!(peak <= 0.25 + 1e-6);
        assert!(peak > 0.2);
    }

    #[test]
    fn velocity_scales_the_sustain_level() {
        let buffer = dc_buffer(10_000);
        let mut quiet = Voice::default();
        quiet.start(60, 0.25, 0, spec(0, 10_000), SAMPLE_RATE);
        let mut loud = Voice::default();
        loud.start(60, 1.0, 0, spec(0, 10_000), SAMPLE_RATE);

        let quiet_peak = (0..4_800)
            .map(|_| quiet.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);
        let loud_peak = (0..4_800)
            .map(|_| loud.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert!(quiet_peak < loud_peak);
        assert!(quiet_peak <= 0.25 + 1e-6);
    }

    #[test]
    fn a_longer_attack_reaches_full_level_later() {
        let buffer = dc_buffer(100_000);
        let mut quick = Voice::default();
        quick.start(
            60,
            1.0,
            0,
            CellSpec {
                attack_ms: 1.0,
                ..spec(0, 100_000)
            },
            SAMPLE_RATE,
        );
        let mut slow = Voice::default();
        slow.start(
            60,
            1.0,
            0,
            CellSpec {
                attack_ms: 200.0,
                ..spec(0, 100_000)
            },
            SAMPLE_RATE,
        );

        // 10 ms in: the short attack is done, the long one is far from it.
        for _ in 0..480 {
            quick.next_frame(&buffer);
            slow.next_frame(&buffer);
        }

        let quick_level = quick.next_frame(&buffer).0;
        let slow_level = slow.next_frame(&buffer).0;
        assert!(
            quick_level > 0.9,
            "short attack should be open: {quick_level}"
        );
        assert!(
            slow_level < 0.2,
            "long attack should still be rising: {slow_level}"
        );
    }

    #[test]
    fn a_longer_release_takes_longer_to_fall_silent() {
        let buffer = dc_buffer(100_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                release_ms: 500.0,
                ..spec(0, 100_000)
            },
            SAMPLE_RATE,
        );
        for _ in 0..1_000 {
            voice.next_frame(&buffer);
        }

        voice.release();
        for _ in 0..4_800 {
            voice.next_frame(&buffer);
        }

        assert!(
            voice.is_active(),
            "a 500 ms release is not over after 100 ms"
        );
    }

    #[test]
    fn output_stays_finite_across_the_rate_range() {
        let buffer = ramp_buffer(10_000);

        for rate in [0.0625f32, 0.5, 1.0, 2.0, 16.0] {
            let mut voice = Voice::default();
            voice.start(
                60,
                1.0,
                0,
                CellSpec {
                    rate,
                    ..spec(0, 10_000)
                },
                SAMPLE_RATE,
            );

            for _ in 0..5_000 {
                let (left, right) = voice.next_frame(&buffer);
                assert!(left.is_finite() && right.is_finite(), "rate {rate}");
                assert!(left.abs() <= 1.5, "rate {rate} produced {left}");
            }
        }
    }

    #[test]
    fn a_loop_keeps_returning_to_the_trigger_point() {
        let buffer = ramp_buffer(100_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                loop_frames: 1_000,
                ..spec(0, 50_000)
            },
            SAMPLE_RATE,
        );

        for _ in 0..10_000 {
            voice.next_frame(&buffer);
            assert!(
                voice.position() <= 1_000,
                "a looping voice left its loop at {}",
                voice.position()
            );
        }
        assert!(voice.is_active(), "a loop plays until the key is released");
    }

    #[test]
    fn a_loop_longer_than_the_slice_is_no_loop() {
        let buffer = dc_buffer(100_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                loop_frames: 1_000_000,
                ..spec(0, 5_000)
            },
            SAMPLE_RATE,
        );

        for _ in 0..50_000 {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active(), "it must still end at the slice");
    }

    #[test]
    fn a_looping_voice_still_stops_on_note_off() {
        let buffer = dc_buffer(100_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                loop_frames: 1_000,
                ..spec(0, 50_000)
            },
            SAMPLE_RATE,
        );
        for _ in 0..5_000 {
            voice.next_frame(&buffer);
        }

        voice.release();
        for _ in 0..10_000 {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active());
    }

    #[test]
    fn a_reversed_loop_returns_to_its_own_start() {
        let buffer = ramp_buffer(100_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                reverse: true,
                loop_frames: 1_000,
                ..spec(0, 50_000)
            },
            SAMPLE_RATE,
        );

        for _ in 0..10_000 {
            voice.next_frame(&buffer);
            assert!(
                (48_999..=49_999).contains(&voice.position()),
                "a reversed loop wandered to {}",
                voice.position()
            );
        }
    }

    #[test]
    fn a_tape_stop_slows_down_and_ends() {
        let buffer = ramp_buffer(200_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                tape_stop_frames: 4_800,
                ..spec(0, 200_000)
            },
            SAMPLE_RATE,
        );

        let mut positions = Vec::new();
        for frame in 0..4_800 {
            voice.next_frame(&buffer);
            if frame % 800 == 0 {
                positions.push(voice.position());
            }
        }

        // Each step covers less ground than the one before it.
        let steps: Vec<u64> = positions.windows(2).map(|p| p[1] - p[0]).collect();
        for pair in steps.windows(2) {
            assert!(pair[1] < pair[0], "the brake did not slow down: {steps:?}");
        }
        assert!(!voice.is_active(), "the tape must come to rest");
    }

    #[test]
    fn a_tape_stop_never_runs_backwards() {
        let buffer = ramp_buffer(200_000);
        let mut voice = Voice::default();
        voice.start(
            60,
            1.0,
            0,
            CellSpec {
                tape_stop_frames: 2_400,
                ..spec(1_000, 200_000)
            },
            SAMPLE_RATE,
        );

        let mut previous = voice.position();
        loop {
            voice.next_frame(&buffer);
            if !voice.is_active() {
                // An ended voice reports no position at all.
                break;
            }
            let current = voice.position();
            assert!(current >= previous, "{previous} -> {current}");
            previous = current;
        }
    }

    #[test]
    fn kill_silences_without_release() {
        let buffer = dc_buffer(100);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, spec(0, 100), SAMPLE_RATE);

        voice.kill();

        assert!(!voice.is_active());
        assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
    }

    #[test]
    fn reaching_the_end_of_the_buffer_is_silent_rather_than_a_panic() {
        let buffer = dc_buffer(10);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, spec(1_000, 2_000), SAMPLE_RATE);

        for _ in 0..64 {
            assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
        }
    }
}
