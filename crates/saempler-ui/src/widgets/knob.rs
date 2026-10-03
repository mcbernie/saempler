use nih_plug::prelude::{Param, ParamSetter};
use nih_plug_egui::egui::{
    epaint::PathShape, vec2, Align2, FontId, Pos2, Response, Sense, Stroke, Ui, Vec2,
};

use crate::theme::Theme;

/// Normalized value change per dragged pixel.
const DRAG_SENSITIVITY: f32 = 0.005;
/// Multiplier applied while shift is held, for fine adjustment.
const FINE_DRAG_FACTOR: f32 = 0.15;

/// Angle of the knob's minimum position, measured clockwise from straight up.
const START_ANGLE: f32 = -0.75 * std::f32::consts::PI;
/// Angle of the knob's maximum position.
const END_ANGLE: f32 = 0.75 * std::f32::consts::PI;
/// Number of line segments used to draw the value arc.
const ARC_SEGMENTS: usize = 48;

/// A rotary control bound to a plugin parameter.
///
/// Supported interactions: vertical drag, shift+drag for fine adjustment and
/// double click to return to the parameter's default.
pub fn knob<P: Param>(
    ui: &mut Ui,
    theme: &Theme,
    param: &P,
    setter: &ParamSetter,
    diameter: f32,
) -> Response {
    let label_height = theme.font_sm * 2.5;
    let (rect, response) = ui.allocate_exact_size(
        vec2(diameter, diameter + label_height),
        Sense::click_and_drag(),
    );

    let dial_rect = rect.with_max_y(rect.min.y + diameter);
    let center = dial_rect.center();
    let radius = diameter * 0.5 - theme.stroke_thick;

    handle_input(&response, ui, param, setter);

    let normalized = param.modulated_normalized_value().clamp(0.0, 1.0);
    let painter = ui.painter();

    let body = if response.hovered() || response.dragged() {
        theme.control_hover_bg
    } else {
        theme.control_bg
    };
    painter.circle_filled(center, radius, body);
    painter.circle_stroke(center, radius, theme.outline_stroke());

    // The track shows the full travel, the arc on top shows the current value.
    let track_radius = radius - theme.stroke_thick;
    painter.add(arc(
        center,
        track_radius,
        START_ANGLE,
        END_ANGLE,
        Stroke::new(theme.stroke_thick, theme.outline),
    ));

    let value_angle = START_ANGLE + (END_ANGLE - START_ANGLE) * normalized;
    if normalized > 0.0 {
        painter.add(arc(
            center,
            track_radius,
            START_ANGLE,
            value_angle,
            Stroke::new(theme.stroke_thick, theme.accent),
        ));
    }

    let pointer_outer = center + angle_vec(value_angle) * (track_radius - theme.spacing_sm);
    let pointer_inner = center + angle_vec(value_angle) * (track_radius * 0.35);
    painter.line_segment(
        [pointer_inner, pointer_outer],
        Stroke::new(theme.stroke_thick, theme.text),
    );

    let text_color = if response.hovered() {
        theme.value
    } else {
        theme.label
    };
    painter.text(
        Pos2::new(center.x, dial_rect.max.y + theme.spacing_sm * 0.5),
        Align2::CENTER_TOP,
        param.name(),
        FontId::proportional(theme.font_sm),
        text_color,
    );
    painter.text(
        Pos2::new(center.x, dial_rect.max.y + theme.font_sm + theme.spacing_sm),
        Align2::CENTER_TOP,
        param.to_string(),
        FontId::proportional(theme.font_sm),
        theme.value,
    );

    response
}

/// Translate pointer interaction into parameter gestures.
fn handle_input<P: Param>(response: &Response, ui: &Ui, param: &P, setter: &ParamSetter) {
    if response.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.default_plain_value());
        setter.end_set_parameter(param);
        return;
    }

    if response.drag_started() {
        setter.begin_set_parameter(param);
    }

    if response.dragged() {
        let fine = ui.input(|input| input.modifiers.shift);
        let sensitivity = if fine {
            DRAG_SENSITIVITY * FINE_DRAG_FACTOR
        } else {
            DRAG_SENSITIVITY
        };

        // Dragging up increases the value, so the vertical delta is inverted.
        let delta = -response.drag_delta().y * sensitivity;
        if delta != 0.0 {
            let normalized = (param.modulated_normalized_value() + delta).clamp(0.0, 1.0);
            setter.set_parameter_normalized(param, normalized);
        }
    }

    if response.drag_stopped() {
        setter.end_set_parameter(param);
    }
}

/// Build a stroked arc as a polyline.
fn arc(center: Pos2, radius: f32, from: f32, to: f32, stroke: Stroke) -> PathShape {
    let points = (0..=ARC_SEGMENTS)
        .map(|step| {
            let t = step as f32 / ARC_SEGMENTS as f32;
            center + angle_vec(from + (to - from) * t) * radius
        })
        .collect();

    PathShape::line(points, stroke)
}

/// Unit vector for an angle measured clockwise from straight up.
fn angle_vec(angle: f32) -> Vec2 {
    vec2(angle.sin(), -angle.cos())
}
