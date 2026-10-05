use nih_plug_egui::egui::{
    epaint::PathShape, popup_below_widget, pos2, vec2, Align, Align2, Color32, FontId, Painter,
    PopupCloseBehavior, Pos2, Response, ScrollArea, Sense, Stroke, Ui,
};

use crate::theme::Theme;
use crate::widgets::panel::legend;
use crate::widgets::surface::{control_surface, label_color, SurfaceState, LABEL_PADDING};

/// Rim of an open list: polished nickel, like the selector rings.
const LIST_RIM: Color32 = Color32::from_rgb(0xb8, 0xb1, 0xa3);
/// Tallest an open list grows before it scrolls: about ten rows.
const LIST_HEIGHT: f32 = 270.0;
/// Room on the left of a list row for the tick.
const TICK_ROOM: f32 = 26.0;

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

    let height = theme.control_height;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let popup_id = ui.make_persistent_id(("dropdown", id));

    // Opened this frame: the list scrolls its current value into view, so a
    // long one such as the keyboard does not open at the top.
    let just_opened = response.clicked() && !ui.memory(|memory| memory.is_popup_open(popup_id));
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

    // The value on the left, and the mark that opens the list in a field of
    // its own on the right, set off by a groove.
    let painter = ui.painter();
    painter.text(
        pos2(rect.min.x + LABEL_PADDING * 0.8, rect.center().y),
        Align2::LEFT_CENTER,
        labels.get(selected).copied().unwrap_or("—"),
        FontId::proportional(theme.font_md),
        label_color(theme, state),
    );
    let field = rect.height();
    let groove = rect.max.x - field;
    painter.line_segment(
        [
            pos2(groove, rect.min.y + 6.0),
            pos2(groove, rect.max.y - 6.0),
        ],
        Stroke::new(1.0_f32, Color32::from_black_alpha(140)),
    );
    painter.line_segment(
        [
            pos2(groove + 1.0, rect.min.y + 6.0),
            pos2(groove + 1.0, rect.max.y - 6.0),
        ],
        Stroke::new(1.0_f32, Color32::from_white_alpha(18)),
    );
    chevron(
        painter,
        pos2(groove + field * 0.5, rect.center().y),
        theme.font_sm * 0.3,
        open,
        theme.chassis_top,
    );

    // The list opens on a dark plate with a nickel rim, as a display would;
    // egui frames a popup from the style of the Ui it opens from.
    let mut chosen = None;
    ui.scope(|ui| {
        let visuals = &mut ui.style_mut().visuals;
        visuals.window_fill = theme.control_pressed_bg;
        visuals.window_stroke = Stroke::new(1.5_f32, LIST_RIM);
        visuals.menu_corner_radius = theme.radius_sm;
        popup_below_widget(
            ui,
            popup_id,
            &response,
            PopupCloseBehavior::CloseOnClick,
            |ui| {
                ui.set_min_width(width - 4.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                // A list longer than a screen scrolls rather than running
                // off it: the keyboard alone has six octaves.
                ScrollArea::vertical()
                    .max_height(LIST_HEIGHT)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (index, label) in labels.iter().enumerate() {
                            if index > 0 {
                                separator(ui, theme, width - 4.0);
                            }
                            let row = option(ui, theme, label, index == selected, width - 4.0);
                            if just_opened && index == selected {
                                row.scroll_to_me(Some(Align::Center));
                            }
                            if row.clicked() {
                                chosen = Some(index);
                            }
                        }
                    });
            },
        );
    });

    chosen
}

/// A list with its name printed above it, for a row of captioned controls.
pub fn labelled_dropdown(
    ui: &mut Ui,
    theme: &Theme,
    name: &str,
    id: impl std::hash::Hash,
    labels: &[&str],
    selected: usize,
    width: f32,
) -> Option<usize> {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm;
        // The name centred over the list, a rule either side of it.
        let (rect, _) = ui.allocate_exact_size(vec2(width, theme.font_sm), Sense::hover());
        let painter = ui.painter();
        let printed = legend(painter, theme, rect.center(), Align2::CENTER_CENTER, name);
        let rule = Stroke::new(1.0_f32, theme.chassis_shadow.gamma_multiply(0.6));
        let y = rect.center().y.round() + 0.5;
        for (from, to) in [
            (rect.min.x, printed.min.x - 5.0),
            (printed.max.x + 5.0, rect.max.x),
        ] {
            if to - from > 4.0 {
                painter.line_segment([pos2(from, y), pos2(to, y)], rule);
            }
        }
        dropdown(ui, theme, id, labels, selected, width)
    })
    .inner
}

/// One row of an open list: flat on the dark plate, lighter under the
/// pointer, and teal with a tick for the current value.
fn option(ui: &mut Ui, theme: &Theme, label: &str, selected: bool, width: f32) -> Response {
    let height = theme.control_height + 2.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let painter = ui.painter();

    if selected {
        painter.rect_filled(rect, 0.0, theme.control_selected_bg);
        tick(
            painter,
            pos2(rect.min.x + 12.0, rect.center().y),
            theme.accent.lerp_to_gamma(Color32::WHITE, 0.3),
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, theme.control_hover_top);
    }
    painter.text(
        pos2(rect.min.x + TICK_ROOM, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme.font_md),
        theme.text,
    );

    response
}

/// A hairline between two rows of an open list.
fn separator(ui: &mut Ui, theme: &Theme, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0_f32, theme.control_top.gamma_multiply(0.6)),
    );
}

/// The check mark beside the current value.
fn tick(painter: &Painter, centre: Pos2, color: Color32) {
    painter.add(PathShape::line(
        vec![
            centre + vec2(-4.0, 0.0),
            centre + vec2(-1.2, 3.0),
            centre + vec2(4.5, -3.5),
        ],
        Stroke::new(2.0_f32, color),
    ));
}

/// The triangle that says a control opens a list: down while it is shut,
/// up while it is open.
fn chevron(painter: &Painter, centre: Pos2, size: f32, open: bool, color: Color32) {
    let flip = if open { -1.0 } else { 1.0 };
    painter.add(PathShape::convex_polygon(
        vec![
            pos2(centre.x - size * 2.0, centre.y - size * flip),
            pos2(centre.x + size * 2.0, centre.y - size * flip),
            pos2(centre.x, centre.y + size * 1.6 * flip),
        ],
        color,
        Stroke::NONE,
    ));
}
