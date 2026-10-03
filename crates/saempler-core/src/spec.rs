use saempler_audio::{CellSpec, SliceBounds};
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
        attack_ms: playback.attack_ms,
        release_ms: playback.release_ms,
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
        let cell = *project.cell(id).expect("cell exists");

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
        let cell = *project.cell(id).expect("cell exists");

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
                attack_ms: 20.0,
                release_ms: 150.0,
                ..Default::default()
            },
        );
        let cell = *project.cell(id).expect("cell exists");

        let spec = cell_spec(&project, &cell).expect("the slice is still there");

        assert!(spec.reverse);
        assert_eq!(spec.gain, 0.5);
        assert_eq!(spec.attack_ms, 20.0);
        assert_eq!(spec.release_ms, 150.0);
    }

    #[test]
    fn a_cell_without_its_slice_produces_nothing() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        let cell = *project.cell(id).expect("cell exists");
        project.remove_slice(slice);

        assert!(cell_spec(&project, &cell).is_none());
    }

    #[test]
    fn broken_settings_never_reach_the_engine() {
        let (mut project, slice) = project_with_slice();
        let id = project.assign(60, slice).expect("the slice exists");
        // `set_playback` sanitizes, so the only way in is a hand-built cell.
        let mut cell = *project.cell(id).expect("cell exists");
        cell.playback.speed = 0.0;
        cell.playback.gain = f32::NAN;

        let spec = cell_spec(&project, &cell).expect("the slice is still there");

        assert!(spec.rate > 0.0);
        assert!(spec.gain.is_finite());
    }
}
