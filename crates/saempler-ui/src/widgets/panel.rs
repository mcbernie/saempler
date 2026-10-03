use nih_plug_egui::egui::{
    epaint::{CircleShape, Mesh, RectShape, Vertex, WHITE_UV},
    pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui,
};

use crate::theme::Theme;

/// Distance from a panel corner to the middle of its screw.
const SCREW_INSET: f32 = 9.0;
/// Radius of a screw head.
const SCREW_RADIUS: f32 = 4.0;
/// Spacing between the brushed lines drawn across the metal.
const BRUSH_SPACING: f32 = 3.0;
/// Radius of an indicator lamp.
const LED_RADIUS: f32 = 4.0;
/// Space a panel legend occupies above the panel's contents.
pub const HEADER_HEIGHT: f32 = 22.0;

/// A panel milled from brushed metal, with a screw in every corner.
///
/// Returned as a shape rather than painted, because a panel sits behind
/// contents whose height is only known once they have been laid out: the
/// caller reserves an index before the contents and fills it in afterwards.
///
/// The gradient is a two-triangle mesh. egui has no gradient fill, and vertex
/// colours cost nothing to draw.
pub fn metal_panel(theme: &Theme, rect: Rect) -> Shape {
    let mut shapes = Vec::with_capacity(16);

    // The shadow the panel casts on what is behind it.
    shapes.push(Shape::Rect(RectShape::filled(
        rect.translate(vec2(0.0, 1.5)),
        theme.radius_md,
        theme.chassis_shadow,
    )));

    let mut mesh = Mesh::default();
    for (point, color) in [
        (rect.left_top(), theme.chassis_top),
        (rect.right_top(), theme.chassis_top),
        (rect.right_bottom(), theme.chassis_bottom),
        (rect.left_bottom(), theme.chassis_bottom),
    ] {
        mesh.vertices.push(Vertex {
            pos: point,
            uv: WHITE_UV,
            color,
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    shapes.push(Shape::mesh(mesh));

    brushed_lines(&mut shapes, rect);

    // The lit top edge and the dark border give the panel its thickness.
    shapes.push(Shape::line_segment(
        [
            pos2(rect.min.x + 3.0, rect.min.y + 0.5),
            pos2(rect.max.x - 3.0, rect.min.y + 0.5),
        ],
        Stroke::new(1.0, theme.chassis_edge),
    ));
    shapes.push(Shape::Rect(RectShape::stroke(
        rect,
        theme.radius_md,
        Stroke::new(theme.stroke_thin, theme.chassis_shadow),
        StrokeKind::Inside,
    )));

    for corner in [
        pos2(rect.min.x + SCREW_INSET, rect.min.y + SCREW_INSET),
        pos2(rect.max.x - SCREW_INSET, rect.min.y + SCREW_INSET),
        pos2(rect.min.x + SCREW_INSET, rect.max.y - SCREW_INSET),
        pos2(rect.max.x - SCREW_INSET, rect.max.y - SCREW_INSET),
    ] {
        screw(&mut shapes, theme, corner);
    }

    Shape::Vec(shapes)
}

/// The fine horizontal grain of brushed aluminium.
///
/// Drawn at a fixed spacing rather than per pixel: the point is a texture the
/// eye reads as metal, not a faithful simulation.
fn brushed_lines(shapes: &mut Vec<Shape>, rect: Rect) {
    let grain = Color32::from_white_alpha(9);
    let mut y = rect.min.y + BRUSH_SPACING;
    while y < rect.max.y {
        shapes.push(Shape::line_segment(
            [pos2(rect.min.x + 2.0, y), pos2(rect.max.x - 2.0, y)],
            Stroke::new(0.5, grain),
        ));
        y += BRUSH_SPACING;
    }
}

/// One screw head, lit from the top left.
fn screw(shapes: &mut Vec<Shape>, theme: &Theme, centre: Pos2) {
    shapes.push(Shape::Circle(CircleShape::filled(
        centre,
        SCREW_RADIUS,
        theme.chassis_shadow,
    )));
    shapes.push(Shape::Circle(CircleShape::filled(
        centre - vec2(0.4, 0.4),
        SCREW_RADIUS - 0.8,
        theme.screw_highlight,
    )));
    shapes.push(Shape::Circle(CircleShape::filled(
        centre,
        SCREW_RADIUS - 1.6,
        theme.screw,
    )));
    // The slot, turned a little so the screws do not look stamped out.
    shapes.push(Shape::line_segment(
        [
            centre + vec2(-SCREW_RADIUS * 0.6, -SCREW_RADIUS * 0.3),
            centre + vec2(SCREW_RADIUS * 0.6, SCREW_RADIUS * 0.3),
        ],
        Stroke::new(1.2, theme.chassis_shadow),
    ));
}

/// Draw an indicator lamp.
///
/// `color` is `None` when the lamp is dark; a lit lamp gets a halo, because a
/// flat dot reads as a printed mark rather than as a light.
pub fn lamp(painter: &Painter, theme: &Theme, centre: Pos2, color: Option<Color32>) {
    painter.circle_filled(centre, LED_RADIUS + 1.5, theme.chassis_shadow);
    match color {
        Some(color) => {
            painter.circle_filled(centre, LED_RADIUS + 3.0, color.gamma_multiply(0.22));
            painter.circle_filled(centre, LED_RADIUS, color);
            painter.circle_filled(
                centre - vec2(1.0, 1.0),
                LED_RADIUS * 0.4,
                Color32::from_white_alpha(150),
            );
        }
        None => {
            painter.circle_filled(centre, LED_RADIUS, theme.led_off);
        }
    }
}

/// The legend printed along the top of a panel, with its lamp.
pub fn panel_header(ui: &Ui, theme: &Theme, rect: Rect, title: &str, lit: Option<Color32>) {
    let painter = ui.painter();
    let centre_y = rect.min.y + HEADER_HEIGHT * 0.5;

    lamp(painter, theme, pos2(rect.min.x + 7.0, centre_y), lit);
    painter.text(
        pos2(rect.min.x + 20.0, centre_y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(theme.font_md),
        theme.title,
    );
}

/// Cut a recessed area into the metal, the way a display or a pad bay sits in
/// a real panel.
pub fn inset(painter: &Painter, theme: &Theme, rect: Rect, fill: Color32) {
    // A light hairline below the opening reads as the far wall of the cut.
    painter.rect_filled(
        rect.translate(vec2(0.0, 1.0)),
        theme.radius_sm,
        theme.chassis_edge,
    );
    painter.rect_filled(rect, theme.radius_sm, fill);
    painter.rect_stroke(
        rect,
        theme.radius_sm,
        Stroke::new(theme.stroke_thin, theme.chassis_shadow),
        StrokeKind::Inside,
    );
}
