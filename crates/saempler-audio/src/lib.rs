//! Realtime audio engine for Sämpler.
//!
//! Everything in this crate runs on, or is read by, the audio thread. It knows
//! nothing about egui, VST3, CLAP or the plugin wrapper, and it never touches
//! the filesystem.
//!
//! The communication model is deliberately narrow:
//!
//! * UI/main thread -> audio thread: [`EngineCommand`] values pushed through a
//!   wait-free SPSC queue ([`command_queue`]).
//! * audio thread -> UI/main thread: plain atomics in [`Meters`], plus retired
//!   sample buffers handed back through [`disposal_queue`] so that the audio
//!   thread never frees them.

mod command;
mod engine;
mod meters;
mod modifiers;
mod sample;
mod voice;

pub use command::{
    command_queue, disposal_queue, CellSpec, CommandConsumer, CommandProducer, DisposalConsumer,
    DisposalProducer, EngineCommand, SliceBounds, DISPOSAL_CAPACITY, QUEUE_CAPACITY,
};
pub use engine::{Engine, MAX_VOICES, NOTE_COUNT};
pub use meters::{Meters, PLAYHEAD_SLOTS};
pub use modifiers::{division_frames, ModifierState, DEFAULT_TEMPO};
pub use sample::SampleBuffer;
