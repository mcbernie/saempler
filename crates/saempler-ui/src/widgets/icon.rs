use nih_plug_egui::egui::{pos2, vec2, Color32, Painter, Rect, Sense, Stroke, Ui};

use crate::theme::Theme;
use crate::widgets::surface::{control_surface, label_color, SurfaceState};

/// Side length of an icon button.
const BUTTON_SIZE: f32 = 30.0;
/// Side length of the glyph inside it.
const GLYPH_SIZE: f32 = 15.0;

/// The marks drawn on the icon buttons.
///
/// Drawn rather than typed: the interface font carries no symbol set worth
/// using, and a missing glyph renders as an empty box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Open a file.
    Open,
    /// Throw the selection away.
    Trash,
    /// Silence everything.
    Stop,
    /// Lay the chops across the keyboard.
    Keyboard,
    /// Duplicate.
    Copy,
    /// Empty the lot.
    Clear,
    /// Open an editor window.
    Edit,
    /// Add one.
    Plus,
    /// Take this one out.
    Cross,
    /// Step back.
    Previous,
    /// Step on.
    Next,
}

/// A square button carrying a drawn mark, with its meaning on hover.
///
/// Returns true when it was clicked this frame.
pub fn icon_button(ui: &mut Ui, theme: &Theme, icon: Icon, tooltip: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON_SIZE, BUTTON_SIZE), Sense::click());

    let state = if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };
    control_surface(ui, theme, rect, state);

    let glyph = Rect::from_center_size(rect.center(), vec2(GLYPH_SIZE, GLYPH_SIZE));
    draw(ui.painter(), icon, glyph, label_color(theme, state), theme);

    // Without the label the mark has to be explained somewhere, and the
    // pointer resting on it is the only place that costs no room.
    response.on_hover_text(tooltip).clicked()
}

/// A button carrying a mark and a label, for actions whose name earns the room.
pub fn icon_label_button(ui: &mut Ui, theme: &Theme, icon: Icon, label: &str) -> bool {
    let width = BUTTON_SIZE + label.chars().count() as f32 * theme.font_md * 0.62 + 12.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, BUTTON_SIZE), Sense::click());

    let state = if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };
    control_surface(ui, theme, rect, state);

    let color = label_color(theme, state);
    let glyph = Rect::from_center_size(
        pos2(rect.min.x + BUTTON_SIZE * 0.5, rect.center().y),
        vec2(GLYPH_SIZE, GLYPH_SIZE),
    );
    draw(ui.painter(), icon, glyph, color, theme);
    ui.painter().text(
        pos2(rect.min.x + BUTTON_SIZE - 2.0, rect.center().y),
        nih_plug_egui::egui::Align2::LEFT_CENTER,
        label,
        nih_plug_egui::egui::FontId::proportional(theme.font_md),
        color,
    );

    response.clicked()
}

/// Draw one mark inside `rect`.
fn draw(painter: &Painter, icon: Icon, rect: Rect, color: Color32, theme: &Theme) {
    let stroke = Stroke::new(theme.stroke_thin + 0.4, color);
    let (left, right, top, bottom) = (rect.min.x, rect.max.x, rect.min.y, rect.max.y);
    let centre = rect.center();

    match icon {
        Icon::Open => {
            // A folder: the tab behind, the body in front.
            painter.line_segment([pos2(left, top + 3.0), pos2(left + 6.0, top + 3.0)], stroke);
            painter.rect_stroke(
                Rect::from_min_max(pos2(left, top + 3.0), pos2(right, bottom)),
                theme.radius_sm,
                stroke,
                nih_plug_egui::egui::StrokeKind::Inside,
            );
        }
        Icon::Trash => {
            painter.line_segment([pos2(left, top + 3.0), pos2(right, top + 3.0)], stroke);
            painter.line_segment(
                [pos2(centre.x - 2.0, top), pos2(centre.x + 2.0, top)],
                stroke,
            );
            painter.rect_stroke(
                Rect::from_min_max(pos2(left + 2.0, top + 3.0), pos2(right - 2.0, bottom)),
                theme.radius_sm,
                stroke,
                nih_plug_egui::egui::StrokeKind::Inside,
            );
        }
        Icon::Stop => {
            painter.rect_filled(rect.shrink(2.0), theme.radius_sm, color);
        }
        Icon::Keyboard => {
            // Three keys with two sharps on top: the keyboard the chops go to.
            painter.rect_stroke(
                rect,
                theme.radius_sm,
                stroke,
                nih_plug_egui::egui::StrokeKind::Inside,
            );
            let step = rect.width() / 3.0;
            for index in 1..3 {
                let x = left + step * index as f32;
                painter.line_segment([pos2(x, top), pos2(x, bottom)], stroke);
                painter.rect_filled(
                    Rect::from_min_max(pos2(x - 2.0, top), pos2(x + 2.0, centre.y)),
                    0,
                    color,
                );
            }
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(pos2(left, top), pos2(right - 4.0, bottom - 4.0)),
                theme.radius_sm,
                stroke,
                nih_plug_egui::egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                Rect::from_min_max(pos2(left + 4.0, top + 4.0), pos2(right, bottom)),
                theme.radius_sm,
                stroke,
                nih_plug_egui::egui::StrokeKind::Inside,
            );
        }
        Icon::Clear => {
            painter.circle_stroke(centre, rect.width() * 0.5, stroke);
            painter.line_segment([centre + vec2(-3.5, 3.5), centre + vec2(3.5, -3.5)], stroke);
        }
        Icon::Edit => {
            // A pencil: the shaft, and a tip that touches the lower left.
            painter.line_segment([pos2(left + 2.0, bottom - 2.0), pos2(right, top)], stroke);
            painter.line_segment(
                [
                    pos2(left + 2.0, bottom - 2.0),
                    pos2(left + 2.0, bottom - 6.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(left + 2.0, bottom - 2.0),
                    pos2(left + 6.0, bottom - 2.0),
                ],
                stroke,
            );
        }
        Icon::Plus => {
            painter.line_segment([pos2(centre.x, top), pos2(centre.x, bottom)], stroke);
            painter.line_segment([pos2(left, centre.y), pos2(right, centre.y)], stroke);
        }
        Icon::Cross => {
            painter.line_segment([pos2(left, top), pos2(right, bottom)], stroke);
            painter.line_segment([pos2(right, top), pos2(left, bottom)], stroke);
        }
        Icon::Previous | Icon::Next => {
            let direction = if icon == Icon::Previous { -1.0 } else { 1.0 };
            painter.add(nih_plug_egui::egui::epaint::PathShape::convex_polygon(
                vec![
                    pos2(centre.x - 3.0 * direction, top + 1.0),
                    pos2(centre.x + 4.0 * direction, centre.y),
                    pos2(centre.x - 3.0 * direction, bottom - 1.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
    }
}
