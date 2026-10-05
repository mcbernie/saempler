use nih_plug_egui::egui::{ScrollArea, Ui};
use saempler_audio::EngineCommand;
use saempler_core::cell_spec;
use saempler_model::ProjectFile;

use crate::screens::main::{placeholder, section, ViewState, THEME};
use crate::widgets::{icon_button, performance_pad, toggle, Icon, PadView, MIN_PAD_SIZE, PAD_SIZE};

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
        ui.add_space(THEME.spacing_sm);
        toolbar(ui, state);
        ui.add_space(THEME.spacing_sm);

        let Ok(mut project) = state.project.lock() else {
            return;
        };
        let Ok(sample) = state.sample.lock() else {
            return;
        };

        if project.project.cells().is_empty() {
            let detail = if project.project.sample.is_some() {
                "Mit der Tastatur-Taste oben die Slices auf Noten legen"
            } else {
                "Erst ein Sample laden, dann die Slices auf Noten legen"
            };
            placeholder(ui, "Noch keine Noten belegt", detail);
            return;
        }

        let sounding_notes: Vec<u8> = state.meters.voices().map(|(note, _)| note).collect();
        let mut edit: Option<PadEdit> = None;

        let dragged: Option<saempler_model::CellId> =
            ui.memory(|memory| memory.data.get_temp(dragged_id()));
        let pointer = ui.ctx().pointer_interact_pos();
        // The key a drag is over, from the frame before: the pads work it out
        // as they are drawn, and the one being hovered has to glow on the same
        // frame the pointer is over it.
        let target: Option<u8> = dragged.and_then(|_| ui.memory(|m| m.data.get_temp(target_id())));
        let mut hovered_note: Option<u8> = None;
        let mut released = false;

        let (size, per_row, fits) = grid(
            ui.available_width(),
            ui.available_height(),
            project.project.cells().len(),
        );

        let mut rows = |ui: &mut Ui| {
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
                        // By note rather than by position: a copied cell plays the
                        // same slice, so a pad that lit for anything inside its
                        // region lit for its twin as well.
                        let sounding = sounding_notes.contains(&cell.midi_note);

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
                                size,
                                dragging: dragged == Some(cell.id),
                                drop_target: target == Some(cell.midi_note)
                                    && dragged != Some(cell.id),
                            },
                        );

                        if let Some(position) = pointer {
                            if action.rect.contains(position) {
                                hovered_note = Some(cell.midi_note);
                            }
                        }
                        if action.drag_started {
                            ui.memory_mut(|memory| memory.data.insert_temp(dragged_id(), cell.id));
                        }
                        if action.drag_released {
                            released = true;
                        }

                        if action.clear {
                            edit = Some(PadEdit::Clear(cell.midi_note));
                        } else if action.trigger {
                            edit = Some(PadEdit::Trigger(cell.id));
                        }
                    }
                });
            }
        };
        // Pads that would have to shrink past the point where their display
        // still says anything scroll instead.
        if fits {
            rows(ui);
        } else {
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| rows(ui));
        }

        match hovered_note {
            Some(note) => ui.memory_mut(|m| m.data.insert_temp(target_id(), note)),
            None => ui.memory_mut(|m| m.data.remove::<u8>(target_id())),
        }
        if released {
            if let (Some(id), Some(note)) = (dragged, hovered_note) {
                edit = Some(PadEdit::Move(id, note));
            }
            ui.memory_mut(|memory| {
                memory.data.remove::<saempler_model::CellId>(dragged_id());
                memory.data.remove::<u8>(target_id());
            });
        }

        match edit {
            Some(PadEdit::Move(id, note)) => {
                if project.project.move_cell_to_note(id, note) {
                    project.project.select_cell(Some(id));
                    sync_cells(state, &project);
                }
            }
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

/// Pad size and row length for `count` pads in the given area, and whether
/// they fit without scrolling.
///
/// The grid shrinks its pads first: a key that is mapped but out of sight is
/// worse than a small one. Only once a pad is too small to read does it give
/// up and let the grid scroll.
fn grid(width: f32, height: f32, count: usize) -> (f32, usize, bool) {
    let gap = THEME.spacing_md;
    let mut size = PAD_SIZE;
    loop {
        let per_row = (((width + gap) / (size + gap)).floor() as usize).max(1);
        let rows = count.div_ceil(per_row);
        let fits = rows as f32 * (size + gap) <= height;
        if fits || size <= MIN_PAD_SIZE {
            return (size, per_row, fits);
        }
        size -= 2.0;
    }
}

/// What a pad asked for this frame.
enum PadEdit {
    Trigger(saempler_model::CellId),
    Clear(u8),
    /// A pad was dragged onto a key: move it there, swapping if that key is
    /// taken. Dragging is how a mapping is rearranged.
    Move(saempler_model::CellId, u8),
}

/// Memory key for the cell a drag is carrying.
fn dragged_id() -> nih_plug_egui::egui::Id {
    nih_plug_egui::egui::Id::new("pad-drag")
}

/// Memory key for the key a drag is currently over.
fn target_id() -> nih_plug_egui::egui::Id {
    nih_plug_egui::egui::Id::new("pad-drag-target")
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

        if icon_button(ui, &THEME, Icon::Keyboard, "Alle Slices auf Noten legen") {
            if let Ok(mut project) = state.project.lock() {
                project.project.map_slices_from(BASE_NOTE);
                let first = project.project.cells().first().map(|cell| cell.id);
                project.project.select_cell(first);
                sync_cells(state, &project);
            }
        }

        if icon_button(
            ui,
            &THEME,
            Icon::Copy,
            "Gewählte Cell auf die nächste Note kopieren",
        ) {
            copy_selected_to_next_note(state);
        }

        ui.add_space(THEME.spacing_md);
        let white_only = state
            .project
            .lock()
            .map(|project| project.project.white_keys_only())
            .unwrap_or(false);
        if toggle(ui, &THEME, "Nur weiße Tasten", white_only) {
            if let Ok(mut project) = state.project.lock() {
                project.project.set_white_keys_only(!white_only);
            }
        }
        ui.add_space(THEME.spacing_md);

        if icon_button(ui, &THEME, Icon::Clear, "Alle Noten leeren") {
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
    let Some(from) = cell.midi_note.checked_add(1) else {
        return;
    };

    // The first free key above, not the next one along: duplicating onto a
    // key that is already performed would quietly replace it.
    if let Some(copy) = project.project.copy_cell_to_free_note(cell.id, from) {
        project.project.select_cell(Some(copy));
        sync_cells(state, &project);
    }
}
