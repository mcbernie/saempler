use saempler_model::Waveform;

/// Number of commands the queue can hold between two audio callbacks.
///
/// The queue is drained once per processing block. The capacity only needs to
/// absorb a burst of UI edits; pushes beyond it are dropped rather than
/// blocking the UI thread.
pub const QUEUE_CAPACITY: usize = 256;

/// Producing end of the command queue, owned by the UI/main thread.
pub type CommandProducer = rtrb::Producer<EngineCommand>;
/// Consuming end of the command queue, owned by the audio thread.
pub type CommandConsumer = rtrb::Consumer<EngineCommand>;

/// A state change requested by the UI and applied by the engine.
///
/// Variants must stay small and `Copy`-cheap: they are memcpy'd into a
/// preallocated ring buffer, and the audio thread must be able to drop them
/// without deallocating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineCommand {
    /// Switch the waveform used by newly rendered samples.
    SetWaveform(Waveform),
    /// Release every sounding voice immediately.
    AllNotesOff,
}

/// Create a preallocated command queue.
pub fn command_queue() -> (CommandProducer, CommandConsumer) {
    rtrb::RingBuffer::new(QUEUE_CAPACITY)
}
