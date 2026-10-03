use std::sync::Arc;

use crate::command::{CellSpec, CommandConsumer, DisposalProducer, EngineCommand};
use crate::meters::Meters;
use crate::sample::SampleBuffer;
use crate::voice::Voice;

/// Number of preallocated voices. Notes beyond this steal the oldest voice.
pub const MAX_VOICES: usize = 16;

/// Number of MIDI notes a cell can sit on.
pub const NOTE_COUNT: usize = 128;

/// Fallback sample rate used before the host reports the real one.
const DEFAULT_SAMPLE_RATE: f32 = 44_100.0;

/// Note number a preview voice runs under.
///
/// Outside the MIDI range on purpose, so that releasing a played note can
/// never cut an audition short.
const PREVIEW_NOTE: u8 = u8::MAX;

/// The realtime engine.
///
/// All state is preallocated, including the note table. [`Engine::render`]
/// performs no allocation, takes no locks and does no I/O, so it is safe to
/// call from an audio callback.
pub struct Engine {
    sample_rate: f32,
    sample: Option<Arc<SampleBuffer>>,
    /// What each MIDI note plays. A fixed array rather than a map, so that a
    /// note on is a single index instead of a lookup.
    cells: [Option<CellSpec>; NOTE_COUNT],
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
            cells: [None; NOTE_COUNT],
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

    /// What `note` currently plays.
    pub fn cell(&self, note: u8) -> Option<CellSpec> {
        self.cells.get(note as usize).copied().flatten()
    }

    /// How many notes carry a cell.
    pub fn mapped_notes(&self) -> usize {
        self.cells.iter().filter(|cell| cell.is_some()).count()
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
                EngineCommand::SetCell { note, spec } => {
                    if let Some(slot) = self.cells.get_mut(note as usize) {
                        *slot = spec;
                    }
                }
                EngineCommand::ClearCells => self.cells = [None; NOTE_COUNT],
                EngineCommand::Preview(spec) => self.trigger(PREVIEW_NOTE, 1.0, spec),
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

    /// Start the cell sitting on `note`, if there is one.
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        let Some(spec) = self.cell(note) else {
            return;
        };
        self.trigger(note, velocity, spec);
    }

    /// Start a voice over `spec`, stealing the oldest one if needed.
    fn trigger(&mut self, note: u8, velocity: f32, spec: CellSpec) {
        if self.sample.is_none() || spec.bounds.is_empty() {
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

        self.voices[slot].start(note, velocity, age, spec, self.sample_rate);
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
    use crate::command::{
        command_queue, disposal_queue, CommandProducer, DisposalConsumer, SliceBounds,
    };

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

    fn spec(start: u64, end: u64) -> CellSpec {
        CellSpec {
            bounds: SliceBounds {
                start_frame: start,
                end_frame: end,
            },
            ..CellSpec::default()
        }
    }

    fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        engine.render(&mut left, &mut right, 1.0, 1.0);
        left
    }

    /// Load a sample and put one cell covering all of it on note 60.
    fn load(harness: &mut Harness, frames: usize) {
        harness
            .commands
            .push(EngineCommand::SetSample(dc_sample(frames)))
            .expect("the queue is empty and has capacity");
        harness
            .commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(spec(0, frames as u64)),
            })
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
    fn an_unmapped_note_starts_no_voice() {
        let mut h = harness();
        load(&mut h, 48_000);

        h.engine.note_on(61, 1.0);

        assert_eq!(h.engine.active_voices(), 0);
    }

    #[test]
    fn a_mapped_note_plays_its_cell() {
        let mut h = harness();
        load(&mut h, 48_000);

        h.engine.note_on(60, 1.0);
        let output = render(&mut h.engine, 4_800);

        assert!(output.iter().any(|sample| sample.abs() > 0.5));
        assert_eq!(h.meters.active_voices(), 1);
    }

    #[test]
    fn different_notes_play_their_own_regions() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(spec(0, 1_000)),
            })
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 62,
                spec: Some(spec(30_000, 31_000)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        h.engine.note_on(62, 1.0);
        render(&mut h.engine, 256);

        let playhead = h.meters.playhead().expect("a voice is sounding");
        assert!(
            (30_000..31_000).contains(&playhead),
            "note 62 must play its own region, got {playhead}"
        );
    }

    #[test]
    fn two_notes_may_share_one_region_with_different_settings() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(spec(0, 10_000)),
            })
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 61,
                spec: Some(CellSpec {
                    reverse: true,
                    gain: 0.25,
                    ..spec(0, 10_000)
                }),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        let forward = h.engine.cell(60).expect("note 60 is mapped");
        let reversed = h.engine.cell(61).expect("note 61 is mapped");

        assert_eq!(forward.bounds, reversed.bounds);
        assert!(!forward.reverse);
        assert!(reversed.reverse);
        assert_eq!(reversed.gain, 0.25);
    }

    #[test]
    fn taking_a_cell_off_a_note_silences_it() {
        let mut h = harness();
        load(&mut h, 48_000);

        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: None,
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();
        h.engine.note_on(60, 1.0);

        assert_eq!(h.engine.active_voices(), 0);
        assert_eq!(h.engine.cell(60), None);
    }

    #[test]
    fn clearing_takes_every_cell_off() {
        let mut h = harness();
        load(&mut h, 48_000);
        assert_eq!(h.engine.mapped_notes(), 1);

        h.commands
            .push(EngineCommand::ClearCells)
            .expect("the queue has capacity");
        h.engine.apply_commands();

        assert_eq!(h.engine.mapped_notes(), 0);
    }

    #[test]
    fn a_whole_keyboard_can_be_mapped_in_one_block() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        for note in 0..NOTE_COUNT as u8 {
            h.commands
                .push(EngineCommand::SetCell {
                    note,
                    spec: Some(spec(0, 1_000)),
                })
                .expect("the queue must hold a full remap");
        }

        h.engine.apply_commands();

        assert_eq!(h.engine.mapped_notes(), NOTE_COUNT);
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
    fn a_short_cell_stops_on_its_own() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(spec(0, 480)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 9_600);

        assert_eq!(
            h.engine.active_voices(),
            0,
            "a voice must end when its cell does, without a note off"
        );
    }

    #[test]
    fn output_stays_finite_with_every_voice_sounding() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        for note in 36..60u8 {
            h.commands
                .push(EngineCommand::SetCell {
                    note,
                    spec: Some(spec(0, 48_000)),
                })
                .expect("the queue has capacity");
        }
        h.engine.apply_commands();
        for note in 36..60 {
            h.engine.note_on(note, 1.0);
        }

        let output = render(&mut h.engine, 4_800);

        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn voices_are_stolen_instead_of_growing() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        for note in 0..(MAX_VOICES as u8 + 8) {
            h.commands
                .push(EngineCommand::SetCell {
                    note,
                    spec: Some(spec(0, 48_000)),
                })
                .expect("the queue has capacity");
        }
        h.engine.apply_commands();

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
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(spec(0, 9_999)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();
        h.engine.note_on(60, 1.0);
        assert_eq!(h.engine.active_voices(), 1);
        assert!(h.disposal.pop().is_ok(), "the swap handed a buffer back");
    }

    #[test]
    fn a_preview_plays_its_own_region_without_a_note() {
        let mut h = harness();
        load(&mut h, 48_000);

        h.commands
            .push(EngineCommand::Preview(spec(10_000, 20_000)))
            .expect("the queue has capacity");
        h.engine.apply_commands();

        assert_eq!(h.engine.active_voices(), 1);
        render(&mut h.engine, 512);
        let playhead = h.meters.playhead().expect("the preview is sounding");
        assert!(
            (10_000..=10_600).contains(&playhead),
            "the preview must play its own region, got {playhead}"
        );
    }

    #[test]
    fn releasing_a_note_does_not_cut_a_preview() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.commands
            .push(EngineCommand::Preview(spec(0, 48_000)))
            .expect("the queue has capacity");
        h.engine.apply_commands();
        render(&mut h.engine, 256);

        for note in [0u8, 60, 127] {
            h.engine.note_off(note);
        }
        render(&mut h.engine, 4_800);

        assert_eq!(h.engine.active_voices(), 1);
    }

    #[test]
    fn a_preview_without_a_sample_does_nothing() {
        let mut h = harness();

        h.commands
            .push(EngineCommand::Preview(spec(0, 1_000)))
            .expect("the queue has capacity");
        h.engine.apply_commands();

        assert_eq!(h.engine.active_voices(), 0);
    }

    #[test]
    fn all_notes_off_releases_every_voice() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        for note in [60u8, 64] {
            h.commands
                .push(EngineCommand::SetCell {
                    note,
                    spec: Some(spec(0, 48_000)),
                })
                .expect("the queue has capacity");
        }
        h.engine.apply_commands();
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
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(spec(10_000, 20_000)),
            })
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
            "the playhead should start inside the cell, got {first}"
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
