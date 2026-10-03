use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::slice::{Slice, SliceId};

/// Version of the serialized project layout understood by this build.
///
/// Every stored project carries this number so that future layout changes can
/// be detected instead of silently misinterpreting old data.
pub const PROJECT_VERSION: u32 = 1;

/// Where the source sample came from and what it contains.
///
/// Only the reference is persisted, never the audio itself: a project must not
/// grow by the size of its sample, and the decoded buffer is rebuilt on load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleRef {
    pub path: PathBuf,
    pub frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
}

impl SampleRef {
    /// File name without the directory, for display.
    pub fn display_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }

    /// Duration in seconds. Seconds exist for presentation only; everything
    /// internal counts frames.
    pub fn duration_seconds(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames as f32 / self.sample_rate as f32
    }
}

/// Editable project state that is not exposed as a host parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub sample: Option<SampleRef>,
    slices: Vec<Slice>,
    selection: Option<SliceId>,
    /// Hands out the next slice identity. Kept in the project so identities
    /// stay unique across a session even when slices are deleted.
    next_slice_id: u32,
}

impl Project {
    /// All slices, ordered by start frame.
    pub fn slices(&self) -> &[Slice] {
        &self.slices
    }

    /// Create a slice covering `start_frame..end_frame`.
    ///
    /// The bounds are normalized, so a marker dragged past its partner still
    /// produces a usable slice.
    pub fn add_slice(&mut self, start_frame: u64, end_frame: u64) -> SliceId {
        let (start_frame, end_frame) = if start_frame <= end_frame {
            (start_frame, end_frame)
        } else {
            (end_frame, start_frame)
        };

        let id = SliceId(self.next_slice_id);
        self.next_slice_id += 1;
        self.slices.push(Slice {
            id,
            start_frame,
            end_frame,
        });
        self.sort_slices();
        id
    }

    /// Remove a slice, clearing the selection if it pointed at that slice.
    pub fn remove_slice(&mut self, id: SliceId) -> bool {
        let before = self.slices.len();
        self.slices.retain(|slice| slice.id != id);
        if self.selection == Some(id) {
            self.selection = None;
        }
        self.slices.len() != before
    }

    /// Look up a slice by identity.
    pub fn slice(&self, id: SliceId) -> Option<&Slice> {
        self.slices.iter().find(|slice| slice.id == id)
    }

    /// Move a slice's bounds, normalizing them and restoring the ordering.
    pub fn set_slice_bounds(&mut self, id: SliceId, start_frame: u64, end_frame: u64) -> bool {
        let (start_frame, end_frame) = if start_frame <= end_frame {
            (start_frame, end_frame)
        } else {
            (end_frame, start_frame)
        };

        let Some(slice) = self.slices.iter_mut().find(|slice| slice.id == id) else {
            return false;
        };
        slice.start_frame = start_frame;
        slice.end_frame = end_frame;
        self.sort_slices();
        true
    }

    /// Split a slice in two at `frame`, the manual way to place a marker.
    ///
    /// Returns the two resulting identities, or `None` when `frame` does not
    /// lie strictly inside the slice: a split at either boundary would produce
    /// an empty slice.
    pub fn split_slice(&mut self, id: SliceId, frame: u64) -> Option<(SliceId, SliceId)> {
        let slice = *self.slice(id)?;
        if frame <= slice.start_frame || frame >= slice.end_frame {
            return None;
        }

        let was_selected = self.selection == Some(id);
        self.remove_slice(id);
        let left = self.add_slice(slice.start_frame, frame);
        let right = self.add_slice(frame, slice.end_frame);
        if was_selected {
            self.selection = Some(left);
        }

        Some((left, right))
    }

    /// The slice covering `frame`, if any.
    pub fn slice_at(&self, frame: u64) -> Option<&Slice> {
        self.slices.iter().find(|slice| slice.contains(frame))
    }

    /// The currently selected slice, if it still exists.
    pub fn selected(&self) -> Option<&Slice> {
        self.selection.and_then(|id| self.slice(id))
    }

    /// Identity of the current selection.
    pub fn selection(&self) -> Option<SliceId> {
        self.selection
    }

    /// Select a slice. Selecting an unknown slice clears the selection.
    pub fn select(&mut self, id: Option<SliceId>) {
        self.selection = match id {
            Some(id) if self.slice(id).is_some() => Some(id),
            _ => None,
        };
    }

    /// Replace the source sample and drop everything that referred to the old
    /// one. Slices are frame offsets into a specific sample and are meaningless
    /// against a different one.
    pub fn set_sample(&mut self, sample: Option<SampleRef>) {
        self.sample = sample;
        self.slices.clear();
        self.selection = None;
    }

    /// Divide the sample into `count` slices of equal length.
    ///
    /// Replaces any existing slices. This is the quickest way to get usable
    /// markers; individual bounds are adjusted afterwards.
    pub fn slice_evenly(&mut self, count: u32) {
        let Some(frames) = self.sample.as_ref().map(|sample| sample.frames) else {
            return;
        };
        if count == 0 || frames == 0 {
            return;
        }

        self.slices.clear();
        self.selection = None;
        for index in 0..u64::from(count) {
            let start = frames * index / u64::from(count);
            let end = frames * (index + 1) / u64::from(count);
            if end > start {
                self.add_slice(start, end);
            }
        }
    }

    /// Keep slices ordered by position so the user interface can draw and hit
    /// test them without sorting on every frame.
    fn sort_slices(&mut self) {
        self.slices
            .sort_by_key(|slice| (slice.start_frame, slice.end_frame, slice.id));
    }
}

/// Versioned envelope around [`Project`].
///
/// Serialization always goes through this type so that the version travels
/// with the data. Fields added to [`Project`] later are covered by serde
/// defaults; structural changes are handled in [`ProjectFile::migrate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub version: u32,
    pub project: Project,
}

impl Default for ProjectFile {
    fn default() -> Self {
        Self {
            version: PROJECT_VERSION,
            project: Project::default(),
        }
    }
}

impl ProjectFile {
    /// Bring a loaded project up to [`PROJECT_VERSION`].
    ///
    /// Returns an error for projects written by a newer build, because their
    /// contents cannot be interpreted correctly here.
    pub fn migrate(&mut self) -> Result<(), ProjectError> {
        if self.version > PROJECT_VERSION {
            return Err(ProjectError::UnsupportedVersion(self.version));
        }

        // No structural migrations exist yet. Older versions only ever differ
        // by added or removed fields, which serde handles with defaults and by
        // ignoring unknown keys.
        self.version = PROJECT_VERSION;
        Ok(())
    }
}

/// Failures that can occur while loading persisted project state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectError {
    /// The project was written by a build with a newer project version.
    UnsupportedVersion(u32),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::UnsupportedVersion(version) => write!(
                f,
                "Projektversion {version} wird von dieser Version nicht unterstützt \
                 (unterstützt bis {PROJECT_VERSION})"
            ),
        }
    }
}

impl std::error::Error for ProjectError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(frames: u64) -> SampleRef {
        SampleRef {
            path: PathBuf::from("/tmp/vocal.wav"),
            frames,
            sample_rate: 48_000,
            channels: 2,
        }
    }

    fn project_with_sample(frames: u64) -> Project {
        let mut project = Project::default();
        project.set_sample(Some(sample(frames)));
        project
    }

    #[test]
    fn slices_get_distinct_identities() {
        let mut project = project_with_sample(1_000);

        let first = project.add_slice(0, 100);
        let second = project.add_slice(100, 200);

        assert_ne!(first, second);
        assert_eq!(project.slices().len(), 2);
    }

    #[test]
    fn identities_are_not_reused_after_removal() {
        let mut project = project_with_sample(1_000);
        let first = project.add_slice(0, 100);
        project.remove_slice(first);

        let second = project.add_slice(0, 100);

        assert_ne!(first, second);
    }

    #[test]
    fn slices_stay_ordered_by_position() {
        let mut project = project_with_sample(1_000);
        project.add_slice(600, 700);
        project.add_slice(100, 200);
        project.add_slice(300, 400);

        let starts: Vec<u64> = project.slices().iter().map(|s| s.start_frame).collect();

        assert_eq!(starts, vec![100, 300, 600]);
    }

    #[test]
    fn inverted_bounds_are_normalized() {
        let mut project = project_with_sample(1_000);

        let id = project.add_slice(500, 200);
        let slice = project.slice(id).expect("slice was just added");

        assert_eq!((slice.start_frame, slice.end_frame), (200, 500));
    }

    #[test]
    fn moving_a_slice_restores_the_ordering() {
        let mut project = project_with_sample(1_000);
        let first = project.add_slice(100, 200);
        project.add_slice(300, 400);

        assert!(project.set_slice_bounds(first, 900, 950));

        let starts: Vec<u64> = project.slices().iter().map(|s| s.start_frame).collect();
        assert_eq!(starts, vec![300, 900]);
    }

    #[test]
    fn removing_the_selected_slice_clears_the_selection() {
        let mut project = project_with_sample(1_000);
        let id = project.add_slice(0, 100);
        project.select(Some(id));
        assert!(project.selected().is_some());

        project.remove_slice(id);

        assert_eq!(project.selection(), None);
        assert!(project.selected().is_none());
    }

    #[test]
    fn selecting_an_unknown_slice_clears_the_selection() {
        let mut project = project_with_sample(1_000);
        let id = project.add_slice(0, 100);
        project.select(Some(id));

        project.select(Some(SliceId(999)));

        assert_eq!(project.selection(), None);
    }

    #[test]
    fn replacing_the_sample_drops_slices_and_selection() {
        let mut project = project_with_sample(1_000);
        let id = project.add_slice(0, 100);
        project.select(Some(id));

        project.set_sample(Some(sample(2_000)));

        assert!(project.slices().is_empty());
        assert_eq!(project.selection(), None);
    }

    #[test]
    fn even_slicing_covers_the_sample_without_gaps() {
        let mut project = project_with_sample(1_000);

        project.slice_evenly(4);

        let slices = project.slices();
        assert_eq!(slices.len(), 4);
        assert_eq!(slices[0].start_frame, 0);
        assert_eq!(slices[3].end_frame, 1_000);
        for pair in slices.windows(2) {
            assert_eq!(pair[0].end_frame, pair[1].start_frame);
        }
    }

    #[test]
    fn even_slicing_handles_lengths_that_do_not_divide_evenly() {
        let mut project = project_with_sample(1_001);

        project.slice_evenly(3);

        let slices = project.slices();
        assert_eq!(slices.len(), 3);
        assert_eq!(slices[0].start_frame, 0);
        assert_eq!(slices[2].end_frame, 1_001);
        let total: u64 = slices.iter().map(|slice| slice.len_frames()).sum();
        assert_eq!(total, 1_001);
    }

    #[test]
    fn even_slicing_without_a_sample_does_nothing() {
        let mut project = Project::default();

        project.slice_evenly(8);

        assert!(project.slices().is_empty());
    }

    #[test]
    fn splitting_replaces_one_slice_with_two_adjacent_ones() {
        let mut project = project_with_sample(1_000);
        let id = project.add_slice(100, 500);

        let (left, right) = project.split_slice(id, 300).expect("300 lies inside");

        assert_eq!(project.slices().len(), 2);
        let left = *project.slice(left).expect("left half exists");
        let right = *project.slice(right).expect("right half exists");
        assert_eq!((left.start_frame, left.end_frame), (100, 300));
        assert_eq!((right.start_frame, right.end_frame), (300, 500));
        assert!(project.slice(id).is_none(), "the original is replaced");
    }

    #[test]
    fn splitting_keeps_the_selection_on_the_left_half() {
        let mut project = project_with_sample(1_000);
        let id = project.add_slice(0, 400);
        project.select(Some(id));

        let (left, _right) = project.split_slice(id, 200).expect("200 lies inside");

        assert_eq!(project.selection(), Some(left));
    }

    #[test]
    fn splitting_at_a_boundary_is_refused() {
        let mut project = project_with_sample(1_000);
        let id = project.add_slice(100, 500);

        assert_eq!(project.split_slice(id, 100), None);
        assert_eq!(project.split_slice(id, 500), None);
        assert_eq!(project.split_slice(id, 50), None);
        assert_eq!(project.split_slice(id, 900), None);
        assert_eq!(project.slices().len(), 1, "nothing may have changed");
    }

    #[test]
    fn splitting_an_unknown_slice_does_nothing() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 100);

        assert_eq!(project.split_slice(SliceId(999), 50), None);
        assert_eq!(project.slices().len(), 1);
    }

    #[test]
    fn a_frame_maps_to_the_slice_covering_it() {
        let mut project = project_with_sample(1_000);
        let first = project.add_slice(0, 300);
        let second = project.add_slice(300, 600);

        assert_eq!(project.slice_at(0).map(|s| s.id), Some(first));
        assert_eq!(project.slice_at(299).map(|s| s.id), Some(first));
        assert_eq!(project.slice_at(300).map(|s| s.id), Some(second));
        assert_eq!(project.slice_at(700), None);
    }

    #[test]
    fn round_trip_preserves_project() {
        let mut project = project_with_sample(4_800);
        let id = project.add_slice(0, 2_400);
        project.add_slice(2_400, 4_800);
        project.select(Some(id));
        let original = ProjectFile {
            version: PROJECT_VERSION,
            project,
        };

        let json = serde_json::to_string(&original).expect("serialization must succeed");
        let restored: ProjectFile =
            serde_json::from_str(&json).expect("deserialization must succeed");

        assert_eq!(restored, original);
        assert_eq!(restored.project.selection(), Some(id));
    }

    #[test]
    fn state_from_an_earlier_layout_still_loads() {
        // Phase 1 stored a test tone waveform and no sample. The field is gone;
        // loading such state must not fail.
        let restored: ProjectFile =
            serde_json::from_str(r#"{"version":1,"project":{"waveform":"square"}}"#)
                .expect("unknown fields must be ignored");

        assert!(restored.project.sample.is_none());
        assert!(restored.project.slices().is_empty());
    }

    #[test]
    fn migrate_accepts_current_version() {
        let mut file = ProjectFile::default();

        assert_eq!(file.migrate(), Ok(()));
        assert_eq!(file.version, PROJECT_VERSION);
    }

    #[test]
    fn migrate_rejects_future_versions() {
        let mut file = ProjectFile {
            version: PROJECT_VERSION + 1,
            project: Project::default(),
        };

        assert_eq!(
            file.migrate(),
            Err(ProjectError::UnsupportedVersion(PROJECT_VERSION + 1))
        );
    }
}
