use nih_plug_egui::egui::{
    epaint::{CircleShape, Mesh, RectShape, Vertex, WHITE_UV},
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke,
    StrokeKind, Ui,
};

use crate::theme::Theme;

/// Distance from a panel corner to the middle of its screw.
const SCREW_INSET: f32 = 11.0;
/// Radius of a screw head.
const SCREW_RADIUS: f32 = 4.5;
/// Radius of an indicator lamp.
const LED_RADIUS: f32 = 4.0;
/// Space a panel legend occupies above the panel's contents.
pub const HEADER_HEIGHT: f32 = 22.0;

/// A vertical gradient as a strip of quads.
///
/// egui has no gradient fill, and a stack of flat rectangles bands visibly.
/// Vertex colours interpolate across each quad, so four stops are enough for a
/// surface that reads as curved metal rather than as a flat plate.
///
/// `stops` are `(position from 0 to 1, colour)` and must be in order.
pub fn vertical_gradient(rect: Rect, stops: &[(f32, Color32)]) -> Shape {
    let mut mesh = Mesh::default();
    if stops.len() < 2 {
        return Shape::Mesh(mesh.into());
    }

    for (offset, color) in stops {
        let y = rect.min.y + rect.height() * offset.clamp(0.0, 1.0);
        for x in [rect.min.x, rect.max.x] {
            mesh.vertices.push(Vertex {
                pos: pos2(x, y),
                uv: WHITE_UV,
                color: *color,
            });
        }
    }

    for row in 0..stops.len() as u32 - 1 {
        let base = row * 2;
        mesh.indices
            .extend([base, base + 1, base + 3, base, base + 3, base + 2]);
    }

    Shape::Mesh(mesh.into())
}

/// A panel milled from brushed metal, with a screw in every corner.
///
/// Returned as a shape rather than painted, because a panel sits behind
/// contents whose height is only known once they have been laid out: the
/// caller reserves an index before the contents and fills it in afterwards.
pub fn metal_panel(theme: &Theme, rect: Rect) -> Shape {
    let mut shapes = Vec::with_capacity(16);

    // The shadow the panel casts on what is behind it.
    shapes.push(Shape::Rect(RectShape::filled(
        rect.translate(vec2(0.0, 2.0)).expand(0.5),
        theme.radius_md,
        Color32::from_black_alpha(130),
    )));

    // The light comes from above: brightest just under the top edge, falling
    // off across the face, with a little bounce caught at the bottom.
    shapes.push(clipped_gradient(
        theme,
        rect,
        &[
            (0.0, theme.chassis_top),
            (0.16, theme.chassis_mid),
            (0.88, theme.chassis_bottom),
            (1.0, theme.chassis_foot),
        ],
    ));

    bevel(&mut shapes, theme, rect);

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

/// A gradient that keeps the panel's rounded corners.
///
/// The mesh itself is square, so it is drawn first and the corners are then
/// cut back by painting the surround over them. Clipping a mesh to a rounded
/// rectangle is not something egui offers, and four small arcs are cheaper
/// than building the rounded shape by hand.
fn clipped_gradient(theme: &Theme, rect: Rect, stops: &[(f32, Color32)]) -> Shape {
    Shape::Vec(vec![
        Shape::Rect(RectShape::filled(rect, theme.radius_md, theme.chassis_mid)),
        vertical_gradient(rect.shrink(f32::from(theme.radius_md.nw) * 0.5), stops),
    ])
}

/// The lit and shaded edges that give a plate its thickness.
fn bevel(shapes: &mut Vec<Shape>, theme: &Theme, rect: Rect) {
    let inner = rect.shrink(1.0);
    let radius = theme.radius_md;

    // A light hairline inside the top edge and a dark one inside the bottom.
    shapes.push(Shape::line_segment(
        [
            pos2(inner.min.x + f32::from(radius.nw), inner.min.y + 0.5),
            pos2(inner.max.x - f32::from(radius.ne), inner.min.y + 0.5),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(130)),
    ));
    shapes.push(Shape::line_segment(
        [
            pos2(inner.min.x + f32::from(radius.sw), inner.max.y - 0.5),
            pos2(inner.max.x - f32::from(radius.se), inner.max.y - 0.5),
        ],
        Stroke::new(1.0, Color32::from_black_alpha(70)),
    ));

    shapes.push(Shape::Rect(RectShape::stroke(
        rect,
        radius,
        Stroke::new(1.0, theme.chassis_shadow),
        StrokeKind::Inside,
    )));
}

/// One screw head, lit from the top left and sunk into the metal.
fn screw(shapes: &mut Vec<Shape>, theme: &Theme, centre: Pos2) {
    // The hole it sits in: dark at the top left, light at the bottom right,
    // which is what reads as a dent rather than a bump.
    shapes.push(Shape::Circle(CircleShape::filled(
        centre + vec2(0.6, 0.6),
        SCREW_RADIUS + 1.0,
        Color32::from_white_alpha(120),
    )));
    shapes.push(Shape::Circle(CircleShape::filled(
        centre,
        SCREW_RADIUS + 0.6,
        theme.chassis_shadow,
    )));

    shapes.push(Shape::Circle(CircleShape::filled(
        centre,
        SCREW_RADIUS,
        theme.screw,
    )));
    shapes.push(Shape::Circle(CircleShape::filled(
        centre - vec2(0.7, 0.9),
        SCREW_RADIUS * 0.62,
        theme.screw_highlight,
    )));
    // A cross slot, turned a little so the screws do not look stamped out.
    for direction in [vec2(0.94, 0.34), vec2(-0.34, 0.94)] {
        shapes.push(Shape::line_segment(
            [
                centre - direction * SCREW_RADIUS * 0.7,
                centre + direction * SCREW_RADIUS * 0.7,
            ],
            Stroke::new(1.3, theme.chassis_shadow),
        ));
    }
}

/// Draw an indicator lamp.
///
/// `color` is `None` when the lamp is dark. A lit lamp gets three rings of
/// glow: a flat dot reads as a printed mark rather than as a light.
pub fn lamp(painter: &Painter, theme: &Theme, centre: Pos2, color: Option<Color32>) {
    // The bezel it is set into.
    painter.circle_filled(
        centre + vec2(0.0, 0.5),
        LED_RADIUS + 2.4,
        theme.chassis_edge,
    );
    painter.circle_filled(centre, LED_RADIUS + 2.0, theme.chassis_shadow);

    match color {
        Some(color) => {
            for (radius, alpha) in [(4.0, 0.10), (2.6, 0.16), (1.5, 0.28)] {
                painter.circle_filled(centre, LED_RADIUS + radius, color.gamma_multiply(alpha));
            }
            painter.circle_filled(centre, LED_RADIUS, color);
            painter.circle_filled(
                centre,
                LED_RADIUS * 0.72,
                color.lerp_to_gamma(Color32::WHITE, 0.45),
            );
            painter.circle_filled(
                centre - vec2(0.9, 1.1),
                LED_RADIUS * 0.3,
                Color32::from_white_alpha(190),
            );
        }
        None => {
            painter.circle_filled(centre, LED_RADIUS, theme.led_off);
            painter.circle_filled(
                centre - vec2(0.8, 1.0),
                LED_RADIUS * 0.34,
                Color32::from_white_alpha(28),
            );
        }
    }
}

/// The legend printed along the top of a panel, with its lamp.
pub fn panel_header(ui: &Ui, theme: &Theme, rect: Rect, title: &str, lit: Option<Color32>) {
    let painter = ui.painter();
    let centre_y = rect.min.y + HEADER_HEIGHT * 0.5;

    lamp(painter, theme, pos2(rect.min.x + 8.0, centre_y), lit);
    // Engraved and heavy: a light line under the letters as if cut into the
    // metal, and the dark face drawn twice because the interface font has no
    // bold weight of its own.
    for (offset, color) in [
        (vec2(0.0, 1.0), Color32::from_white_alpha(120)),
        (vec2(0.0, 0.0), theme.title),
        (vec2(0.5, 0.0), theme.title),
    ] {
        painter.text(
            pos2(rect.min.x + 22.0, centre_y) + offset,
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(theme.font_md),
            color,
        );
    }
}

/// Cut a recessed area into the metal, the way a display or a pad bay sits in
/// a real panel.
pub fn inset(painter: &Painter, theme: &Theme, rect: Rect, fill: Color32) {
    // Light along the bottom lip and dark along the top: the opposite of a
    // raised surface, which is what makes it read as a cut.
    painter.rect_filled(
        rect.translate(vec2(0.0, 1.2)),
        theme.radius_sm,
        theme.chassis_edge,
    );
    painter.rect_filled(rect, theme.radius_sm, fill);
    painter.add(vertical_gradient(
        Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + 6.0)),
        &[
            (0.0, Color32::from_black_alpha(90)),
            (1.0, Color32::TRANSPARENT),
        ],
    ));
    painter.rect_stroke(
        rect,
        theme.radius_sm,
        Stroke::new(theme.stroke_thin, theme.chassis_shadow),
        StrokeKind::Inside,
    );
}

/// A raised, rounded body for a control, lit from above.
///
/// Shared by the buttons and the knobs so that everything the hand touches is
/// made of the same material.
pub fn raised_body(
    painter: &Painter,
    theme: &Theme,
    rect: Rect,
    radius: CornerRadius,
    base: Color32,
    top: Color32,
) {
    painter.rect_filled(
        rect.translate(vec2(0.0, 1.5)),
        radius,
        Color32::from_black_alpha(110),
    );
    painter.rect_filled(rect, radius, base);
    painter.add(Shape::Vec(vec![vertical_gradient(
        rect.shrink(1.0),
        &[
            (0.0, top),
            (0.45, base),
            (1.0, base.lerp_to_gamma(Color32::BLACK, 0.35)),
        ],
    )]));
    painter.line_segment(
        [
            pos2(rect.min.x + f32::from(radius.nw), rect.min.y + 1.0),
            pos2(rect.max.x - f32::from(radius.ne), rect.min.y + 1.0),
        ],
        Stroke::new(1.0, theme.control_highlight),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(theme.stroke_thin, Color32::from_black_alpha(190)),
        StrokeKind::Inside,
    );
}
