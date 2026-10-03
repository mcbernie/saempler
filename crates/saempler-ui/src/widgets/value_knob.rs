use nih_plug_egui::egui::{epaint::PathShape, vec2, Align2, FontId, Pos2, Sense, Stroke, Ui, Vec2};

use crate::theme::Theme;

/// Value change per dragged pixel, as a fraction of the control's range.
const DRAG_SENSITIVITY: f32 = 0.005;
/// Multiplier applied while shift is held, for fine adjustment.
const FINE_DRAG_FACTOR: f32 = 0.15;

/// Angle of the knob's minimum position, measured clockwise from straight up.
const START_ANGLE: f32 = -0.75 * std::f32::consts::PI;
/// Angle of the knob's maximum position.
const END_ANGLE: f32 = 0.75 * std::f32::consts::PI;
/// Number of line segments used to draw the value arc.
const ARC_SEGMENTS: usize = 48;

/// Unit a knob's readout is written in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Unit {
    /// A bare number.
    #[default]
    Plain,
    /// A factor on something else, as speed is on the recorded rate.
    Multiplier,
    Milliseconds,
    Semitones,
    Hertz,
}

/// How a control maps its position to a value.
#[derive(Debug, Clone, Copy)]
pub enum Taper {
    /// Position and value are proportional.
    Linear,
    /// Equal distances mean equal ratios, so halving and doubling sit the same
    /// distance from the centre. Used for speed and for anything else where
    /// the useful range spans octaves.
    Logarithmic,
    /// Most of the travel goes to the bottom of the range, which a logarithmic
    /// taper cannot do because it never reaches zero. Used for envelope times,
    /// where the step from 3 to 10 ms matters and the step from 3.0 to 3.5
    /// seconds does not.
    Skewed,
}

/// Power the skewed taper raises its position to.
///
/// At four, half the travel stays inside the lowest sixteenth of the range.
const SKEW: f32 = 4.0;

/// What a value knob shows, apart from the value it edits.
///
/// Grouped into one value because the page carries a dozen knobs and a list of
/// positional arguments stopped saying which was which.
#[derive(Debug, Clone, Copy)]
pub struct KnobSpec<'a> {
    pub label: &'a str,
    pub range: (f32, f32),
    /// Value a double click returns to.
    pub default: f32,
    pub taper: Taper,
    pub unit: Unit,
    pub diameter: f32,
}

/// A rotary control over a plain value, not a host parameter.
///
/// Cell settings are not automatable, so they cannot go through `ParamSetter`
/// the way the master gain does. Interaction matches the parameter knob:
/// vertical drag, shift for fine adjustment, double click to reset.
///
/// Returns true when the value changed this frame.
pub fn value_knob(ui: &mut Ui, theme: &Theme, spec: KnobSpec<'_>, value: &mut f32) -> bool {
    let KnobSpec {
        label,
        range,
        default,
        taper,
        unit,
        diameter,
    } = spec;

    let label_height = theme.font_sm * 2.6;
    let (rect, response) = ui.allocate_exact_size(
        vec2(diameter.max(theme.font_sm * 5.0), diameter + label_height),
        Sense::click_and_drag(),
    );

    let dial = rect.with_max_y(rect.min.y + diameter);
    let centre = dial.center();
    let radius = diameter * 0.5 - theme.stroke_thick;
    let mut changed = false;

    if response.double_clicked() {
        *value = default;
        changed = true;
    } else if response.dragged() {
        let fine = ui.input(|input| input.modifiers.shift);
        let sensitivity = if fine {
            DRAG_SENSITIVITY * FINE_DRAG_FACTOR
        } else {
            DRAG_SENSITIVITY
        };
        // Dragging up increases the value, so the vertical delta is inverted.
        let delta = -response.drag_delta().y * sensitivity;
        if delta != 0.0 {
            let normalized = (to_normalized(*value, range, taper) + delta).clamp(0.0, 1.0);
            *value = from_normalized(normalized, range, taper);
            changed = true;
        }
    }

    let normalized = to_normalized(*value, range, taper);
    let painter = ui.painter();

    let body = if response.hovered() || response.dragged() {
        theme.control_hover_bg
    } else {
        theme.control_bg
    };
    painter.circle_filled(centre, radius, body);
    painter.circle_stroke(centre, radius, theme.outline_stroke());

    let track_radius = radius - theme.stroke_thick;
    painter.add(arc(
        centre,
        track_radius,
        START_ANGLE,
        END_ANGLE,
        Stroke::new(theme.stroke_thick, theme.outline),
    ));

    let value_angle = START_ANGLE + (END_ANGLE - START_ANGLE) * normalized;
    if normalized > 0.0 {
        painter.add(arc(
            centre,
            track_radius,
            START_ANGLE,
            value_angle,
            Stroke::new(theme.stroke_thick, theme.accent),
        ));
    }

    painter.line_segment(
        [
            centre + angle_vec(value_angle) * (track_radius * 0.35),
            centre + angle_vec(value_angle) * (track_radius - theme.spacing_sm),
        ],
        Stroke::new(theme.stroke_thick, theme.text),
    );

    painter.text(
        Pos2::new(centre.x, dial.max.y + theme.spacing_sm * 0.5),
        Align2::CENTER_TOP,
        label,
        FontId::proportional(theme.font_sm),
        theme.text_dim,
    );
    painter.text(
        Pos2::new(centre.x, dial.max.y + theme.font_sm + theme.spacing_sm),
        Align2::CENTER_TOP,
        format_value(*value, unit),
        FontId::proportional(theme.font_sm),
        theme.text,
    );

    changed
}

/// Where `value` sits in `range`, as a fraction.
fn to_normalized(value: f32, range: (f32, f32), taper: Taper) -> f32 {
    let (min, max) = range;
    match taper {
        Taper::Linear => {
            if (max - min).abs() < f32::EPSILON {
                0.0
            } else {
                ((value - min) / (max - min)).clamp(0.0, 1.0)
            }
        }
        Taper::Logarithmic => {
            let (min, max, value) = (min.max(1e-6), max.max(1e-6), value.max(1e-6));
            let span = (max / min).ln();
            if span.abs() < f32::EPSILON {
                0.0
            } else {
                ((value / min).ln() / span).clamp(0.0, 1.0)
            }
        }
        Taper::Skewed => {
            if (max - min).abs() < f32::EPSILON {
                0.0
            } else {
                ((value - min) / (max - min))
                    .clamp(0.0, 1.0)
                    .powf(1.0 / SKEW)
            }
        }
    }
}

/// The value a fraction of the way through `range`.
fn from_normalized(normalized: f32, range: (f32, f32), taper: Taper) -> f32 {
    let (min, max) = range;
    let normalized = normalized.clamp(0.0, 1.0);
    match taper {
        Taper::Linear => min + (max - min) * normalized,
        Taper::Logarithmic => {
            let (min, max) = (min.max(1e-6), max.max(1e-6));
            min * (max / min).powf(normalized)
        }
        Taper::Skewed => min + (max - min) * normalized.powf(SKEW),
    }
}

/// Short display form of a value.
///
/// Readouts are kept to a handful of characters: they sit under a knob barely
/// wider than the text, and a long number would run into its neighbour.
pub(crate) fn format_value(value: f32, unit: Unit) -> String {
    match unit {
        Unit::Multiplier => format!("{value:.2}×"),
        Unit::Milliseconds => {
            if value >= 1_000.0 {
                format!("{:.2} s", value / 1_000.0)
            } else if value >= 100.0 {
                format!("{value:.0} ms")
            } else {
                format!("{value:.1} ms")
            }
        }
        Unit::Semitones => format!("{value:+.1} st"),
        Unit::Hertz => format!("{value:.2} Hz"),
        Unit::Plain => {
            if value.abs() >= 100.0 {
                format!("{value:.0}")
            } else {
                format!("{value:.2}")
            }
        }
    }
}

/// Build a stroked arc as a polyline.
fn arc(centre: Pos2, radius: f32, from: f32, to: f32, stroke: Stroke) -> PathShape {
    let points = (0..=ARC_SEGMENTS)
        .map(|step| {
            let t = step as f32 / ARC_SEGMENTS as f32;
            centre + angle_vec(from + (to - from) * t) * radius
        })
        .collect();

    PathShape::line(points, stroke)
}

/// Unit vector for an angle measured clockwise from straight up.
fn angle_vec(angle: f32) -> Vec2 {
    vec2(angle.sin(), -angle.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linear_taper_maps_the_ends_and_the_middle() {
        let range = (0.0, 100.0);

        assert_eq!(to_normalized(0.0, range, Taper::Linear), 0.0);
        assert_eq!(to_normalized(100.0, range, Taper::Linear), 1.0);
        assert!((to_normalized(50.0, range, Taper::Linear) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_logarithmic_taper_puts_the_geometric_middle_in_the_centre() {
        // Half and double sit the same distance either side of 1.0.
        let range = (0.25, 4.0);

        let centre = to_normalized(1.0, range, Taper::Logarithmic);

        assert!((centre - 0.5).abs() < 1e-5, "{centre}");
    }

    #[test]
    fn a_skewed_taper_gives_the_bottom_of_the_range_most_of_the_travel() {
        let range = (0.0, 4_000.0);

        // Half the travel should still be below a tenth of the range.
        let halfway = from_normalized(0.5, range, Taper::Skewed);

        assert!(halfway < 400.0, "{halfway}");
        assert_eq!(from_normalized(0.0, range, Taper::Skewed), 0.0);
        assert_eq!(from_normalized(1.0, range, Taper::Skewed), 4_000.0);
    }

    #[test]
    fn both_tapers_round_trip() {
        for taper in [Taper::Linear, Taper::Logarithmic, Taper::Skewed] {
            let range = (0.0625, 16.0);
            for value in [0.0625f32, 0.5, 1.0, 4.0, 16.0] {
                let back = from_normalized(to_normalized(value, range, taper), range, taper);
                assert!(
                    (back - value).abs() < value * 1e-3 + 1e-4,
                    "{value} -> {back}"
                );
            }
        }
    }

    #[test]
    fn values_outside_the_range_are_clamped() {
        let range = (0.0, 10.0);

        assert_eq!(to_normalized(-5.0, range, Taper::Linear), 0.0);
        assert_eq!(to_normalized(50.0, range, Taper::Linear), 1.0);
        assert_eq!(from_normalized(-1.0, range, Taper::Linear), 0.0);
        assert_eq!(from_normalized(2.0, range, Taper::Linear), 10.0);
    }

    #[test]
    fn a_degenerate_range_does_not_divide_by_zero() {
        let range = (5.0, 5.0);

        assert_eq!(to_normalized(5.0, range, Taper::Linear), 0.0);
        assert_eq!(to_normalized(5.0, range, Taper::Logarithmic), 0.0);
        assert_eq!(to_normalized(5.0, range, Taper::Skewed), 0.0);
        assert!(from_normalized(0.5, range, Taper::Linear).is_finite());
    }

    #[test]
    fn a_readout_says_what_the_number_means() {
        assert_eq!(format_value(1.0, Unit::Multiplier), "1.00×");
        assert_eq!(format_value(3.0, Unit::Milliseconds), "3.0 ms");
        assert_eq!(format_value(250.0, Unit::Milliseconds), "250 ms");
        assert_eq!(format_value(2_000.0, Unit::Milliseconds), "2.00 s");
        assert_eq!(format_value(-7.0, Unit::Semitones), "-7.0 st");
        assert_eq!(format_value(0.0, Unit::Semitones), "+0.0 st");
        assert_eq!(format_value(2.0, Unit::Hertz), "2.00 Hz");
        assert_eq!(format_value(0.5, Unit::Plain), "0.50");
    }

    #[test]
    fn negative_ranges_work_linearly() {
        let range = (-24.0, 24.0);

        assert!((to_normalized(0.0, range, Taper::Linear) - 0.5).abs() < 1e-6);
        assert!((from_normalized(0.5, range, Taper::Linear)).abs() < 1e-4);
    }
}
