use saempler_audio::{CellSpec, ModulationSpec, SliceBounds, NO_SLOT};
use saempler_model::{PerformanceCell, Project};

/// Flatten a cell into the form the engine plays.
///
/// This is where the domain stops and the realtime side begins: the slice is
/// resolved to frame bounds and speed and pitch collapse into one read rate,
/// so the audio thread never has to look anything up or do any arithmetic
/// that could have been done ahead of time.
///
/// Returns `None` when the cell points at a slice that no longer exists.
pub fn cell_spec(project: &Project, cell: &PerformanceCell) -> Option<CellSpec> {
    let slice = project.slice(cell.slice)?;
    let playback = cell.playback.sanitized();

    Some(CellSpec {
        bounds: SliceBounds {
            start_frame: slice.start_frame,
            end_frame: slice.end_frame,
        },
        rate: playback.rate(),
        reverse: playback.reverse,
        gain: playback.gain,
        modulation: ModulationSpec::new(
            cell.envelopes.map(|envelope| envelope.sanitized()),
            cell.lfos.map(|lfo| lfo.sanitized()),
            &cell.routes,
        ),
        // Looping and braking come from modifier keys at trigger time, never
        // from the cell itself.
        loop_frames: 0,
        tape_stop_frames: 0,
        mode: playback.mode,
        cycle_whole_notes: if playback.cycle_whole_slice {
            0.0
        } else {
            playback.division.whole_notes()
        },
        collapse: playback.collapse,
        release_trigger: playback.release_trigger,
        effects: cell.effects.sanitized(),
        // The host automates by slice number, which is what the markers and
        // the pads are labelled with. A slice past the bank has no slot, which
        // the model's own limit means cannot happen.
        slot: project.automation_slot(cell.slice).unwrap_or(NO_SLOT),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use saempler_model::{PlaybackSettings, SampleRef};
    use std::path::PathBuf;

    fn project_with_slice() -> (Project, saempler_model::SliceId) {
        let mut project = Project::default();
        project.set_sample(Some(SampleRef {
            path: PathBuf::from("/tmp/x.wav"),
            frames: 10_000,
            sample_rate: 48_000,
            channels: 2,
        }));
        let slice = project.add_slice(1_000, 3_000);
        (project, slice)
    }

    #[test]
    fn the_slice_becomes_frame_bounds() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        let cell = project.cell(id).expect("cell exists").clone();

        let spec = cell_spec(&project, &cell).expect("the slice is still there");

        assert_eq!(spec.bounds.start_frame, 1_000);
        assert_eq!(spec.bounds.end_frame, 3_000);
    }

    #[test]
    fn speed_and_pitch_collapse_into_one_rate() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        project.set_playback(
            id,
            PlaybackSettings {
                speed: 0.5,
                pitch_semitones: 12.0,
                ..Default::default()
            },
        );
        let cell = project.cell(id).expect("cell exists").clone();

        let spec = cell_spec(&project, &cell).expect("the slice is still there");

        assert!((spec.rate - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_remaining_settings_carry_over() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        project.set_playback(
            id,
            PlaybackSettings {
                reverse: true,
                gain: 0.5,
                ..Default::default()
            },
        );
        let cell = project.cell(id).expect("cell exists").clone();

        let spec = cell_spec(&project, &cell).expect("the slice is still there");

        assert!(spec.reverse);
        assert_eq!(spec.gain, 0.5);
    }

    #[test]
    fn the_modulation_travels_with_the_cell() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        project.set_envelope(
            id,
            0,
            saempler_model::EnvelopeDefinition {
                attack_ms: 123.0,
                ..Default::default()
            },
        );
        let cell = project.cell(id).expect("cell exists").clone();

        let spec = cell_spec(&project, &cell).expect("the slice is still there");

        assert_eq!(spec.modulation.envelopes[0].attack_ms, 123.0);
        assert_eq!(
            spec.modulation.routes.iter().flatten().count(),
            1,
            "the amplitude route comes along"
        );
    }

    #[test]
    fn broken_modulation_never_reaches_the_engine() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        project.with_cell_mut(id, |cell| {
            cell.envelopes[0].sustain = f32::NAN;
            cell.lfos[0].rate_hz = -1.0;
        });
        let cell = project.cell(id).expect("cell exists").clone();

        let spec = cell_spec(&project, &cell).expect("the slice is still there");

        assert!(spec.modulation.envelopes[0].sustain.is_finite());
        assert!(spec.modulation.lfos[0].rate_hz > 0.0);
    }

    #[test]
    fn a_cell_without_its_slice_produces_nothing() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        let cell = project.cell(id).expect("cell exists").clone();
        project.remove_slice(slice);

        assert!(cell_spec(&project, &cell).is_none());
    }

    #[test]
    fn broken_settings_never_reach_the_engine() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        // `set_playback` sanitizes, so the only way in is a hand-built cell.
        let mut cell = project.cell(id).expect("cell exists").clone();
        cell.playback.speed = 0.0;
        cell.playback.gain = f32::NAN;

        let spec = cell_spec(&project, &cell).expect("the slice is still there");

        assert!(spec.rate > 0.0);
        assert!(spec.gain.is_finite());
    }
}
