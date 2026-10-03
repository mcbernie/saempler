use nih_plug_egui::egui::{
    pos2, vec2, Align2, FontId, PointerButton, Sense, Stroke, StrokeKind, Ui,
};
use saempler_audio::EngineCommand;
use saempler_model::{note_name, Modifier, ModifierMode, ProjectFile};

use crate::screens::main::{section, ViewState, THEME};
use crate::widgets::surface::{control_surface, SurfaceState};

/// Size of one modifier pad.
const PAD_WIDTH: f32 = 112.0;
const PAD_HEIGHT: f32 = 62.0;

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

/// The modifier keys and what they are set to do.
pub fn modifier_section(ui: &mut Ui, state: &ViewState<'_>) {
    section(ui, "MODIFIERS", |ui| {
        let engaged = state.meters.modifiers();
        let mut cycled: Option<(Modifier, ModifierMode)> = None;

        {
            let Ok(project) = state.project.lock() else {
                return;
            };

            ui.horizontal(|ui| {
                for entry in project.project.modifiers() {
                    let is_engaged = engaged & (1 << entry.modifier.index()) != 0;
                    if modifier_pad(ui, entry.modifier, entry.note, entry.mode, is_engaged) {
                        cycled = Some((entry.modifier, entry.mode.next()));
                    }
                }
            });
        }

        if let Some((modifier, mode)) = cycled {
            if let Ok(mut project) = state.project.lock() {
                project.project.set_modifier_mode(modifier, mode);
                sync_modifiers(state, &project);
            }
        }

        hint(
            ui,
            "Klick wechselt den Modus  ·  Hold: solange gehalten  ·  \
             Toggle: bis zum nächsten Druck  ·  One Shot: nur die nächste Note",
        );
    });
}

/// Draw one modifier pad. Returns true when it was clicked.
fn modifier_pad(
    ui: &mut Ui,
    modifier: Modifier,
    note: u8,
    mode: ModifierMode,
    engaged: bool,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(PAD_WIDTH, PAD_HEIGHT), Sense::click_and_drag());

    let state = if engaged {
        SurfaceState::Selected
    } else if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };
    control_surface(ui, &THEME, rect, state);

    let painter = ui.painter();
    painter.text(
        pos2(rect.min.x + THEME.spacing_md, rect.min.y + THEME.spacing_md),
        Align2::LEFT_TOP,
        modifier.label(),
        FontId::proportional(THEME.font_md),
        if engaged { THEME.accent } else { THEME.text },
    );
    painter.text(
        pos2(rect.min.x + THEME.spacing_md, rect.max.y - THEME.spacing_md),
        Align2::LEFT_BOTTOM,
        mode.label(),
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );
    painter.text(
        pos2(rect.max.x - THEME.spacing_md, rect.max.y - THEME.spacing_md),
        Align2::RIGHT_BOTTOM,
        note_name(note),
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );

    // A lamp is easier to read at a glance than a change of fill alone.
    let lamp = pos2(
        rect.max.x - THEME.spacing_md - 4.0,
        rect.min.y + THEME.spacing_md + 4.0,
    );
    painter.circle_filled(
        lamp,
        4.0,
        if engaged {
            THEME.active
        } else {
            THEME.control_pressed_bg
        },
    );
    if engaged {
        painter.rect_stroke(
            rect,
            THEME.radius_sm,
            Stroke::new(THEME.stroke_thick, THEME.active),
            StrokeKind::Inside,
        );
    }

    response.clicked() && !response.clicked_by(PointerButton::Secondary)
}

/// A dimmed line of explanatory text.
fn hint(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), THEME.font_sm * 1.6),
        Sense::hover(),
    );
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );
}
