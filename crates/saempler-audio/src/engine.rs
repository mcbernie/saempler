use std::sync::Arc;

use crate::command::{CommandConsumer, DisposalProducer, EngineCommand, SliceBounds};
use crate::meters::Meters;
use crate::sample::SampleBuffer;
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
    sample: Option<Arc<SampleBuffer>>,
    /// Region that newly triggered voices play.
    bounds: SliceBounds,
    voices: [Voice; MAX_VOICES],
    /// Monotonic counter assigning an age to each started voice.
    next_age: u64,
    commands: CommandConsumer,
    /// Retired buffers travel back to a non-realtime thread through here.
    disposal: DisposalProducer,
    meters: Arc<Meters>,
}

impl Engine {
    pub fn new(commands: CommandConsumer, disposal: DisposalProducer, meters: Arc<Meters>) -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            sample: None,
            bounds: SliceBounds::default(),
            voices: [Voice::default(); MAX_VOICES],
            next_age: 0,
            commands,
            disposal,
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
        self.meters.store_playhead(None);
    }

    /// Whether a sample buffer is loaded.
    pub fn has_sample(&self) -> bool {
        self.sample.is_some()
    }

    /// Region newly triggered voices will play.
    pub fn bounds(&self) -> SliceBounds {
        self.bounds
    }

    /// Drain the command queue. Call once at the start of a processing block.
    ///
    /// A command that would retire the current buffer is left in the queue
    /// while the disposal queue is full, and retried in a later block. That is
    /// the only way to guarantee the retired buffer is never released here:
    /// dropping the last reference to it would call the allocator on the audio
    /// thread. It needs a UI thread that has stopped draining entirely, and
    /// the engine keeps playing normally in the meantime.
    pub fn apply_commands(&mut self) {
        while let Ok(command) = self.commands.peek() {
            if self.retires_sample(command) && self.disposal.slots() == 0 {
                return;
            }

            let command = self
                .commands
                .pop()
                .expect("a peeked command is still there: this is the only consumer");

            match command {
                EngineCommand::SetSample(sample) => self.swap_sample(Some(sample)),
                EngineCommand::ClearSample => self.swap_sample(None),
                EngineCommand::SetSlice(bounds) => self.bounds = bounds,
                EngineCommand::AllNotesOff => {
                    for voice in &mut self.voices {
                        voice.release();
                    }
                }
            }
        }
    }

    /// Whether applying `command` would hand the current buffer back.
    fn retires_sample(&self, command: &EngineCommand) -> bool {
        self.sample.is_some()
            && matches!(
                command,
                EngineCommand::SetSample(_) | EngineCommand::ClearSample
            )
    }

    /// Install a new buffer and hand the previous one back for disposal.
    ///
    /// Voices are killed rather than released: their playback positions refer
    /// to the old buffer and would read unrelated audio from the new one.
    fn swap_sample(&mut self, sample: Option<Arc<SampleBuffer>>) {
        let previous = self.sample.take();
        self.sample = sample;

        for voice in &mut self.voices {
            voice.kill();
        }

        if let Some(previous) = previous {
            // `apply_commands` checked that a slot is free before letting this
            // command through, so the buffer cannot bounce back here.
            let _ = self.disposal.push(previous);
        }
    }

    /// Start a voice. Steals the oldest voice when all are in use.
    ///
    /// Does nothing without a loaded sample or an empty slice, so a trigger
    /// can never produce a voice that has nothing to play.
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        if self.sample.is_none() || self.bounds.is_empty() {
            return;
        }

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

        self.voices[slot].start(note, velocity, age, self.bounds, self.sample_rate);
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

    /// Playback position of the most recently started sounding voice.
    ///
    /// With several voices at once the newest one is the one the player just
    /// triggered, so that is the position worth showing.
    fn playhead(&self) -> Option<u64> {
        self.voices
            .iter()
            .filter(|voice| voice.is_active())
            .max_by_key(|voice| voice.age())
            .map(|voice| voice.position())
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

        // Borrowed rather than cloned: an `Arc` clone here would add atomic
        // traffic to every block for no benefit.
        let Some(sample) = self.sample.as_ref() else {
            left[..frames].fill(0.0);
            right[..frames].fill(0.0);
            self.meters.store_peaks(0.0, 0.0);
            self.meters.store_active_voices(0);
            self.meters.store_playhead(None);
            return;
        };

        let gain_step = (gain_end - gain_start) / frames as f32;
        let mut gain = gain_start;
        let mut peak_left = 0.0f32;
        let mut peak_right = 0.0f32;

        for frame in 0..frames {
            let mut mix_left = 0.0;
            let mut mix_right = 0.0;
            for voice in &mut self.voices {
                let (voice_left, voice_right) = voice.next_frame(sample);
                mix_left += voice_left;
                mix_right += voice_right;
            }

            mix_left *= gain;
            mix_right *= gain;
            gain += gain_step;

            peak_left = peak_left.max(mix_left.abs());
            peak_right = peak_right.max(mix_right.abs());
            left[frame] = mix_left;
            right[frame] = mix_right;
        }

        self.meters.store_peaks(peak_left, peak_right);
        self.meters.store_active_voices(self.active_voices() as u32);
        self.meters.store_playhead(self.playhead());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{command_queue, disposal_queue, CommandProducer, DisposalConsumer};

    const SAMPLE_RATE: f32 = 48_000.0;

    struct Harness {
        engine: Engine,
        commands: CommandProducer,
        disposal: DisposalConsumer,
        meters: Arc<Meters>,
    }

    fn harness() -> Harness {
        let (commands, command_consumer) = command_queue();
        let (disposal_producer, disposal) = disposal_queue();
        let meters = Arc::new(Meters::new());
        let mut engine = Engine::new(command_consumer, disposal_producer, Arc::clone(&meters));
        engine.prepare(SAMPLE_RATE);

        Harness {
            engine,
            commands,
            disposal,
            meters,
        }
    }

    /// A buffer of constant full-scale samples, so measured output reflects
    /// envelope and gain alone.
    fn dc_sample(frames: usize) -> Arc<SampleBuffer> {
        Arc::new(SampleBuffer::new(
            vec![vec![1.0; frames], vec![1.0; frames]],
            48_000,
        ))
    }

    fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        engine.render(&mut left, &mut right, 1.0, 1.0);
        left
    }

    /// Load a sample covering the whole buffer and select it as the slice.
    fn load(harness: &mut Harness, frames: usize) {
        harness
            .commands
            .push(EngineCommand::SetSample(dc_sample(frames)))
            .expect("the queue is empty and has capacity");
        harness
            .commands
            .push(EngineCommand::SetSlice(SliceBounds {
                start_frame: 0,
                end_frame: frames as u64,
            }))
            .expect("the queue is empty and has capacity");
        harness.engine.apply_commands();
    }

    #[test]
    fn silence_without_a_sample() {
        let mut h = harness();

        h.engine.note_on(60, 1.0);
        let output = render(&mut h.engine, 512);

        assert!(output.iter().all(|sample| *sample == 0.0));
        assert_eq!(h.engine.active_voices(), 0);
        assert_eq!(h.meters.peaks(), (0.0, 0.0));
    }

    #[test]
    fn triggering_without_a_slice_starts_no_voice() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(1_000)))
            .expect("the queue is empty and has capacity");
        h.engine.apply_commands();

        h.engine.note_on(60, 1.0);

        assert_eq!(h.engine.active_voices(), 0);
    }

    #[test]
    fn note_on_plays_the_selected_slice() {
        let mut h = harness();
        load(&mut h, 48_000);

        h.engine.note_on(60, 1.0);
        let output = render(&mut h.engine, 4_800);

        assert!(output.iter().any(|sample| sample.abs() > 0.5));
        assert_eq!(h.meters.active_voices(), 1);
    }

    #[test]
    fn note_off_silences_the_voice() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 1_000);

        h.engine.note_off(60);
        render(&mut h.engine, 4_800);

        assert_eq!(h.engine.active_voices(), 0);
        let after = render(&mut h.engine, 256);
        assert!(after.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn a_short_slice_stops_on_its_own() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue is empty and has capacity");
        h.commands
            .push(EngineCommand::SetSlice(SliceBounds {
                start_frame: 0,
                end_frame: 480,
            }))
            .expect("the queue is empty and has capacity");
        h.engine.apply_commands();

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 9_600);

        assert_eq!(
            h.engine.active_voices(),
            0,
            "a voice must end when its slice does, without a note off"
        );
    }

    #[test]
    fn output_stays_finite_with_every_voice_sounding() {
        let mut h = harness();
        load(&mut h, 48_000);
        for note in 36..60 {
            h.engine.note_on(note, 1.0);
        }

        let output = render(&mut h.engine, 4_800);

        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn voices_are_stolen_instead_of_growing() {
        let mut h = harness();
        load(&mut h, 48_000);

        for note in 0..(MAX_VOICES as u8 + 8) {
            h.engine.note_on(note, 1.0);
        }
        render(&mut h.engine, 64);

        assert_eq!(h.engine.active_voices(), MAX_VOICES);
    }

    #[test]
    fn replacing_the_sample_hands_the_old_buffer_back() {
        let mut h = harness();
        load(&mut h, 1_000);

        h.commands
            .push(EngineCommand::SetSample(dc_sample(2_000)))
            .expect("the queue is empty and has capacity");
        h.engine.apply_commands();

        let retired = h
            .disposal
            .pop()
            .expect("the old buffer must be handed back");
        assert_eq!(retired.frames(), 1_000);
    }

    #[test]
    fn clearing_the_sample_hands_the_buffer_back_and_silences_output() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 1_000);

        h.commands
            .push(EngineCommand::ClearSample)
            .expect("the queue is empty and has capacity");
        h.engine.apply_commands();

        assert!(h.disposal.pop().is_ok(), "the buffer must be handed back");
        assert!(!h.engine.has_sample());
        assert_eq!(h.engine.active_voices(), 0);
        assert!(render(&mut h.engine, 256).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn a_full_disposal_queue_defers_the_swap_instead_of_freeing_the_buffer() {
        let mut h = harness();
        load(&mut h, 100);

        // Fill the disposal queue by swapping without ever draining it.
        for index in 1..=crate::command::DISPOSAL_CAPACITY {
            h.commands
                .push(EngineCommand::SetSample(dc_sample(100 + index)))
                .expect("the queue has capacity");
            h.engine.apply_commands();
        }
        let blocked = dc_sample(9_999);
        let kept = Arc::clone(&blocked);
        h.commands
            .push(EngineCommand::SetSample(blocked))
            .expect("the queue has capacity");

        h.engine.apply_commands();

        // The command was left in the queue rather than retiring a buffer the
        // engine would have had to free itself.
        assert_eq!(
            Arc::strong_count(&kept),
            2,
            "the new buffer is still only held by the test and the queue"
        );

        // Draining the disposal queue unblocks the swap on the next block.
        while h.disposal.pop().is_ok() {}
        h.engine.apply_commands();

        h.commands
            .push(EngineCommand::SetSlice(SliceBounds {
                start_frame: 0,
                end_frame: 9_999,
            }))
            .expect("the queue has capacity");
        h.engine.apply_commands();
        h.engine.note_on(60, 1.0);
        assert_eq!(h.engine.active_voices(), 1);
        assert!(h.disposal.pop().is_ok(), "the swap handed a buffer back");
    }

    #[test]
    fn switching_slices_moves_newly_triggered_voices() {
        let mut h = harness();
        load(&mut h, 48_000);

        h.commands
            .push(EngineCommand::SetSlice(SliceBounds {
                start_frame: 1_000,
                end_frame: 2_000,
            }))
            .expect("the queue is empty and has capacity");
        h.engine.apply_commands();

        assert_eq!(h.engine.bounds().start_frame, 1_000);
    }

    #[test]
    fn all_notes_off_releases_every_voice() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        h.engine.note_on(64, 1.0);
        render(&mut h.engine, 256);
        assert_eq!(h.engine.active_voices(), 2);

        h.commands
            .push(EngineCommand::AllNotesOff)
            .expect("the queue is empty and has capacity");
        h.engine.apply_commands();
        render(&mut h.engine, 4_800);

        assert_eq!(h.engine.active_voices(), 0);
    }

    #[test]
    fn gain_ramp_is_applied_across_the_block() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        // Let the attack finish so the level is steady during the measurement.
        render(&mut h.engine, 2_400);

        let mut left = vec![0.0; 1_024];
        let mut right = vec![0.0; 1_024];
        h.engine.render(&mut left, &mut right, 0.0, 1.0);

        let first_half = left[..512].iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        let second_half = left[512..].iter().fold(0.0f32, |acc, s| acc.max(s.abs()));

        assert!(
            second_half > first_half,
            "gain ramp should rise: {first_half} -> {second_half}"
        );
    }

    #[test]
    fn the_playhead_follows_the_newest_voice() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetSlice(SliceBounds {
                start_frame: 10_000,
                end_frame: 20_000,
            }))
            .expect("the queue has capacity");
        h.engine.apply_commands();
        assert_eq!(h.meters.playhead(), None);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 512);

        let first = h
            .meters
            .playhead()
            .expect("a sounding voice has a position");
        assert!(
            (10_000..=10_600).contains(&first),
            "the playhead should start inside the slice, got {first}"
        );

        render(&mut h.engine, 512);
        let later = h.meters.playhead().expect("still sounding");
        assert!(later > first, "the playhead must advance");
    }

    #[test]
    fn the_playhead_clears_when_nothing_sounds() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 512);
        assert!(h.meters.playhead().is_some());

        h.engine.note_off(60);
        render(&mut h.engine, 4_800);

        assert_eq!(h.meters.playhead(), None);
    }

    #[test]
    fn reset_drops_all_voices() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 256);

        h.engine.reset();

        assert_eq!(h.engine.active_voices(), 0);
        assert_eq!(h.meters.peaks(), (0.0, 0.0));
    }
}
