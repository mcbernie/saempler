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
mod sample;
mod voice;

pub use command::{
    command_queue, disposal_queue, CommandConsumer, CommandProducer, DisposalConsumer,
    DisposalProducer, EngineCommand, SliceBounds, DISPOSAL_CAPACITY, QUEUE_CAPACITY,
};
pub use engine::{Engine, MAX_VOICES};
pub use meters::Meters;
pub use sample::SampleBuffer;
