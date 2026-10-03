//! Non-realtime project work for Sämpler.
//!
//! Sample import and the waveform peak cache live here: everything that reads
//! files or walks whole buffers, and therefore belongs on a background thread
//! rather than in the audio callback or a draw call.
//!
//! This crate draws nothing and knows nothing about plugin formats.

mod loader;
mod peaks;
mod spec;

pub use loader::{load_sample, LoadError, LoadedSample, MAX_FRAMES};
pub use peaks::{Peak, PeakCache, BASE_FRAMES_PER_PEAK};
pub use spec::cell_spec;
