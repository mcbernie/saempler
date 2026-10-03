use nih_plug_egui::egui::{pos2, vec2, Align2, FontId, PointerButton, Sense, Ui};
use saempler_audio::EngineCommand;
use saempler_model::{note_name, Modifier, ModifierMode, ProjectFile};

use crate::screens::main::{hint, placeholder, section, ViewState, THEME};
use crate::widgets::{button, led, segmented};

/// Width of the note column in a modifier row.
const NOTE_WIDTH: f32 = 72.0;
/// Height of a row.
const ROW_HEIGHT: f32 = 34.0;
/// How many notes one pixel of a note drag is worth.
const NOTE_DRAG_SENSITIVITY: f32 = 0.08;

/// Push the modifier layout to the engine.
pub fn sync_modifiers(state: &ViewState<'_>, project: &ProjectFile) {
    let Ok(mut producer) = state.commands.lock() else {
        return;
    };

    let _ = producer.push(EngineCommand::ClearModifiers);
    for entry in project.project.modifiers() {
        let _ = producer.push(EngineCommand::SetModifier {
            note: entry.note,
            assignment: Some((entry.modifier, entry.mode)),
        });
    }
}

/// What a row asked for this frame.
enum RowEdit {
    Mode(u8, ModifierMode),
    Kind(u8, Modifier),
    Move(u8, u8),
    Remove(u8),
}

/// The modifier keys: which note, what it does, and how it responds.
pub fn modifier_section(ui: &mut Ui, state: &ViewState<'_>) {
    section(ui, "MODIFIER KEYS", None, |ui| {
        toolbar(ui, state);
        ui.add_space(THEME.spacing_sm);

        let engaged = state.meters.modifiers();
        let mut edit: Option<RowEdit> = None;

        {
            let Ok(project) = state.project.lock() else {
                return;
            };

            if project.project.modifiers().is_empty() {
                placeholder(ui, "Keine Modifier belegt");
            }

            for entry in project.project.modifiers() {
                let lit = engaged & (1 << entry.modifier.index()) != 0;
                if let Some(row) = modifier_row(ui, entry.note, entry.modifier, entry.mode, lit) {
                    edit = Some(row);
                }
            }
        }

        if let Some(edit) = edit {
            if let Ok(mut project) = state.project.lock() {
                let changed = match edit {
                    RowEdit::Mode(note, mode) => project.project.set_modifier_mode(note, mode),
                    RowEdit::Kind(note, modifier) => project.project.set_modifier(note, modifier),
                    RowEdit::Move(from, to) => project.project.move_modifier(from, to),
                    RowEdit::Remove(note) => project.project.remove_modifier(note),
                };
                if changed {
                    sync_modifiers(state, &project);
                }
            }
        }

        hint(
            ui,
            "Note ziehen verschiebt die Taste  ·  Rechtsklick auf die Note entfernt die Zeile",
        );
    });
}

/// Buttons above the list.
fn toolbar(ui: &mut Ui, state: &ViewState<'_>) {
    ui.horizontal(|ui| {
        if button(ui, &THEME, "Modifier hinzufügen") {
            if let Ok(mut project) = state.project.lock() {
                // Below the playing range, next to whatever is already there.
                let from = project
                    .project
                    .modifiers()
                    .last()
                    .map(|entry| entry.note.saturating_add(1))
                    .unwrap_or(saempler_model::MODIFIER_BASE_NOTE);
                if let Some(note) = project.project.first_free_note(from) {
                    project
                        .project
                        .add_modifier(note, Modifier::Reverse, ModifierMode::Hold);
                    sync_modifiers(state, &project);
                }
            }
        }

        if button(ui, &THEME, "Standardbelegung") {
            if let Ok(mut project) = state.project.lock() {
                project.project.reset_modifiers();
                sync_modifiers(state, &project);
            }
        }

        let count = state
            .project
            .lock()
            .map(|project| project.project.modifiers().len())
            .unwrap_or(0);
        ui.add_space(THEME.spacing_md);
        hint(ui, &format!("{count} Tasten belegt"));
    });
}

/// One row: lamp, note, what it does, how it responds.
fn modifier_row(
    ui: &mut Ui,
    note: u8,
    modifier: Modifier,
    mode: ModifierMode,
    engaged: bool,
) -> Option<RowEdit> {
    let mut edit = None;

    ui.horizontal(|ui| {
        let (lamp, _) = ui.allocate_exact_size(vec2(18.0, ROW_HEIGHT), Sense::hover());
        led(
            ui,
            &THEME,
            pos2(lamp.center().x, lamp.center().y),
            engaged.then_some(THEME.active),
        );

        // Dragging the note is the quickest way to move a key, and it needs
        // no second interaction mode for picking one.
        let (note_rect, response) =
            ui.allocate_exact_size(vec2(NOTE_WIDTH, ROW_HEIGHT), Sense::click_and_drag());
        let hovered = response.hovered() || response.dragged();
        ui.painter().rect_filled(
            note_rect.shrink(2.0),
            THEME.radius_sm,
            if hovered {
                THEME.control_hover_bg
            } else {
                THEME.control_pressed_bg
            },
        );
        ui.painter().text(
            note_rect.center(),
            Align2::CENTER_CENTER,
            note_name(note),
            FontId::proportional(THEME.font_md),
            if engaged { THEME.accent } else { THEME.text },
        );

        if response.dragged() {
            let steps = -response.drag_delta().y * NOTE_DRAG_SENSITIVITY;
            let target = (note as f32 + steps).round().clamp(0.0, 127.0) as u8;
            if target != note {
                edit = Some(RowEdit::Move(note, target));
            }
        }
        if response.clicked_by(PointerButton::Secondary) {
            edit = Some(RowEdit::Remove(note));
        }

        ui.add_space(THEME.spacing_sm);

        let names: Vec<&str> = Modifier::ALL.iter().map(|m| m.label()).collect();
        let selected = Modifier::ALL
            .iter()
            .position(|m| *m == modifier)
            .unwrap_or(0);
        if let Some(index) = segmented(ui, &THEME, &names, selected) {
            edit = Some(RowEdit::Kind(note, Modifier::ALL[index]));
        }

        ui.add_space(THEME.spacing_sm);

        let modes: Vec<&str> = ModifierMode::ALL.iter().map(|m| m.label()).collect();
        let selected = ModifierMode::ALL
            .iter()
            .position(|m| *m == mode)
            .unwrap_or(0);
        if let Some(index) = segmented(ui, &THEME, &modes, selected) {
            edit = Some(RowEdit::Mode(note, ModifierMode::ALL[index]));
        }
    });

    edit
}
