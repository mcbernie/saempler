use nih_plug::prelude::{Param, ParamSetter};
use nih_plug_egui::egui::{pos2, vec2, Rect, Response, Sense, Ui};

use crate::theme::Theme;
use crate::widgets::dial::{caption, dial, name_width, DialState};

/// Normalized value change per dragged pixel.
const DRAG_SENSITIVITY: f32 = 0.005;
/// Multiplier applied while shift is held, for fine adjustment.
const FINE_DRAG_FACTOR: f32 = 0.15;

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
    // As wide as its caption: a name wider than the knob would otherwise run
    // out past the room it was given.
    let (rect, response) = ui.allocate_exact_size(
        vec2(
            diameter.max(name_width(ui, theme, param.name())),
            diameter + label_height,
        ),
        Sense::click_and_drag(),
    );

    let dial_rect = Rect::from_center_size(
        pos2(rect.center().x, rect.min.y + diameter * 0.5),
        vec2(diameter, diameter),
    );
    let center = dial_rect.center();
    let radius = diameter * 0.5 - theme.stroke_thick;

    handle_input(&response, ui, param, setter);

    let normalized = param.modulated_normalized_value().clamp(0.0, 1.0);
    dial(
        ui.painter(),
        theme,
        center,
        radius,
        DialState {
            normalized,
            modulated: None,
            hovered: response.hovered(),
            dragged: response.dragged(),
            bipolar: false,
        },
    );

    caption(
        ui.painter(),
        theme,
        center.x,
        dial_rect.max.y,
        param.name(),
        &param.to_string(),
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
