use nih_plug_egui::egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke, Ui};

use crate::theme::Theme;
use crate::widgets::surface::{
    control_surface, label_color, label_size, SurfaceState, LABEL_PADDING,
};

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

/// A button that stays lit while its setting is on.
///
/// Returns true when it was clicked this frame; the caller owns the value.
pub fn toggle(ui: &mut Ui, theme: &Theme, label: &str, on: bool) -> bool {
    let size = label_size(ui, theme, label);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let state = if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if on {
        SurfaceState::Selected
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

/// A button that steps through a fixed list of options.
///
/// Used where a segmented row would be too wide to fit, as in the modulation
/// matrix. Left click advances, right click steps back, so a value a few
/// places away is still two clicks rather than five.
pub fn cycle(
    ui: &mut Ui,
    theme: &Theme,
    labels: &[&str],
    selected: usize,
    width: f32,
) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }

    let height = theme.font_md + theme.spacing_md * 2.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());

    let state = if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };
    control_surface(ui, theme, rect, state);

    let painter = ui.painter();
    let label = labels.get(selected).copied().unwrap_or("—");
    painter.text(
        pos2(rect.min.x + LABEL_PADDING * 0.6, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme.font_md),
        label_color(theme, state),
    );

    // A small mark on the right so the control reads as a list, not a button.
    let marker = pos2(rect.max.x - LABEL_PADDING * 0.6, rect.center().y);
    let arrow = theme.font_sm * 0.34;
    painter.add(nih_plug_egui::egui::epaint::PathShape::convex_polygon(
        vec![
            pos2(marker.x - arrow * 2.0, marker.y - arrow),
            pos2(marker.x, marker.y - arrow),
            pos2(marker.x - arrow, marker.y + arrow),
        ],
        theme.text_dim,
        Stroke::NONE,
    ));

    let step = if response.clicked() {
        1
    } else if response.secondary_clicked() {
        labels.len() - 1
    } else {
        return None;
    };

    Some((selected + step) % labels.len())
}

#[cfg(test)]
mod tests {
    /// The step a click applies, factored out of [`cycle`] so the wrapping is
    /// testable without a running interface.
    fn advanced(selected: usize, len: usize, forwards: bool) -> usize {
        let step = if forwards { 1 } else { len - 1 };
        (selected + step) % len
    }

    #[test]
    fn stepping_forwards_wraps_to_the_start() {
        assert_eq!(advanced(0, 5, true), 1);
        assert_eq!(advanced(4, 5, true), 0);
    }

    #[test]
    fn stepping_backwards_wraps_to_the_end() {
        assert_eq!(advanced(0, 5, false), 4);
        assert_eq!(advanced(3, 5, false), 2);
    }

    #[test]
    fn a_single_option_stays_where_it_is() {
        assert_eq!(advanced(0, 1, true), 0);
        assert_eq!(advanced(0, 1, false), 0);
    }
}
