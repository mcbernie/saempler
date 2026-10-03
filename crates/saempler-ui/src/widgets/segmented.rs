use nih_plug_egui::egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke, Ui};

use crate::theme::Theme;
use crate::widgets::surface::{control_surface, label_color, label_size, SurfaceState};

/// Thickness of the bar marking the selected segment.
const SELECTION_BAR: f32 = 2.0;

/// A momentary button, sized to its label.
pub fn button(ui: &mut Ui, theme: &Theme, label: &str) -> bool {
    let size = label_size(ui, theme, label);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let state = if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };

    control_surface(ui, theme, rect, state);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(theme.font_md),
        label_color(theme, state),
    );

    response.clicked()
}

/// A row of mutually exclusive options sharing one recessed track.
///
/// Returns the index that was clicked this frame, if any. Pass `usize::MAX` as
/// `selected` when none of the options is the current value.
pub fn segmented(ui: &mut Ui, theme: &Theme, labels: &[&str], selected: usize) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }

    // Every segment gets the width of the widest label, so the row reads as
    // one control rather than as buttons that happen to sit together.
    let segment = labels
        .iter()
        .map(|label| label_size(ui, theme, label))
        .fold(vec2(0.0, 0.0), |acc, size| {
            vec2(acc.x.max(size.x), acc.y.max(size.y))
        });
    let total = vec2(segment.x * labels.len() as f32, segment.y);
    // The id comes from the allocated area rather than from the enclosing Ui,
    // so several selectors in one row do not collide.
    let (rect, response) = ui.allocate_exact_size(total, Sense::hover());
    let id = response.id;

    // The track is the recessed surface the segments sit in.
    ui.painter()
        .rect_filled(rect, theme.radius_sm, theme.control_pressed_bg);

    let mut clicked = None;
    for (index, label) in labels.iter().enumerate() {
        let bounds = Rect::from_min_size(
            pos2(rect.min.x + segment.x * index as f32, rect.min.y),
            segment,
        );
        let response = ui.interact(bounds, id.with(("segment", index)), Sense::click());
        if response.clicked() {
            clicked = Some(index);
        }

        let is_selected = index == selected;
        let state = if is_selected {
            SurfaceState::Selected
        } else if response.is_pointer_button_down_on() {
            SurfaceState::Pressed
        } else if response.hovered() {
            SurfaceState::Hover
        } else {
            SurfaceState::Rest
        };

        // Segments keep a hairline between them instead of their own border.
        if state == SurfaceState::Rest {
            let painter = ui.painter();
            painter.rect_filled(bounds.shrink(1.0), theme.radius_sm, theme.control_bg);
            if index > 0 {
                painter.line_segment(
                    [
                        pos2(bounds.min.x, bounds.min.y + 4.0),
                        pos2(bounds.min.x, bounds.max.y - 4.0),
                    ],
                    Stroke::new(1.0, theme.outline),
                );
            }
        } else {
            control_surface(ui, theme, bounds.shrink(1.0), state);
        }

        let painter = ui.painter();
        painter.text(
            bounds.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(theme.font_md),
            label_color(theme, state),
        );

        if is_selected {
            painter.line_segment(
                [
                    pos2(bounds.min.x + 6.0, bounds.max.y - SELECTION_BAR),
                    pos2(bounds.max.x - 6.0, bounds.max.y - SELECTION_BAR),
                ],
                Stroke::new(SELECTION_BAR, theme.accent),
            );
        }
    }

    clicked
}
