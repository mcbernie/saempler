use nih_plug_egui::egui::{
    pos2, Color32, CornerRadius, FontId, Rect, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::Theme;

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
        SurfaceState::Rest => (theme.control_bg, theme.control_top, theme.outline),
        SurfaceState::Hover => (
            theme.control_hover_bg,
            theme.control_hover_top,
            theme.outline,
        ),
        // Pressed reads as pushed in: the lit edge goes away and the fill
        // drops below the surrounding panel.
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

    painter.rect_filled(rect, radius, base);

    // Upper half in the lighter tone, flat along the middle so the two tones
    // meet in a straight line rather than a visible seam.
    if top != base {
        let upper = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.center().y));
        painter.rect_filled(
            upper,
            CornerRadius {
                nw: radius.nw,
                ne: radius.ne,
                sw: 0,
                se: 0,
            },
            top,
        );
    }

    if state != SurfaceState::Pressed {
        // A single lit pixel along the top edge gives the surface its relief.
        let inset = f32::from(radius.nw).max(1.0);
        painter.line_segment(
            [
                pos2(rect.min.x + inset, rect.min.y + 0.5),
                pos2(rect.max.x - inset, rect.min.y + 0.5),
            ],
            Stroke::new(1.0, theme.control_highlight),
        );
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
