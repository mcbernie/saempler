use std::sync::Arc;

use saempler_dsp::{Delay, Flanger, Phaser, Reverb, SaturationKind, Saturator};
use saempler_model::{DriveShape, Modifier, ModifierMode, SendRack};

use crate::automation::{SliceAutomation, AUTOMATION_SLOTS};

use crate::command::{CellSpec, CommandConsumer, DisposalProducer, EngineCommand};
use crate::meters::Meters;
use crate::modifiers::{ModifierState, DEFAULT_TEMPO};
use crate::sample::SampleBuffer;
use crate::voice::Voice;

/// Number of preallocated voices. Notes beyond this steal the oldest voice.
///
/// Tied to the number of published playback positions, so that every sounding
/// voice has a slot to report from.
pub const MAX_VOICES: usize = crate::meters::PLAYHEAD_SLOTS;

/// Number of MIDI notes a cell can sit on.
pub const NOTE_COUNT: usize = 128;

/// Fallback sample rate used before the host reports the real one.
const DEFAULT_SAMPLE_RATE: f32 = 44_100.0;

/// Note number a preview voice runs under.
///
/// Outside the MIDI range on purpose, so that releasing a played note can
/// never cut an audition short.
const PREVIEW_NOTE: u8 = u8::MAX;

/// Number of shared sends: delay, reverb, phaser and flanger, in that order.
const SEND_COUNT: usize = 4;

/// How long an audition is held before the key is let go for it, in seconds.
///
/// An audition has no key to release, so a looping cell would sound until
/// something else stopped it. Holding it for a couple of seconds lets a loop,
/// a repeat and a collapse be heard doing what they do, and then ends.
const PREVIEW_SECONDS: f32 = 2.0;

/// The realtime engine.
///
/// All state is preallocated, including the note table. [`Engine::render`]
/// performs no allocation, takes no locks and does no I/O, so it is safe to
/// call from an audio callback.
pub struct Engine {
    sample_rate: f32,
    /// Output frames until the audition is released. Zero means none is held.
    preview_left: u64,
    /// The sends, behind the mixer and shared by every voice.
    ///
    /// Shared rather than one set per voice: sixteen voices would mean
    /// sixteen reverbs, which is the cost of the whole instrument again for a
    /// difference nobody can hear under a beat.
    delay: Delay,
    reverb: Reverb,
    phaser: Phaser,
    flanger: Flanger,
    sends: SendRack,
    /// How much of each send comes back into the mix.
    ///
    /// Held here rather than read from the settings per frame so that the
    /// render loop does not have to know which sends are being driven.
    levels: [f32; SEND_COUNT],
    /// Saturates the wet path of a send while its modifier is held.
    send_drive: Saturator,
    /// Which sends are running from the driven settings.
    driven: [bool; SEND_COUNT],
    sample: Option<Arc<SampleBuffer>>,
    /// What each MIDI note plays. A fixed array rather than a map, so that a
    /// note on is a single index instead of a lookup.
    cells: [Option<CellSpec>; NOTE_COUNT],
    /// What each MIDI note does to the *next* performance note.
    modifier_notes: [Option<(Modifier, ModifierMode)>; NOTE_COUNT],
    modifiers: ModifierState,
    /// Host tempo, for the musical lengths stutter and brake work in.
    tempo: f64,
    voices: [Voice; MAX_VOICES],
    /// What the host rides each send's return level by.
    ///
    /// A multiplier on top of the rack's own level rather than a replacement,
    /// so the knob in the window still says what the send is set to and the
    /// automation lane says how much of it is wanted right now.
    send_scale: [f32; SEND_COUNT],
    /// What the host is automating, one slot per slice.
    ///
    /// One longer than the bank: the last entry is left at its defaults and is
    /// what a voice without a slot reads, so the render loop never has to ask
    /// whether a voice has one.
    automation: [SliceAutomation; AUTOMATION_SLOTS + 1],
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
            preview_left: 0,
            delay: Delay::default(),
            reverb: Reverb::default(),
            phaser: Phaser::default(),
            flanger: Flanger::default(),
            sends: SendRack::default(),
            levels: [0.0; SEND_COUNT],
            send_drive: Saturator::default(),
            driven: [false; SEND_COUNT],
            sample: None,
            cells: [None; NOTE_COUNT],
            modifier_notes: [None; NOTE_COUNT],
            modifiers: ModifierState::new(),
            tempo: DEFAULT_TEMPO,
            voices: [Voice::default(); MAX_VOICES],
            send_scale: [1.0; SEND_COUNT],
            automation: [SliceAutomation::default(); AUTOMATION_SLOTS + 1],
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

        // The sends take their buffers here, which is the one place before
        // playback where allocating is allowed.
        self.delay.prepare(sample_rate);
        self.reverb.prepare(sample_rate);
        self.phaser.prepare(sample_rate);
        self.flanger.prepare(sample_rate);
        self.apply_sends();
        self.reset();
    }

    /// Drop all sounding voices and reset metering.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.kill();
        }
        self.next_age = 0;
        self.preview_left = 0;
        self.delay.reset();
        self.reverb.reset();
        self.phaser.reset();
        self.flanger.reset();
        self.modifiers.clear();
        self.meters.store_peaks(0.0, 0.0);
        self.meters.store_active_voices(0);
        self.meters.clear_playheads();
        self.meters.clear_modulation();
        self.meters.store_modifiers(0);
    }

    /// Tell the engine the host tempo. Called once per block, no lock taken.
    pub fn set_tempo(&mut self, tempo: f64) {
        if !tempo.is_finite() || tempo <= 1.0 || tempo == self.tempo {
            return;
        }

        self.tempo = tempo;
        self.meters.store_tempo(tempo);
        // A synced delay follows the host rather than the setting alone.
        self.apply_sends();
        // Synced LFOs follow without restarting: a tempo change mid-note is a
        // change of speed, not a new note.
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.set_tempo(tempo);
            }
        }
    }

    /// Which modifiers would affect the next performance note.
    pub fn engaged_modifiers(&self) -> u32 {
        self.modifiers.bits()
    }

    /// What `note` does as a modifier, if anything.
    pub fn modifier_note(&self, note: u8) -> Option<(Modifier, ModifierMode)> {
        self.modifier_notes.get(note as usize).copied().flatten()
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
                    // A voice already sounding this key follows the edit. A
                    // chop instrument is played and adjusted at the same time,
                    // and a change that is only heard on the next note is not
                    // an adjustment anyone can make by ear.
                    if let Some(spec) = spec {
                        for voice in &mut self.voices {
                            if voice.is_playing_note(note) {
                                voice.rebase(spec);
                            }
                        }
                        self.retune_voices();
                    }
                }
                EngineCommand::ClearCells => self.cells = [None; NOTE_COUNT],
                EngineCommand::SetModifier { note, assignment } => {
                    if let Some(slot) = self.modifier_notes.get_mut(note as usize) {
                        *slot = assignment;
                    }
                }
                EngineCommand::ClearModifiers => {
                    self.modifier_notes = [None; NOTE_COUNT];
                    self.modifiers.clear();
                    self.retune_voices();
                }
                EngineCommand::Preview(spec) => {
                    self.trigger(PREVIEW_NOTE, 1.0, spec, spec);
                    // A release trigger would never fire without this, and a
                    // loop would never end.
                    self.preview_left = (PREVIEW_SECONDS * self.sample_rate) as u64;
                }
                EngineCommand::SetModifierSettings(settings) => {
                    self.modifiers.set_settings(settings);
                }
                EngineCommand::SetSends(sends) => {
                    self.sends = sends;
                    self.apply_sends();
                }
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

    /// Handle a note going down.
    ///
    /// A modifier note changes state and makes no sound. A performance note
    /// starts its cell with whatever modifiers are engaged, which also
    /// consumes any one shot that was waiting.
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        if let Some((modifier, mode)) = self.modifier_note(note) {
            self.modifiers.press(modifier, mode);
            self.retune_voices();
            // An effect modifier changes the shared sends, not only the
            // voices, so they are pointed at their driven settings here.
            self.apply_sends();
            self.meters.store_modifiers(self.modifiers.bits());
            return;
        }

        let Some(base) = self.cell(note) else {
            return;
        };
        let spec = self.modifiers.apply(base, self.tempo, self.sample_rate);
        self.meters.store_modifiers(self.modifiers.bits());
        self.trigger(note, velocity, base, spec);
    }

    /// Hand every sounding voice the modifiers that are in effect now.
    ///
    /// Called when a modifier key moves, so that pressing one in the middle of
    /// a note changes that note rather than only the next one.
    fn retune_voices(&mut self) {
        let (tempo, sample_rate) = (self.tempo, self.sample_rate);
        for voice in &mut self.voices {
            if !voice.is_active() {
                continue;
            }
            let spec = self
                .modifiers
                .applied_live(voice.base(), tempo, sample_rate);
            voice.retune(spec);
        }
    }

    /// Start a voice over `spec`, stealing the oldest one if needed.
    ///
    /// `base` is the cell before modifiers, kept so that a modifier pressed
    /// later can be applied to the untouched cell.
    fn trigger(&mut self, note: u8, velocity: f32, base: CellSpec, spec: CellSpec) {
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

        self.voices[slot].start(
            note,
            velocity,
            age,
            base,
            spec,
            self.sample_rate,
            self.tempo,
        );
    }

    /// Release every voice currently holding `note`.
    pub fn note_off(&mut self, note: u8) {
        if let Some((modifier, mode)) = self.modifier_note(note) {
            self.modifiers.release(modifier, mode);
            self.retune_voices();
            self.apply_sends();
            self.meters.store_modifiers(self.modifiers.bits());
            return;
        }

        for voice in &mut self.voices {
            if voice.is_playing_note(note) {
                voice.release();
            }
        }
    }

    /// Take the host's send levels for this block.
    pub fn set_send_scale(&mut self, scale: [f32; SEND_COUNT]) {
        for (slot, value) in self.send_scale.iter_mut().zip(scale) {
            *slot = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                1.0
            };
        }
    }

    /// Take the host's automation for this block.
    ///
    /// Written straight into the engine rather than sent through the command
    /// queue: the plugin owns the engine and calls this from the audio
    /// callback, so there is nothing to synchronise and nothing to allocate.
    /// Values are repaired here, once per block, rather than per frame.
    pub fn set_automation(&mut self, slots: &[SliceAutomation; AUTOMATION_SLOTS]) {
        for (slot, value) in self.automation.iter_mut().zip(slots) {
            *slot = value.sanitized();
        }
    }

    /// What a slot is currently doing, for tests.
    pub fn automation(&self, slot: usize) -> SliceAutomation {
        self.automation[slot.min(AUTOMATION_SLOTS)]
    }

    /// How hard each playback modifier hits, for tests.
    pub fn modifier_settings(&self) -> saempler_model::ModifierSettings {
        self.modifiers.settings()
    }

    /// The send settings the engine is running from.
    ///
    /// For tests and inspection: the engine is handed its settings and keeps
    /// no project of its own, so this is the only way to ask whether a
    /// restored session actually reached it.
    pub fn sends(&self) -> SendRack {
        self.sends
    }

    /// Number of voices that are currently sounding.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| voice.is_active()).count()
    }

    /// Point the sends at their settings.
    ///
    /// Called whenever the settings or the tempo change, which is cheap: none
    /// of these allocate, and a synced delay has to follow the host.
    fn apply_sends(&mut self) {
        let rack = self.sends.sanitized();

        // Which settings a send runs from is decided per send rather than for
        // the rack as a whole: holding the reverb key should not also wreck
        // the delay. The settings themselves are untouched, so letting go
        // puts the send straight back where it was.
        for (index, driven) in self.driven.iter_mut().enumerate() {
            *driven = self.modifiers.driven_send(index);
        }
        let settings = |driven: bool| if driven { rack.driven } else { rack.normal };

        let delay = settings(self.driven[0]);
        self.delay.set(
            delay.delay_time(self.tempo),
            delay.delay_feedback,
            delay.delay_damping_hz,
        );

        let reverb = settings(self.driven[1]);
        self.reverb.set(reverb.reverb_size, reverb.reverb_damping);

        let phaser = settings(self.driven[2]);
        self.phaser.set(
            phaser.phaser_rate_hz,
            phaser.phaser_depth,
            phaser.phaser_feedback,
        );

        let flanger = settings(self.driven[3]);
        self.flanger.set(
            flanger.flanger_rate_hz,
            flanger.flanger_depth,
            flanger.flanger_feedback,
        );

        self.levels = [
            delay.delay_level,
            reverb.reverb_level,
            phaser.phaser_level,
            flanger.flanger_level,
        ];

        // The wet path is saturated while a send is being driven, which is
        // what makes it sound thrown rather than merely turned up.
        let kind = match rack.drive_shape {
            DriveShape::Soft => SaturationKind::Soft,
            DriveShape::Hard => SaturationKind::Hard,
            DriveShape::Tube => SaturationKind::Tube,
        };
        self.send_drive.set(kind, rack.drive);
    }

    /// Count down the audition and let go of its key when the time is up.
    fn advance_preview(&mut self, frames: u64) {
        if self.preview_left == 0 {
            return;
        }

        self.preview_left = self.preview_left.saturating_sub(frames);
        if self.preview_left == 0 {
            for voice in &mut self.voices {
                if voice.is_playing_note(PREVIEW_NOTE) {
                    voice.release();
                }
            }
        }
    }

    /// Publish where every voice is reading.
    fn publish_playheads(&self) {
        for (slot, voice) in self.voices.iter().enumerate() {
            self.meters.store_playhead(slot, voice.active_at());
        }
    }

    /// Publish the modulation of the voice that started most recently.
    ///
    /// One voice rather than all of them: averaging several would produce a
    /// reading that matches none of the notes being played, and the newest is
    /// the one the player just triggered.
    fn publish_modulation(&self) {
        let newest = self
            .voices
            .iter()
            .filter(|voice| voice.is_active())
            .max_by_key(|voice| voice.age());

        match newest {
            Some(voice) => {
                let monitor = voice.monitor();
                self.meters
                    .store_modulation(monitor.envelopes, monitor.lfos, monitor.destinations);
            }
            None => self.meters.clear_modulation(),
        }
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

        // Before the block is rendered, so a released audition fades inside it
        // rather than a block later.
        self.advance_preview(frames as u64);

        // Borrowed rather than cloned: an `Arc` clone here would add atomic
        // traffic to every block for no benefit.
        let Some(sample) = self.sample.as_ref() else {
            left[..frames].fill(0.0);
            right[..frames].fill(0.0);
            self.meters.store_peaks(0.0, 0.0);
            self.meters.store_active_voices(0);
            self.meters.clear_playheads();
            self.meters.clear_modulation();
            return;
        };

        let gain_step = (gain_end - gain_start) / frames as f32;
        let mut gain = gain_start;
        let mut peak_left = 0.0f32;
        let mut peak_right = 0.0f32;

        for frame in 0..frames {
            let mut mix_left = 0.0;
            let mut mix_right = 0.0;
            // What each send is fed this frame: a voice contributes to a send
            // in proportion to its own send amount, so one chop can be soaked
            // in reverb while the next one beside it stays dry.
            let mut feed = [(0.0f32, 0.0f32); SEND_COUNT];

            for voice in &mut self.voices {
                let automation = &self.automation[voice.slot()];
                let (voice_left, voice_right) = voice.next_frame(sample, automation);
                mix_left += voice_left;
                mix_right += voice_right;

                let amounts = voice.sends(automation);
                for (index, amount) in amounts.iter().enumerate() {
                    if *amount > 0.0 {
                        feed[index].0 += voice_left * amount;
                        feed[index].1 += voice_right * amount;
                    }
                }
            }

            // The wet signal of every send is added back to the mix. They run
            // every frame whether or not anything is feeding them, because a
            // reverb tail has to carry on after the last voice has stopped.
            let wet = [
                self.delay.process(feed[0].0, feed[0].1),
                self.reverb.process(feed[1].0, feed[1].1),
                self.phaser.process(feed[2].0, feed[2].1),
                self.flanger.process(feed[3].0, feed[3].1),
            ];
            for (index, (wet_left, wet_right)) in wet.into_iter().enumerate() {
                let level = self.levels[index] * self.send_scale[index];
                if level <= 0.0 {
                    continue;
                }
                // The return level is the last thing applied, so turning a
                // send down turns down what it produced rather than changing
                // how hard it was driven.
                if self.driven[index] {
                    mix_left += self.send_drive.process(wet_left) * level;
                    mix_right += self.send_drive.process(wet_right) * level;
                } else {
                    mix_left += wet_left * level;
                    mix_right += wet_right * level;
                }
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
        self.publish_playheads();
        self.publish_modulation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::NO_SLOT;
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
    /// A steady tone, for the one thing a constant cannot be asked about.
    ///
    /// A low pass passes a constant whatever its corner, so a filter test on
    /// the DC buffer measures nothing at all.
    fn tone_sample(frames: usize) -> Arc<SampleBuffer> {
        let step = std::f32::consts::TAU * 4_000.0 / SAMPLE_RATE;
        let data: Vec<f32> = (0..frames)
            .map(|index| (index as f32 * step).sin())
            .collect();
        Arc::new(SampleBuffer::new(vec![data.clone(), data], 48_000))
    }

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

    /// Loudest sample of what is still ringing once the note itself is over.
    ///
    /// Measured after the release rather than across it: a send comes back
    /// well below the sound that fed it, so a peak taken over the dry signal
    /// is the dry signal whatever the sends are doing.
    fn tail(engine: &mut Engine, frames: usize) -> f32 {
        // Long enough for the slice's release to have finished.
        render(engine, 9_600);
        peak(engine, frames)
    }

    /// Loudest sample over the next `frames`, once any ramp has settled.
    fn peak(engine: &mut Engine, frames: usize) -> f32 {
        render(engine, frames)
            .into_iter()
            .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()))
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
    fn editing_a_cell_is_heard_on_the_note_already_sounding() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 4_800);

        let before = peak(&mut h.engine, 480);

        // Half the gain, while the note rings.
        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(CellSpec {
                    gain: 0.5,
                    ..spec(0, 48_000)
                }),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        let after = peak(&mut h.engine, 480);

        assert!(
            after < before * 0.7,
            "the edit was not heard on the ringing note: {before} -> {after}"
        );
    }

    #[test]
    fn editing_a_cell_does_not_restart_the_note() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 9_600);
        let before = h.meters.playheads().next().expect("the note is sounding");

        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(CellSpec {
                    gain: 0.5,
                    ..spec(0, 48_000)
                }),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();
        render(&mut h.engine, 64);

        let after = h
            .meters
            .playheads()
            .next()
            .expect("it should still be sounding");

        assert!(
            after > before,
            "the playhead jumped back: {before} -> {after}"
        );
    }

    #[test]
    fn an_edit_reaches_a_cell_that_is_not_sounding_too() {
        let mut h = harness();
        load(&mut h, 48_000);

        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(CellSpec {
                    gain: 0.25,
                    ..spec(0, 48_000)
                }),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();
        h.engine.note_on(60, 1.0);

        let level = peak(&mut h.engine, 4_800);

        assert!(level < 0.4, "the stored cell was not used: {level}");
    }

    #[test]
    fn a_cells_filter_shapes_its_own_voice() {
        use saempler_model::{CellEffects, FilterShape};

        let mut dry = harness();
        load(&mut dry, 48_000);
        dry.engine.note_on(60, 1.0);
        let open = peak(&mut dry.engine, 4_800);

        let mut filtered = harness();
        load(&mut filtered, 48_000);
        filtered
            .commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(CellSpec {
                    effects: CellEffects {
                        filter_on: true,
                        // The test sample is a steady level, so a high pass
                        // is what shows the filter doing something: a low
                        // pass would correctly leave it alone.
                        filter_shape: FilterShape::HighPass,
                        cutoff_hz: 2_000.0,
                        ..CellEffects::default()
                    },
                    ..spec(0, 48_000)
                }),
            })
            .expect("the queue has capacity");
        filtered.engine.apply_commands();
        filtered.engine.note_on(60, 1.0);
        let closed = peak(&mut filtered.engine, 4_800);

        assert!(
            closed < open * 0.5,
            "the filter did nothing: {open} -> {closed}"
        );
    }

    #[test]
    fn an_effect_modifier_throws_a_dry_cell_into_its_send() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.commands
            .push(EngineCommand::SetModifier {
                note: 48,
                assignment: Some((Modifier::Reverb, ModifierMode::Hold)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        // The cell feeds nothing: without the key held there is no tail.
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 480);
        h.engine.note_off(60);
        let dry_tail = tail(&mut h.engine, 24_000);

        let mut wet = harness();
        load(&mut wet, 48_000);
        wet.commands
            .push(EngineCommand::SetModifier {
                note: 48,
                assignment: Some((Modifier::Reverb, ModifierMode::Hold)),
            })
            .expect("the queue has capacity");
        wet.engine.apply_commands();

        wet.engine.note_on(48, 1.0);
        wet.engine.note_on(60, 1.0);
        render(&mut wet.engine, 480);
        wet.engine.note_off(60);
        let wet_tail = tail(&mut wet.engine, 24_000);

        assert!(
            wet_tail > dry_tail + 0.001,
            "the key did not throw it into the send: {dry_tail} vs {wet_tail}"
        );
    }

    #[test]
    fn letting_go_of_an_effect_modifier_puts_the_send_back() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.commands
            .push(EngineCommand::SetModifier {
                note: 48,
                assignment: Some((Modifier::Delay, ModifierMode::Hold)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        h.engine.note_on(48, 1.0);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 4_800);
        h.engine.note_off(48);
        h.engine.note_off(60);

        // Whatever is still ringing has to die away rather than build up on
        // the driven feedback it was left with.
        let output = render(&mut h.engine, 48_000 * 10);

        assert!(output.iter().all(|sample| sample.is_finite()));
        let tail = output
            .iter()
            .rev()
            .take(4_800)
            .fold(0.0f32, |loudest, sample| loudest.max(sample.abs()));
        assert!(tail < 0.5, "it never settled: {tail}");
    }

    #[test]
    fn a_send_only_receives_the_cells_that_feed_it() {
        use saempler_model::CellEffects;

        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 480);
        h.engine.note_off(60);
        let dry_tail = tail(&mut h.engine, 24_000);

        let mut wet = harness();
        load(&mut wet, 48_000);
        wet.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(CellSpec {
                    effects: CellEffects {
                        reverb_send: 1.0,
                        ..CellEffects::default()
                    },
                    ..spec(0, 48_000)
                }),
            })
            .expect("the queue has capacity");
        wet.engine.apply_commands();
        wet.engine.note_on(60, 1.0);
        render(&mut wet.engine, 480);
        wet.engine.note_off(60);
        let wet_tail = tail(&mut wet.engine, 24_000);

        assert!(
            wet_tail > dry_tail + 0.001,
            "the send never rang: {dry_tail} vs {wet_tail}"
        );
    }

    #[test]
    fn the_sends_follow_their_settings() {
        use saempler_model::{SendEffects, SendRack};

        let mut h = harness();
        load(&mut h, 48_000);

        h.commands
            .push(EngineCommand::SetSends(SendRack {
                normal: SendEffects {
                    reverb_size: 1.0,
                    reverb_damping: 0.0,
                    ..SendEffects::default()
                },
                ..SendRack::default()
            }))
            .expect("the queue has capacity");
        h.engine.apply_commands();

        // Nothing is playing, so the only thing this proves is that applying
        // a setting neither panics nor leaves the engine producing rubbish.
        let output = render(&mut h.engine, 4_800);

        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    /// Loudest the engine gets with one note fed into the reverb at `level`.
    fn reverb_peak(level: f32) -> f32 {
        use saempler_model::{CellEffects, SendEffects, SendRack};

        let mut h = harness();
        load(&mut h, 48_000);
        h.commands
            .push(EngineCommand::SetSends(SendRack {
                normal: SendEffects {
                    reverb_level: level,
                    ..SendEffects::default()
                },
                ..SendRack::default()
            }))
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(CellSpec {
                    effects: CellEffects {
                        reverb_send: 1.0,
                        ..CellEffects::default()
                    },
                    ..spec(0, 48_000)
                }),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 480);
        h.engine.note_off(60);
        tail(&mut h.engine, 24_000)
    }

    #[test]
    fn a_send_turned_down_is_not_heard() {
        // The complaint the return level exists for: before it, the only way
        // to make an effect quieter was to feed it less, which changes how it
        // sounds as well as how loud it is.
        let silent = reverb_peak(0.0);

        assert!(silent < 1e-6, "the send came back anyway: {silent}");
    }

    #[test]
    fn a_send_comes_back_louder_the_further_it_is_turned_up() {
        let quiet = reverb_peak(0.2);
        let loud = reverb_peak(0.8);

        assert!(quiet > 0.0, "nothing came back at all");
        assert!(
            loud > quiet * 2.0,
            "the level barely did anything: {quiet} -> {loud}"
        );
    }

    #[test]
    fn the_four_sends_wide_open_do_not_overwhelm_the_dry_signal() {
        use saempler_model::CellEffects;

        let mut dry = harness();
        load(&mut dry, 48_000);
        dry.engine.note_on(60, 1.0);
        let plain = peak(&mut dry.engine, 4_800);

        let mut wet = harness();
        load(&mut wet, 48_000);
        wet.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(CellSpec {
                    effects: CellEffects {
                        delay_send: 1.0,
                        reverb_send: 1.0,
                        phaser_send: 1.0,
                        flanger_send: 1.0,
                        ..CellEffects::default()
                    },
                    ..spec(0, 48_000)
                }),
            })
            .expect("the queue has capacity");
        wet.engine.apply_commands();
        wet.engine.note_on(60, 1.0);
        let soaked = peak(&mut wet.engine, 4_800);

        // The extreme: every send fed at full from one cell, measured on a
        // constant, which is the worst case for four wet paths summing. What
        // this guards against is an effect that is internally many times
        // louder than its level says, not the user asking for a lot of effect.
        assert!(
            soaked < plain * 4.0,
            "the sends drowned the dry signal: {plain} -> {soaked}"
        );
        assert!(
            soaked > plain * 1.5,
            "the sends were not heard at all: {plain} -> {soaked}"
        );
    }

    #[test]
    fn an_effect_modifier_uses_the_driven_settings_and_gives_them_back() {
        use saempler_model::{Modifier, ModifierMode, SendEffects, SendRack};

        let mut h = harness();
        load(&mut h, 48_000);
        h.commands
            .push(EngineCommand::SetSends(SendRack {
                // Nothing comes back normally, everything comes back driven,
                // which makes the switch audible rather than merely plausible.
                normal: SendEffects {
                    reverb_level: 0.0,
                    ..SendEffects::default()
                },
                driven: SendEffects {
                    reverb_level: 1.0,
                    ..SendEffects::default()
                },
                ..SendRack::default()
            }))
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetModifier {
                note: 48,
                assignment: Some((Modifier::Reverb, ModifierMode::Hold)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        // Nothing comes back normally, so the tail is whatever the driven
        // setting put there and nothing else.
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 480);
        h.engine.note_off(60);
        let before = tail(&mut h.engine, 24_000);

        h.engine.note_on(48, 1.0);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 480);
        h.engine.note_off(60);
        let held = tail(&mut h.engine, 24_000);

        h.engine.note_off(48);
        // Long enough for the tail the driven setting left to die away.
        render(&mut h.engine, 48_000 * 4);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 480);
        h.engine.note_off(60);
        let after = tail(&mut h.engine, 24_000);

        assert!(before < 1e-6, "the send came back at level zero: {before}");
        assert!(held > 0.01, "holding the key did not open the send: {held}");
        assert!(
            after < held * 0.5,
            "letting go did not put the send back: {held} -> {after}"
        );
    }

    /// A harness with a cell on note 60 following automation slot `slot`.
    fn slotted(slot: u8) -> Harness {
        let mut h = harness();
        load(&mut h, 48_000);
        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(CellSpec {
                    slot,
                    ..spec(0, 48_000)
                }),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();
        h
    }

    /// Set one slot and leave the rest of the bank alone.
    fn automate(engine: &mut Engine, slot: usize, values: SliceAutomation) {
        let mut bank = [SliceAutomation::default(); AUTOMATION_SLOTS];
        bank[slot] = values;
        engine.set_automation(&bank);
    }

    #[test]
    fn an_untouched_bank_leaves_the_sound_exactly_as_it_was() {
        // The promise the whole design rests on: a project nobody automated
        // has to sound the way it was built.
        let mut plain = slotted(NO_SLOT);
        plain.engine.note_on(60, 1.0);
        let before = render(&mut plain.engine, 2_400);

        let mut banked = slotted(0);
        banked
            .engine
            .set_automation(&[SliceAutomation::default(); AUTOMATION_SLOTS]);
        banked.engine.note_on(60, 1.0);
        let after = render(&mut banked.engine, 2_400);

        assert_eq!(before, after);
    }

    #[test]
    fn the_gain_of_a_slot_reaches_the_voice_that_follows_it() {
        let mut h = slotted(0);
        h.engine.note_on(60, 1.0);
        let full = peak(&mut h.engine, 2_400);

        let mut quiet = slotted(0);
        automate(
            &mut quiet.engine,
            0,
            SliceAutomation {
                gain: 0.25,
                ..Default::default()
            },
        );
        quiet.engine.note_on(60, 1.0);
        let turned_down = peak(&mut quiet.engine, 2_400);

        assert!(
            (turned_down - full * 0.25).abs() < 0.05,
            "the slot did not reach the voice: {full} -> {turned_down}"
        );
    }

    #[test]
    fn a_voice_only_follows_its_own_slot() {
        // Two chops side by side must not move together because the host
        // automated one of them.
        let mut h = slotted(0);
        h.engine.note_on(60, 1.0);
        let untouched = peak(&mut h.engine, 2_400);

        let mut other = slotted(0);
        automate(
            &mut other.engine,
            1,
            SliceAutomation {
                gain: 0.0,
                ..Default::default()
            },
        );
        other.engine.note_on(60, 1.0);
        let still_playing = peak(&mut other.engine, 2_400);

        assert!((untouched - still_playing).abs() < 1e-6);
    }

    #[test]
    fn a_cell_without_a_slot_ignores_the_whole_bank() {
        let mut h = slotted(NO_SLOT);
        h.engine.note_on(60, 1.0);
        let untouched = peak(&mut h.engine, 2_400);

        let mut silenced = slotted(NO_SLOT);
        silenced.engine.set_automation(
            &[SliceAutomation {
                gain: 0.0,
                ..Default::default()
            }; AUTOMATION_SLOTS],
        );
        silenced.engine.note_on(60, 1.0);
        let still_playing = peak(&mut silenced.engine, 2_400);

        assert!((untouched - still_playing).abs() < 1e-6);
    }

    #[test]
    fn the_speed_of_a_slot_moves_the_playhead() {
        let mut h = slotted(0);
        automate(
            &mut h.engine,
            0,
            SliceAutomation {
                speed: 2.0,
                ..Default::default()
            },
        );
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 12_000);

        // At twice the speed the slice is read out in half the frames, so a
        // cell covering the whole sample is finished long before it would be.
        render(&mut h.engine, 12_500);
        assert_eq!(
            h.engine.active_voices(),
            0,
            "the voice should have run out of slice"
        );
    }

    #[test]
    fn the_reverb_of_a_slot_adds_to_what_the_cell_sends() {
        let mut dry = slotted(0);
        dry.engine.note_on(60, 1.0);
        render(&mut dry.engine, 480);
        dry.engine.note_off(60);
        let without = tail(&mut dry.engine, 24_000);

        let mut wet = slotted(0);
        automate(
            &mut wet.engine,
            0,
            SliceAutomation {
                reverb_send: 1.0,
                ..Default::default()
            },
        );
        wet.engine.note_on(60, 1.0);
        render(&mut wet.engine, 480);
        wet.engine.note_off(60);
        let with = tail(&mut wet.engine, 24_000);

        assert!(
            with > without + 0.001,
            "the slot did not open the send: {without} -> {with}"
        );
    }

    #[test]
    fn the_cutoff_of_a_slot_moves_the_filter() {
        use saempler_model::CellEffects;

        let shut = CellEffects {
            filter_on: true,
            cutoff_hz: 200.0,
            ..CellEffects::default()
        };
        let cell = |engine: &mut Engine, commands: &mut CommandProducer| {
            commands
                .push(EngineCommand::SetSample(tone_sample(48_000)))
                .expect("the queue has capacity");
            commands
                .push(EngineCommand::SetCell {
                    note: 60,
                    spec: Some(CellSpec {
                        slot: 0,
                        effects: shut,
                        ..spec(0, 48_000)
                    }),
                })
                .expect("the queue has capacity");
            engine.apply_commands();
        };

        let mut closed = harness();
        cell(&mut closed.engine, &mut closed.commands);
        closed.engine.note_on(60, 1.0);
        let dark = peak(&mut closed.engine, 2_400);

        let mut opened = harness();
        cell(&mut opened.engine, &mut opened.commands);
        automate(
            &mut opened.engine,
            0,
            SliceAutomation {
                cutoff_octaves: 4.0,
                ..Default::default()
            },
        );
        opened.engine.note_on(60, 1.0);
        let bright = peak(&mut opened.engine, 2_400);

        assert!(
            bright > dark * 1.5,
            "the slot did not open the filter: {dark} -> {bright}"
        );
    }

    #[test]
    fn the_host_can_turn_a_send_down_without_touching_its_settings() {
        use saempler_model::CellEffects;

        let wet_cell = CellSpec {
            effects: CellEffects {
                reverb_send: 1.0,
                ..CellEffects::default()
            },
            ..spec(0, 48_000)
        };

        let mut open = harness();
        load(&mut open, 48_000);
        open.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(wet_cell),
            })
            .expect("the queue has capacity");
        open.engine.apply_commands();
        open.engine.note_on(60, 1.0);
        render(&mut open.engine, 480);
        open.engine.note_off(60);
        let heard = tail(&mut open.engine, 24_000);

        let mut shut = harness();
        load(&mut shut, 48_000);
        shut.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(wet_cell),
            })
            .expect("the queue has capacity");
        shut.engine.apply_commands();
        shut.engine.set_send_scale([1.0, 0.0, 1.0, 1.0]);
        shut.engine.note_on(60, 1.0);
        render(&mut shut.engine, 480);
        shut.engine.note_off(60);
        let silenced = tail(&mut shut.engine, 24_000);

        assert!(heard > 0.0, "nothing came back to turn down");
        assert!(silenced < 1e-6, "the send was still heard: {silenced}");
        assert_eq!(
            shut.engine.sends(),
            open.engine.sends(),
            "the settings themselves must be untouched"
        );
    }

    #[test]
    fn a_broken_value_from_the_host_does_not_reach_the_voice() {
        let mut h = slotted(0);
        automate(
            &mut h.engine,
            0,
            SliceAutomation {
                gain: f32::NAN,
                speed: 0.0,
                cutoff_octaves: f32::INFINITY,
                ..Default::default()
            },
        );
        h.engine.note_on(60, 1.0);

        let output = render(&mut h.engine, 4_800);

        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(h.engine.automation(0).gain.is_finite());
    }

    #[test]
    fn previewing_a_looping_cell_still_ends() {
        use saempler_model::PlaybackMode;

        let mut h = harness();
        load(&mut h, 48_000);

        h.commands
            .push(EngineCommand::Preview(CellSpec {
                mode: PlaybackMode::Loop,
                ..spec(0, 4_800)
            }))
            .expect("the queue has capacity");
        h.engine.apply_commands();
        assert_eq!(h.engine.active_voices(), 1);

        // Long enough to outlast the hold plus the release that follows it.
        render(&mut h.engine, 48_000 * 4);

        assert_eq!(
            h.engine.active_voices(),
            0,
            "an audition has no key to let go of, so it has to stop by itself"
        );
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

        let playhead = h.meters.playheads().next().expect("a voice is sounding");
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
        let playhead = h
            .meters
            .playheads()
            .next()
            .expect("the preview is sounding");
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
    fn a_sounding_voice_publishes_its_position() {
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
        assert!(!h.meters.any_playhead());

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 512);

        let first = h
            .meters
            .playheads()
            .next()
            .expect("a sounding voice has a position");
        assert!(
            (10_000..=10_600).contains(&first),
            "the playhead should start inside the cell, got {first}"
        );

        render(&mut h.engine, 512);
        let later = h.meters.playheads().next().expect("still sounding");
        assert!(later > first, "the playhead must advance");
    }

    #[test]
    fn notes_played_together_each_publish_their_own_position() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(spec(0, 20_000)),
            })
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 64,
                spec: Some(spec(30_000, 48_000)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        h.engine.note_on(60, 1.0);
        h.engine.note_on(64, 1.0);
        render(&mut h.engine, 512);

        let mut positions: Vec<u64> = h.meters.playheads().collect();
        positions.sort_unstable();

        assert_eq!(positions.len(), 2, "both voices must be visible");
        assert!((0..20_000).contains(&positions[0]), "{positions:?}");
        assert!((30_000..48_000).contains(&positions[1]), "{positions:?}");
    }

    #[test]
    fn a_voice_that_ends_takes_its_position_with_it() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 60,
                spec: Some(spec(0, 48_000)),
            })
            .expect("the queue has capacity");
        h.commands
            .push(EngineCommand::SetCell {
                note: 64,
                spec: Some(spec(0, 2_000)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();

        h.engine.note_on(60, 1.0);
        h.engine.note_on(64, 1.0);
        render(&mut h.engine, 512);
        assert_eq!(h.meters.playheads().count(), 2);

        // The short cell runs out while the long one keeps going.
        render(&mut h.engine, 4_800);

        assert_eq!(h.meters.playheads().count(), 1);
    }

    #[test]
    fn the_playhead_clears_when_nothing_sounds() {
        let mut h = harness();
        load(&mut h, 48_000);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 512);
        assert!(h.meters.any_playhead());

        h.engine.note_off(60);
        render(&mut h.engine, 4_800);

        assert!(!h.meters.any_playhead());
    }

    /// Put the default modifier layout on the keyboard.
    fn map_modifiers(harness: &mut Harness, mode: ModifierMode) {
        for assignment in saempler_model::default_layout() {
            harness
                .commands
                .push(EngineCommand::SetModifier {
                    note: assignment.note,
                    assignment: Some((assignment.modifier, mode)),
                })
                .expect("the queue has capacity");
        }
        harness.engine.apply_commands();
    }

    /// Note the default layout puts a modifier on.
    fn modifier_note(modifier: Modifier) -> u8 {
        saempler_model::default_layout()
            .into_iter()
            .find(|entry| entry.modifier == modifier)
            .expect("every modifier is in the layout")
            .note
    }

    #[test]
    fn a_modifier_note_makes_no_sound() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);

        h.engine.note_on(modifier_note(Modifier::Reverse), 1.0);

        assert_eq!(
            h.engine.active_voices(),
            0,
            "a modifier is not an instrument"
        );
        assert_ne!(h.engine.engaged_modifiers(), 0);
    }

    #[test]
    fn a_held_modifier_changes_the_notes_played_under_it() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);

        h.engine.note_on(modifier_note(Modifier::Reverse), 1.0);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 512);

        let playhead = h.meters.playheads().next().expect("a voice is sounding");
        assert!(
            playhead > 40_000,
            "a reversed voice starts at the far end, got {playhead}"
        );
    }

    #[test]
    fn releasing_a_held_modifier_restores_plain_playback() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);
        let note = modifier_note(Modifier::Reverse);

        h.engine.note_on(note, 1.0);
        h.engine.note_off(note);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 512);

        let playhead = h.meters.playheads().next().expect("a voice is sounding");
        assert!(
            playhead < 10_000,
            "expected forward playback, got {playhead}"
        );
    }

    #[test]
    fn a_one_shot_applies_to_the_next_note_only() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::OneShot);
        let note = modifier_note(Modifier::Reverse);

        h.engine.note_on(note, 1.0);
        h.engine.note_off(note);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 512);
        let first = h.meters.playheads().next().expect("a voice is sounding");
        assert!(first > 40_000, "the armed one shot was not used: {first}");

        h.engine.note_off(60);
        render(&mut h.engine, 9_600);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 512);
        let second = h.meters.playheads().next().expect("a voice is sounding");
        assert!(second < 10_000, "the one shot outlived its note: {second}");
    }

    #[test]
    fn a_toggle_survives_the_key_coming_up() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Toggle);
        let note = modifier_note(Modifier::Reverse);

        h.engine.note_on(note, 1.0);
        h.engine.note_off(note);
        assert_ne!(h.engine.engaged_modifiers(), 0);

        h.engine.note_on(note, 1.0);
        assert_eq!(h.engine.engaged_modifiers(), 0);
    }

    #[test]
    fn half_time_makes_a_cell_last_longer() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 4_800);
        let plain = h.meters.playheads().next().expect("sounding");
        h.engine.note_off(60);
        render(&mut h.engine, 9_600);

        h.engine.note_on(modifier_note(Modifier::HalfTime), 1.0);
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 4_800);
        let halved = h.meters.playheads().next().expect("sounding");

        assert!(
            halved < plain,
            "half time should read slower: {halved} vs {plain}"
        );
    }

    #[test]
    fn stutter_holds_a_voice_near_its_trigger_point() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);
        h.engine.set_tempo(120.0);

        h.engine.note_on(modifier_note(Modifier::Stutter), 1.0);
        h.engine.note_on(60, 1.0);

        // Far longer than a sixteenth at 120 bpm, which is 6000 frames.
        for _ in 0..40 {
            render(&mut h.engine, 1_024);
            let playhead = h.meters.playheads().next().expect("sounding");
            assert!(
                playhead <= 6_100,
                "the stutter escaped its loop: {playhead}"
            );
        }
    }

    #[test]
    fn a_modifier_note_is_not_also_a_performance_note() {
        let mut h = harness();
        h.commands
            .push(EngineCommand::SetSample(dc_sample(48_000)))
            .expect("the queue has capacity");
        let note = modifier_note(Modifier::Reverse);
        // Both a cell and a modifier on the same key: the modifier wins, so a
        // mistaken mapping cannot make a modifier audible.
        h.commands
            .push(EngineCommand::SetCell {
                note,
                spec: Some(spec(0, 48_000)),
            })
            .expect("the queue has capacity");
        h.engine.apply_commands();
        map_modifiers(&mut h, ModifierMode::Hold);

        h.engine.note_on(note, 1.0);

        assert_eq!(h.engine.active_voices(), 0);
    }

    #[test]
    fn clearing_the_modifiers_releases_what_was_engaged() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Toggle);
        h.engine.note_on(modifier_note(Modifier::Reverse), 1.0);
        assert_ne!(h.engine.engaged_modifiers(), 0);

        h.commands
            .push(EngineCommand::ClearModifiers)
            .expect("the queue has capacity");
        h.engine.apply_commands();

        assert_eq!(h.engine.engaged_modifiers(), 0);
        assert!(h
            .engine
            .modifier_note(modifier_note(Modifier::Reverse))
            .is_none());
    }

    #[test]
    fn brake_brings_a_voice_to_a_stop() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);
        h.engine.set_tempo(120.0);

        h.engine.note_on(modifier_note(Modifier::Brake), 1.0);
        h.engine.note_on(60, 1.0);
        assert_eq!(h.engine.active_voices(), 1);

        // One whole note at 120 bpm is two seconds.
        render(&mut h.engine, 96_000 + 4_800);

        assert_eq!(h.engine.active_voices(), 0, "the brake must come to rest");
    }

    #[test]
    fn a_modifier_pressed_mid_note_turns_that_note_round() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 9_600);
        let before = h.meters.playheads().next().expect("sounding");

        h.engine.note_on(modifier_note(Modifier::Reverse), 1.0);
        render(&mut h.engine, 4_800);
        let after = h.meters.playheads().next().expect("still sounding");

        assert!(
            after < before,
            "reverse engaged mid-note must play backwards from where it was: \
             {before} -> {after}"
        );
    }

    #[test]
    fn a_stutter_pressed_mid_note_loops_under_the_playhead() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);
        h.engine.set_tempo(120.0);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 12_000);
        let caught = h.meters.playheads().next().expect("sounding");

        h.engine.note_on(modifier_note(Modifier::Stutter), 1.0);
        for _ in 0..30 {
            render(&mut h.engine, 1_024);
            let playhead = h.meters.playheads().next().expect("still sounding");
            assert!(
                playhead >= caught && playhead <= caught + 6_100,
                "the loop must stay where it was engaged: {caught} vs {playhead}"
            );
        }
    }

    #[test]
    fn releasing_a_stutter_lets_the_note_carry_on() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);
        h.engine.set_tempo(120.0);
        let note = modifier_note(Modifier::Stutter);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 2_000);
        h.engine.note_on(note, 1.0);
        render(&mut h.engine, 12_000);
        let looped = h.meters.playheads().next().expect("sounding");

        h.engine.note_off(note);
        render(&mut h.engine, 6_000);
        let freed = h.meters.playheads().next().expect("still sounding");

        assert!(
            freed > looped,
            "the voice must move on again: {looped} -> {freed}"
        );
    }

    #[test]
    fn a_brake_pressed_mid_note_stops_that_note() {
        let mut h = harness();
        load(&mut h, 480_000);
        map_modifiers(&mut h, ModifierMode::Hold);
        h.engine.set_tempo(120.0);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 4_800);
        assert_eq!(h.engine.active_voices(), 1);

        h.engine.note_on(modifier_note(Modifier::Brake), 1.0);
        render(&mut h.engine, 96_000 + 4_800);

        assert_eq!(h.engine.active_voices(), 0);
    }

    #[test]
    fn releasing_a_brake_winds_the_note_back_up() {
        let mut h = harness();
        load(&mut h, 480_000);
        map_modifiers(&mut h, ModifierMode::Hold);
        h.engine.set_tempo(120.0);
        let note = modifier_note(Modifier::Brake);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 2_400);
        h.engine.note_on(note, 1.0);
        // Partway into the stop, which is one beat by default.
        render(&mut h.engine, 6_000);
        let braked_start = h.meters.playheads().next().expect("sounding");
        render(&mut h.engine, 4_800);
        let braked_end = h.meters.playheads().next().expect("sounding");
        let braked_step = braked_end - braked_start;

        h.engine.note_off(note);
        render(&mut h.engine, 6_000);
        let freed_start = h.meters.playheads().next().expect("sounding");
        render(&mut h.engine, 4_800);
        let freed_step = h.meters.playheads().next().expect("sounding") - freed_start;

        assert!(
            freed_step > braked_step,
            "the tape must come back up to speed: {braked_step} -> {freed_step}"
        );
    }

    #[test]
    fn an_armed_one_shot_leaves_sounding_notes_alone() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::OneShot);

        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 4_800);
        let before = h.meters.playheads().next().expect("sounding");

        h.engine.note_on(modifier_note(Modifier::Reverse), 1.0);
        render(&mut h.engine, 2_400);
        let after = h.meters.playheads().next().expect("still sounding");

        assert!(after > before, "the sounding note must keep going forwards");
    }

    #[test]
    fn a_modifier_change_does_not_disturb_the_cell_it_came_from() {
        let mut h = harness();
        load(&mut h, 48_000);
        map_modifiers(&mut h, ModifierMode::Hold);
        let note = modifier_note(Modifier::Reverse);

        h.engine.note_on(60, 1.0);
        h.engine.note_on(note, 1.0);
        h.engine.note_off(note);
        h.engine.note_off(60);
        render(&mut h.engine, 9_600);

        // Playing it again must behave exactly as the cell says.
        h.engine.note_on(60, 1.0);
        render(&mut h.engine, 1_024);
        let playhead = h.meters.playheads().next().expect("sounding");
        assert!(playhead < 10_000, "the cell was left modified: {playhead}");
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
