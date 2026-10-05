use nih_plug_egui::egui::{
    emath::Rot2, epaint::Mesh, pos2, vec2, Color32, CursorIcon, Pos2, Rect, Sense, Shape, Ui,
};

use crate::theme::Theme;
use crate::widgets::dial::{caption_at, captioned_size, dial_rect, Placement};
use crate::widgets::texture::{self, SELECTOR_RING_RADIUS};

/// Angle between two neighbouring positions, in radians.
const STEP: f32 = 50.0 * std::f32::consts::PI / 180.0;
/// Widest the positions may spread, in radians: past that the outermost
/// ones would sit under the caption.
const MAX_SWEEP: f32 = 220.0 * std::f32::consts::PI / 180.0;
/// Room between the position dots and the ring.
const DOT_ROOM: f32 = 5.0;

/// A detented rotary selector: a chicken-head knob on a nickel ring.
///
/// Each option is a dot around the ring and the head points at the chosen
/// one; the caption under it names the choice. Clicking near a dot picks it,
/// dragging the head round picks whichever dot it is nearest, and clicking
/// the head itself steps to the next position.
///
/// Only the head turns. The ring and its light stay where they are, so the
/// part keeps reading as lit from the top left.
///
/// Returns the newly chosen index when it changed this frame.
pub fn rotary_selector(
    ui: &mut Ui,
    theme: &Theme,
    name: &str,
    labels: &[&str],
    selected: usize,
    diameter: f32,
    placement: Placement,
) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }
    let count = labels.len();
    let selected = selected.min(count - 1);

    let (rect, response) = ui.allocate_exact_size(
        captioned_size(theme, diameter, placement),
        Sense::click_and_drag(),
    );
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let dial = dial_rect(rect, diameter, placement);
    let centre = dial.center();
    let radius = diameter * 0.5;

    let mut chosen = None;
    if let Some(pointer) = response.interact_pointer_pos() {
        let offset = pointer - centre;
        if response.clicked() && offset.length() < radius * 0.45 {
            chosen = Some((selected + 1) % count);
        } else if response.clicked() || response.dragged() {
            let angle = offset.x.atan2(-offset.y);
            chosen = Some(nearest(angle, count));
        }
    }
    let chosen = chosen.filter(|index| *index != selected);
    let shown = chosen.unwrap_or(selected);

    let painter = ui.painter();
    for index in 0..count {
        let direction = direction_of(angle_of(index, count));
        if index == shown {
            painter.circle_filled(centre + direction * (radius - 1.5), 2.0, theme.accent);
        } else {
            painter.circle_filled(centre + direction * (radius - 1.5), 1.3, theme.title);
        }
    }

    let textures = texture::textures(ui.ctx());
    let side = (radius - DOT_ROOM) / SELECTOR_RING_RADIUS;
    let face = Rect::from_center_size(centre, vec2(side, side));
    painter.add(texture::image(
        &textures.selector_ring,
        face,
        Color32::WHITE,
    ));

    // The head with its own short shadow, both turned to the position.
    let turn = Rot2::from_angle(angle_of(shown, count));
    painter.add(turned(
        &textures.selector_head,
        face.translate(vec2(0.8, 1.6)),
        turn,
        centre + vec2(0.8, 1.6),
        Color32::from_black_alpha(110),
    ));
    painter.add(turned(
        &textures.selector_head,
        face,
        turn,
        centre,
        Color32::WHITE,
    ));

    caption_at(
        painter,
        theme,
        dial,
        placement,
        name,
        labels.get(shown).copied().unwrap_or(""),
    );

    chosen
}

/// A texture over `rect`, turned about `origin`.
fn turned(
    texture: &nih_plug_egui::egui::TextureHandle,
    rect: Rect,
    turn: Rot2,
    origin: Pos2,
    tint: Color32,
) -> Shape {
    let mut mesh = Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
    mesh.rotate(turn, origin);
    Shape::mesh(mesh)
}

/// The angle of a position, clockwise from straight up.
fn angle_of(index: usize, count: usize) -> f32 {
    if count < 2 {
        return 0.0;
    }
    let sweep = (STEP * (count - 1) as f32).min(MAX_SWEEP);
    -sweep * 0.5 + sweep * index as f32 / (count - 1) as f32
}

/// The position whose dot lies nearest an angle.
fn nearest(angle: f32, count: usize) -> usize {
    (0..count)
        .min_by(|a, b| {
            let distance = |index: usize| (angle_of(index, count) - angle).abs();
            distance(*a).total_cmp(&distance(*b))
        })
        .unwrap_or(0)
}

/// Unit vector for an angle measured clockwise from straight up.
fn direction_of(angle: f32) -> nih_plug_egui::egui::Vec2 {
    vec2(angle.sin(), -angle.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_spread_evenly_around_straight_up() {
        assert!((angle_of(2, 5)).abs() < 1e-6, "the middle points up");
        assert!((angle_of(0, 5) + angle_of(4, 5)).abs() < 1e-6);
        assert!(angle_of(0, 5) < angle_of(1, 5));
    }

    #[test]
    fn many_positions_stay_inside_the_widest_sweep() {
        let count = 12;

        let span = angle_of(count - 1, count) - angle_of(0, count);

        assert!(span <= MAX_SWEEP + 1e-5);
    }

    #[test]
    fn an_angle_picks_the_nearest_position() {
        assert_eq!(nearest(0.0, 5), 2);
        assert_eq!(nearest(angle_of(4, 5) + 0.3, 5), 4);
        assert_eq!(nearest(angle_of(1, 5) - 0.1, 5), 1);
        assert_eq!(nearest(1.0, 1), 0);
    }
}
