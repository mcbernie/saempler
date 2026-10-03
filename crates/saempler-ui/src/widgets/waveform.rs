use nih_plug_egui::egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, PointerButton, Rect, Response, Sense, Stroke,
    StrokeKind, Ui,
};
use saempler_audio::SampleBuffer;
use saempler_core::{PeakCache, BASE_FRAMES_PER_PEAK};
use saempler_model::{Slice, SliceId};

use crate::theme::Theme;
use crate::widgets::view_range::ViewRange;

/// Half-width of a marker's grab area, in pixels.
const MARKER_GRAB_RADIUS: f32 = 8.0;
/// Scroll units egui reports for one wheel notch.
///
/// Measured rather than assumed: the raw delta is not the platform's own 120,
/// because egui converts wheel lines into points on the way in.
const SCROLL_PER_NOTCH: f32 = 40.0;
/// Zoom change per wheel notch. Scrolling up zooms in.
const ZOOM_PER_NOTCH: f32 = 0.85;
/// Fraction of the visible range a shift+wheel notch pans by.
const PAN_PER_NOTCH: f32 = 0.2;
/// Below this many frames per pixel the peak cache is too coarse and the
/// widget reads the audio itself.
const DETAIL_THRESHOLD: f64 = BASE_FRAMES_PER_PEAK as f64;
/// Below this many frames per pixel individual samples get a dot.
const SAMPLE_DOT_THRESHOLD: f64 = 0.25;

/// Everything the waveform draws.
pub struct WaveformSource<'a> {
    pub peaks: &'a PeakCache,
    /// The decoded audio, used when zoomed in past the peak cache resolution.
    pub buffer: Option<&'a SampleBuffer>,
    pub slices: &'a [Slice],
    pub selected: Option<SliceId>,
    /// Frame the engine is currently playing, if any.
    pub playhead: Option<u64>,
    pub view: ViewRange,
}

/// What the user did on the waveform this frame.
///
/// The widget reports intent and changes nothing itself, so the caller decides
/// what a gesture means for the project.
#[derive(Debug, Default)]
pub struct WaveformAction {
    /// A slice was clicked and should become the selection.
    pub select: Option<SliceId>,
    /// A boundary was dragged: move the one at `.0` to `.1`.
    pub move_boundary: Option<(u64, u64)>,
    /// A slice was double clicked at this frame and should be split there.
    pub split: Option<(SliceId, u64)>,
    /// The view was zoomed or panned.
    pub view: Option<ViewRange>,
    /// The pointer is over this frame, for the position readout.
    pub hovered_frame: Option<u64>,
}

/// The boundary a drag grabbed, remembered for the length of that drag.
///
/// Without this the widget would look for a marker near the *current* pointer
/// on every frame, and lose the boundary as soon as the pointer moved past the
/// grab radius.
#[derive(Debug, Clone, Copy)]
struct GrabbedBoundary {
    frame: u64,
}

/// Draw the source waveform with its slice markers.
///
/// Cost depends on the width in pixels, not on the length of the audio: one
/// column is read per pixel, from the peak level that matches the zoom, or
/// from the audio itself once the view is closer than the cache resolution.
pub fn waveform(
    ui: &mut Ui,
    theme: &Theme,
    source: &WaveformSource<'_>,
    height: f32,
) -> WaveformAction {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let mut action = WaveformAction::default();

    ui.painter()
        .rect_filled(rect, theme.radius_md, theme.waveform_bg);

    let total = source.peaks.frames();
    let view = source.view.clamped(total);
    if total == 0 || view.is_empty() || rect.width() < 1.0 {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Kein Sample geladen",
            FontId::proportional(theme.font_md),
            theme.text_dim,
        );
        outline(ui, theme, rect);
        return action;
    }

    draw_slice_backgrounds(ui, theme, rect, view, source);
    draw_trace(ui, theme, rect, view, source);
    draw_markers(ui, theme, rect, view, source);
    draw_playhead(ui, theme, rect, view, source);
    outline(ui, theme, rect);

    handle_input(ui, rect, view, source, &response, &mut action);
    action
}

fn outline(ui: &Ui, theme: &Theme, rect: Rect) {
    ui.painter().rect_stroke(
        rect,
        theme.radius_md,
        theme.outline_stroke(),
        StrokeKind::Inside,
    );
}

/// Convert a frame position to an x coordinate inside `rect`.
fn frame_to_x(rect: Rect, view: ViewRange, frame: u64) -> f32 {
    let len = view.len_frames();
    if len == 0 {
        return rect.min.x;
    }
    let offset = frame as f64 - view.start_frame as f64;
    let ratio = (offset / len as f64).clamp(-1.0, 2.0) as f32;
    rect.min.x + rect.width() * ratio
}

/// Convert an x coordinate to a frame position inside the view.
fn x_to_frame(rect: Rect, view: ViewRange, x: f32) -> u64 {
    if rect.width() <= 0.0 {
        return view.start_frame;
    }
    let ratio = ((x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64;
    view.start_frame + (ratio * view.len_frames() as f64) as u64
}

/// Shade the span of every slice so the divisions are readable at a glance.
fn draw_slice_backgrounds(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    view: ViewRange,
    source: &WaveformSource<'_>,
) {
    let painter = ui.painter();
    for (index, slice) in source.slices.iter().enumerate() {
        if slice.end_frame <= view.start_frame || slice.start_frame >= view.end_frame {
            continue;
        }

        let left = frame_to_x(rect, view, slice.start_frame).max(rect.min.x);
        let right = frame_to_x(rect, view, slice.end_frame).min(rect.max.x);
        if right - left < 1.0 {
            continue;
        }

        let span = Rect::from_min_max(pos2(left, rect.min.y), pos2(right, rect.max.y));
        let color = if source.selected == Some(slice.id) {
            theme.slice_selected_fill
        } else if index % 2 == 0 {
            theme.slice_fill
        } else {
            theme.slice_fill_alternate
        };
        painter.rect_filled(span, 0.0, color);
    }
}

/// Draw the waveform itself, from peaks or from the audio.
fn draw_trace(ui: &Ui, theme: &Theme, rect: Rect, view: ViewRange, source: &WaveformSource<'_>) {
    let columns = rect.width().floor().max(1.0) as u32;
    let frames_per_pixel = view.len_frames() as f64 / columns as f64;

    match source.buffer {
        Some(buffer) if frames_per_pixel < DETAIL_THRESHOLD => {
            draw_detailed(ui, theme, rect, view, buffer, columns, frames_per_pixel)
        }
        _ => draw_from_peaks(ui, theme, rect, view, source.peaks, columns),
    }
}

/// One minimum/maximum column per pixel, read from the peak pyramid.
fn draw_from_peaks(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    view: ViewRange,
    peaks: &PeakCache,
    columns: u32,
) {
    let painter = ui.painter();
    let level = peaks.level_for(view.len_frames(), columns as f32);
    let half_height = rect.height() * 0.5;
    let centre = rect.center().y;

    for column in 0..columns {
        let start = frame_at_column(view, columns, column);
        let end = frame_at_column(view, columns, column + 1).max(start + 1);
        let peak = peaks.peak_in(level, start, end);

        let x = rect.min.x + column as f32 + 0.5;
        draw_column(painter, theme, x, centre, half_height, peak.min, peak.max);
    }
}

/// Read the audio directly, for zoom levels the peak cache cannot resolve.
fn draw_detailed(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    view: ViewRange,
    buffer: &SampleBuffer,
    columns: u32,
    frames_per_pixel: f64,
) {
    let painter = ui.painter();
    let half_height = rect.height() * 0.5;
    let centre = rect.center().y;
    let samples = buffer.channel(0);

    for column in 0..columns {
        let start = frame_at_column(view, columns, column) as usize;
        let end = (frame_at_column(view, columns, column + 1) as usize).max(start + 1);
        let from = start.min(samples.len());
        let to = end.min(samples.len());
        if from >= to {
            continue;
        }

        let (mut min, mut max) = (f32::MAX, f32::MIN);
        for sample in &samples[from..to] {
            min = min.min(*sample);
            max = max.max(*sample);
        }

        let x = rect.min.x + column as f32 + 0.5;
        draw_column(painter, theme, x, centre, half_height, min, max);

        // Zoomed in far enough that individual samples are distinguishable.
        if frames_per_pixel < SAMPLE_DOT_THRESHOLD {
            let y = centre - samples[from].clamp(-1.0, 1.0) * half_height;
            painter.circle_filled(pos2(x, y), 1.5, theme.accent);
        }
    }
}

/// First frame shown in `column`.
fn frame_at_column(view: ViewRange, columns: u32, column: u32) -> u64 {
    view.start_frame + view.len_frames() * u64::from(column) / u64::from(columns)
}

/// Draw one vertical minimum/maximum segment.
fn draw_column(
    painter: &nih_plug_egui::egui::Painter,
    theme: &Theme,
    x: f32,
    centre: f32,
    half_height: f32,
    min: f32,
    max: f32,
) {
    let top = centre - max.clamp(-1.0, 1.0) * half_height;
    let bottom = centre - min.clamp(-1.0, 1.0) * half_height;

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

/// Draw the start and end marker of every visible slice.
fn draw_markers(ui: &Ui, theme: &Theme, rect: Rect, view: ViewRange, source: &WaveformSource<'_>) {
    let painter = ui.painter();
    for slice in source.slices {
        let is_selected = source.selected == Some(slice.id);
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
            if frame < view.start_frame || frame > view.end_frame {
                continue;
            }
            let x = frame_to_x(rect, view, frame);
            painter.line_segment(
                [pos2(x, rect.min.y), pos2(x, rect.max.y)],
                Stroke::new(width, color),
            );
        }
    }
}

/// Draw the position the engine is playing.
fn draw_playhead(ui: &Ui, theme: &Theme, rect: Rect, view: ViewRange, source: &WaveformSource<'_>) {
    let Some(frame) = source.playhead else {
        return;
    };
    if frame < view.start_frame || frame > view.end_frame {
        return;
    }

    let x = frame_to_x(rect, view, frame);
    ui.painter().line_segment(
        [pos2(x, rect.min.y), pos2(x, rect.max.y)],
        Stroke::new(theme.stroke_thick, theme.playhead),
    );
}

/// Turn pointer activity into an intent for the caller.
fn handle_input(
    ui: &Ui,
    rect: Rect,
    view: ViewRange,
    source: &WaveformSource<'_>,
    response: &Response,
    action: &mut WaveformAction,
) {
    let total = source.peaks.frames();
    let grab_id = response.id.with("grabbed-boundary");

    // Zoom follows the pointer even without a click.
    if response.hovered() {
        // The raw delta is used rather than the smoothed one: egui spreads the
        // smoothed value over several frames, and applying a zoom factor on
        // each of them would multiply a single notch many times over.
        let (scroll, shift) = ui.input(|input| (input.raw_scroll_delta.y, input.modifiers.shift));
        if scroll != 0.0 {
            let notches = scroll / SCROLL_PER_NOTCH;
            let anchor = response
                .hover_pos()
                .map(|p| x_to_frame(rect, view, p.x))
                .unwrap_or(view.start_frame);
            let next = if shift {
                view.panned(frames_for_notches(view, notches), total)
            } else {
                view.zoomed(anchor, ZOOM_PER_NOTCH.powf(notches), total)
            };
            if next != view {
                action.view = Some(next);
            }
        }
    }

    let Some(pointer) = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos())
    else {
        return;
    };
    let frame = x_to_frame(rect, view, pointer.x);
    action.hovered_frame = Some(frame);

    // Holding the right button drags the view itself, the way a map pans.
    if response.dragged_by(PointerButton::Secondary) {
        let pixels = response.drag_delta().x;
        if pixels != 0.0 {
            let per_pixel = view.len_frames() as f64 / rect.width().max(1.0) as f64;
            let delta = -(pixels as f64 * per_pixel) as i64;
            let next = view.panned(delta, total);
            if next != view {
                action.view = Some(next);
            }
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        return;
    }

    if response.drag_started() {
        // Where the button went down, not where the pointer is now: egui only
        // reports a drag once it has moved, by which time the pointer may
        // already have left the marker's grab area.
        let origin = ui
            .input(|input| input.pointer.press_origin())
            .unwrap_or(pointer);
        let grabbed = nearest_boundary(rect, view, source.slices, origin.x)
            .map(|frame| GrabbedBoundary { frame });
        ui.memory_mut(|memory| memory.data.insert_temp(grab_id, grabbed));
    }

    if response.dragged_by(PointerButton::Primary) {
        // The boundary is the one grabbed when the drag started, not whatever
        // happens to be near the pointer now.
        let grabbed: Option<GrabbedBoundary> =
            ui.memory(|memory| memory.data.get_temp(grab_id)).flatten();
        if let Some(grabbed) = grabbed {
            if frame != grabbed.frame {
                action.move_boundary = Some((grabbed.frame, frame));
                // Follow the boundary so the next frame grabs the same one.
                ui.memory_mut(|memory| {
                    memory
                        .data
                        .insert_temp(grab_id, Some(GrabbedBoundary { frame }))
                });
            }
        }
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        return;
    }

    if response.drag_stopped() {
        ui.memory_mut(|memory| memory.data.remove::<Option<GrabbedBoundary>>(grab_id));
    }

    if response.double_clicked() {
        if let Some(slice) = source.slices.iter().find(|slice| slice.contains(frame)) {
            action.split = Some((slice.id, frame));
        }
        return;
    }

    if response.clicked() {
        if let Some(slice) = source.slices.iter().find(|slice| slice.contains(frame)) {
            action.select = Some(slice.id);
        }
    }

    // Keeps the pointer shape honest about what a drag would do.
    if nearest_boundary(rect, view, source.slices, pointer.x).is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
}

/// Frames a wheel movement of `notches` should shift the view by.
fn frames_for_notches(view: ViewRange, notches: f32) -> i64 {
    -(view.len_frames() as f64 * PAN_PER_NOTCH as f64 * notches as f64) as i64
}

/// The slice boundary within [`MARKER_GRAB_RADIUS`] pixels of `x`, if any.
fn nearest_boundary(rect: Rect, view: ViewRange, slices: &[Slice], x: f32) -> Option<u64> {
    let mut best: Option<(u64, f32)> = None;

    for slice in slices {
        for frame in [slice.start_frame, slice.end_frame] {
            let distance = (frame_to_x(rect, view, frame) - x).abs();
            if distance > MARKER_GRAB_RADIUS {
                continue;
            }
            if best.map(|(_, best)| distance < best).unwrap_or(true) {
                best = Some((frame, distance));
            }
        }
    }

    best.map(|(frame, _)| frame)
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
        let view = ViewRange::full(48_000);

        for frame in [0u64, 1_000, 24_000, 47_999] {
            let x = frame_to_x(rect, view, frame);
            let back = x_to_frame(rect, view, x);
            let tolerance = view.len_frames() / rect.width() as u64 + 1;
            assert!(
                back.abs_diff(frame) <= tolerance,
                "{frame} -> {x} -> {back}"
            );
        }
    }

    #[test]
    fn a_zoomed_view_maps_only_its_own_range() {
        let rect = rect();
        let view = ViewRange {
            start_frame: 10_000,
            end_frame: 20_000,
        };

        assert_eq!(x_to_frame(rect, view, rect.min.x), 10_000);
        assert_eq!(x_to_frame(rect, view, rect.max.x), 20_000);
        assert!((frame_to_x(rect, view, 15_000) - rect.center().x).abs() < 1.0);
    }

    #[test]
    fn positions_outside_the_widget_are_clamped_to_the_view() {
        let rect = rect();
        let view = ViewRange {
            start_frame: 10_000,
            end_frame: 20_000,
        };

        assert_eq!(x_to_frame(rect, view, -5_000.0), 10_000);
        assert_eq!(x_to_frame(rect, view, 99_999.0), 20_000);
    }

    #[test]
    fn an_empty_view_does_not_divide_by_zero() {
        let rect = rect();
        let view = ViewRange::default();

        assert_eq!(frame_to_x(rect, view, 0), rect.min.x);
        assert_eq!(x_to_frame(rect, view, 500.0), 0);
    }

    #[test]
    fn the_nearest_boundary_is_found_within_the_grab_radius() {
        let rect = rect();
        let view = ViewRange::full(800);
        let slices = [slice(0, 0, 400), slice(1, 400, 800)];

        let middle = frame_to_x(rect, view, 400);

        assert_eq!(
            nearest_boundary(rect, view, &slices, middle + 2.0),
            Some(400)
        );
    }

    #[test]
    fn boundaries_further_away_than_the_grab_radius_are_ignored() {
        let rect = rect();
        let view = ViewRange::full(800);
        let slices = [slice(0, 0, 400)];

        let middle = frame_to_x(rect, view, 400);
        let far = middle + MARKER_GRAB_RADIUS + 2.0;

        assert_eq!(nearest_boundary(rect, view, &slices, far), None);
    }

    #[test]
    fn the_closer_of_two_boundaries_wins() {
        let rect = rect();
        // Two boundaries a few pixels apart at this zoom.
        let view = ViewRange::full(8_000);
        let slices = [slice(0, 4_000, 4_030), slice(1, 4_030, 8_000)];

        let x = frame_to_x(rect, view, 4_030);

        assert_eq!(nearest_boundary(rect, view, &slices, x), Some(4_030));
    }

    #[test]
    fn columns_cover_the_view_without_gaps() {
        let view = ViewRange {
            start_frame: 1_000,
            end_frame: 5_000,
        };
        let columns = 400;

        assert_eq!(frame_at_column(view, columns, 0), 1_000);
        assert_eq!(frame_at_column(view, columns, columns), 5_000);
        for column in 0..columns {
            assert!(
                frame_at_column(view, columns, column)
                    <= frame_at_column(view, columns, column + 1)
            );
        }
    }

    #[test]
    fn slice_colors_cycle_rather_than_running_out() {
        let theme = Theme::dark();

        assert_eq!(
            slice_color(&theme, 0),
            slice_color(&theme, theme.slice_palette.len())
        );
    }
}
