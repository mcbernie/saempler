use std::sync::Arc;

use crate::sample::SampleBuffer;

/// Number of commands the queue can hold between two audio callbacks.
///
/// The queue is drained once per processing block. The capacity only needs to
/// absorb a burst of UI edits; pushes beyond it are dropped rather than
/// blocking the UI thread.
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

/// The region of the sample that triggered voices play.
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
    /// Set the region that newly triggered voices play.
    SetSlice(SliceBounds),
    /// Play this region once, without a note.
    ///
    /// Used by the interface to audition a slice on click. The voice ends by
    /// itself at the end of the region, so no release command follows.
    Preview(SliceBounds),
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
}
