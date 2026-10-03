use nih_plug_egui::egui::{pos2, vec2, Sense, Ui};
use saempler_audio::EngineCommand;
use saempler_model::{note_name, Modifier, ModifierMode, ProjectFile};

use crate::screens::main::{hint, placeholder, section, ViewState, THEME};
use crate::widgets::{button, dropdown, lamp, segmented};

/// Keys a modifier may be put on.
///
/// The whole keyboard would be a list of 128 entries to scroll through. These
/// four octaves sit below where chops are usually mapped, which is where
/// modifier keys belong.
const NOTE_RANGE: std::ops::Range<u8> = 24..72;

/// Width of the note column in a modifier row.
const NOTE_WIDTH: f32 = 72.0;
/// Height of a row.
const ROW_HEIGHT: f32 = 34.0;

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

        hint(ui, "Taste aus der Liste wählen  ·  × entfernt die Zeile");
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
        let (bezel, _) = ui.allocate_exact_size(vec2(18.0, ROW_HEIGHT), Sense::hover());
        lamp(
            ui.painter(),
            &THEME,
            pos2(bezel.center().x, bezel.center().y),
            engaged.then_some(THEME.active),
        );

        // The key is picked from a list rather than only dragged: dragging is
        // quick once you know it is there, and invisible until then.
        let names: Vec<String> = NOTE_RANGE.map(note_name).collect();
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        let selected = usize::from(note.saturating_sub(NOTE_RANGE.start));
        if let Some(index) = dropdown(ui, &THEME, ("note", note), &labels, selected, NOTE_WIDTH) {
            let target = NOTE_RANGE.start + index as u8;
            if target != note {
                edit = Some(RowEdit::Move(note, target));
            }
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

        ui.add_space(THEME.spacing_sm);
        if button(ui, &THEME, "×") {
            edit = Some(RowEdit::Remove(note));
        }
    });

    edit
}
