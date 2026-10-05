use nih_plug_egui::egui::{
    epaint::{Mesh, Vertex, WHITE_UV},
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Shadow, Shape,
    Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::Theme;
use crate::widgets::texture::{self, Textures, PANEL_CORNER};

/// Radius of an indicator lamp.
const LED_RADIUS: f32 = 4.0;
/// Space a panel legend occupies above the panel's contents.
pub const HEADER_HEIGHT: f32 = 22.0;
/// Size of a panel title.
const TITLE_SIZE: f32 = 15.0;
/// Radius of a screw head.
const SCREW_RADIUS: f32 = 6.0;
/// Corner radius of the plate in the panel texture, in points.
const PANEL_RADIUS: u8 = 8;
/// Width of the milled edge around a plate, which the grain stays inside.
const PANEL_EDGE: f32 = 7.0;

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

/// An ivory front plate, cut from the design render.
///
/// Returned as a shape rather than painted, because a panel sits behind
/// contents whose height is only known once they have been laid out: the
/// caller reserves an index before the contents and fills it in afterwards.
pub fn metal_panel(textures: &Textures, rect: Rect) -> Shape {
    Shape::Vec(vec![
        // A short, soft contact shadow, thrown down and to the right by a
        // light in the top left.
        Shadow {
            offset: [1, 2],
            blur: 6,
            spread: 0,
            color: Color32::from_black_alpha(46),
        }
        .as_shape(rect, CornerRadius::same(PANEL_RADIUS))
        .into(),
        texture::nine_slice(&textures.panel, rect, PANEL_CORNER, Color32::WHITE),
        // The grain stops short of the milled edge, which has its own.
        texture::grain(
            &textures.panel_grain,
            rect.shrink(PANEL_EDGE),
            Color32::WHITE.gamma_multiply(0.7),
        ),
    ])
}

/// A screw holding a plate down.
pub fn screw(painter: &Painter, textures: &Textures, centre: Pos2) {
    // Its own little contact shadow; the texture is cut tight to the head.
    painter.circle_filled(
        centre + vec2(0.6, 1.0),
        SCREW_RADIUS + 0.6,
        Color32::from_black_alpha(50),
    );
    painter.add(texture::image(
        &textures.screw,
        Rect::from_center_size(centre, vec2(SCREW_RADIUS, SCREW_RADIUS) * 2.0),
        Color32::WHITE,
    ));
}

/// Draw an indicator lamp.
///
/// `color` is `None` when the lamp is dark. Both states are the glass from
/// the render; a lit one is tinted to its colour and throws a little glow
/// onto the plate, because a flat dot reads as a printed mark, not a light.
pub fn lamp(painter: &Painter, theme: &Theme, centre: Pos2, color: Option<Color32>) {
    let _ = theme;
    let textures = texture::textures(painter.ctx());
    let rect = Rect::from_center_size(centre, Vec2::splat((LED_RADIUS + 1.5) * 2.0));
    match color {
        Some(color) => {
            for (radius, alpha) in [(5.0, 0.08), (3.4, 0.14), (2.0, 0.24)] {
                painter.circle_filled(centre, LED_RADIUS + radius, color.gamma_multiply(alpha));
            }
            painter.add(texture::image(&textures.led_on, rect, color));
        }
        None => {
            painter.add(texture::image(&textures.led_off, rect, Color32::WHITE));
        }
    }
}

/// The legend printed along the top of a panel: a screw and the title. Returns where the title ends, so the caller can place controls
/// and the rule beside it.
pub fn panel_header(ui: &Ui, theme: &Theme, rect: Rect, title: &str) -> f32 {
    let painter = ui.painter();
    let centre_y = rect.center().y;
    let textures = texture::textures(ui.ctx());

    screw(
        painter,
        &textures,
        pos2(rect.min.x + SCREW_RADIUS, centre_y),
    );
    screw(
        painter,
        &textures,
        pos2(rect.max.x - SCREW_RADIUS, centre_y),
    );

    // Printed heavy: the interface font has no bold weight of its own, so
    // the face is drawn twice, half a point apart, over a light line that
    // sets it into the plate.
    let font = FontId::proportional(TITLE_SIZE);
    let anchor = pos2(rect.min.x + SCREW_RADIUS * 2.0 + 10.0, centre_y);
    let mut end = anchor.x;
    for (offset, color) in [
        (vec2(0.0, 1.0), theme.chassis_top),
        (vec2(0.0, 0.0), theme.title),
        (vec2(0.5, 0.0), theme.title),
    ] {
        let drawn = painter.text(
            anchor + offset,
            Align2::LEFT_CENTER,
            title,
            font.clone(),
            color,
        );
        end = end.max(drawn.max.x);
    }
    end
}

/// The engraved rule a panel title runs into, from `from` to the panel's
/// lamp at the right end of the header.
///
/// The lamp takes the place of a second screw: an unlit lamp and a screw
/// side by side read as two screws.
pub fn header_rule(painter: &Painter, theme: &Theme, rect: Rect, from: f32, lit: Option<Color32>) {
    let centre_y = rect.center().y;
    let lamp_x = rect.max.x - SCREW_RADIUS;
    lamp(painter, theme, pos2(lamp_x, centre_y), lit);

    let to = lamp_x - LED_RADIUS - 10.0;
    if to - from < 12.0 {
        return;
    }
    let y = centre_y.round() + 0.5;
    painter.line_segment(
        [pos2(from, y), pos2(to, y)],
        Stroke::new(1.0_f32, theme.chassis_shadow.gamma_multiply(0.55)),
    );
    painter.line_segment(
        [pos2(from, y + 1.0), pos2(to, y + 1.0)],
        Stroke::new(1.0_f32, theme.chassis_top),
    );
}

/// Cut a recessed area into the metal, the way a display or a pad bay sits in
/// a real panel.
pub fn inset(painter: &Painter, theme: &Theme, rect: Rect, fill: Color32) {
    // Light along the bottom lip and dark along the top: the opposite of a
    // raised surface, which is what makes it read as a cut.
    painter.rect_filled(
        rect.translate(vec2(0.0, 1.2)),
        theme.radius_sm,
        theme.chassis_edge.gamma_multiply(0.7),
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
        Stroke::new(theme.stroke_thin, Color32::from_black_alpha(150)),
        StrokeKind::Inside,
    );
}

/// A legend printed on the plate: capitals, heavy, in the label colour.
///
/// The interface font has no bold weight, so the text is drawn twice, half a
/// point apart.
pub fn legend(painter: &Painter, theme: &Theme, at: Pos2, align: Align2, text: &str) -> Rect {
    let text = text.to_uppercase();
    let font = FontId::proportional(theme.font_sm);
    let mut drawn = painter.text(at, align, &text, font.clone(), theme.label);
    drawn = drawn.union(painter.text(at + vec2(0.4, 0.0), align, &text, font, theme.label));
    drawn
}

/// A groove milled down the plate between two groups of controls.
pub fn divider(ui: &mut Ui, theme: &Theme, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(theme.spacing_md * 2.0, height), Sense::hover());
    let x = rect.center().x.round() + 0.5;
    let painter = ui.painter();
    painter.line_segment(
        [pos2(x, rect.min.y + 4.0), pos2(x, rect.max.y - 4.0)],
        Stroke::new(1.0_f32, theme.chassis_shadow.gamma_multiply(0.55)),
    );
    painter.line_segment(
        [
            pos2(x + 1.0, rect.min.y + 4.0),
            pos2(x + 1.0, rect.max.y - 4.0),
        ],
        Stroke::new(1.0_f32, theme.chassis_top),
    );
}

/// An area set off on a plate by a groove milled around it, for a group of
/// controls that belong together.
///
/// Light falls from the top left, so the groove's far wall catches it: a
/// light line just below and right of a dark one.
pub fn engraved(painter: &Painter, theme: &Theme, rect: Rect) {
    painter.rect_stroke(
        rect.translate(vec2(0.8, 1.0)),
        theme.radius_md,
        Stroke::new(1.0_f32, theme.chassis_top),
        StrokeKind::Inside,
    );
    painter.rect_stroke(
        rect,
        theme.radius_md,
        Stroke::new(1.0_f32, theme.chassis_shadow.gamma_multiply(0.6)),
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
        Stroke::new(1.0_f32, theme.control_highlight),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(theme.stroke_thin, Color32::from_black_alpha(190)),
        StrokeKind::Inside,
    );
}
