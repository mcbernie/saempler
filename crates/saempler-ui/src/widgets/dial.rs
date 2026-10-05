use nih_plug_egui::egui::{
    epaint::{Mesh, PathShape, Vertex, WHITE_UV},
    pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Ui, Vec2,
};

use crate::theme::Theme;
use crate::widgets::panel::legend;
use crate::widgets::texture::{self, KNOB_CAP_RADIUS, KNOB_PIVOT};

/// Angle of a dial's minimum position, measured clockwise from straight up.
pub const START_ANGLE: f32 = -0.75 * std::f32::consts::PI;
/// Angle of a dial's maximum position.
pub const END_ANGLE: f32 = 0.75 * std::f32::consts::PI;
/// Number of line segments used to draw an arc.
const ARC_SEGMENTS: usize = 48;
/// Points around the rim of a gradient disc.
const BODY_SEGMENTS: usize = 40;
/// Dots on the printed scale around a knob.
const SCALE_DOTS: usize = 11;
/// Room between the scale and the knob's skirt.
const SCALE_ROOM: f32 = 4.0;
/// Radius of the knurled skirt, as a fraction of the knob texture's side.
const KNOB_SKIRT_RADIUS: f32 = 0.4912;
/// Colour of the pointer: cream plastic, as in the render.
const POINTER: Color32 = Color32::from_rgb(0xee, 0xe4, 0xd2);

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
    /// Whether the middle of the range means "no change", as it does for a
    /// transposition. The scale then marks its centre.
    pub bipolar: bool,
}

/// Draw a knob: a ribbed cap inside a fixed, printed scale.
///
/// Shared by the host-parameter knob and the plain value knob so that every
/// dial in the instrument is made of the same part.
///
/// The cap is a picture whose light and shadow stay put; only the pointer
/// turns. A rotating picture would turn its highlight with it, and the knob
/// would stop looking lit from the top left.
pub fn dial(painter: &Painter, theme: &Theme, centre: Pos2, radius: f32, state: DialState) {
    let value_angle = angle_of(state.normalized);

    scale(painter, theme, centre, radius, state);

    // Modulation runs along the scale from where the control is set to where
    // the engine has pushed it, in the colour of something happening.
    if let Some(modulated) = state.modulated {
        let target = angle_of(modulated);
        if (target - value_angle).abs() > 0.01 {
            painter.add(arc(
                centre,
                radius,
                value_angle,
                target,
                Stroke::new(2.5_f32, theme.active),
            ));
        }
    }

    let (pivot, top) = cap(painter, centre, radius - SCALE_ROOM, state);
    pointer(painter, pivot, top, value_angle);
}

/// The printed scale: a dot per step, the ones the value has passed darker.
///
/// A bipolar control gets a longer mark at its centre and lights the dots
/// from there, so "no change" can be found by eye.
fn scale(painter: &Painter, theme: &Theme, centre: Pos2, radius: f32, state: DialState) {
    let lit = theme.title;
    let unlit = theme.chassis_shadow.gamma_multiply(0.55);
    let (from, to) = if state.bipolar {
        let half = 0.5_f32;
        (half.min(state.normalized), half.max(state.normalized))
    } else {
        (0.0, state.normalized)
    };

    for step in 0..SCALE_DOTS {
        let at = step as f32 / (SCALE_DOTS - 1) as f32;
        let direction = angle_vec(angle_of(at));
        let on = at >= from - 1e-3 && at <= to + 1e-3;
        let color = if on { lit } else { unlit };
        if state.bipolar && step == SCALE_DOTS / 2 {
            painter.line_segment(
                [
                    centre + direction * (radius - 3.0),
                    centre + direction * (radius + 1.0),
                ],
                Stroke::new(1.6_f32, lit),
            );
        } else {
            painter.circle_filled(centre + direction * (radius - 1.0), 0.9, color);
        }
    }
}

/// The ribbed cap, drawn from the texture. Returns the point the pointer
/// turns on and the radius of the flat top around it.
fn cap(painter: &Painter, centre: Pos2, radius: f32, state: DialState) -> (Pos2, f32) {
    let textures = texture::textures(painter.ctx());

    // A short, soft contact shadow down and to the right.
    for (grow, alpha) in [(2.0, 18), (1.0, 34), (0.0, 54)] {
        painter.circle_filled(
            centre + vec2(1.0, 1.8),
            radius + grow,
            Color32::from_black_alpha(alpha),
        );
    }

    let side = radius / KNOB_SKIRT_RADIUS;
    let rect = Rect::from_center_size(centre, vec2(side, side));
    painter.add(texture::image(&textures.knob, rect, Color32::WHITE));

    let pivot = rect.min + KNOB_PIVOT * side;
    let top = KNOB_CAP_RADIUS * side;
    // Under the pointer the top catches a little more light, from above.
    if state.hovered || state.dragged {
        painter.add(disc_gradient(
            pivot,
            top,
            Color32::from_white_alpha(26),
            Color32::TRANSPARENT,
        ));
    }
    (pivot, top)
}

/// The pointer: a cream bar from the middle of the cap to its edge.
fn pointer(painter: &Painter, pivot: Pos2, radius: f32, angle: f32) {
    let direction = angle_vec(angle);
    let width = (radius * 0.2).max(2.0);
    let from = pivot + direction * (radius * 0.12);
    let to = pivot + direction * (radius * 0.84);

    // Raised a little off the cap: a soft shadow down and to the right.
    capsule(
        painter,
        from + vec2(0.5, 0.9),
        to + vec2(0.5, 0.9),
        width + 0.6,
        Color32::from_black_alpha(150),
    );
    capsule(painter, from, to, width, POINTER);
    capsule(
        painter,
        from - vec2(0.3, 0.3),
        to - vec2(0.3, 0.3),
        width * 0.35,
        Color32::from_white_alpha(90),
    );
}

/// Where a knob's name and reading are printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Centred under the knob, for a row of knobs on their own.
    Below,
    /// To the right of it, for the slim rows of a modulation strip.
    Beside,
}

/// Width given to a caption printed beside a knob.
const BESIDE_WIDTH: f32 = 58.0;

/// Room a knob of `diameter` takes with its caption.
pub fn captioned_size(theme: &Theme, diameter: f32, placement: Placement) -> Vec2 {
    match placement {
        Placement::Below => vec2(
            diameter.max(theme.font_sm * 5.0),
            diameter + theme.font_sm * 2.4,
        ),
        Placement::Beside => vec2(diameter + theme.spacing_sm + BESIDE_WIDTH, diameter),
    }
}

/// Width a knob's name takes printed under it, so a knob can be made as wide
/// as its caption rather than letting the name run into its neighbour.
pub fn name_width(ui: &Ui, theme: &Theme, name: &str) -> f32 {
    ui.fonts(|fonts| {
        fonts.layout_no_wrap(
            name.to_uppercase(),
            FontId::proportional(theme.font_sm),
            theme.label,
        )
    })
    .size()
    .x + 2.0
}

/// The circle a knob sits in within the room [`captioned_size`] gave it.
pub fn dial_rect(rect: Rect, diameter: f32, placement: Placement) -> Rect {
    match placement {
        Placement::Below => Rect::from_center_size(
            pos2(rect.center().x, rect.min.y + diameter * 0.5),
            vec2(diameter, diameter),
        ),
        Placement::Beside => Rect::from_min_size(rect.min, vec2(diameter, diameter)),
    }
}

/// Print a knob's name and reading where `placement` puts them.
pub fn caption_at(
    painter: &Painter,
    theme: &Theme,
    dial: Rect,
    placement: Placement,
    name: &str,
    value: &str,
) {
    match placement {
        Placement::Below => caption(painter, theme, dial.center().x, dial.max.y, name, value),
        Placement::Beside => {
            let x = dial.max.x + theme.spacing_sm;
            legend(
                painter,
                theme,
                pos2(x, dial.center().y - 1.0),
                Align2::LEFT_BOTTOM,
                name,
            );
            painter.text(
                pos2(x, dial.center().y + 1.0),
                Align2::LEFT_TOP,
                value,
                FontId::proportional(theme.font_sm),
                theme.value,
            );
        }
    }
}

/// The name and the reading printed under a knob.
///
/// The name in capitals and heavy, the way a front plate is lettered; the
/// interface font has no bold weight, so it is drawn twice half a point apart.
pub fn caption(painter: &Painter, theme: &Theme, centre_x: f32, top: f32, name: &str, value: &str) {
    legend(
        painter,
        theme,
        pos2(centre_x, top + theme.spacing_sm * 0.5),
        Align2::CENTER_TOP,
        name,
    );
    painter.text(
        pos2(centre_x, top + theme.font_sm + theme.spacing_sm),
        Align2::CENTER_TOP,
        value,
        FontId::proportional(theme.font_sm),
        theme.value,
    );
}

/// A bar with rounded ends.
fn capsule(painter: &Painter, from: Pos2, to: Pos2, width: f32, color: Color32) {
    painter.line_segment([from, to], Stroke::new(width, color));
    painter.circle_filled(from, width * 0.5, color);
    painter.circle_filled(to, width * 0.5, color);
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
