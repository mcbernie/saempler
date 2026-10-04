use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cell::{is_black_key, CellId, PerformanceCell, PlaybackSettings};
use crate::effect::{CellEffects, SendRack};
use crate::modifier::{default_layout, Modifier, ModifierAssignment, ModifierMode};
use crate::modulation::{EnvelopeDefinition, LfoDefinition};
use crate::slice::{Slice, SliceId};

/// Version of the serialized project layout understood by this build.
///
/// Every stored project carries this number so that future layout changes can
/// be detected instead of silently misinterpreting old data.
pub const PROJECT_VERSION: u32 = 1;

/// Most slices a project may hold.
///
/// Each slice owns one slot in the host's automation bank, and those slots
/// have to exist before the host ever asks for the parameter list. The limit
/// is therefore what makes every slice automatable rather than only the ones
/// that happened to be made first.
pub const MAX_SLICES: usize = 20;

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
// `Eq` is deliberately absent: playback settings carry floats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub sample: Option<SampleRef>,
    slices: Vec<Slice>,
    selection: Option<SliceId>,
    cells: Vec<PerformanceCell>,
    cell_selection: Option<CellId>,
    #[serde(default = "default_layout")]
    modifiers: Vec<ModifierAssignment>,
    /// The sends, shared by every voice.
    #[serde(default)]
    sends: SendRack,
    /// Whether chops are kept off the raised keys.
    ///
    /// On a white-keys-only layout a run of chops lines up with the scale
    /// under the hand, which is how most people play a chop kit. It applies
    /// wherever the instrument places a cell itself and wherever one is moved
    /// to, so that a layout cannot drift off the rule once it is set.
    #[serde(default)]
    white_keys_only: bool,
    /// Hands out the next slice identity. Kept in the project so identities
    /// stay unique across a session even when slices are deleted.
    next_slice_id: u32,
    next_cell_id: u32,
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

    /// Remove a slice, along with anything that pointed at it.
    ///
    /// Cells referencing the slice go too: a cell without a slice has nothing
    /// to play, and leaving one behind would mean a note that silently does
    /// nothing.
    pub fn remove_slice(&mut self, id: SliceId) -> bool {
        let before = self.slices.len();
        self.slices.retain(|slice| slice.id != id);
        if self.selection == Some(id) {
            self.selection = None;
        }

        self.cells.retain(|cell| cell.slice != id);
        self.forget_missing_cell_selection();

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
    /// an empty slice. Also `None` at [`MAX_SLICES`], since a split adds one.
    pub fn split_slice(&mut self, id: SliceId, frame: u64) -> Option<(SliceId, SliceId)> {
        let slice = *self.slice(id)?;
        if frame <= slice.start_frame || frame >= slice.end_frame {
            return None;
        }
        if self.slices.len() >= MAX_SLICES {
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

    /// Move the slice boundary sitting at `from` to `to`.
    ///
    /// A boundary is identified by its position rather than by a slice and an
    /// edge, so that neighbouring slices which share it move together. That is
    /// what dragging a marker in an evenly divided sample should do.
    ///
    /// The target is clamped so that no affected slice becomes empty, and so
    /// that the boundary stays inside `total_frames`. Returns whether any
    /// slice changed.
    pub fn move_boundary(&mut self, from: u64, to: u64, total_frames: u64) -> bool {
        // A slice must keep at least one frame on either side of the boundary.
        let mut lower = 0u64;
        let mut upper = total_frames;
        let mut affected = false;

        for slice in &self.slices {
            if slice.end_frame == from {
                lower = lower.max(slice.start_frame + 1);
                affected = true;
            }
            if slice.start_frame == from {
                upper = upper.min(slice.end_frame.saturating_sub(1));
                affected = true;
            }
        }

        if !affected || lower > upper {
            return false;
        }

        let to = to.clamp(lower, upper);
        if to == from {
            return false;
        }

        for slice in &mut self.slices {
            if slice.end_frame == from {
                slice.end_frame = to;
            }
            if slice.start_frame == from {
                slice.start_frame = to;
            }
        }
        self.sort_slices();
        true
    }

    /// Remove the slice boundary sitting at `from`.
    ///
    /// Between two slices this merges them into one. At an outer edge the
    /// slice has nothing left to be bounded by, so it goes away entirely.
    /// Returns whether anything changed.
    pub fn remove_boundary(&mut self, frame: u64) -> bool {
        let left = self
            .slices
            .iter()
            .find(|slice| slice.end_frame == frame)
            .map(|slice| slice.id);
        let right = self
            .slices
            .iter()
            .find(|slice| slice.start_frame == frame)
            .map(|slice| slice.id);

        match (left, right) {
            (Some(left), Some(right)) => {
                let end = match self.slice(right) {
                    Some(slice) => slice.end_frame,
                    None => return false,
                };
                if let Some(slice) = self.slices.iter_mut().find(|slice| slice.id == left) {
                    slice.end_frame = end;
                }
                // The merged slice lives on as the left one, so a selection on
                // the right half follows it rather than disappearing.
                if self.selection == Some(right) {
                    self.selection = Some(left);
                }
                self.remove_slice(right);
                self.sort_slices();
                true
            }
            (Some(id), None) | (None, Some(id)) => self.remove_slice(id),
            (None, None) => false,
        }
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
        self.cells.clear();
        self.cell_selection = None;
    }

    /// Divide the sample into `count` slices of equal length.
    ///
    /// Replaces any existing slices. This is the quickest way to get usable
    /// markers; individual bounds are adjusted afterwards. More than
    /// [`MAX_SLICES`] is cut down rather than refused, so that asking for too
    /// many still leaves a usable set of markers.
    pub fn slice_evenly(&mut self, count: u32) {
        let Some(frames) = self.sample.as_ref().map(|sample| sample.frames) else {
            return;
        };
        let count = count.min(MAX_SLICES as u32);
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

    /// Which automation slot a slice owns, by its position in the list.
    ///
    /// The list is kept ordered by start frame, so the slot is the number the
    /// interface already shows on the marker: slot 0 is S1. Two cells playing
    /// the same slice share its slot, which is the price of indexing by what
    /// the user can see rather than by a cell they cannot name.
    pub fn automation_slot(&self, slice: SliceId) -> Option<u8> {
        let index = self.slices.iter().position(|entry| entry.id == slice)?;
        (index < MAX_SLICES).then_some(index as u8)
    }

    /// All performance cells, ordered by MIDI note.
    pub fn cells(&self) -> &[PerformanceCell] {
        &self.cells
    }

    /// The cell a note triggers, if any.
    pub fn cell_for_note(&self, note: u8) -> Option<&PerformanceCell> {
        self.cells.iter().find(|cell| cell.midi_note == note)
    }

    /// Look up a cell by identity.
    pub fn cell(&self, id: CellId) -> Option<&PerformanceCell> {
        self.cells.iter().find(|cell| cell.id == id)
    }

    /// Put `slice` on `note`, replacing whatever was there.
    ///
    /// A note triggers exactly one cell, so assigning to an occupied note
    /// takes that cell over rather than stacking a second one on it.
    pub fn assign(&mut self, note: u8, slice: SliceId) -> Option<CellId> {
        // A cell must point at a slice that exists, or the note would be
        // silently dead.
        self.slice(slice)?;

        // A modifier key is not available for playing.
        if self.modifier_for_note(note).is_some() {
            return None;
        }

        if let Some(cell) = self.cells.iter_mut().find(|cell| cell.midi_note == note) {
            cell.slice = slice;
            return Some(cell.id);
        }

        let id = CellId(self.next_cell_id);
        self.next_cell_id += 1;
        self.cells.push(PerformanceCell {
            id,
            midi_note: note,
            slice,
            ..PerformanceCell::placeholder()
        });
        self.sort_cells();
        Some(id)
    }

    /// Copy a cell onto the first free key at or above `from`.
    ///
    /// Free rather than the next one along: duplicating onto an occupied key
    /// would quietly replace whatever was already performed there, and the
    /// whole point of duplicating is to end up with two of something.
    ///
    /// Returns `None` when there is no free key left above `from`.
    pub fn copy_cell_to_free_note(&mut self, id: CellId, from: u8) -> Option<CellId> {
        let note = self.first_free_note(from)?;
        self.copy_cell_to_note(id, note)
    }

    /// Move a cell to another key.
    ///
    /// Two cells swap keys rather than one destroying the other: a mapping is
    /// work, and a drag that lands a pad on an occupied one should rearrange
    /// the keyboard, not empty part of it.
    ///
    /// Returns whether anything moved.
    pub fn move_cell_to_note(&mut self, id: CellId, note: u8) -> bool {
        // A modifier key is not available for playing, and nor is a raised
        // one while they are being avoided.
        if !self.key_allows_cell(note) {
            return false;
        }

        let Some(from) = self.cell(id).map(|cell| cell.midi_note) else {
            return false;
        };
        if from == note {
            return false;
        }

        let occupant = self
            .cells
            .iter()
            .find(|cell| cell.midi_note == note)
            .map(|cell| cell.id);

        for cell in &mut self.cells {
            if cell.id == id {
                cell.midi_note = note;
            } else if Some(cell.id) == occupant {
                cell.midi_note = from;
            }
        }
        self.sort_cells();
        true
    }

    /// Copy a cell onto another note, settings and all.
    ///
    /// This is how one slice ends up performed several ways: duplicate the
    /// cell, then change the copy.
    pub fn copy_cell_to_note(&mut self, id: CellId, note: u8) -> Option<CellId> {
        let source = self.cell(id)?.clone();
        let new_id = self.assign(note, source.slice)?;
        if let Some(cell) = self.cells.iter_mut().find(|cell| cell.id == new_id) {
            // Everything but the identity and the key it sits on.
            cell.playback = source.playback;
            cell.envelopes = source.envelopes;
            cell.lfos = source.lfos;
            cell.routes = source.routes;
        }
        Some(new_id)
    }

    /// Take the cell off a note.
    pub fn clear_note(&mut self, note: u8) -> bool {
        let before = self.cells.len();
        self.cells.retain(|cell| cell.midi_note != note);
        self.forget_missing_cell_selection();
        self.cells.len() != before
    }

    /// Every modifier note, in keyboard order.
    pub fn modifiers(&self) -> &[ModifierAssignment] {
        &self.modifiers
    }

    /// The modifier a note carries, if any.
    pub fn modifier_for_note(&self, note: u8) -> Option<&ModifierAssignment> {
        self.modifiers.iter().find(|entry| entry.note == note)
    }

    /// Whether a note is already doing something.
    ///
    /// One key, one job: a note cannot be both a modifier and a cell, and two
    /// modifiers cannot share a key.
    pub fn note_is_taken(&self, note: u8) -> bool {
        self.modifiers.iter().any(|entry| entry.note == note)
            || self.cells.iter().any(|cell| cell.midi_note == note)
    }

    /// Put a modifier on a free note.
    ///
    /// The same modifier may sit on several notes, each with its own mode:
    /// stutter held on one key and armed as a one shot on the next is a
    /// combination worth playing.
    pub fn add_modifier(&mut self, note: u8, modifier: Modifier, mode: ModifierMode) -> bool {
        if self.note_is_taken(note) {
            return false;
        }

        self.modifiers.push(ModifierAssignment {
            note,
            modifier,
            mode,
        });
        self.modifiers.sort_by_key(|entry| entry.note);
        true
    }

    /// Copy a modifier onto the first free key above its own.
    ///
    /// Two keys doing the same thing in different modes is a real layout: one
    /// reverse held, another latched. Cloning is how you reach that without
    /// setting the second one up by hand.
    ///
    /// Returns the key the copy landed on.
    pub fn clone_modifier(&mut self, note: u8) -> Option<u8> {
        let entry = *self.modifier_for_note(note)?;
        let target = self.first_free_note(note.saturating_add(1))?;
        self.add_modifier(target, entry.modifier, entry.mode)
            .then_some(target)
    }

    /// Take the modifier off a note.
    pub fn remove_modifier(&mut self, note: u8) -> bool {
        let before = self.modifiers.len();
        self.modifiers.retain(|entry| entry.note != note);
        self.modifiers.len() != before
    }

    /// Change what the modifier on a note does.
    pub fn set_modifier(&mut self, note: u8, modifier: Modifier) -> bool {
        match self.modifiers.iter_mut().find(|entry| entry.note == note) {
            Some(entry) => {
                entry.modifier = modifier;
                true
            }
            None => false,
        }
    }

    /// Change how the modifier on a note responds to its key.
    pub fn set_modifier_mode(&mut self, note: u8, mode: ModifierMode) -> bool {
        match self.modifiers.iter_mut().find(|entry| entry.note == note) {
            Some(entry) => {
                entry.mode = mode;
                true
            }
            None => false,
        }
    }

    /// Move a modifier to another note, if that note is free.
    pub fn move_modifier(&mut self, from: u8, to: u8) -> bool {
        if from == to || self.note_is_taken(to) {
            return false;
        }

        match self.modifiers.iter_mut().find(|entry| entry.note == from) {
            Some(entry) => {
                entry.note = to;
                self.modifiers.sort_by_key(|entry| entry.note);
                true
            }
            None => false,
        }
    }

    /// Put the starting layout back.
    pub fn reset_modifiers(&mut self) {
        self.modifiers = default_layout();
    }

    /// The lowest free note at or above `from`, for placing a new modifier.
    pub fn first_free_note(&self, from: u8) -> Option<u8> {
        (from..=127).find(|note| !self.note_is_taken(*note) && self.key_allows_cell(*note))
    }

    /// Whether chops are kept off the raised keys.
    pub fn white_keys_only(&self) -> bool {
        self.white_keys_only
    }

    /// Keep chops off the raised keys, or stop doing so.
    ///
    /// Switching it on does not move what is already mapped: a layout someone
    /// built by hand is theirs, and rearranging it under them would lose work.
    pub fn set_white_keys_only(&mut self, only: bool) {
        self.white_keys_only = only;
    }

    /// Whether a cell may be put on this key.
    pub fn key_allows_cell(&self, note: u8) -> bool {
        self.modifier_for_note(note).is_none() && !(self.white_keys_only && is_black_key(note))
    }

    /// The sends every voice shares.
    pub fn sends(&self) -> SendRack {
        self.sends
    }

    /// Replace the send settings.
    pub fn set_sends(&mut self, sends: SendRack) {
        self.sends = sends.sanitized();
    }

    /// Replace one cell's own effects.
    pub fn set_cell_effects(&mut self, id: CellId, effects: CellEffects) -> bool {
        self.with_cell_mut(id, |cell| cell.effects = effects.sanitized())
    }

    /// Take every cell off the keyboard.
    pub fn clear_cells(&mut self) {
        self.cells.clear();
        self.cell_selection = None;
    }

    /// Lay every slice out chromatically, starting at `base_note`.
    ///
    /// Replaces the existing mapping. Slices that would run past note 127 are
    /// left unassigned rather than wrapping around.
    pub fn map_slices_from(&mut self, base_note: u8) {
        self.cells.clear();
        self.cell_selection = None;

        let ids: Vec<SliceId> = self.slices.iter().map(|slice| slice.id).collect();
        let mut note = base_note;
        for slice in ids {
            // Step over anything a modifier owns, and over the raised keys
            // when they are being avoided, rather than losing that slice to a
            // key it could never be played from.
            while note <= 127 && !self.key_allows_cell(note) {
                note += 1;
            }
            if note > 127 {
                break;
            }
            self.assign(note, slice);
            match note.checked_add(1) {
                Some(next) => note = next,
                None => break,
            }
        }
    }

    /// Change how a cell plays, keeping the values usable.
    pub fn set_playback(&mut self, id: CellId, playback: PlaybackSettings) -> bool {
        match self.cells.iter_mut().find(|cell| cell.id == id) {
            Some(cell) => {
                cell.playback = playback.sanitized();
                true
            }
            None => false,
        }
    }

    /// Change one of a cell's envelopes.
    pub fn set_envelope(&mut self, id: CellId, index: usize, envelope: EnvelopeDefinition) -> bool {
        match self.cells.iter_mut().find(|cell| cell.id == id) {
            Some(cell) => match cell.envelopes.get_mut(index) {
                Some(slot) => {
                    *slot = envelope.sanitized();
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// Change one of a cell's LFOs.
    pub fn set_lfo(&mut self, id: CellId, index: usize, lfo: LfoDefinition) -> bool {
        match self.cells.iter_mut().find(|cell| cell.id == id) {
            Some(cell) => match cell.lfos.get_mut(index) {
                Some(slot) => {
                    *slot = lfo.sanitized();
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// Edit the modulation matrix of a cell.
    pub fn with_cell_mut(&mut self, id: CellId, edit: impl FnOnce(&mut PerformanceCell)) -> bool {
        match self.cells.iter_mut().find(|cell| cell.id == id) {
            Some(cell) => {
                edit(cell);
                true
            }
            None => false,
        }
    }

    /// The cell currently being edited.
    pub fn selected_cell(&self) -> Option<&PerformanceCell> {
        self.cell_selection.and_then(|id| self.cell(id))
    }

    /// Identity of the cell currently being edited.
    pub fn cell_selection(&self) -> Option<CellId> {
        self.cell_selection
    }

    /// Select a cell for editing. Selecting an unknown cell clears it.
    pub fn select_cell(&mut self, id: Option<CellId>) {
        self.cell_selection = match id {
            Some(id) if self.cell(id).is_some() => Some(id),
            _ => None,
        };
    }

    /// Drop a selection that points at a cell which no longer exists.
    fn forget_missing_cell_selection(&mut self) {
        if let Some(selected) = self.cell_selection {
            if !self.cells.iter().any(|cell| cell.id == selected) {
                self.cell_selection = None;
            }
        }
    }

    /// Keep cells ordered by note so pads can be drawn in playing order.
    fn sort_cells(&mut self) {
        self.cells.sort_by_key(|cell| (cell.midi_note, cell.id));
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub version: u32,
    pub project: Project,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            sample: None,
            slices: Vec::new(),
            selection: None,
            cells: Vec::new(),
            cell_selection: None,
            modifiers: default_layout(),
            sends: SendRack::default(),
            white_keys_only: false,
            next_slice_id: 0,
            next_cell_id: 0,
        }
    }
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
    fn moving_a_shared_boundary_moves_both_neighbours() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);
        project.add_slice(400, 800);

        assert!(project.move_boundary(400, 600, 1_000));

        let slices = project.slices();
        assert_eq!((slices[0].start_frame, slices[0].end_frame), (0, 600));
        assert_eq!((slices[1].start_frame, slices[1].end_frame), (600, 800));
    }

    #[test]
    fn moving_an_outer_boundary_moves_only_that_slice() {
        let mut project = project_with_sample(1_000);
        project.add_slice(200, 400);
        project.add_slice(400, 800);

        assert!(project.move_boundary(200, 100, 1_000));

        let slices = project.slices();
        assert_eq!((slices[0].start_frame, slices[0].end_frame), (100, 400));
        assert_eq!((slices[1].start_frame, slices[1].end_frame), (400, 800));
    }

    #[test]
    fn a_boundary_cannot_be_dragged_past_its_neighbours() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);
        project.add_slice(400, 800);

        // Far past the right neighbour's end.
        project.move_boundary(400, 5_000, 1_000);

        let slices = project.slices();
        assert_eq!(slices[1].end_frame, 800);
        assert!(slices[1].len_frames() >= 1, "no slice may collapse");
        assert_eq!(slices[0].end_frame, slices[1].start_frame);
    }

    #[test]
    fn a_boundary_cannot_be_dragged_before_its_left_neighbour() {
        let mut project = project_with_sample(1_000);
        project.add_slice(100, 400);
        project.add_slice(400, 800);

        project.move_boundary(400, 0, 1_000);

        let slices = project.slices();
        assert!(slices[0].len_frames() >= 1);
        assert_eq!(slices[0].start_frame, 100);
        assert_eq!(slices[0].end_frame, 101);
        assert_eq!(slices[1].start_frame, 101);
    }

    #[test]
    fn moving_a_boundary_that_does_not_exist_changes_nothing() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);
        let before = project.slices().to_vec();

        assert!(!project.move_boundary(777, 500, 1_000));

        assert_eq!(project.slices(), before.as_slice());
    }

    #[test]
    fn moving_a_boundary_nowhere_reports_no_change() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);
        project.add_slice(400, 800);

        assert!(!project.move_boundary(400, 400, 1_000));
    }

    #[test]
    fn removing_an_inner_boundary_merges_the_two_slices() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);
        project.add_slice(400, 900);

        assert!(project.remove_boundary(400));

        let slices = project.slices();
        assert_eq!(slices.len(), 1);
        assert_eq!((slices[0].start_frame, slices[0].end_frame), (0, 900));
    }

    #[test]
    fn a_selection_on_the_right_half_survives_the_merge() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);
        let right = project.add_slice(400, 900);
        project.select(Some(right));

        project.remove_boundary(400);

        let selected = project.selected().expect("the merged slice stays selected");
        assert_eq!((selected.start_frame, selected.end_frame), (0, 900));
    }

    #[test]
    fn removing_an_outer_boundary_removes_that_slice() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);
        project.add_slice(400, 900);

        assert!(project.remove_boundary(0));

        let slices = project.slices();
        assert_eq!(slices.len(), 1);
        assert_eq!((slices[0].start_frame, slices[0].end_frame), (400, 900));
    }

    #[test]
    fn removing_the_last_boundary_removes_the_last_slice() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);
        project.add_slice(400, 900);

        assert!(project.remove_boundary(900));

        assert_eq!(project.slices().len(), 1);
        assert_eq!(project.slices()[0].end_frame, 400);
    }

    #[test]
    fn removing_a_boundary_that_does_not_exist_changes_nothing() {
        let mut project = project_with_sample(1_000);
        project.add_slice(0, 400);

        assert!(!project.remove_boundary(777));
        assert_eq!(project.slices().len(), 1);
    }

    #[test]
    fn merging_repeatedly_ends_with_one_slice() {
        let mut project = project_with_sample(8_000);
        project.slice_evenly(8);
        assert_eq!(project.slices().len(), 8);

        // Always remove the boundary after the first slice.
        for _ in 0..7 {
            let boundary = project.slices()[0].end_frame;
            assert!(project.remove_boundary(boundary));
        }

        let slices = project.slices();
        assert_eq!(slices.len(), 1);
        assert_eq!((slices[0].start_frame, slices[0].end_frame), (0, 8_000));
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
    fn assigning_a_slice_to_a_note_creates_a_cell() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);

        let cell = project.assign(60, slice).expect("the slice exists");

        let stored = project.cell_for_note(60).expect("note 60 is mapped");
        assert_eq!(stored.id, cell);
        assert_eq!(stored.slice, slice);
        assert_eq!(stored.playback, PlaybackSettings::default());
    }

    #[test]
    fn a_note_holds_only_one_cell() {
        let mut project = project_with_sample(1_000);
        let first = project.add_slice(0, 500);
        let second = project.add_slice(500, 1_000);

        project.assign(60, first);
        project.assign(60, second);

        assert_eq!(project.cells().len(), 1);
        assert_eq!(project.cell_for_note(60).map(|c| c.slice), Some(second));
    }

    #[test]
    fn assigning_an_unknown_slice_is_refused() {
        let mut project = project_with_sample(1_000);

        assert_eq!(project.assign(60, SliceId(999)), None);
        assert!(project.cells().is_empty());
    }

    #[test]
    fn several_notes_may_share_one_slice() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);

        project.assign(60, slice);
        project.assign(61, slice);
        project.assign(62, slice);

        assert_eq!(project.cells().len(), 3);
        for note in [60, 61, 62] {
            assert_eq!(project.cell_for_note(note).map(|c| c.slice), Some(slice));
        }
    }

    #[test]
    fn copying_a_cell_carries_its_settings_but_not_its_identity() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let source = project.assign(60, slice).expect("the slice exists");
        project.set_playback(
            source,
            PlaybackSettings {
                reverse: true,
                speed: 0.5,
                ..Default::default()
            },
        );

        let copy = project
            .copy_cell_to_note(source, 61)
            .expect("source exists");

        assert_ne!(copy, source);
        let original = project.cell(source).expect("original survives").clone();
        let copied = project.cell(copy).expect("copy exists").clone();
        assert_eq!(copied.slice, original.slice);
        assert_eq!(copied.playback, original.playback);
        assert_eq!(copied.midi_note, 61);
    }

    #[test]
    fn changing_a_copy_leaves_the_original_alone() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let source = project.assign(60, slice).expect("the slice exists");
        let copy = project
            .copy_cell_to_note(source, 61)
            .expect("source exists");

        project.set_playback(
            copy,
            PlaybackSettings {
                pitch_semitones: 7.0,
                ..Default::default()
            },
        );

        assert_eq!(
            project.cell(source).map(|c| c.playback.pitch_semitones),
            Some(0.0)
        );
        assert_eq!(
            project.cell(copy).map(|c| c.playback.pitch_semitones),
            Some(7.0)
        );
    }

    #[test]
    fn cells_are_ordered_by_note() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        project.assign(64, slice);
        project.assign(60, slice);
        project.assign(62, slice);

        let notes: Vec<u8> = project.cells().iter().map(|cell| cell.midi_note).collect();

        assert_eq!(notes, vec![60, 62, 64]);
    }

    #[test]
    fn removing_a_slice_removes_the_cells_that_played_it() {
        let mut project = project_with_sample(1_000);
        let doomed = project.add_slice(0, 500);
        let kept = project.add_slice(500, 1_000);
        project.assign(60, doomed);
        project.assign(61, kept);

        project.remove_slice(doomed);

        assert_eq!(project.cells().len(), 1);
        assert!(project.cell_for_note(60).is_none());
        assert!(project.cell_for_note(61).is_some());
    }

    #[test]
    fn mapping_lays_the_slices_out_chromatically() {
        let mut project = project_with_sample(8_000);
        project.slice_evenly(8);

        project.map_slices_from(60);

        assert_eq!(project.cells().len(), 8);
        for (offset, cell) in project.cells().iter().enumerate() {
            assert_eq!(cell.midi_note, 60 + offset as u8);
            assert_eq!(cell.slice, project.slices()[offset].id);
        }
    }

    #[test]
    fn mapping_stops_at_the_top_of_the_midi_range() {
        let mut project = project_with_sample(16_000);
        project.slice_evenly(16);

        project.map_slices_from(120);

        // Notes 120..=127 fit, the remaining slices stay unassigned.
        assert_eq!(project.cells().len(), 8);
        assert!(project.cells().iter().all(|cell| cell.midi_note <= 127));
    }

    #[test]
    fn mapping_skips_the_black_keys_when_asked_to() {
        let mut project = project_with_sample(8_000);
        project.slice_evenly(7);
        project.set_white_keys_only(true);

        project.map_slices_from(72);

        let notes: Vec<u8> = project.cells().iter().map(|cell| cell.midi_note).collect();
        assert_eq!(notes, vec![72, 74, 76, 77, 79, 81, 83], "{notes:?}");
    }

    #[test]
    fn mapping_uses_every_key_when_not_asked_to() {
        let mut project = project_with_sample(8_000);
        project.slice_evenly(4);

        project.map_slices_from(72);

        let notes: Vec<u8> = project.cells().iter().map(|cell| cell.midi_note).collect();
        assert_eq!(notes, vec![72, 73, 74, 75]);
    }

    #[test]
    fn a_cell_will_not_move_onto_a_black_key_when_they_are_avoided() {
        let mut project = project_with_sample(4_000);
        let slice = project.add_slice(0, 1_000);
        let id = project.assign(72, slice).expect("the slice exists");
        project.set_white_keys_only(true);

        assert!(!project.move_cell_to_note(id, 73), "C#4 is a black key");
        assert!(project.move_cell_to_note(id, 74));
    }

    #[test]
    fn switching_the_rule_on_leaves_what_is_already_mapped_alone() {
        let mut project = project_with_sample(4_000);
        let slice = project.add_slice(0, 1_000);
        project.assign(73, slice).expect("the slice exists");

        project.set_white_keys_only(true);

        assert!(
            project.cell_for_note(73).is_some(),
            "a layout built by hand should not be rearranged under the user"
        );
    }

    #[test]
    fn cloning_a_modifier_lands_on_the_next_free_key() {
        let mut project = Project::default();
        project.reset_modifiers();
        let first = project.modifiers()[0];

        let target = project
            .clone_modifier(first.note)
            .expect("there is room above");

        let copy = project
            .modifier_for_note(target)
            .expect("the copy is there");
        assert_eq!(copy.modifier, first.modifier);
        assert_eq!(copy.mode, first.mode);
        assert!(target > first.note);
    }

    #[test]
    fn cloning_a_key_that_holds_no_modifier_does_nothing() {
        let mut project = Project::default();
        project.reset_modifiers();
        let before = project.modifiers().len();

        assert!(project.clone_modifier(100).is_none());
        assert_eq!(project.modifiers().len(), before);
    }

    #[test]
    fn duplicating_lands_on_a_free_key_rather_than_over_a_used_one() {
        let mut project = project_with_sample(4_000);
        let slice = project.add_slice(0, 1_000);
        let first = project.assign(72, slice).expect("the slice exists");
        project.assign(73, slice).expect("the slice exists");
        let before = project.cells().len();

        let copy = project
            .copy_cell_to_free_note(first, 73)
            .expect("there is room above");

        assert_eq!(project.cells().len(), before + 1, "it replaced a cell");
        assert_eq!(
            project.cell(copy).expect("the copy exists").midi_note,
            74,
            "it should skip the key that was taken"
        );
    }

    #[test]
    fn moving_a_cell_onto_a_free_key_takes_it_there() {
        let mut project = project_with_sample(4_000);
        let slice = project.add_slice(0, 1_000);
        let id = project.assign(72, slice).expect("the slice exists");

        assert!(project.move_cell_to_note(id, 80));

        assert_eq!(project.cell(id).expect("it is still there").midi_note, 80);
        assert!(project.cell_for_note(72).is_none());
    }

    #[test]
    fn moving_a_cell_onto_a_used_key_swaps_the_two() {
        let mut project = project_with_sample(4_000);
        let slice = project.add_slice(0, 1_000);
        let first = project.assign(72, slice).expect("the slice exists");
        let second = project.assign(75, slice).expect("the slice exists");
        let before = project.cells().len();

        assert!(project.move_cell_to_note(first, 75));

        assert_eq!(project.cells().len(), before, "a cell went missing");
        assert_eq!(project.cell(first).expect("first").midi_note, 75);
        assert_eq!(project.cell(second).expect("second").midi_note, 72);
    }

    #[test]
    fn a_cell_will_not_move_onto_a_modifier_key() {
        let mut project = project_with_sample(4_000);
        let slice = project.add_slice(0, 1_000);
        let id = project.assign(72, slice).expect("the slice exists");
        project.add_modifier(40, Modifier::Reverse, ModifierMode::Hold);

        assert!(!project.move_cell_to_note(id, 40));
        assert_eq!(project.cell(id).expect("unmoved").midi_note, 72);
    }

    #[test]
    fn mapping_replaces_the_previous_layout() {
        let mut project = project_with_sample(4_000);
        project.slice_evenly(4);
        project.map_slices_from(60);

        project.map_slices_from(72);

        assert_eq!(project.cells().len(), 4);
        assert!(project.cell_for_note(60).is_none());
        assert!(project.cell_for_note(72).is_some());
    }

    #[test]
    fn clearing_takes_every_cell_off() {
        let mut project = project_with_sample(4_000);
        project.slice_evenly(4);
        project.map_slices_from(60);
        project.select_cell(project.cells().first().map(|cell| cell.id));

        project.clear_cells();

        assert!(project.cells().is_empty());
        assert_eq!(project.cell_selection(), None);
        // The slices themselves are untouched.
        assert_eq!(project.slices().len(), 4);
    }

    #[test]
    fn clearing_a_note_takes_its_cell_off() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        project.assign(60, slice);

        assert!(project.clear_note(60));
        assert!(!project.clear_note(60));
        assert!(project.cell_for_note(60).is_none());
    }

    #[test]
    fn a_cell_selection_disappears_with_its_cell() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let cell = project.assign(60, slice).expect("the slice exists");
        project.select_cell(Some(cell));
        assert!(project.selected_cell().is_some());

        project.clear_note(60);

        assert_eq!(project.cell_selection(), None);
    }

    #[test]
    fn playback_settings_are_repaired_on_the_way_in() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let cell = project.assign(60, slice).expect("the slice exists");

        project.set_playback(
            cell,
            PlaybackSettings {
                speed: 0.0,
                ..Default::default()
            },
        );

        let stored = project.cell(cell).expect("cell exists");
        assert!(stored.playback.rate() > 0.0);
    }

    #[test]
    fn a_new_cell_comes_with_an_amplitude_route() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let id = project.assign(60, slice).expect("the slice exists");

        let cell = project.cell(id).expect("cell exists");

        assert!(cell.has_amplitude());
        assert_eq!(cell.routes.len(), 1);
    }

    #[test]
    fn a_cell_without_a_volume_route_reports_no_amplitude() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let id = project.assign(60, slice).expect("the slice exists");

        project.with_cell_mut(id, |cell| {
            cell.routes.clear();
        });

        assert!(!project.cell(id).expect("cell exists").has_amplitude());
    }

    #[test]
    fn routes_can_be_added_removed_and_changed() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let id = project.assign(60, slice).expect("the slice exists");

        project.with_cell_mut(id, |cell| {
            assert!(cell.add_route(crate::ModulationRoute {
                source: crate::ModSource::Lfo1,
                destination: crate::ModDestination::Pitch,
                amount: 0.5,
            }));
            assert_eq!(cell.routes.len(), 2);

            assert!(cell.set_route(
                1,
                crate::ModulationRoute {
                    source: crate::ModSource::Lfo2,
                    destination: crate::ModDestination::Pan,
                    amount: -0.25,
                }
            ));
            assert_eq!(cell.routes[1].destination, crate::ModDestination::Pan);

            assert!(cell.remove_route(1));
            assert_eq!(cell.routes.len(), 1);
            assert!(!cell.remove_route(9));
        });
    }

    #[test]
    fn the_matrix_has_a_ceiling() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let id = project.assign(60, slice).expect("the slice exists");

        project.with_cell_mut(id, |cell| {
            while cell.routes.len() < crate::MAX_ROUTES {
                assert!(cell.add_route(crate::ModulationRoute::default()));
            }
            assert!(
                !cell.add_route(crate::ModulationRoute::default()),
                "the engine holds a fixed number of routes"
            );
        });
    }

    #[test]
    fn copying_a_cell_carries_its_modulation() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let source = project.assign(60, slice).expect("the slice exists");
        project.with_cell_mut(source, |cell| {
            cell.envelopes[1].attack_ms = 500.0;
            cell.lfos[0].shape = crate::LfoShape::Square;
            cell.add_route(crate::ModulationRoute {
                source: crate::ModSource::Lfo1,
                destination: crate::ModDestination::Pitch,
                amount: 0.5,
            });
        });

        let copy = project
            .copy_cell_to_note(source, 61)
            .expect("the source exists");

        let copied = project.cell(copy).expect("the copy exists");
        assert_eq!(copied.envelopes[1].attack_ms, 500.0);
        assert_eq!(copied.lfos[0].shape, crate::LfoShape::Square);
        assert_eq!(copied.routes.len(), 2);
    }

    #[test]
    fn envelopes_and_lfos_can_be_changed() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let id = project.assign(60, slice).expect("the slice exists");

        assert!(project.set_envelope(
            id,
            1,
            EnvelopeDefinition {
                attack_ms: 100.0,
                ..Default::default()
            }
        ));
        assert!(project.set_lfo(
            id,
            0,
            LfoDefinition {
                sync: true,
                ..Default::default()
            }
        ));
        assert!(!project.set_envelope(id, 9, EnvelopeDefinition::default()));

        let cell = project.cell(id).expect("cell exists");
        assert_eq!(cell.envelopes[1].attack_ms, 100.0);
        assert!(cell.lfos[0].sync);
    }

    #[test]
    fn cells_survive_a_round_trip() {
        let mut project = project_with_sample(4_000);
        project.slice_evenly(4);
        project.map_slices_from(60);
        let cell = project.cells()[1].id;
        project.set_playback(
            cell,
            PlaybackSettings {
                reverse: true,
                speed: 0.5,
                pitch_semitones: -5.0,
                ..Default::default()
            },
        );
        project.select_cell(Some(cell));
        let original = ProjectFile {
            version: PROJECT_VERSION,
            project,
        };

        let json = serde_json::to_string(&original).expect("serialization must succeed");
        let restored: ProjectFile = serde_json::from_str(&json).expect("must deserialize");

        assert_eq!(restored, original);
        let restored_cell = restored
            .project
            .selected_cell()
            .expect("selection survives");
        assert!(restored_cell.playback.reverse);
        assert_eq!(restored_cell.playback.pitch_semitones, -5.0);
    }

    #[test]
    fn a_new_project_comes_with_the_modifier_layout() {
        let project = Project::default();

        assert_eq!(
            project.modifiers().len(),
            crate::Modifier::DEFAULT_LAYOUT.len()
        );
        assert!(project
            .modifier_for_note(crate::modifier::MODIFIER_BASE_NOTE)
            .is_some());
    }

    #[test]
    fn a_modifier_mode_can_be_changed() {
        let mut project = Project::default();
        let note = project.modifiers()[0].note;

        assert!(project.set_modifier_mode(note, ModifierMode::OneShot));

        assert_eq!(
            project.modifier_for_note(note).map(|e| e.mode),
            Some(ModifierMode::OneShot)
        );
    }

    #[test]
    fn a_modifier_can_be_moved_to_a_free_note() {
        let mut project = Project::default();
        let note = project.modifiers()[0].note;

        assert!(project.move_modifier(note, 30));

        assert!(project.modifier_for_note(note).is_none());
        assert!(project.modifier_for_note(30).is_some());
    }

    #[test]
    fn a_modifier_cannot_take_an_occupied_note() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        project.assign(72, slice);
        let first = project.modifiers()[0].note;
        let second = project.modifiers()[1].note;

        assert!(!project.move_modifier(first, second), "another modifier");
        assert!(!project.move_modifier(first, 72), "a performance cell");
    }

    #[test]
    fn a_cell_cannot_take_a_modifier_note() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        let note = project.modifiers()[0].note;

        assert_eq!(project.assign(note, slice), None);
    }

    #[test]
    fn the_automatic_mapping_steps_over_modifier_keys() {
        let mut project = project_with_sample(8_000);
        project.slice_evenly(4);
        // Put a modifier right in the middle of where the slices would land.
        project.add_modifier(61, Modifier::Stutter, ModifierMode::Hold);

        project.map_slices_from(60);

        assert_eq!(project.cells().len(), 4, "no slice may be lost");
        assert!(project.cell_for_note(61).is_none());
        let notes: Vec<u8> = project.cells().iter().map(|c| c.midi_note).collect();
        assert_eq!(notes, vec![60, 62, 63, 64]);
    }

    #[test]
    fn a_modifier_can_be_added_and_removed() {
        let mut project = Project::default();
        let before = project.modifiers().len();

        assert!(project.add_modifier(24, Modifier::Brake, ModifierMode::Toggle));
        assert_eq!(project.modifiers().len(), before + 1);

        assert!(project.remove_modifier(24));
        assert_eq!(project.modifiers().len(), before);
        assert!(!project.remove_modifier(24));
    }

    #[test]
    fn the_same_modifier_may_sit_on_several_notes() {
        let mut project = Project::default();
        let stutter = project
            .modifiers()
            .iter()
            .find(|entry| entry.modifier == Modifier::Stutter)
            .copied()
            .expect("stutter is in the layout");

        assert!(project.add_modifier(25, Modifier::Stutter, ModifierMode::OneShot));

        assert_eq!(
            project.modifier_for_note(25).map(|e| e.mode),
            Some(ModifierMode::OneShot)
        );
        assert_eq!(
            project.modifier_for_note(stutter.note).map(|e| e.mode),
            Some(ModifierMode::Hold),
            "the original keeps its own mode"
        );
    }

    #[test]
    fn modifiers_stay_in_keyboard_order() {
        let mut project = Project::default();
        project.add_modifier(20, Modifier::Brake, ModifierMode::Hold);
        project.add_modifier(100, Modifier::Reverse, ModifierMode::Hold);

        let notes: Vec<u8> = project.modifiers().iter().map(|e| e.note).collect();
        let mut sorted = notes.clone();
        sorted.sort_unstable();

        assert_eq!(notes, sorted);
    }

    #[test]
    fn the_layout_can_be_put_back() {
        let mut project = Project::default();
        project.remove_modifier(project.modifiers()[0].note);
        project.add_modifier(20, Modifier::Brake, ModifierMode::Hold);

        project.reset_modifiers();

        assert_eq!(project.modifiers(), crate::default_layout().as_slice());
    }

    #[test]
    fn a_free_note_is_found_above_whatever_is_taken() {
        let mut project = project_with_sample(1_000);
        let slice = project.add_slice(0, 500);
        project.assign(20, slice);
        project.add_modifier(21, Modifier::Brake, ModifierMode::Hold);

        assert_eq!(project.first_free_note(20), Some(22));
    }

    #[test]
    fn modifiers_survive_a_round_trip() {
        let mut project = Project::default();
        let note = project.modifiers()[0].note;
        project.set_modifier_mode(note, ModifierMode::Toggle);
        project.move_modifier(note, 24);
        let original = ProjectFile {
            version: PROJECT_VERSION,
            project,
        };

        let json = serde_json::to_string(&original).expect("serialization must succeed");
        let restored: ProjectFile = serde_json::from_str(&json).expect("must deserialize");

        assert_eq!(restored, original);
        let entry = restored
            .project
            .modifier_for_note(24)
            .expect("the moved modifier came back");
        assert_eq!(entry.mode, ModifierMode::Toggle);
    }

    #[test]
    fn state_without_a_modifier_layout_gets_the_default_one() {
        // Projects written before modifiers existed carry no such field.
        let restored: ProjectFile =
            serde_json::from_str(r#"{"version":1,"project":{}}"#).expect("must deserialize");

        assert_eq!(
            restored.project.modifiers().len(),
            crate::Modifier::DEFAULT_LAYOUT.len()
        );
    }

    /// A project with something changed in every corner a user can reach.
    ///
    /// Built field by field rather than with `..Default::default()` so that a
    /// newly added setting has to be added here too before this test can be
    /// said to cover the save format.
    fn fully_configured_project() -> Project {
        use crate::cell::PlaybackMode;
        use crate::effect::{DriveShape, FilterShape, SendEffects};
        use crate::modulation::{
            Division, EnvelopeDefinition, LfoDefinition, LfoShape, ModDestination, ModSource,
            ModulationRoute,
        };

        let mut project = project_with_sample(48_000);
        project.set_white_keys_only(true);
        project.slice_evenly(4);
        let slices: Vec<SliceId> = project.slices().iter().map(|slice| slice.id).collect();

        // Two cells on the same slice, set up differently: the one thing the
        // instrument exists to do, and the one thing a broken save format
        // would be most embarrassing to lose.
        let first = project.assign(60, slices[0]).expect("the slice exists");
        let second = project.assign(64, slices[0]).expect("the slice exists");
        project.assign(67, slices[2]).expect("the slice exists");

        project.set_playback(
            first,
            PlaybackSettings {
                reverse: true,
                speed: 0.5,
                pitch_semitones: -7.0,
                gain: 0.8,
                mode: PlaybackMode::Loop,
                division: Division::Sixteenth,
                collapse: 0.75,
                release_trigger: true,
            },
        );
        project.set_playback(
            second,
            PlaybackSettings {
                speed: 2.0,
                pitch_semitones: 12.0,
                ..Default::default()
            },
        );

        project.set_cell_effects(
            first,
            CellEffects {
                filter_on: true,
                filter_shape: FilterShape::BandPass,
                cutoff_hz: 900.0,
                resonance: 4.5,
                drive_on: true,
                drive_shape: DriveShape::Tube,
                drive: 9.0,
                delay_send: 0.3,
                reverb_send: 0.65,
                phaser_send: 0.1,
                flanger_send: 0.2,
            },
        );

        project.with_cell_mut(first, |cell| {
            cell.envelopes[0] = EnvelopeDefinition {
                attack_ms: 55.0,
                decay_ms: 120.0,
                sustain: 0.4,
                release_ms: 900.0,
            };
            cell.envelopes[1] = EnvelopeDefinition {
                attack_ms: 1.0,
                decay_ms: 5.0,
                sustain: 0.0,
                release_ms: 10.0,
            };
            cell.lfos[0] = LfoDefinition {
                shape: LfoShape::SampleHold,
                rate_hz: 7.5,
                sync: true,
                division: Division::ThirtySecond,
                retrigger: false,
            };
            cell.routes = vec![
                ModulationRoute {
                    source: ModSource::EnvelopeA,
                    destination: ModDestination::Volume,
                    amount: 1.0,
                },
                ModulationRoute {
                    source: ModSource::Lfo1,
                    destination: ModDestination::FilterCutoff,
                    amount: -0.6,
                },
                ModulationRoute {
                    source: ModSource::Velocity,
                    destination: ModDestination::Drive,
                    amount: 0.45,
                },
            ];
        });

        project.set_sends(SendRack {
            normal: SendEffects {
                delay_sync: false,
                delay_seconds: 0.33,
                delay_feedback: 0.7,
                delay_level: 0.9,
                reverb_size: 0.95,
                reverb_level: 0.15,
                phaser_feedback: 0.8,
                flanger_rate_hz: 5.0,
                ..SendEffects::default()
            },
            driven: SendEffects {
                delay_division: Division::ThirtySecond,
                reverb_damping: 0.9,
                phaser_level: 0.25,
                flanger_level: 1.0,
                ..SendEffects::default()
            },
            drive: 12.0,
            drive_shape: DriveShape::Hard,
        });

        let modifier_note = project.modifiers()[0].note;
        project.set_modifier_mode(modifier_note, ModifierMode::Toggle);
        project.move_modifier(modifier_note, 36);
        project.select(Some(slices[1]));
        project.select_cell(Some(second));
        project
    }

    #[test]
    fn everything_a_user_configures_survives_a_save_and_load() {
        // What the host does when the session is reopened. If this breaks, a
        // finished arrangement comes back wrong, which is the one failure
        // there is no way to work around from inside the plugin.
        let original = ProjectFile {
            version: PROJECT_VERSION,
            project: fully_configured_project(),
        };

        let json = serde_json::to_string(&original).expect("serialization must succeed");
        let mut restored: ProjectFile =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(restored.migrate(), Ok(()));

        assert_eq!(restored, original);
    }

    #[test]
    fn the_restored_cells_keep_their_own_settings() {
        use crate::cell::PlaybackMode;

        // Spelled out rather than left to the equality above, so a failure
        // says which setting was lost instead of printing two whole projects.
        let original = fully_configured_project();
        let json = serde_json::to_string(&ProjectFile {
            version: PROJECT_VERSION,
            project: original.clone(),
        })
        .expect("serialization must succeed");
        let restored: ProjectFile = serde_json::from_str(&json).expect("must deserialize");
        let project = restored.project;

        let first = project.cell_for_note(60).expect("the cell came back");
        assert!(first.playback.reverse);
        assert_eq!(first.playback.mode, PlaybackMode::Loop);
        assert_eq!(first.playback.collapse, 0.75);
        assert!(first.playback.release_trigger);
        assert!(first.effects.filter_on);
        assert_eq!(first.effects.cutoff_hz, 900.0);
        assert_eq!(first.effects.drive, 9.0);
        assert_eq!(first.effects.reverb_send, 0.65);
        assert_eq!(first.envelopes[0].release_ms, 900.0);
        assert_eq!(first.lfos[0].rate_hz, 7.5);
        assert_eq!(first.routes.len(), 3);

        let second = project.cell_for_note(64).expect("the cell came back");
        assert_eq!(
            first.slice, second.slice,
            "both cells still point at one slice"
        );
        assert_eq!(second.playback.pitch_semitones, 12.0);
        assert!(
            !second.playback.reverse,
            "the two cells did not share settings"
        );

        assert!(project.white_keys_only());
        assert_eq!(project.sends().drive, 12.0);
        assert_eq!(project.sends().normal.delay_seconds, 0.33);
        assert_eq!(project.sends().driven.flanger_level, 1.0);
        assert_eq!(
            project.modifier_for_note(36).map(|entry| entry.mode),
            Some(ModifierMode::Toggle)
        );
    }

    #[test]
    fn a_project_saved_before_the_sends_existed_still_loads() {
        // Every field added since has a default, so an older session opens
        // with the new settings at their defaults rather than failing.
        let restored: ProjectFile = serde_json::from_str(
            r#"{"version":1,"project":{"slices":[],"cells":[],"next_slice_id":0,"next_cell_id":0}}"#,
        )
        .expect("missing fields must fall back to defaults");

        assert_eq!(restored.project.sends(), SendRack::default());
        assert!(!restored.project.white_keys_only());
    }

    #[test]
    fn slicing_evenly_makes_exactly_as_many_as_asked_for() {
        // Integer division across an awkward frame count must not drop the
        // last marker, which is the kind of thing only a real length shows.
        for frames in [48_000, 114_720, 114_719, 100_001] {
            for count in [4u32, 8, 16, MAX_SLICES as u32] {
                let mut project = project_with_sample(frames);
                project.slice_evenly(count);
                assert_eq!(
                    project.slices().len(),
                    count as usize,
                    "{count} slices over {frames} frames"
                );
            }
        }
    }

    #[test]
    fn slicing_evenly_stops_at_the_limit() {
        // The automation bank has one slot per slice, so a project cannot hold
        // more slices than the bank has room for.
        let mut project = project_with_sample(48_000);

        project.slice_evenly(64);

        assert_eq!(project.slices().len(), MAX_SLICES);
    }

    #[test]
    fn a_split_is_refused_once_the_limit_is_reached() {
        let mut project = project_with_sample(48_000);
        project.slice_evenly(MAX_SLICES as u32);
        let first = project.slices()[0];
        let middle = (first.start_frame + first.end_frame) / 2;

        assert_eq!(project.split_slice(first.id, middle), None);
        assert_eq!(project.slices().len(), MAX_SLICES);
    }

    #[test]
    fn a_split_below_the_limit_still_works() {
        let mut project = project_with_sample(48_000);
        project.slice_evenly(4);
        let first = project.slices()[0];
        let middle = (first.start_frame + first.end_frame) / 2;

        assert!(project.split_slice(first.id, middle).is_some());
        assert_eq!(project.slices().len(), 5);
    }

    #[test]
    fn every_slice_owns_the_slot_its_marker_shows() {
        let mut project = project_with_sample(48_000);
        project.slice_evenly(MAX_SLICES as u32);

        for (index, slice) in project.slices().iter().enumerate() {
            assert_eq!(project.automation_slot(slice.id), Some(index as u8));
        }
    }

    #[test]
    fn two_cells_on_one_slice_share_its_slot() {
        // The bank is indexed by slice, which is what the interface labels.
        let mut project = project_with_sample(48_000);
        project.slice_evenly(4);
        let slice = project.slices()[1].id;
        project.assign(60, slice);
        project.assign(64, slice);

        let first = project.cell_for_note(60).expect("assigned");
        let second = project.cell_for_note(64).expect("assigned");
        assert_eq!(
            project.automation_slot(first.slice),
            project.automation_slot(second.slice)
        );
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
