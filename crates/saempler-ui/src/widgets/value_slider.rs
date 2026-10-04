use nih_plug_egui::egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke, StrokeKind, Ui};

use crate::theme::Theme;
use crate::widgets::value_knob::{format_value, Unit};

/// Value change per dragged pixel, as a fraction of the control's range.
///
/// A slider is dragged along its own length, so unlike the knob the sensible
/// rate follows the width rather than a fixed number.
const FINE_DRAG_FACTOR: f32 = 0.15;

/// What a horizontal value bar shows.
#[derive(Debug, Clone, Copy)]
pub struct SliderSpec<'a> {
    pub label: &'a str,
    pub range: (f32, f32),
    /// Value a double click returns to.
    pub default: f32,
    pub unit: Unit,
    pub width: f32,
}

/// A horizontal bar over a plain value.
///
/// Used where a knob would be too tall to repeat, as in the rows of the
/// modulation matrix. A range that spans zero fills outwards from the centre,
/// so the sign of a route is visible at a glance.
///
/// Returns true when the value changed this frame.
pub fn value_slider(ui: &mut Ui, theme: &Theme, spec: SliderSpec<'_>, value: &mut f32) -> bool {
    let height = theme.font_sm + theme.spacing_md;
    let (rect, response) =
        ui.allocate_exact_size(vec2(spec.width, height), Sense::click_and_drag());

    let (min, max) = spec.range;
    let span = max - min;
    let mut changed = false;

    if response.double_clicked() {
        *value = spec.default;
        changed = true;
    } else if response.dragged() && span.abs() > f32::EPSILON {
        let fine = ui.input(|input| input.modifiers.shift);
        let scale = if fine { FINE_DRAG_FACTOR } else { 1.0 };
        let delta = response.drag_delta().x / spec.width.max(1.0) * scale;
        if delta != 0.0 {
            *value = (*value + delta * span).clamp(min, max);
            changed = true;
        }
    }

    let painter = ui.painter();
    painter.rect_filled(rect, theme.radius_sm, theme.control_pressed_bg);

    if span.abs() > f32::EPSILON {
        let inner = rect.shrink(theme.stroke_thin);
        let at = |v: f32| inner.min.x + inner.width() * ((v - min) / span).clamp(0.0, 1.0);
        // A range that crosses zero is filled from zero, so a negative amount
        // reads as a bar growing the other way rather than as a short bar.
        let origin = if min < 0.0 && max > 0.0 { 0.0 } else { min };
        let (from, to) = (at(origin).min(at(*value)), at(origin).max(at(*value)));
        if to - from > 0.5 {
            painter.rect_filled(
                Rect::from_min_max(pos2(from, inner.min.y), pos2(to, inner.max.y)),
                theme.radius_sm,
                theme.accent.gamma_multiply(0.45),
            );
        }
        if origin != min {
            painter.line_segment(
                [pos2(at(0.0), inner.min.y), pos2(at(0.0), inner.max.y)],
                Stroke::new(1.0_f32, theme.outline),
            );
        }
    }

    painter.rect_stroke(
        rect,
        theme.radius_sm,
        theme.outline_stroke(),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.min.x + theme.spacing_sm, rect.center().y),
        Align2::LEFT_CENTER,
        spec.label,
        FontId::proportional(theme.font_sm),
        theme.text_dim,
    );
    painter.text(
        pos2(rect.max.x - theme.spacing_sm, rect.center().y),
        Align2::RIGHT_CENTER,
        format_value(*value, spec.unit),
        FontId::proportional(theme.font_sm),
        theme.text,
    );

    changed
}
