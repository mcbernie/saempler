use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use saempler_audio::{
    SliceAutomation, AUTOMATION_CUTOFF_OCTAVES, AUTOMATION_PITCH_SEMITONES, AUTOMATION_SLOTS,
};
use saempler_model::ProjectFile;

/// Initial editor size in logical pixels.
pub const EDITOR_SIZE: (u32, u32) = (1_240, 1_010);

/// The host-automatable controls of one slice.
///
/// A fixed bank exists for every slice a project may hold, whether or not that
/// slice has been made yet. Host parameter identifiers have to be stable and
/// known before the project is: a lane written today must still point at the
/// same control when the session is reopened, so the bank cannot grow or
/// shrink with the number of slices.
///
/// Every value is an offset on top of what the cell is set to, so a lane that
/// was never touched leaves the chop sounding exactly as it was built.
#[derive(Params)]
pub struct SliceParams {
    #[id = "gain"]
    pub gain: FloatParam,
    #[id = "pitch"]
    pub pitch: FloatParam,
    #[id = "speed"]
    pub speed: FloatParam,
    #[id = "cutoff"]
    pub cutoff: FloatParam,
    #[id = "reverb"]
    pub reverb: FloatParam,
}

impl SliceParams {
    /// The bank for slice `number`, counting from one as the markers do.
    fn new(number: usize) -> Self {
        Self {
            gain: FloatParam::new(
                format!("S{number} Gain"),
                1.0,
                FloatRange::Linear { min: 0.0, max: 2.0 },
            )
            .with_smoother(SmoothingStyle::Linear(10.0))
            .with_unit("×"),

            pitch: FloatParam::new(
                format!("S{number} Pitch"),
                0.0,
                FloatRange::Linear {
                    min: -AUTOMATION_PITCH_SEMITONES,
                    max: AUTOMATION_PITCH_SEMITONES,
                },
            )
            // Not smoothed: a pitch lane is normally stepped from one value to
            // another, and sliding between the steps is not what it means.
            .with_unit(" st"),

            speed: FloatParam::new(
                format!("S{number} Speed"),
                1.0,
                FloatRange::Skewed {
                    min: 0.25,
                    max: 4.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit("×"),

            cutoff: FloatParam::new(
                format!("S{number} Cutoff"),
                0.0,
                FloatRange::Linear {
                    min: -AUTOMATION_CUTOFF_OCTAVES,
                    max: AUTOMATION_CUTOFF_OCTAVES,
                },
            )
            // Smoothed: this one is swept, and a filter moved in steps clicks.
            .with_smoother(SmoothingStyle::Linear(15.0))
            .with_unit(" oct"),

            reverb: FloatParam::new(
                format!("S{number} Reverb"),
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0)),
        }
    }

    /// Where this slice's controls stand right now.
    ///
    /// Called once per sub-block from the audio thread. Reading a smoothed
    /// parameter advances it, so this must be called exactly once per block
    /// per slice, which is why it lives here rather than at each use.
    fn take(&self, frames: u32) -> SliceAutomation {
        SliceAutomation {
            gain: self.gain.smoothed.next_step(frames),
            pitch_semitones: self.pitch.value(),
            speed: self.speed.value(),
            cutoff_octaves: self.cutoff.smoothed.next_step(frames),
            reverb_send: self.reverb.smoothed.next_step(frames),
        }
    }
}

/// Host-visible parameters plus the persisted project state.
///
/// The project itself is not automatable: which slices exist, what they point
/// at and how they are mapped changes with the project and belongs in plugin
/// state. What the host can reach is the fixed bank above plus the global
/// controls here.
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

    /// Rides the return level each send is set to, rather than replacing it.
    #[id = "send_delay"]
    pub send_delay: FloatParam,
    #[id = "send_reverb"]
    pub send_reverb: FloatParam,
    #[id = "send_phaser"]
    pub send_phaser: FloatParam,
    #[id = "send_flanger"]
    pub send_flanger: FloatParam,

    /// One bank per slice, in the order the markers are numbered.
    #[nested(array, group = "Slices")]
    pub slices: [SliceParams; AUTOMATION_SLOTS],
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

            send_delay: send_param("Delay Return"),
            send_reverb: send_param("Reverb Return"),
            send_phaser: send_param("Phaser Return"),
            send_flanger: send_param("Flanger Return"),

            slices: std::array::from_fn(|index| SliceParams::new(index + 1)),
        }
    }
}

impl SaemplerParams {
    /// Where every slice's controls stand right now.
    ///
    /// One call per sub-block, because reading a smoothed parameter advances
    /// its ramp.
    pub fn take_automation(&self, frames: u32) -> [SliceAutomation; AUTOMATION_SLOTS] {
        std::array::from_fn(|index| self.slices[index].take(frames))
    }

    /// Where the four send returns stand right now, in the engine's order.
    pub fn take_send_scale(&self, frames: u32) -> [f32; 4] {
        [
            self.send_delay.smoothed.next_step(frames),
            self.send_reverb.smoothed.next_step(frames),
            self.send_phaser.smoothed.next_step(frames),
            self.send_flanger.smoothed.next_step(frames),
        ]
    }
}

/// A send return control, which starts out of the way at full.
fn send_param(name: &str) -> FloatParam {
    FloatParam::new(name, 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_smoother(SmoothingStyle::Linear(20.0))
        .with_unit("×")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What every host wrapper does once the sample rate is known.
    ///
    /// A smoother that has never been reset reads as zero, so without this a
    /// test would be measuring an uninitialised ramp rather than a default.
    fn as_the_host_leaves_it(params: &SaemplerParams) {
        for slice in &params.slices {
            slice.gain.smoothed.reset(slice.gain.value());
            slice.cutoff.smoothed.reset(slice.cutoff.value());
            slice.reverb.smoothed.reset(slice.reverb.value());
        }
        for send in [
            &params.send_delay,
            &params.send_reverb,
            &params.send_phaser,
            &params.send_flanger,
        ] {
            send.smoothed.reset(send.value());
        }
    }

    #[test]
    fn the_bank_starts_out_of_the_way() {
        // A project that is never automated has to sound exactly as it was
        // built, which is only true while every default is neutral.
        let params = SaemplerParams::default();
        as_the_host_leaves_it(&params);

        for slice in &params.slices {
            assert!(
                slice.take(0).is_idle(),
                "a fresh slice bank already changes the sound"
            );
        }
        for scale in params.take_send_scale(0) {
            assert_eq!(scale, 1.0);
        }
    }

    #[test]
    fn there_is_one_bank_for_every_slice_a_project_may_hold() {
        let params = SaemplerParams::default();

        assert_eq!(params.slices.len(), AUTOMATION_SLOTS);
    }

    #[test]
    fn every_parameter_has_its_own_identifier() {
        use std::collections::HashSet;

        // A repeated id would make the host write one lane into two controls,
        // and the clash is invisible until a session is reopened.
        let params = SaemplerParams::default();
        let ids: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        let unique: HashSet<&String> = ids.iter().collect();

        assert_eq!(unique.len(), ids.len(), "two parameters share an id");
        assert_eq!(
            ids.len(),
            5 + AUTOMATION_SLOTS * 5,
            "the parameter list changed size, which breaks saved automation"
        );
    }

    #[test]
    fn the_slice_identifiers_are_numbered_from_one() {
        // The numbers are what a saved automation lane points at, so they are
        // part of the file format rather than a detail of this type.
        let params = SaemplerParams::default();
        let ids: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();

        assert!(ids.contains(&"cutoff_1".to_string()));
        assert!(ids.contains(&format!("cutoff_{AUTOMATION_SLOTS}")));
        assert!(
            !ids.contains(&"cutoff_0".to_string()),
            "the markers are numbered from one"
        );
        assert_eq!(
            ids.iter().filter(|id| id.starts_with("cutoff_")).count(),
            AUTOMATION_SLOTS
        );
    }
}
