use std::sync::Arc;

use saempler_model::{
    CellEffects, Modifier, ModifierMode, ModifierSettings, PlaybackMode, SendRack,
};

use crate::automation::NO_SLOT;

use crate::modulation::ModulationSpec;
use crate::sample::SampleBuffer;

/// Number of commands the queue can hold between two audio callbacks.
///
/// The queue is drained once per processing block. The capacity has to absorb
/// a full remapping of the keyboard in one go, so it is sized above the 128
/// MIDI notes.
pub const QUEUE_CAPACITY: usize = 256;

/// Number of buffers the disposal queue can hold.
///
/// Small on purpose: the engine holds at most one retired buffer at a time, so
/// a handful of slots is more than enough to cover a UI that is slow to drain.
pub const DISPOSAL_CAPACITY: usize = 8;

/// Producing end of the command queue, owned by the UI/main thread.
pub type CommandProducer = rtrb::Producer<EngineCommand>;
/// Consuming end of the command queue, owned by the audio thread.
pub type CommandConsumer = rtrb::Consumer<EngineCommand>;

/// Producing end of the disposal queue, owned by the audio thread.
pub type DisposalProducer = rtrb::Producer<Arc<SampleBuffer>>;
/// Consuming end of the disposal queue, owned by the UI/main thread.
pub type DisposalConsumer = rtrb::Consumer<Arc<SampleBuffer>>;

/// The region of the sample a voice plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SliceBounds {
    pub start_frame: u64,
    pub end_frame: u64,
}

impl SliceBounds {
    pub fn len_frames(&self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }

    pub fn is_empty(&self) -> bool {
        self.len_frames() == 0
    }
}

/// Everything the engine needs to play one performance cell.
///
/// This is the flattened, realtime-ready form of the cell in the project: the
/// slice has already been resolved to frame bounds, and speed and pitch have
/// been folded into a single read rate. The engine never looks anything up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellSpec {
    pub bounds: SliceBounds,
    /// Frames advanced per output frame.
    pub rate: f32,
    pub reverse: bool,
    pub gain: f32,
    /// Envelopes, LFOs and the matrix that joins them to parameters.
    pub modulation: ModulationSpec,
    /// Length of the loop a stutter imposes, in frames. Zero plays straight
    /// through. Set by modifiers, never by the cell itself.
    pub loop_frames: u64,
    /// Output frames over which playback slows to a stop. Zero plays at a
    /// steady rate. Set by modifiers, never by the cell itself.
    pub tape_stop_frames: u64,
    /// How the cell moves through its slice.
    pub mode: PlaybackMode,
    /// Length of one repeat or collapse pass, in whole notes. Turned into
    /// frames by the voice, which is where the tempo is known.
    ///
    /// Zero means the pass is the whole slice, which needs no tempo.
    pub cycle_whole_notes: f32,
    /// Factor the collapse loop is multiplied by on every pass.
    pub collapse: f32,
    /// Whether the mode's loop starts at the note off rather than the note on.
    pub release_trigger: bool,
    /// Filter, drive and how much of this cell reaches each send.
    pub effects: CellEffects,
    /// Which automation slot this cell follows, or [`NO_SLOT`].
    ///
    /// The slot belongs to the slice rather than to the cell, because the
    /// slice is what the interface numbers and what the host's parameter is
    /// named after.
    pub slot: u8,
}

impl Default for CellSpec {
    fn default() -> Self {
        Self {
            bounds: SliceBounds::default(),
            rate: 1.0,
            reverse: false,
            gain: 1.0,
            modulation: ModulationSpec::default(),
            loop_frames: 0,
            tape_stop_frames: 0,
            mode: PlaybackMode::Gate,
            cycle_whole_notes: 0.0625,
            collapse: 0.75,
            release_trigger: false,
            slot: NO_SLOT,
            effects: CellEffects {
                // An audition and a hand built spec should sound like the
                // cell they stand for, which by default is untouched.
                ..CellEffects::default()
            },
        }
    }
}

/// A state change requested by the UI and applied by the engine.
///
/// Sending an `Arc` through the queue is cheap: the clone happens on the
/// sending thread and the engine only moves the pointer. Releasing the
/// previous buffer is handled separately, see [`DisposalProducer`].
#[derive(Debug)]
pub enum EngineCommand {
    /// Play from this buffer from now on.
    SetSample(Arc<SampleBuffer>),
    /// Forget the current buffer; nothing will sound afterwards.
    ClearSample,
    /// Put a cell on a MIDI note, or take one off with `None`.
    SetCell { note: u8, spec: Option<CellSpec> },
    /// Take every cell off the keyboard.
    ClearCells,
    /// Put a modifier on a MIDI note, or take one off with `None`.
    SetModifier {
        note: u8,
        assignment: Option<(Modifier, ModifierMode)>,
    },
    /// Take every modifier off the keyboard and release the ones in effect.
    ClearModifiers,
    /// Play this region once, without a note.
    ///
    /// Used by the interface to audition a slice on click. The voice ends by
    /// itself at the end of the region, so no release command follows.
    Preview(CellSpec),
    /// Replace the settings of the shared sends.
    SetSends(SendRack),
    /// Replace how hard each playback modifier hits.
    SetModifierSettings(ModifierSettings),
    /// Release every sounding voice immediately.
    AllNotesOff,
}

/// Create the preallocated command queue.
pub fn command_queue() -> (CommandProducer, CommandConsumer) {
    rtrb::RingBuffer::new(QUEUE_CAPACITY)
}

/// Create the preallocated disposal queue.
///
/// Buffers retired by the engine travel back through this queue and are
/// dropped by the receiving thread. Freeing a multi-megabyte buffer on the
/// audio thread would call the allocator in the processing path.
pub fn disposal_queue() -> (DisposalProducer, DisposalConsumer) {
    rtrb::RingBuffer::new(DISPOSAL_CAPACITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bounds_report_no_length() {
        assert!(SliceBounds::default().is_empty());
        assert!(SliceBounds {
            start_frame: 100,
            end_frame: 100,
        }
        .is_empty());
    }

    #[test]
    fn inverted_bounds_do_not_underflow() {
        let bounds = SliceBounds {
            start_frame: 200,
            end_frame: 100,
        };

        assert_eq!(bounds.len_frames(), 0);
        assert!(bounds.is_empty());
    }

    #[test]
    fn the_default_spec_plays_as_recorded() {
        let spec = CellSpec::default();

        assert_eq!(spec.rate, 1.0);
        assert!(!spec.reverse);
        assert_eq!(spec.gain, 1.0);
    }
}
