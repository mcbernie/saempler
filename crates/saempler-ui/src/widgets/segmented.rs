use nih_plug_egui::egui::{pos2, vec2, Align2, FontId, Rect, Sense, StrokeKind, Ui};

use crate::theme::Theme;

/// A horizontal row of mutually exclusive options.
///
/// Returns the index that was clicked this frame, if any. The caller decides
/// what a selection means, so the widget stays independent of the model.
pub fn segmented(
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
    let (rect, _response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, theme.radius_md, theme.panel_bg);

    let segment_width = rect.width() / labels.len() as f32;
    let mut clicked = None;

    for (index, label) in labels.iter().enumerate() {
        let segment = Rect::from_min_size(
            pos2(rect.min.x + segment_width * index as f32, rect.min.y),
            vec2(segment_width, rect.height()),
        )
        .shrink(theme.spacing_sm * 0.5);

        let response = ui.interact(segment, ui.id().with(("segment", index)), Sense::click());
        if response.clicked() {
            clicked = Some(index);
        }

        let painter = ui.painter();
        let is_selected = index == selected;
        let background = if is_selected {
            theme.accent
        } else if response.hovered() {
            theme.control_hover_bg
        } else {
            theme.control_bg
        };
        painter.rect_filled(segment, theme.radius_sm, background);
        if is_selected {
            painter.rect_stroke(
                segment,
                theme.radius_sm,
                theme.accent_stroke(),
                StrokeKind::Inside,
            );
        }

        let text_color = if is_selected {
            theme.window_bg
        } else {
            theme.text
        };
        painter.text(
            segment.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(theme.font_md),
            text_color,
        );
    }

    clicked
}

/// A flat momentary button drawn in the product style.
pub fn button(ui: &mut Ui, theme: &Theme, label: &str, width: f32) -> bool {
    let height = theme.font_md + theme.spacing_md * 2.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());

    let painter = ui.painter();
    let background = if response.is_pointer_button_down_on() {
        theme.accent
    } else if response.hovered() {
        theme.control_hover_bg
    } else {
        theme.control_bg
    };
    painter.rect_filled(rect, theme.radius_sm, background);
    painter.rect_stroke(
        rect,
        theme.radius_sm,
        theme.outline_stroke(),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(theme.font_md),
        if response.is_pointer_button_down_on() {
            theme.window_bg
        } else {
            theme.text
        },
    );

    response.clicked()
}
