use nih_plug_egui::egui::{self, Align2, FontId, Ui};
use saempler_audio::EngineCommand;
use saempler_core::cell_spec;
use saempler_model::{
    note_name, PlaybackSettings, ProjectFile, MAX_PITCH_SEMITONES, MAX_SPEED, MIN_SPEED,
};

use crate::screens::main::{section, ViewState, THEME};
use crate::widgets::{button, performance_pad, value_knob, PadView, Taper, PAD_SIZE};

/// Note the automatic mapping starts at.
const BASE_NOTE: u8 = 60;
/// Diameter of the cell parameter knobs.
const KNOB_DIAMETER: f32 = 46.0;

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
    section(ui, "PERFORMANCE", |ui| {
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

        let playhead = state.meters.playhead();
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
                    // A pad lights up while the engine is inside its slice.
                    let sounding = match (playhead, slice) {
                        (Some(frame), Some(slice)) => slice.contains(frame),
                        _ => false,
                    };

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

        if button(ui, &THEME, "Slices auf Noten legen") {
            if let Ok(mut project) = state.project.lock() {
                project.project.map_slices_from(BASE_NOTE);
                let first = project.project.cells().first().map(|cell| cell.id);
                project.project.select_cell(first);
                sync_cells(state, &project);
            }
        }

        if button(ui, &THEME, "Kopie auf nächste Note") {
            copy_selected_to_next_note(state);
        }

        if button(ui, &THEME, "Alle Noten leeren") {
            if let Ok(mut project) = state.project.lock() {
                project.project.clear_cells();
                sync_cells(state, &project);
            }
        }

        ui.add_space(THEME.spacing_md);
        let count = state
            .project
            .lock()
            .map(|project| project.project.cells().len())
            .unwrap_or(0);
        hint(
            ui,
            &format!(
                "{count} Noten belegt  ·  ab {}  ·  Klick: vorhören  ·  Rechtsklick: Note leeren",
                note_name(BASE_NOTE)
            ),
        );
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
    let Some(cell) = project.project.selected_cell().copied() else {
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

/// Controls for the selected cell.
pub fn cell_section(ui: &mut Ui, state: &ViewState<'_>) {
    section(ui, "CELL", |ui| {
        let Ok(mut project) = state.project.lock() else {
            return;
        };
        let Some(cell) = project.project.selected_cell().copied() else {
            placeholder(ui, "Kein Pad gewählt");
            return;
        };

        let mut playback = cell.playback;
        let mut changed = false;

        ui.horizontal(|ui| {
            let reverse_label = if playback.reverse {
                "Reverse: an"
            } else {
                "Reverse: aus"
            };
            if button(ui, &THEME, reverse_label) {
                playback.reverse = !playback.reverse;
                changed = true;
            }

            ui.add_space(THEME.spacing_md);
            changed |= value_knob(
                ui,
                &THEME,
                "Speed",
                &mut playback.speed,
                (MIN_SPEED, MAX_SPEED),
                1.0,
                Taper::Logarithmic,
                KNOB_DIAMETER,
            );
            changed |= value_knob(
                ui,
                &THEME,
                "Pitch",
                &mut playback.pitch_semitones,
                (-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES),
                0.0,
                Taper::Linear,
                KNOB_DIAMETER,
            );
            changed |= value_knob(
                ui,
                &THEME,
                "Gain",
                &mut playback.gain,
                (0.0, 2.0),
                1.0,
                Taper::Linear,
                KNOB_DIAMETER,
            );
            changed |= value_knob(
                ui,
                &THEME,
                "Attack",
                &mut playback.attack_ms,
                (0.0, 500.0),
                PlaybackSettings::default().attack_ms,
                Taper::Linear,
                KNOB_DIAMETER,
            );
            changed |= value_knob(
                ui,
                &THEME,
                "Release",
                &mut playback.release_ms,
                (0.0, 2_000.0),
                PlaybackSettings::default().release_ms,
                Taper::Linear,
                KNOB_DIAMETER,
            );
        });

        if changed {
            project.project.set_playback(cell.id, playback);
            sync_cells(state, &project);
        }

        hint(
            ui,
            &format!(
                "{}  ·  Speed und Pitch wirken beide auf die Lesegeschwindigkeit, \
                 die Länge ändert sich also mit",
                note_name(cell.midi_note)
            ),
        );
    });
}

/// A dimmed line of explanatory text.
fn hint(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), THEME.font_sm * 1.6),
        egui::Sense::hover(),
    );
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );
}

/// Text standing in for a section that has nothing to show yet.
fn placeholder(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), PAD_SIZE * 0.5),
        egui::Sense::hover(),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(THEME.font_md),
        THEME.text_dim,
    );
}
