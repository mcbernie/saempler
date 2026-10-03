use std::sync::Arc;

use saempler_model::Waveform;

use crate::command::{CommandConsumer, EngineCommand};
use crate::meters::Meters;
use crate::voice::Voice;

/// Number of preallocated voices. Notes beyond this steal the oldest voice.
pub const MAX_VOICES: usize = 16;

/// Fallback sample rate used before the host reports the real one.
const DEFAULT_SAMPLE_RATE: f32 = 44_100.0;

/// The realtime engine.
///
/// All state is preallocated. [`Engine::render`] performs no allocation, takes
/// no locks and does no I/O, so it is safe to call from an audio callback.
pub struct Engine {
    sample_rate: f32,
    waveform: Waveform,
    voices: [Voice; MAX_VOICES],
    /// Monotonic counter assigning an age to each started voice.
    next_age: u64,
    commands: CommandConsumer,
    meters: Arc<Meters>,
}

impl Engine {
    pub fn new(commands: CommandConsumer, meters: Arc<Meters>) -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            waveform: Waveform::default(),
            voices: [Voice::default(); MAX_VOICES],
            next_age: 0,
            commands,
            meters,
        }
    }

    /// Configure the engine for a new sample rate. Called outside the audio
    /// callback, before processing starts.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reset();
    }

    /// Drop all sounding voices and reset metering.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.kill();
        }
        self.next_age = 0;
        self.meters.store_peaks(0.0, 0.0);
        self.meters.store_active_voices(0);
    }

    /// Waveform currently used for rendering.
    pub fn waveform(&self) -> Waveform {
        self.waveform
    }

    /// Set the waveform directly, for applying loaded project state.
    ///
    /// Called outside the audio callback; the command queue is for the
    /// incremental edits that happen while the engine is running.
    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.waveform = waveform;
    }

    /// Drain the command queue. Call once at the start of a processing block.
    pub fn apply_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            match command {
                EngineCommand::SetWaveform(waveform) => self.waveform = waveform,
                EngineCommand::AllNotesOff => {
                    for voice in &mut self.voices {
                        voice.release();
                    }
                }
            }
        }
    }

    /// Start a voice for `note`. Steals the oldest voice when all are in use.
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        let age = self.next_age;
        self.next_age += 1;

        let slot = match self.voices.iter().position(|voice| !voice.is_active()) {
            Some(free) => free,
            // Stealing the oldest voice keeps the most recent part of the
            // performance intact, which matters more than letting an old note
            // ring out.
            None => self
                .voices
                .iter()
                .enumerate()
                .min_by_key(|(_, voice)| voice.age())
                .map(|(index, _)| index)
                .unwrap_or(0),
        };

        self.voices[slot].start(note, velocity, age, self.sample_rate);
    }

    /// Release every voice currently holding `note`.
    pub fn note_off(&mut self, note: u8) {
        for voice in &mut self.voices {
            if voice.is_playing_note(note) {
                voice.release();
            }
        }
    }

    /// Number of voices that are currently sounding.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| voice.is_active()).count()
    }

    /// Render the mixed voices into `left` and `right`, applying a master gain
    /// that ramps linearly from `gain_start` to `gain_end` across the block.
    ///
    /// The ramp replaces per-sample parameter smoothing inside the engine: the
    /// caller owns the smoother and only passes the two endpoints, which keeps
    /// the engine free of parameter concerns.
    ///
    /// Both slices must have the same length; the shorter one bounds the work.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32], gain_start: f32, gain_end: f32) {
        let frames = left.len().min(right.len());
        if frames == 0 {
            return;
        }

        let gain_step = (gain_end - gain_start) / frames as f32;
        let mut gain = gain_start;
        let mut peak = 0.0f32;

        for frame in 0..frames {
            let mut mix = 0.0;
            for voice in &mut self.voices {
                mix += voice.next_sample(self.waveform);
            }

            let sample = mix * gain;
            gain += gain_step;

            peak = peak.max(sample.abs());
            left[frame] = sample;
            right[frame] = sample;
        }

        // The test signal is mono, so both meters show the same value.
        self.meters.store_peaks(peak, peak);
        self.meters.store_active_voices(self.active_voices() as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{command_queue, CommandProducer};

    const SAMPLE_RATE: f32 = 48_000.0;

    fn engine() -> (Engine, CommandProducer, Arc<Meters>) {
        let (producer, consumer) = command_queue();
        let meters = Arc::new(Meters::new());
        let mut engine = Engine::new(consumer, Arc::clone(&meters));
        engine.prepare(SAMPLE_RATE);
        (engine, producer, meters)
    }

    fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        engine.render(&mut left, &mut right, 1.0, 1.0);
        assert_eq!(left, right, "the mono test signal must be centred");
        left
    }

    #[test]
    fn silence_without_notes() {
        let (mut engine, _producer, meters) = engine();

        let output = render(&mut engine, 512);

        assert!(output.iter().all(|sample| *sample == 0.0));
        assert_eq!(meters.peaks(), (0.0, 0.0));
        assert_eq!(meters.active_voices(), 0);
    }

    #[test]
    fn note_on_produces_audio_and_note_off_silences_it() {
        let (mut engine, _producer, meters) = engine();

        engine.note_on(69, 1.0);
        let output = render(&mut engine, 4_800);
        assert!(output.iter().any(|sample| sample.abs() > 0.1));
        assert_eq!(meters.active_voices(), 1);

        engine.note_off(69);
        // Render long enough for the release stage to complete.
        render(&mut engine, 4_800);
        assert_eq!(engine.active_voices(), 0);

        let after = render(&mut engine, 256);
        assert!(after.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn output_stays_finite_for_every_waveform() {
        for waveform in Waveform::ALL {
            let (mut engine, _producer, _meters) = engine();
            engine.set_waveform(waveform);
            for note in 36..48 {
                engine.note_on(note, 1.0);
            }

            let output = render(&mut engine, 4_800);

            assert!(
                output.iter().all(|sample| sample.is_finite()),
                "{waveform:?} produced a non-finite sample"
            );
        }
    }

    #[test]
    fn voices_are_stolen_instead_of_growing() {
        let (mut engine, _producer, _meters) = engine();

        for note in 0..(MAX_VOICES as u8 + 8) {
            engine.note_on(note, 1.0);
        }
        render(&mut engine, 64);

        assert_eq!(engine.active_voices(), MAX_VOICES);
        // The surviving voices are the most recently started ones.
        let oldest_surviving = 8;
        for note in oldest_surviving..(MAX_VOICES as u8 + 8) {
            engine.note_off(note);
        }
        render(&mut engine, 4_800);
        assert_eq!(engine.active_voices(), 0);
    }

    #[test]
    fn set_waveform_command_is_applied() {
        let (mut engine, mut producer, _meters) = engine();
        assert_eq!(engine.waveform(), Waveform::Sine);

        producer
            .push(EngineCommand::SetWaveform(Waveform::Square))
            .expect("the queue is empty and has capacity");
        engine.apply_commands();

        assert_eq!(engine.waveform(), Waveform::Square);
    }

    #[test]
    fn all_notes_off_command_releases_every_voice() {
        let (mut engine, mut producer, _meters) = engine();
        engine.note_on(60, 1.0);
        engine.note_on(64, 1.0);
        render(&mut engine, 256);
        assert_eq!(engine.active_voices(), 2);

        producer
            .push(EngineCommand::AllNotesOff)
            .expect("the queue is empty and has capacity");
        engine.apply_commands();
        render(&mut engine, 4_800);

        assert_eq!(engine.active_voices(), 0);
    }

    #[test]
    fn gain_ramp_is_applied_across_the_block() {
        let (mut engine, _producer, _meters) = engine();
        engine.set_waveform(Waveform::Square);
        engine.note_on(69, 1.0);
        // Let the attack finish so the level is steady during the measurement.
        render(&mut engine, 2_400);

        let mut left = vec![0.0; 1_024];
        let mut right = vec![0.0; 1_024];
        engine.render(&mut left, &mut right, 0.0, 1.0);

        let first_half = left[..512].iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        let second_half = left[512..].iter().fold(0.0f32, |acc, s| acc.max(s.abs()));

        assert!(
            second_half > first_half,
            "gain ramp should rise: {first_half} -> {second_half}"
        );
    }

    #[test]
    fn reset_drops_all_voices() {
        let (mut engine, _producer, meters) = engine();
        engine.note_on(60, 1.0);
        render(&mut engine, 256);

        engine.reset();

        assert_eq!(engine.active_voices(), 0);
        assert_eq!(meters.peaks(), (0.0, 0.0));
    }
}
