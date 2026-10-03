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
    reverse: bool,
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
            reverse: false,
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

    /// Start this voice, replacing whatever it was playing before.
    pub fn start(&mut self, note: u8, velocity: f32, age: u64, spec: CellSpec, sample_rate: f32) {
        self.stage = Stage::Attack;
        self.note = note;
        self.age = age;
        self.level = 0.0;
        self.sustain_level = (velocity.clamp(0.0, 1.0) * spec.gain).clamp(0.0, 4.0);
        self.attack_step = envelope_step(spec.attack_ms, sample_rate);
        self.release_step = envelope_step(spec.release_ms, sample_rate);
        self.rate = spec.rate.max(f32::MIN_POSITIVE) as f64;
        self.reverse = spec.reverse;
        self.spec = spec;
        // Backwards playback starts at the last frame of the slice.
        self.position = if spec.reverse {
            spec.bounds.end_frame.saturating_sub(1) as f64
        } else {
            spec.bounds.start_frame as f64
        };
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
    /// Reaching the end of the slice releases the voice rather than cutting it
    /// off, so the envelope can fade out the last frames instead of clicking.
    pub fn next_frame(&mut self, sample: &SampleBuffer) -> (f32, f32) {
        if self.stage == Stage::Idle {
            return (0.0, 0.0);
        }

        if self.past_the_end() {
            self.release();
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
                self.level -= self.release_step * self.sustain_level;
                if self.level <= 0.0 {
                    self.kill();
                    return (0.0, 0.0);
                }
            }
            Stage::Idle | Stage::Sustain => {}
        }

        // The position may run outside the slice while the release envelope is
        // still active; reads past the buffer come back as silence.
        let (left, right) = interpolated(sample, self.position);

        if self.reverse {
            self.position -= self.rate;
        } else {
            self.position += self.rate;
        }

        (left * self.level, right * self.level)
    }

    /// Whether playback has left the slice in its direction of travel.
    fn past_the_end(&self) -> bool {
        if self.reverse {
            self.position < self.spec.bounds.start_frame as f64
        } else {
            self.position >= self.spec.bounds.end_frame as f64
        }
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

/// Per-sample increment that traverses the full envelope range in `time_ms`.
fn envelope_step(time_ms: f32, sample_rate: f32) -> f32 {
    let samples = (time_ms / 1000.0) * sample_rate;
    if samples >= 1.0 {
        1.0 / samples
    } else {
        // A zero-length stage, or a degenerate sample rate, must not stall the
        // envelope forever.
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
