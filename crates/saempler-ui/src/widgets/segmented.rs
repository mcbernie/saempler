use nih_plug_egui::egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Ui};

use crate::theme::Theme;
use crate::widgets::surface::{control_surface, label_color, label_size, SurfaceState};
use crate::widgets::texture::{self, SMALL_KEY_CORNER};

/// Room between the tray's edge and the keys in it.
const TRAY_PADDING: f32 = 2.5;

/// A momentary button, sized to its label.
pub fn button(ui: &mut Ui, theme: &Theme, label: &str) -> bool {
    let size = label_size(ui, theme, label);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let state = if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };

    control_surface(ui, theme, rect, state);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(theme.font_md),
        label_color(theme, state),
    );

    response.clicked()
}

/// A row of mutually exclusive options: small ivory keys in a dark tray.
///
/// The chosen key is the teal one. Returns the index that was clicked this
/// frame, if any. Pass `usize::MAX` as `selected` when none of the options is
/// the current value.
pub fn segmented(ui: &mut Ui, theme: &Theme, labels: &[&str], selected: usize) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }

    // Every key gets the width of the widest label, so the row reads as one
    // control rather than as buttons that happen to sit together.
    let key_width = labels
        .iter()
        .map(|label| label_size(ui, theme, label).x * 0.85)
        .fold(theme.control_height + 4.0, f32::max);
    let total = vec2(
        key_width * labels.len() as f32 + TRAY_PADDING * 2.0,
        theme.control_height,
    );
    // The id comes from the allocated area rather than from the enclosing Ui,
    // so several selectors in one row do not collide.
    let (rect, response) = ui.allocate_exact_size(total, Sense::hover());
    let id = response.id;
    let textures = texture::textures(ui.ctx());
    ui.painter().add(texture::nine_slice(
        &textures.key_tray,
        rect,
        SMALL_KEY_CORNER,
        Color32::WHITE,
    ));

    let mut clicked = None;
    for (index, label) in labels.iter().enumerate() {
        let bounds = Rect::from_min_size(
            pos2(
                rect.min.x + TRAY_PADDING + key_width * index as f32,
                rect.min.y + TRAY_PADDING,
            ),
            vec2(key_width, rect.height() - TRAY_PADDING * 2.0),
        );
        let response = ui.interact(bounds, id.with(("segment", index)), Sense::click());
        if response.clicked() {
            clicked = Some(index);
        }

        let chosen = index == selected;
        let pressed = response.is_pointer_button_down_on();
        let key = bounds.shrink2(vec2(1.0, 0.0));
        let (texture, key, tint) = if chosen {
            (&textures.key_teal, key, Color32::WHITE)
        } else if pressed {
            // Pushed into the tray: a point lower, out of the light.
            (
                &textures.key_ivory,
                key.translate(vec2(0.0, 1.0)),
                Color32::from_gray(225),
            )
        } else {
            (&textures.key_ivory, key, Color32::WHITE)
        };
        let painter = ui.painter();
        painter.add(texture::nine_slice(texture, key, SMALL_KEY_CORNER, tint));
        if response.hovered() && !chosen && !pressed {
            painter.rect_filled(
                key.shrink(1.5),
                theme.radius_sm,
                Color32::from_white_alpha(40),
            );
        }
        painter.text(
            key.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(theme.font_md),
            if chosen {
                theme.chassis_top
            } else {
                theme.title
            },
        );
    }

    clicked
}

/// A button that stays lit while its setting is on.
///
/// Returns true when it was clicked this frame; the caller owns the value.
pub fn toggle(ui: &mut Ui, theme: &Theme, label: &str, on: bool) -> bool {
    let size = label_size(ui, theme, label);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let state = if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if on {
        SurfaceState::Selected
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };

    control_surface(ui, theme, rect, state);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(theme.font_md),
        label_color(theme, state),
    );

    response.clicked()
}
