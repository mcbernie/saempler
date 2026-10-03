use nih_plug_egui::egui::{self, pos2, vec2, Align2, FontId, Id, Sense, Ui};
use saempler_audio::EngineCommand;
use saempler_model::{note_name, Modifier, ModifierMode, ProjectFile};

use crate::screens::main::{section_with, ViewState, THEME};
use crate::widgets::{button, dropdown, icon_button, inset, lamp, Icon};

/// Keys a modifier may be put on.
///
/// The whole keyboard would be a list of 128 entries to scroll through. These
/// four octaves sit below where chops are usually mapped, which is where
/// modifier keys belong.
const NOTE_RANGE: std::ops::Range<u8> = 24..72;

/// Width of one display card in the footer.
const CARD_WIDTH: f32 = 104.0;
/// Height of one display card.
const CARD_HEIGHT: f32 = 46.0;

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

/// What the editor window asked for this frame.
enum Edit {
    Mode(u8, ModifierMode),
    Kind(u8, Modifier),
    Move(u8, u8),
    Remove(u8),
    Add,
    Reset,
}

/// Memory key for whether the editor window is open.
fn editor_open_id() -> Id {
    Id::new("modifier-editor-open")
}

/// The modifier keys in the footer: a lamp and a label per key.
///
/// The cards only show what the keys do; changing them happens in a small
/// window of its own, so the footer stays the size of a status strip.
pub fn modifier_section(ui: &mut Ui, state: &ViewState<'_>) {
    let engaged = state.meters.modifiers();
    let mut open = ui.memory(|memory| memory.data.get_temp(editor_open_id()).unwrap_or(false));
    let mut toggle_editor = false;

    let lit = (engaged != 0).then_some(THEME.active);
    section_with(
        ui,
        "MODIFIERS",
        lit,
        |ui| {
            if icon_button(ui, &THEME, Icon::Edit, "Modifier-Tasten bearbeiten") {
                toggle_editor = true;
            }
        },
        |ui| {
            let Ok(project) = state.project.lock() else {
                return;
            };

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = THEME.spacing_md;
                let total = project.project.modifiers().len();
                for (shown, entry) in project.project.modifiers().iter().enumerate() {
                    // A card that no longer fits is counted rather than drawn:
                    // painted over the neighbouring panel it would look
                    // broken, and the window has the full list anyway.
                    if ui.available_width() < CARD_WIDTH + 30.0 {
                        ui.painter().text(
                            ui.cursor().min + vec2(2.0, CARD_HEIGHT * 0.5),
                            Align2::LEFT_CENTER,
                            format!("+{}", total - shown),
                            FontId::proportional(THEME.font_md),
                            THEME.label,
                        );
                        break;
                    }
                    let active = engaged & (1 << entry.modifier.index()) != 0;
                    card(ui, entry.note, entry.modifier, entry.mode, active);
                }

                if total == 0 {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(CARD_WIDTH * 2.0, CARD_HEIGHT), Sense::hover());
                    inset(ui.painter(), &THEME, rect, THEME.waveform_bg);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "Keine Modifier belegt",
                        FontId::proportional(THEME.font_sm),
                        THEME.text_dim,
                    );
                }
            });
        },
    );

    if toggle_editor {
        open = !open;
    }
    if open {
        open = editor_window(ui, state, engaged);
    }
    ui.memory_mut(|memory| memory.data.insert_temp(editor_open_id(), open));
}

/// One key's display card: lamp, what it does, which key, how it responds.
fn card(ui: &mut Ui, note: u8, modifier: Modifier, mode: ModifierMode, engaged: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(CARD_WIDTH, CARD_HEIGHT), Sense::hover());
    inset(ui.painter(), &THEME, rect, THEME.control_pressed_bg);

    let painter = ui.painter();
    lamp(
        painter,
        &THEME,
        pos2(rect.min.x + 12.0, rect.min.y + 14.0),
        engaged.then_some(THEME.active),
    );
    painter.text(
        pos2(rect.min.x + 26.0, rect.min.y + 14.0),
        Align2::LEFT_CENTER,
        modifier.label(),
        FontId::proportional(THEME.font_sm),
        if engaged { THEME.active } else { THEME.text },
    );
    painter.text(
        pos2(rect.min.x + 26.0, rect.max.y - 13.0),
        Align2::LEFT_CENTER,
        format!("{}  ·  {}", note_name(note), mode.label()),
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );
}

/// The window the keys are configured in. Returns whether it stays open.
fn editor_window(ui: &Ui, state: &ViewState<'_>, engaged: u32) -> bool {
    let mut open = true;
    let mut edit: Option<Edit> = None;

    egui::Window::new("Modifier-Tasten")
        .id(Id::new("modifier-editor"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(pos2(360.0, 420.0))
        .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(THEME.spacing_sm, THEME.spacing_sm);

            let Ok(project) = state.project.lock() else {
                return;
            };

            for (index, entry) in project.project.modifiers().iter().enumerate() {
                ui.horizontal(|ui| {
                    let active = engaged & (1 << entry.modifier.index()) != 0;
                    let (bezel, _) = ui.allocate_exact_size(vec2(20.0, 28.0), Sense::hover());
                    lamp(
                        ui.painter(),
                        &THEME,
                        bezel.center(),
                        active.then_some(THEME.active),
                    );

                    let kinds: Vec<&str> = Modifier::ALL.iter().map(|kind| kind.label()).collect();
                    let selected = Modifier::ALL
                        .iter()
                        .position(|kind| *kind == entry.modifier)
                        .unwrap_or(0);
                    if let Some(next) =
                        dropdown(ui, &THEME, ("mod-kind", index), &kinds, selected, 104.0)
                    {
                        edit = Some(Edit::Kind(entry.note, Modifier::ALL[next]));
                    }

                    let names: Vec<String> = NOTE_RANGE.map(note_name).collect();
                    let labels: Vec<&str> = names.iter().map(String::as_str).collect();
                    let selected = usize::from(entry.note.saturating_sub(NOTE_RANGE.start));
                    if let Some(chosen) =
                        dropdown(ui, &THEME, ("mod-note", index), &labels, selected, 64.0)
                    {
                        let target = NOTE_RANGE.start + chosen as u8;
                        if target != entry.note {
                            edit = Some(Edit::Move(entry.note, target));
                        }
                    }

                    let modes: Vec<&str> =
                        ModifierMode::ALL.iter().map(|mode| mode.label()).collect();
                    let selected = ModifierMode::ALL
                        .iter()
                        .position(|candidate| *candidate == entry.mode)
                        .unwrap_or(0);
                    if let Some(chosen) =
                        dropdown(ui, &THEME, ("mod-mode", index), &modes, selected, 80.0)
                    {
                        edit = Some(Edit::Mode(entry.note, ModifierMode::ALL[chosen]));
                    }

                    if icon_button(ui, &THEME, Icon::Cross, "Diese Taste entfernen") {
                        edit = Some(Edit::Remove(entry.note));
                    }
                });
            }

            ui.add_space(THEME.spacing_sm);
            ui.horizontal(|ui| {
                if icon_button(ui, &THEME, Icon::Plus, "Taste hinzufügen") {
                    edit = Some(Edit::Add);
                }
                if button(ui, &THEME, "Standardbelegung") {
                    edit = Some(Edit::Reset);
                }
            });
        });

    if let Some(edit) = edit {
        if let Ok(mut project) = state.project.lock() {
            let changed = match edit {
                Edit::Mode(note, mode) => project.project.set_modifier_mode(note, mode),
                Edit::Kind(note, modifier) => project.project.set_modifier(note, modifier),
                Edit::Move(from, to) => project.project.move_modifier(from, to),
                Edit::Remove(note) => project.project.remove_modifier(note),
                Edit::Add => add_modifier(&mut project),
                Edit::Reset => {
                    project.project.reset_modifiers();
                    true
                }
            };
            if changed {
                sync_modifiers(state, &project);
            }
        }
    }

    open
}

/// Put a new modifier on the next free key below the playing range.
fn add_modifier(project: &mut ProjectFile) -> bool {
    let from = project
        .project
        .modifiers()
        .last()
        .map(|entry| entry.note.saturating_add(1))
        .unwrap_or(saempler_model::MODIFIER_BASE_NOTE);
    match project.project.first_free_note(from) {
        Some(note) => project
            .project
            .add_modifier(note, Modifier::Reverse, ModifierMode::Hold),
        None => false,
    }
}
