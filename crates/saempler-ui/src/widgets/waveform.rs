use nih_plug_egui::egui::{
    pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui,
};
use saempler_core::PeakCache;
use saempler_model::{Slice, SliceId};

use crate::theme::Theme;

/// Half-width of a marker's grab area, in pixels.
const MARKER_GRAB_RADIUS: f32 = 5.0;

/// Which end of a slice a marker belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerEdge {
    Start,
    End,
}

/// What the user did on the waveform this frame.
///
/// The widget reports intent and changes nothing itself, so the caller decides
/// what a gesture means for the project.
#[derive(Debug, Default)]
pub struct WaveformAction {
    /// A slice was clicked and should become the selection.
    pub select: Option<SliceId>,
    /// A marker was dragged to a new frame.
    pub move_marker: Option<(SliceId, MarkerEdge, u64)>,
    /// A slice was double clicked at this frame and should be split there.
    pub split: Option<(SliceId, u64)>,
    /// The pointer is over this frame, for the position readout.
    pub hovered_frame: Option<u64>,
}

/// Draw the source waveform with its slice markers.
///
/// The drawing cost depends on the width in pixels, not on the length of the
/// audio: one peak is read per pixel column from the cache level that matches
/// the current zoom.
pub fn waveform(
    ui: &mut Ui,
    theme: &Theme,
    peaks: &PeakCache,
    slices: &[Slice],
    selected: Option<SliceId>,
    height: f32,
) -> WaveformAction {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let mut action = WaveformAction::default();

    let painter = ui.painter();
    painter.rect_filled(rect, theme.radius_md, theme.waveform_bg);

    let frames = peaks.frames();
    if frames == 0 || rect.width() < 1.0 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Kein Sample geladen",
            FontId::proportional(theme.font_md),
            theme.text_dim,
        );
        painter.rect_stroke(
            rect,
            theme.radius_md,
            theme.outline_stroke(),
            StrokeKind::Inside,
        );
        return action;
    }

    draw_slice_backgrounds(ui, theme, rect, frames, slices, selected);
    draw_peaks(ui, theme, rect, peaks);
    draw_markers(ui, theme, rect, frames, slices, selected);

    ui.painter().rect_stroke(
        rect,
        theme.radius_md,
        theme.outline_stroke(),
        StrokeKind::Inside,
    );

    // The centre line is drawn last so it stays visible over the peaks.
    ui.painter().line_segment(
        [
            pos2(rect.min.x, rect.center().y),
            pos2(rect.max.x, rect.center().y),
        ],
        Stroke::new(theme.stroke_thin, theme.waveform_axis),
    );

    handle_input(ui, rect, frames, slices, &response, &mut action);
    action
}

/// Convert a frame position to an x coordinate inside `rect`.
fn frame_to_x(rect: Rect, frames: u64, frame: u64) -> f32 {
    if frames == 0 {
        return rect.min.x;
    }
    let ratio = (frame as f64 / frames as f64).clamp(0.0, 1.0) as f32;
    rect.min.x + rect.width() * ratio
}

/// Convert an x coordinate to a frame position, clamped to the sample.
fn x_to_frame(rect: Rect, frames: u64, x: f32) -> u64 {
    if rect.width() <= 0.0 {
        return 0;
    }
    let ratio = ((x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64;
    ((ratio * frames as f64) as u64).min(frames)
}

/// Shade the span of every slice so the divisions are readable at a glance.
fn draw_slice_backgrounds(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    frames: u64,
    slices: &[Slice],
    selected: Option<SliceId>,
) {
    let painter = ui.painter();
    for (index, slice) in slices.iter().enumerate() {
        let left = frame_to_x(rect, frames, slice.start_frame);
        let right = frame_to_x(rect, frames, slice.end_frame);
        if right - left < 1.0 {
            continue;
        }

        let span = Rect::from_min_max(pos2(left, rect.min.y), pos2(right, rect.max.y));
        let color = if selected == Some(slice.id) {
            theme.slice_selected_fill
        } else if index % 2 == 0 {
            theme.slice_fill
        } else {
            theme.slice_fill_alternate
        };
        painter.rect_filled(span, 0.0, color);
    }
}

/// Draw one minimum/maximum column per pixel.
fn draw_peaks(ui: &Ui, theme: &Theme, rect: Rect, peaks: &PeakCache) {
    let painter = ui.painter();
    let frames = peaks.frames();
    let columns = rect.width().floor().max(1.0) as u32;
    let level = peaks.level_for(frames, columns as f32);
    let half_height = rect.height() * 0.5;
    let centre = rect.center().y;

    for column in 0..columns {
        let start = frames * u64::from(column) / u64::from(columns);
        let end = frames * u64::from(column + 1) / u64::from(columns);
        let peak = peaks.peak_in(level, start, end.max(start + 1));

        let x = rect.min.x + column as f32 + 0.5;
        let top = centre - peak.max.clamp(-1.0, 1.0) * half_height;
        let bottom = centre - peak.min.clamp(-1.0, 1.0) * half_height;

        // A silent column would be an invisible zero-length segment.
        let (top, bottom) = if (bottom - top).abs() < 1.0 {
            (centre - 0.5, centre + 0.5)
        } else {
            (top, bottom)
        };

        painter.line_segment(
            [pos2(x, top), pos2(x, bottom)],
            Stroke::new(1.0, theme.waveform),
        );
    }
}

/// Draw the start and end marker of every slice.
fn draw_markers(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    frames: u64,
    slices: &[Slice],
    selected: Option<SliceId>,
) {
    let painter = ui.painter();
    for slice in slices {
        let is_selected = selected == Some(slice.id);
        let color = if is_selected {
            theme.accent
        } else {
            theme.marker
        };
        let width = if is_selected {
            theme.stroke_thick
        } else {
            theme.stroke_thin
        };

        for frame in [slice.start_frame, slice.end_frame] {
            let x = frame_to_x(rect, frames, frame);
            painter.line_segment(
                [pos2(x, rect.min.y), pos2(x, rect.max.y)],
                Stroke::new(width, color),
            );
        }
    }
}

/// Turn pointer activity into an intent for the caller.
fn handle_input(
    ui: &Ui,
    rect: Rect,
    frames: u64,
    slices: &[Slice],
    response: &nih_plug_egui::egui::Response,
    action: &mut WaveformAction,
) {
    let Some(pointer) = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos())
    else {
        return;
    };
    let frame = x_to_frame(rect, frames, pointer.x);
    action.hovered_frame = Some(frame);

    if response.dragged() {
        // Dragging grabs the marker nearest to where the drag is happening, so
        // a boundary can be adjusted without hitting it exactly.
        if let Some((id, edge)) = nearest_marker(rect, frames, slices, pointer.x) {
            action.move_marker = Some((id, edge, frame));
        }
        return;
    }

    if response.double_clicked() {
        if let Some(slice) = slices.iter().find(|slice| slice.contains(frame)) {
            action.split = Some((slice.id, frame));
        }
        return;
    }

    if response.clicked() {
        if let Some(slice) = slices.iter().find(|slice| slice.contains(frame)) {
            action.select = Some(slice.id);
        }
    }

    // Keeps the pointer shape honest about what a drag would do.
    if nearest_marker(rect, frames, slices, pointer.x).is_some() {
        ui.ctx()
            .set_cursor_icon(nih_plug_egui::egui::CursorIcon::ResizeHorizontal);
    }
}

/// The marker within [`MARKER_GRAB_RADIUS`] pixels of `x`, if any.
fn nearest_marker(
    rect: Rect,
    frames: u64,
    slices: &[Slice],
    x: f32,
) -> Option<(SliceId, MarkerEdge)> {
    let mut best: Option<(SliceId, MarkerEdge, f32)> = None;

    for slice in slices {
        for (frame, edge) in [
            (slice.start_frame, MarkerEdge::Start),
            (slice.end_frame, MarkerEdge::End),
        ] {
            let distance = (frame_to_x(rect, frames, frame) - x).abs();
            if distance > MARKER_GRAB_RADIUS {
                continue;
            }
            if best.map(|(_, _, best)| distance < best).unwrap_or(true) {
                best = Some((slice.id, edge, distance));
            }
        }
    }

    best.map(|(id, edge, _)| (id, edge))
}

/// Colour used for a slice's label, cycling through the palette.
pub fn slice_color(theme: &Theme, index: usize) -> Color32 {
    theme.slice_palette[index % theme.slice_palette.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use nih_plug_egui::egui::pos2;

    fn rect() -> Rect {
        Rect::from_min_max(pos2(100.0, 0.0), pos2(900.0, 100.0))
    }

    fn slice(id: u32, start: u64, end: u64) -> Slice {
        Slice {
            id: SliceId(id),
            start_frame: start,
            end_frame: end,
        }
    }

    #[test]
    fn frame_and_pixel_conversions_are_inverse() {
        let rect = rect();
        let frames = 48_000;

        for frame in [0u64, 1_000, 24_000, 47_999] {
            let x = frame_to_x(rect, frames, frame);
            let back = x_to_frame(rect, frames, x);
            let tolerance = frames / rect.width() as u64 + 1;
            assert!(
                back.abs_diff(frame) <= tolerance,
                "{frame} -> {x} -> {back}"
            );
        }
    }

    #[test]
    fn positions_outside_the_widget_are_clamped() {
        let rect = rect();

        assert_eq!(x_to_frame(rect, 48_000, 0.0), 0);
        assert_eq!(x_to_frame(rect, 48_000, 10_000.0), 48_000);
        assert_eq!(frame_to_x(rect, 48_000, 1_000_000), rect.max.x);
    }

    #[test]
    fn a_zero_length_sample_does_not_divide_by_zero() {
        let rect = rect();

        assert_eq!(frame_to_x(rect, 0, 0), rect.min.x);
        assert_eq!(x_to_frame(rect, 0, 500.0), 0);
    }

    #[test]
    fn the_nearest_marker_is_found_within_the_grab_radius() {
        let rect = rect();
        let frames = 800;
        let slices = [slice(0, 0, 400), slice(1, 400, 800)];

        // Frame 400 sits at the middle of an 800 pixel wide widget.
        let middle = frame_to_x(rect, frames, 400);
        let found = nearest_marker(rect, frames, &slices, middle + 2.0);

        assert!(found.is_some());
    }

    #[test]
    fn markers_further_away_than_the_grab_radius_are_ignored() {
        let rect = rect();
        let frames = 800;
        let slices = [slice(0, 0, 400)];

        let middle = frame_to_x(rect, frames, 400);
        let found = nearest_marker(rect, frames, &slices, middle + MARKER_GRAB_RADIUS + 1.0);

        assert_eq!(found, None);
    }

    #[test]
    fn slice_colors_cycle_rather_than_running_out() {
        let theme = Theme::dark();

        let first = slice_color(&theme, 0);
        let wrapped = slice_color(&theme, theme.slice_palette.len());

        assert_eq!(first, wrapped);
    }
}
