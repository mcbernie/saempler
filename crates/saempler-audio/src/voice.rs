use crate::command::SliceBounds;
use crate::sample::SampleBuffer;

/// Attack time of the amplitude envelope. Long enough to avoid a click at the
/// slice boundary, short enough that the hit still feels immediate.
const ATTACK_MS: f32 = 3.0;
/// Release time of the amplitude envelope.
const RELEASE_MS: f32 = 30.0;

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
    /// Target level while the key is held, derived from note velocity.
    sustain_level: f32,
    attack_step: f32,
    release_step: f32,
    /// Next frame to read from the sample buffer.
    position: u64,
    bounds: SliceBounds,
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
            position: 0,
            bounds: SliceBounds::default(),
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

    /// Frame this voice will read next, for the playhead display.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Start this voice, replacing whatever it was playing before.
    pub fn start(&mut self, note: u8, velocity: f32, age: u64, bounds: SliceBounds, rate: f32) {
        self.stage = Stage::Attack;
        self.note = note;
        self.age = age;
        self.level = 0.0;
        self.sustain_level = velocity.clamp(0.0, 1.0);
        self.attack_step = envelope_step(ATTACK_MS, rate);
        self.release_step = envelope_step(RELEASE_MS, rate);
        self.bounds = bounds;
        self.position = bounds.start_frame;
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
        self.position = 0;
    }

    /// Render the next frame, advancing both playback position and envelope.
    ///
    /// Reaching the end of the slice releases the voice rather than cutting it
    /// off, so the envelope can fade out the last frames instead of clicking.
    pub fn next_frame(&mut self, sample: &SampleBuffer) -> (f32, f32) {
        if self.stage == Stage::Idle {
            return (0.0, 0.0);
        }

        if self.position >= self.bounds.end_frame {
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

        // The position may run past the slice end while the release envelope
        // is still active; `SampleBuffer::frame` reads those as silence.
        let (left, right) = sample.frame(self.position as usize);
        self.position += 1;

        (left * self.level, right * self.level)
    }
}

/// Per-sample increment that traverses the full envelope range in `time_ms`.
fn envelope_step(time_ms: f32, sample_rate: f32) -> f32 {
    let samples = (time_ms / 1000.0) * sample_rate;
    if samples >= 1.0 {
        1.0 / samples
    } else {
        // Degenerate sample rates must not stall the envelope forever.
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    /// A buffer of constant full-scale samples, so the measured output is the
    /// envelope level alone.
    fn dc_buffer(frames: usize) -> SampleBuffer {
        SampleBuffer::new(vec![vec![1.0; frames], vec![1.0; frames]], 48_000)
    }

    fn bounds(start: u64, end: u64) -> SliceBounds {
        SliceBounds {
            start_frame: start,
            end_frame: end,
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
        // Only the second half of the buffer carries audio.
        let mut data = vec![0.0f32; 1_000];
        data[500..].fill(1.0);
        let buffer = SampleBuffer::new(vec![data], 48_000);

        let mut inside = Voice::default();
        inside.start(60, 1.0, 0, bounds(500, 700), SAMPLE_RATE);
        let heard = (0..100)
            .map(|_| inside.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        let mut before = Voice::default();
        before.start(60, 1.0, 0, bounds(0, 200), SAMPLE_RATE);
        let silent = (0..100)
            .map(|_| before.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert!(heard > 0.0, "a slice over audio must sound");
        assert_eq!(silent, 0.0, "a slice over silence must stay silent");
    }

    #[test]
    fn a_voice_reaching_the_slice_end_stops_on_its_own() {
        let buffer = dc_buffer(10_000);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, bounds(0, 500), SAMPLE_RATE);

        // 500 frames of slice plus the release stage.
        let release_frames = (RELEASE_MS / 1000.0 * SAMPLE_RATE) as usize;
        for _ in 0..(500 + release_frames * 2) {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active());
        assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
    }

    #[test]
    fn the_end_frame_marks_the_start_of_the_release() {
        let buffer = dc_buffer(10_000);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, bounds(0, 2), SAMPLE_RATE);

        // The two frames inside the slice are played as held.
        voice.next_frame(&buffer);
        voice.next_frame(&buffer);
        assert!(voice.is_playing_note(60));

        // The next call finds the position at the end frame and releases.
        voice.next_frame(&buffer);
        assert!(
            !voice.is_playing_note(60),
            "reaching the end frame must release the voice"
        );
    }

    #[test]
    fn the_release_reads_on_past_the_slice_end() {
        // Deliberate: cutting playback at the slice boundary would click. The
        // envelope fades out over the audio that follows the slice instead.
        let buffer = SampleBuffer::new(vec![vec![0.0, 0.0, 1.0, 1.0, 1.0]], 48_000);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, bounds(0, 2), SAMPLE_RATE);

        let peak = (0..5)
            .map(|_| voice.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert!(peak > 0.0);
        assert!(peak < 1.0, "the audio past the end must already be fading");
    }

    #[test]
    fn velocity_scales_the_sustain_level() {
        let buffer = dc_buffer(10_000);
        let mut quiet = Voice::default();
        quiet.start(60, 0.25, 0, bounds(0, 10_000), SAMPLE_RATE);
        let mut loud = Voice::default();
        loud.start(60, 1.0, 0, bounds(0, 10_000), SAMPLE_RATE);

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
    fn output_never_exceeds_the_source_level() {
        let buffer = dc_buffer(5_000);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, bounds(0, 5_000), SAMPLE_RATE);

        for _ in 0..5_000 {
            let (left, right) = voice.next_frame(&buffer);
            assert!(left.is_finite() && right.is_finite());
            assert!(left.abs() <= 1.0 + 1e-6);
        }
    }

    #[test]
    fn release_fades_out_rather_than_cutting_off() {
        let buffer = dc_buffer(10_000);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, bounds(0, 10_000), SAMPLE_RATE);
        for _ in 0..1_000 {
            voice.next_frame(&buffer);
        }

        voice.release();
        let first = voice.next_frame(&buffer).0;
        let second = voice.next_frame(&buffer).0;

        assert!(second < first, "level should decrease during release");
        assert!(second > 0.0, "release must not jump straight to silence");
    }

    #[test]
    fn kill_silences_without_release() {
        let buffer = dc_buffer(100);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, bounds(0, 100), SAMPLE_RATE);

        voice.kill();

        assert!(!voice.is_active());
        assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
    }

    #[test]
    fn reaching_the_end_of_the_buffer_is_silent_rather_than_a_panic() {
        let buffer = dc_buffer(10);
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, bounds(1_000, 2_000), SAMPLE_RATE);

        for _ in 0..64 {
            assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
        }
    }
}
