//! Domain model for Sämpler.
//!
//! This crate holds the serializable project state that is shared between the
//! user interface, the realtime engine and the plugin wrapper. It deliberately
//! has no knowledge of audio processing, egui or any plugin format.

mod cell;
mod project;
mod slice;

pub use cell::{
    note_name, CellId, PerformanceCell, PlaybackSettings, MAX_PITCH_SEMITONES, MAX_SPEED, MIN_SPEED,
};
pub use project::{Project, ProjectError, ProjectFile, SampleRef, PROJECT_VERSION};
pub use slice::{Slice, SliceId};
