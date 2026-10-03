use nih_plug_egui::egui::{
    epaint::{Mesh, PathShape, Vertex, WHITE_UV},
    vec2, Color32, Painter, Pos2, Shape, Stroke, Vec2,
};

use crate::theme::Theme;

/// Angle of a dial's minimum position, measured clockwise from straight up.
pub const START_ANGLE: f32 = -0.75 * std::f32::consts::PI;
/// Angle of a dial's maximum position.
pub const END_ANGLE: f32 = 0.75 * std::f32::consts::PI;
/// Number of line segments used to draw an arc.
const ARC_SEGMENTS: usize = 48;
/// Points around the rim of the knob body.
const BODY_SEGMENTS: usize = 40;

/// What a dial shows beyond its own position.
#[derive(Debug, Clone, Copy, Default)]
pub struct DialState {
    /// Where the control is set, from 0 to 1.
    pub normalized: f32,
    /// Where modulation has pushed it, when the engine is moving it.
    ///
    /// Drawn as a second, brighter arc from the set position, so a knob under
    /// an envelope or an LFO visibly runs while the note sounds.
    pub modulated: Option<f32>,
    pub hovered: bool,
    pub dragged: bool,
}

/// Draw a knob: a turned metal cap over a recessed track.
///
/// Shared by the host-parameter knob and the plain value knob so that every
/// dial in the instrument is made of the same part.
pub fn dial(painter: &Painter, theme: &Theme, centre: Pos2, radius: f32, state: DialState) {
    let track_radius = radius;
    let body_radius = radius - 5.0;

    // The track the cap turns in, sunk into the panel.
    painter.add(arc(
        centre,
        track_radius,
        START_ANGLE,
        END_ANGLE,
        Stroke::new(4.0, theme.chassis_shadow),
    ));
    painter.add(arc(
        centre + vec2(0.0, 1.0),
        track_radius,
        START_ANGLE,
        END_ANGLE,
        Stroke::new(1.0, theme.chassis_edge),
    ));

    let value_angle = angle_of(state.normalized);
    let lit = if state.dragged || state.hovered {
        theme.accent.lerp_to_gamma(Color32::WHITE, 0.3)
    } else {
        theme.accent
    };

    if state.normalized > 0.001 {
        // A wash of colour under the arc, so the lit part glows rather than
        // merely being coloured.
        painter.add(arc(
            centre,
            track_radius,
            START_ANGLE,
            value_angle,
            Stroke::new(7.0, lit.gamma_multiply(0.18)),
        ));
        painter.add(arc(
            centre,
            track_radius,
            START_ANGLE,
            value_angle,
            Stroke::new(3.0, lit),
        ));
    }

    // Modulation runs from where the control is set to where the engine has
    // pushed it, in the colour reserved for something actually happening.
    if let Some(modulated) = state.modulated {
        let target = angle_of(modulated);
        if (target - value_angle).abs() > 0.01 {
            painter.add(arc(
                centre,
                track_radius,
                value_angle,
                target,
                Stroke::new(7.0, theme.active.gamma_multiply(0.22)),
            ));
            painter.add(arc(
                centre,
                track_radius,
                value_angle,
                target,
                Stroke::new(3.0, theme.active),
            ));
        }
    }

    body(painter, theme, centre, body_radius, state);
    pointer(painter, theme, centre, body_radius, value_angle, state);
}

/// The turned cap itself.
fn body(painter: &Painter, theme: &Theme, centre: Pos2, radius: f32, state: DialState) {
    painter.circle_filled(
        centre + vec2(0.0, 1.5),
        radius + 1.0,
        Color32::from_black_alpha(140),
    );

    let (top, bottom) = if state.hovered || state.dragged {
        (theme.control_hover_top, theme.control_hover_bg)
    } else {
        (theme.control_top, theme.control_bg)
    };
    // A cap is a cylinder seen from above, so the shading runs top to bottom
    // rather than out from the middle.
    painter.add(disc_gradient(centre, radius, top, bottom));

    // The milled rim: light where the light falls, dark on the far side.
    painter.add(arc(
        centre,
        radius - 0.5,
        -0.95 * std::f32::consts::PI,
        -0.05 * std::f32::consts::PI,
        Stroke::new(1.2, Color32::from_white_alpha(70)),
    ));
    painter.circle_stroke(
        centre,
        radius,
        Stroke::new(1.0, Color32::from_black_alpha(200)),
    );
}

/// The line that says which way the cap is turned.
fn pointer(
    painter: &Painter,
    theme: &Theme,
    centre: Pos2,
    radius: f32,
    angle: f32,
    state: DialState,
) {
    let direction = angle_vec(angle);
    let color = if state.dragged || state.hovered {
        Color32::WHITE
    } else {
        theme.text
    };

    painter.line_segment(
        [
            centre + direction * (radius * 0.3) + vec2(0.0, 1.0),
            centre + direction * (radius - 2.0) + vec2(0.0, 1.0),
        ],
        Stroke::new(2.4, Color32::from_black_alpha(160)),
    );
    painter.line_segment(
        [
            centre + direction * (radius * 0.3),
            centre + direction * (radius - 2.0),
        ],
        Stroke::new(2.0, color),
    );
}

/// A circle filled with a vertical gradient, as a triangle fan.
///
/// egui can fill a circle or interpolate a mesh, but not both at once; the fan
/// colours each rim vertex by how far down the circle it sits.
fn disc_gradient(centre: Pos2, radius: f32, top: Color32, bottom: Color32) -> Shape {
    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex {
        pos: centre,
        uv: WHITE_UV,
        color: top.lerp_to_gamma(bottom, 0.5),
    });

    for step in 0..=BODY_SEGMENTS {
        let angle = step as f32 / BODY_SEGMENTS as f32 * std::f32::consts::TAU;
        let point = centre + vec2(angle.sin(), -angle.cos()) * radius;
        // 0 at the top of the circle, 1 at the bottom.
        let depth = (point.y - (centre.y - radius)) / (radius * 2.0);
        mesh.vertices.push(Vertex {
            pos: point,
            uv: WHITE_UV,
            color: top.lerp_to_gamma(bottom, depth),
        });
    }

    for step in 0..BODY_SEGMENTS as u32 {
        mesh.indices.extend([0, step + 1, step + 2]);
    }

    Shape::Mesh(mesh.into())
}

/// The angle a normalized position sits at.
pub fn angle_of(normalized: f32) -> f32 {
    START_ANGLE + (END_ANGLE - START_ANGLE) * normalized.clamp(0.0, 1.0)
}

/// Build a stroked arc as a polyline.
pub fn arc(centre: Pos2, radius: f32, from: f32, to: f32, stroke: Stroke) -> PathShape {
    let points = (0..=ARC_SEGMENTS)
        .map(|step| {
            let t = step as f32 / ARC_SEGMENTS as f32;
            centre + angle_vec(from + (to - from) * t) * radius
        })
        .collect();

    PathShape::line(points, stroke)
}

/// Unit vector for an angle measured clockwise from straight up.
pub fn angle_vec(angle: f32) -> Vec2 {
    vec2(angle.sin(), -angle.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ends_of_the_sweep_sit_either_side_of_the_bottom() {
        assert!(angle_of(0.0) < 0.0);
        assert!(angle_of(1.0) > 0.0);
        assert!(
            (angle_of(0.5)).abs() < 1e-6,
            "the middle points straight up"
        );
    }

    #[test]
    fn a_position_outside_the_range_stops_at_the_end() {
        assert_eq!(angle_of(-1.0), START_ANGLE);
        assert_eq!(angle_of(2.0), END_ANGLE);
    }

    #[test]
    fn straight_up_is_the_negative_y_direction() {
        let up = angle_vec(0.0);

        assert!(up.x.abs() < 1e-6);
        assert!((up.y + 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_disc_covers_its_whole_rim() {
        let shape = disc_gradient(Pos2::ZERO, 10.0, Color32::WHITE, Color32::BLACK);

        let Shape::Mesh(mesh) = shape else {
            panic!("a disc is a mesh");
        };
        assert_eq!(mesh.vertices.len(), BODY_SEGMENTS + 2);
        assert_eq!(mesh.indices.len(), BODY_SEGMENTS * 3);
    }
}
