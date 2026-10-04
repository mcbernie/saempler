//! Domain model for Sämpler.
//!
//! This crate holds the serializable project state that is shared between the
//! user interface, the realtime engine and the plugin wrapper. It deliberately
//! has no knowledge of audio processing, egui or any plugin format.

mod cell;
mod effect;
mod modifier;
mod modulation;
mod project;
mod slice;

pub use cell::{
    is_black_key, note_name, CellId, PerformanceCell, PlaybackMode, PlaybackSettings, MAX_COLLAPSE,
    MAX_PITCH_SEMITONES, MAX_SPEED, MIN_COLLAPSE, MIN_SPEED,
};
pub use effect::{
    CellEffects, DriveShape, FilterShape, SendEffects, SendRack, MAX_CUTOFF_HZ, MAX_DRIVE,
    MAX_RESONANCE, MAX_SEND_DRIVE, MAX_SEND_LEVEL, MIN_CUTOFF_HZ, MIN_RESONANCE,
};
pub use modifier::{
    default_layout, Modifier, ModifierAssignment, ModifierMode, MODIFIER_BASE_NOTE, MODIFIER_COUNT,
};
pub use modulation::{
    default_routes, Division, EnvelopeDefinition, LfoDefinition, LfoShape, ModDestination,
    ModSource, ModulationRoute, DESTINATION_COUNT, ENVELOPE_COUNT, LFO_COUNT, MAX_ROUTES,
};
pub use project::{Project, ProjectError, ProjectFile, SampleRef, PROJECT_VERSION};
pub use slice::{Slice, SliceId};
