//! Domain model for Sämpler.
//!
//! This crate holds the serializable project state that is shared between the
//! user interface, the realtime engine and the plugin wrapper. It deliberately
//! has no knowledge of audio processing, egui or any plugin format.

mod project;

pub use project::{Project, ProjectError, ProjectFile, Waveform, PROJECT_VERSION};
