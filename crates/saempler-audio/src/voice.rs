use saempler_model::Waveform;

use crate::osc::{note_frequency, Oscillator};

/// Attack time of the test tone envelope. Long enough to avoid a click, short
/// enough that the note still feels immediate.
const ATTACK_MS: f32 = 5.0;
/// Release time of the test tone envelope.
const RELEASE_MS: f32 = 40.0;

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
    oscillator: Oscillator,
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
            oscillator: Oscillator::new(),
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

    /// Start this voice, replacing whatever it was playing before.
    pub fn start(&mut self, note: u8, velocity: f32, age: u64, sample_rate: f32) {
        self.stage = Stage::Attack;
        self.note = note;
        self.age = age;
        self.level = 0.0;
        self.sustain_level = velocity.clamp(0.0, 1.0);
        self.attack_step = envelope_step(ATTACK_MS, sample_rate);
        self.release_step = envelope_step(RELEASE_MS, sample_rate);
        self.oscillator.reset();
        self.oscillator
            .set_frequency(note_frequency(note), sample_rate);
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
    }

    /// Render the next sample, advancing both oscillator and envelope.
    pub fn next_sample(&mut self, waveform: Waveform) -> f32 {
        if self.stage == Stage::Idle {
            return 0.0;
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
                    return 0.0;
                }
            }
            Stage::Idle | Stage::Sustain => {}
        }

        self.oscillator.next_sample(waveform) * self.level
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

    #[test]
    fn idle_voice_is_silent() {
        let mut voice = Voice::default();

        assert!(!voice.is_active());
        assert_eq!(voice.next_sample(Waveform::Sine), 0.0);
    }

    #[test]
    fn started_voice_becomes_active_and_sounds() {
        let mut voice = Voice::default();
        voice.start(69, 1.0, 0, SAMPLE_RATE);

        assert!(voice.is_active());
        assert!(voice.is_playing_note(69));

        let peak = (0..SAMPLE_RATE as usize / 100)
            .map(|_| voice.next_sample(Waveform::Sine).abs())
            .fold(0.0f32, f32::max);
        assert!(peak > 0.5, "voice should reach a usable level, got {peak}");
    }

    #[test]
    fn released_voice_reaches_silence_and_frees_its_slot() {
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, SAMPLE_RATE);
        for _ in 0..1_000 {
            voice.next_sample(Waveform::Sine);
        }

        voice.release();
        assert!(!voice.is_playing_note(60));

        // The release stage must finish well within twice its nominal length.
        let release_samples = (RELEASE_MS / 1000.0 * SAMPLE_RATE) as usize;
        for _ in 0..release_samples * 2 {
            voice.next_sample(Waveform::Sine);
        }

        assert!(!voice.is_active());
        assert_eq!(voice.next_sample(Waveform::Sine), 0.0);
    }

    #[test]
    fn velocity_scales_the_sustain_level() {
        let mut quiet = Voice::default();
        quiet.start(60, 0.25, 0, SAMPLE_RATE);
        let mut loud = Voice::default();
        loud.start(60, 1.0, 0, SAMPLE_RATE);

        let quiet_peak = (0..4_800)
            .map(|_| quiet.next_sample(Waveform::Saw).abs())
            .fold(0.0f32, f32::max);
        let loud_peak = (0..4_800)
            .map(|_| loud.next_sample(Waveform::Saw).abs())
            .fold(0.0f32, f32::max);

        assert!(quiet_peak < loud_peak);
        assert!(quiet_peak <= 0.25 + 1e-6);
    }

    #[test]
    fn kill_silences_without_release() {
        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, SAMPLE_RATE);
        voice.kill();

        assert!(!voice.is_active());
        assert_eq!(voice.next_sample(Waveform::Square), 0.0);
    }
}
