use nih_plug_egui::egui::{Color32, FontId, Rect, Stroke, StrokeKind, Ui, Vec2};

use crate::theme::Theme;
use crate::widgets::panel::{raised_body, vertical_gradient};

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

/// Horizontal padding around a control's label.
pub const LABEL_PADDING: f32 = 14.0;

/// Draw a control surface: a two-tone fill, a lit top edge and a border.
///
/// Every interactive surface in the interface goes through this, so buttons,
/// segments and pads share one physical look instead of each inventing one.
pub fn control_surface(ui: &Ui, theme: &Theme, rect: Rect, state: SurfaceState) {
    let painter = ui.painter();
    let radius = theme.radius_sm;

    let (base, top, border) = match state {
        SurfaceState::Rest => (theme.control_bg, theme.control_top, Color32::BLACK),
        SurfaceState::Hover => (
            theme.control_hover_bg,
            theme.control_hover_top,
            Color32::BLACK,
        ),
        SurfaceState::Pressed => (
            theme.control_pressed_bg,
            theme.control_pressed_bg,
            theme.accent,
        ),
        SurfaceState::Selected => (
            theme.control_selected_bg,
            theme.control_selected_top,
            theme.accent,
        ),
    };

    if state == SurfaceState::Pressed {
        // Pushed in: no shadow under it, a dark lip above and a light one
        // below, which is the opposite of a raised key.
        painter.rect_filled(rect, radius, base);
        painter.add(vertical_gradient(
            rect.shrink(1.0),
            &[
                (0.0, Color32::from_black_alpha(120)),
                (0.5, Color32::TRANSPARENT),
                (1.0, Color32::from_white_alpha(28)),
            ],
        ));
    } else {
        raised_body(painter, theme, rect, radius, base, top);
    }

    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(theme.stroke_thin, border),
        StrokeKind::Inside,
    );
}

/// Text colour that belongs to a surface state.
pub fn label_color(theme: &Theme, state: SurfaceState) -> Color32 {
    match state {
        SurfaceState::Rest => theme.text,
        SurfaceState::Hover => theme.text,
        SurfaceState::Pressed | SurfaceState::Selected => theme.accent,
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

    Vec2::new(
        galley.size().x + LABEL_PADDING * 2.0,
        theme.font_md + theme.spacing_md * 2.0,
    )
}
