use nih_plug_egui::egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, Ui};

use crate::theme::Theme;
use crate::widgets::surface::label_size;

/// Thickness of the bar under the selected tab.
const UNDERLINE: f32 = 3.0;

/// A row of tabs across the top of the editor.
///
/// Returns the index that was clicked this frame, if any. Drawn flat with an
/// accent bar rather than as raised buttons: the tabs name where you are, they
/// are not controls that do something.
pub fn tab_bar(ui: &mut Ui, theme: &Theme, labels: &[&str], selected: usize) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }

    let height = theme.font_md + theme.spacing_md * 2.0 + UNDERLINE;
    let widths: Vec<f32> = labels
        .iter()
        .map(|label| label_size(ui, theme, label).x)
        .collect();
    let total: f32 = widths.iter().sum();
    // The id comes from the allocated area rather than from the enclosing Ui,
    // so the tabs cannot collide with another control in the same scope.
    let (rect, response) = ui.allocate_exact_size(
        vec2(total.min(ui.available_width()), height),
        Sense::hover(),
    );
    let id = response.id;

    let mut clicked = None;
    let mut x = rect.min.x;

    for (index, label) in labels.iter().enumerate() {
        let bounds = Rect::from_min_size(pos2(x, rect.min.y), vec2(widths[index], height));
        x += widths[index];

        let response = ui.interact(bounds, id.with(("tab", index)), Sense::click());
        if response.clicked() {
            clicked = Some(index);
        }

        let is_selected = index == selected;
        let painter = ui.painter();
        let color = if is_selected {
            theme.accent
        } else if response.hovered() {
            theme.text
        } else {
            theme.text_dim
        };

        painter.text(
            pos2(bounds.center().x, bounds.min.y + theme.spacing_md),
            Align2::CENTER_TOP,
            *label,
            FontId::proportional(theme.font_md),
            color,
        );

        if is_selected {
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(bounds.min.x + theme.spacing_sm, bounds.max.y - UNDERLINE),
                    vec2(bounds.width() - theme.spacing_sm * 2.0, UNDERLINE),
                ),
                theme.radius_sm,
                theme.accent,
            );
        }
    }

    // A hairline along the whole width ties the unselected tabs to the page.
    ui.painter().line_segment(
        [
            pos2(rect.min.x, rect.max.y - UNDERLINE * 0.5),
            pos2(ui.max_rect().max.x, rect.max.y - UNDERLINE * 0.5),
        ],
        Stroke::new(1.0, theme.outline),
    );

    clicked
}

/// A small round indicator, the way a panel light reads at a glance.
pub fn led(ui: &Ui, theme: &Theme, centre: nih_plug_egui::egui::Pos2, lit: Option<Color32>) {
    let painter = ui.painter();
    let radius = 4.0;

    painter.circle_filled(centre, radius + 1.0, theme.control_pressed_bg);
    match lit {
        Some(color) => {
            // A ring of the same colour at low alpha stands in for a glow.
            painter.circle_filled(centre, radius + 2.0, color.gamma_multiply(0.25));
            painter.circle_filled(centre, radius, color);
        }
        None => {
            painter.circle_stroke(centre, radius, Stroke::new(1.0, theme.outline));
        }
    }
}
