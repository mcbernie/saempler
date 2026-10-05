use nih_plug_egui::egui::{Color32, FontId, Rect, Stroke, StrokeKind, Ui, Vec2};

use crate::theme::Theme;
use crate::widgets::texture::{self, KEY_CORNER};

/// How a control looks right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceState {
    Rest,
    Hover,
    /// Held down by the pointer.
    Pressed,
    /// Carrying the current value, such as the chosen segment.
    Selected,
}

/// Width of the bezel round a key's cap, in points.
const KEY_BEZEL: f32 = 2.5;

/// Horizontal padding around a control's label.
pub const LABEL_PADDING: f32 = 10.0;

/// Draw a control surface: a dark key in its bezel, cut from the render.
///
/// Every interactive surface in the interface goes through this, so buttons,
/// lists and modifier keys share one physical look instead of each inventing
/// one. Held down, the key sits deeper in its bezel; carrying the current
/// value, its edge glows.
pub fn control_surface(ui: &Ui, theme: &Theme, rect: Rect, state: SurfaceState) {
    let textures = texture::textures(ui.ctx());
    let key = match state {
        SurfaceState::Pressed => &textures.key_pressed,
        _ => &textures.key,
    };
    let painter = ui.painter();
    painter.add(texture::nine_slice(key, rect, KEY_CORNER, Color32::WHITE));
    match state {
        SurfaceState::Hover => {
            painter.rect_filled(
                rect.shrink(2.0),
                theme.radius_sm,
                Color32::from_white_alpha(8),
            );
        }
        // Engaged: a teal line round the cap, inside the bezel, with a
        // faint glow off it.
        SurfaceState::Selected => {
            let cap = rect.shrink(KEY_BEZEL);
            painter.rect_stroke(
                cap.expand(1.0),
                theme.radius_sm,
                Stroke::new(2.0_f32, theme.accent.gamma_multiply(0.25)),
                StrokeKind::Middle,
            );
            painter.rect_stroke(
                cap,
                theme.radius_sm,
                Stroke::new(1.4_f32, theme.accent),
                StrokeKind::Middle,
            );
        }
        SurfaceState::Rest | SurfaceState::Pressed => {}
    }
}

/// Text colour that belongs to a surface state.
pub fn label_color(theme: &Theme, state: SurfaceState) -> Color32 {
    match state {
        SurfaceState::Rest => theme.text,
        SurfaceState::Hover => theme.text,
        SurfaceState::Pressed | SurfaceState::Selected => theme.text,
    }
}

/// Size a control needs to show `label` comfortably.
pub fn label_size(ui: &Ui, theme: &Theme, label: &str) -> Vec2 {
    let galley = ui.fonts(|fonts| {
        fonts.layout_no_wrap(
            label.to_owned(),
            FontId::proportional(theme.font_md),
            theme.text,
        )
    });

    Vec2::new(galley.size().x + LABEL_PADDING * 2.0, theme.control_height)
}
