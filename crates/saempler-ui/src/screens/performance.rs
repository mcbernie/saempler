use nih_plug_egui::egui::Ui;
use saempler_audio::EngineCommand;
use saempler_core::cell_spec;
use saempler_model::ProjectFile;

use crate::screens::main::{placeholder, section, ViewState, THEME};
use crate::widgets::{button, performance_pad, PadView, PAD_SIZE};

/// Note the automatic mapping starts at.
const BASE_NOTE: u8 = 60;

/// Push the whole keyboard mapping to the engine.
///
/// A full resync rather than a difference: the mapping only changes on a user
/// edit, and 128 notes plus the clearing command fit in one queue, so there is
/// nothing to gain from tracking what moved.
pub fn sync_cells(state: &ViewState<'_>, project: &ProjectFile) {
    let Ok(mut producer) = state.commands.lock() else {
        return;
    };

    let _ = producer.push(EngineCommand::ClearCells);
    for cell in project.project.cells() {
        if let Some(spec) = cell_spec(&project.project, cell) {
            let _ = producer.push(EngineCommand::SetCell {
                note: cell.midi_note,
                spec: Some(spec),
            });
        }
    }
}

/// The pad grid: which note plays which slice, and how.
pub fn performance_section(ui: &mut Ui, state: &ViewState<'_>) {
    let sounding = state.meters.any_playhead().then_some(THEME.active);
    section(ui, "PERFORMANCE", sounding, |ui| {
        // Down to the column's floor, whatever the grid needs: the panel's
        // lower edge lines up with the matrix across the aisle, which is
        // what keeps the picture calm.
        ui.set_min_height(ui.available_height() - 12.0);
        toolbar(ui, state);
        ui.add_space(THEME.spacing_sm);

        let Ok(mut project) = state.project.lock() else {
            return;
        };
        let Ok(sample) = state.sample.lock() else {
            return;
        };

        if project.project.cells().is_empty() {
            placeholder(ui, "Noch keine Noten belegt");
            return;
        }

        let playheads: Vec<u64> = state.meters.playheads().collect();
        let mut edit: Option<PadEdit> = None;

        let available = ui.available_width();
        let per_row = ((available / (PAD_SIZE + THEME.spacing_sm)).floor() as usize).max(1);

        for row in project.project.cells().chunks(per_row) {
            ui.horizontal(|ui| {
                for cell in row {
                    let slice = project.project.slice(cell.slice).copied();
                    let index = slice
                        .and_then(|slice| {
                            project
                                .project
                                .slices()
                                .iter()
                                .position(|candidate| candidate.id == slice.id)
                        })
                        .unwrap_or(0);
                    // A pad lights up while any voice is inside its slice, so
                    // notes played together all light up at once.
                    let sounding = slice
                        .map(|slice| playheads.iter().any(|frame| slice.contains(*frame)))
                        .unwrap_or(false);

                    let action = performance_pad(
                        ui,
                        &THEME,
                        &PadView {
                            cell,
                            slice: slice.as_ref(),
                            slice_index: index,
                            peaks: &sample.peaks,
                            selected: project.project.cell_selection() == Some(cell.id),
                            sounding,
                        },
                    );

                    if action.clear {
                        edit = Some(PadEdit::Clear(cell.midi_note));
                    } else if action.trigger {
                        edit = Some(PadEdit::Trigger(cell.id));
                    }
                }
            });
        }

        match edit {
            Some(PadEdit::Clear(note)) => {
                if project.project.clear_note(note) {
                    sync_cells(state, &project);
                }
            }
            Some(PadEdit::Trigger(id)) => {
                project.project.select_cell(Some(id));
                // The waveform above follows: its highlighted span is the
                // chop this pad plays.
                if let Some(slice) = project.project.cell(id).map(|cell| cell.slice) {
                    project.project.select(Some(slice));
                }
                if let Some(cell) = project.project.cell(id) {
                    if let Some(spec) = cell_spec(&project.project, cell) {
                        state.send(EngineCommand::Preview(spec));
                    }
                }
            }
            None => {}
        }
    });
}

/// What a pad asked for this frame.
enum PadEdit {
    Trigger(saempler_model::CellId),
    Clear(u8),
}

/// Mapping controls above the pads.
fn toolbar(ui: &mut Ui, state: &ViewState<'_>) {
    ui.horizontal(|ui| {
        let has_slices = state
            .project
            .lock()
            .map(|project| !project.project.slices().is_empty())
            .unwrap_or(false);
        if !has_slices {
            return;
        }

        if button(ui, &THEME, "Auf Noten legen") {
            if let Ok(mut project) = state.project.lock() {
                project.project.map_slices_from(BASE_NOTE);
                let first = project.project.cells().first().map(|cell| cell.id);
                project.project.select_cell(first);
                sync_cells(state, &project);
            }
        }

        if button(ui, &THEME, "Duplizieren") {
            copy_selected_to_next_note(state);
        }

        if button(ui, &THEME, "Leeren") {
            if let Ok(mut project) = state.project.lock() {
                project.project.clear_cells();
                sync_cells(state, &project);
            }
        }
    });
}

/// Duplicate the selected cell one semitone up, keeping its settings.
///
/// This is the quickest route to the idea the instrument is built on: the
/// same chop on the next key, then changed.
fn copy_selected_to_next_note(state: &ViewState<'_>) {
    let Ok(mut project) = state.project.lock() else {
        return;
    };
    let Some(cell) = project.project.selected_cell().cloned() else {
        return;
    };
    let Some(target) = cell.midi_note.checked_add(1).filter(|note| *note <= 127) else {
        return;
    };

    if let Some(copy) = project.project.copy_cell_to_note(cell.id, target) {
        project.project.select_cell(Some(copy));
        sync_cells(state, &project);
    }
}
