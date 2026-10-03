use nih_plug_egui::egui::{
    epaint::PathShape, popup_below_widget, pos2, vec2, Align2, Color32, FontId, Painter,
    PopupCloseBehavior, Pos2, Sense, Stroke, Ui,
};

use crate::theme::Theme;
use crate::widgets::surface::{control_surface, label_color, SurfaceState, LABEL_PADDING};

/// A list of options opened from a closed control.
///
/// Used where a segmented row would be too wide to fit, as in the rows of the
/// modulation matrix. The closed control carries a mark, so it reads as a list
/// rather than as a button, and clicking it opens the options over the page.
///
/// Opening and closing go through egui's own popup handling, so a click
/// outside puts the list away and two lists are never open at once.
///
/// Returns the index chosen this frame, if any.
pub fn dropdown(
    ui: &mut Ui,
    theme: &Theme,
    id: impl std::hash::Hash,
    labels: &[&str],
    selected: usize,
    width: f32,
) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }

    let height = theme.font_md + theme.spacing_md * 2.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let popup_id = ui.make_persistent_id(("dropdown", id));

    if response.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup_id));
    }

    let open = ui.memory(|memory| memory.is_popup_open(popup_id));
    let state = if open {
        SurfaceState::Selected
    } else if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };
    control_surface(ui, theme, rect, state);

    let painter = ui.painter();
    painter.text(
        pos2(rect.min.x + LABEL_PADDING * 0.6, rect.center().y),
        Align2::LEFT_CENTER,
        labels.get(selected).copied().unwrap_or("—"),
        FontId::proportional(theme.font_md),
        label_color(theme, state),
    );
    chevron(
        painter,
        pos2(rect.max.x - LABEL_PADDING * 0.8, rect.center().y),
        theme.font_sm * 0.34,
        theme.text_dim,
    );

    let mut chosen = None;
    popup_below_widget(
        ui,
        popup_id,
        &response,
        PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
            for (index, label) in labels.iter().enumerate() {
                if option(ui, theme, label, index == selected, width) {
                    chosen = Some(index);
                }
            }
        },
    );

    chosen
}

/// One row of an open list.
fn option(ui: &mut Ui, theme: &Theme, label: &str, selected: bool, width: f32) -> bool {
    let height = theme.font_md + theme.spacing_md * 1.4;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());

    let state = if response.hovered() {
        SurfaceState::Hover
    } else if selected {
        SurfaceState::Selected
    } else {
        SurfaceState::Rest
    };
    control_surface(ui, theme, rect, state);
    ui.painter().text(
        pos2(rect.min.x + LABEL_PADDING * 0.6, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme.font_md),
        label_color(theme, state),
    );

    response.clicked()
}

/// The small mark that says a control opens a list.
fn chevron(painter: &Painter, centre: Pos2, size: f32, color: Color32) {
    painter.add(PathShape::convex_polygon(
        vec![
            pos2(centre.x - size * 2.0, centre.y - size),
            pos2(centre.x + size * 2.0, centre.y - size),
            pos2(centre.x, centre.y + size * 1.4),
        ],
        color,
        Stroke::NONE,
    ));
}
