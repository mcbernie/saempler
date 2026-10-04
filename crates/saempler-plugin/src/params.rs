use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use saempler_model::ProjectFile;

/// Initial editor size in logical pixels.
pub const EDITOR_SIZE: (u32, u32) = (1_240, 1_010);

/// Host-visible parameters plus the persisted project state.
///
/// Only stable global controls are exposed as host parameters. Everything that
/// changes with the project structure lives in [`ProjectFile`] and is stored
/// as plugin state instead, so that automation lanes never point at a slice or
/// cell that no longer exists.
#[derive(Params)]
pub struct SaemplerParams {
    /// Editor size, persisted so a resized window reopens the same way.
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// Project state that is not automatable.
    ///
    /// The mutex is only ever locked from the UI/main thread. The audio thread
    /// reads the derived values through the command queue instead.
    #[persist = "project"]
    pub project: Arc<Mutex<ProjectFile>>,

    #[id = "master_gain"]
    pub gain: FloatParam,
}

impl Default for SaemplerParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(EDITOR_SIZE.0, EDITOR_SIZE.1),
            project: Arc::new(Mutex::new(ProjectFile::default())),

            gain: FloatParam::new(
                "Master Gain",
                util::db_to_gain(-6.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(6.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 6.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}
