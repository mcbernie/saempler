use nih_plug_egui::egui::{pos2, vec2, Align2, FontId, PointerButton, Rect, Sense, Stroke, Ui};
use saempler_core::PeakCache;
use saempler_model::{note_name, PerformanceCell, Slice};

use crate::theme::Theme;
use crate::widgets::surface::{control_surface, SurfaceState};
use crate::widgets::waveform::slice_color;

/// Size of one performance pad.
pub const PAD_SIZE: f32 = 86.0;
/// Height of the waveform thumbnail inside a pad.
const THUMBNAIL_HEIGHT: f32 = 26.0;

/// What the user did on a pad.
#[derive(Debug, Default)]
pub struct PadAction {
    /// The pad was clicked: select the cell and play it.
    pub trigger: bool,
    /// The pad was right clicked: take the cell off its note.
    pub clear: bool,
}

/// Everything one pad shows.
pub struct PadView<'a> {
    pub cell: &'a PerformanceCell,
    pub slice: Option<&'a Slice>,
    /// Position of the slice in the project, for numbering and colour.
    pub slice_index: usize,
    pub peaks: &'a PeakCache,
    pub selected: bool,
    /// Whether the engine is currently inside this pad's slice.
    pub sounding: bool,
}

/// Draw one performance pad.
///
/// A pad shows what a note will do: which note it is, which slice it plays,
/// the shape of that slice, and the settings that differ from plain playback.
pub fn performance_pad(ui: &mut Ui, theme: &Theme, view: &PadView<'_>) -> PadAction {
    let PadView {
        cell,
        slice,
        slice_index,
        peaks,
        selected,
        sounding,
    } = *view;
    let (rect, response) =
        ui.allocate_exact_size(vec2(PAD_SIZE, PAD_SIZE), Sense::click_and_drag());

    let state = if selected {
        SurfaceState::Selected
    } else if response.is_pointer_button_down_on() {
        SurfaceState::Pressed
    } else if response.hovered() {
        SurfaceState::Hover
    } else {
        SurfaceState::Rest
    };
    control_surface(ui, theme, rect, state);

    let accent = slice_color(theme, slice_index);
    let painter = ui.painter();

    // A bar in the slice's colour ties the pad to the waveform above.
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)),
        theme.radius_sm,
        accent,
    );

    painter.text(
        pos2(rect.min.x + theme.spacing_md, rect.min.y + theme.spacing_md),
        Align2::LEFT_TOP,
        note_name(cell.midi_note),
        FontId::proportional(theme.font_md),
        if selected { theme.accent } else { theme.text },
    );
    painter.text(
        pos2(rect.max.x - theme.spacing_md, rect.min.y + theme.spacing_md),
        Align2::RIGHT_TOP,
        format!("S{}", slice_index + 1),
        FontId::proportional(theme.font_sm),
        theme.text_dim,
    );

    if let Some(slice) = slice {
        let thumb = Rect::from_min_size(
            pos2(
                rect.min.x + theme.spacing_sm,
                rect.min.y + theme.font_md + theme.spacing_md * 2.0,
            ),
            vec2(rect.width() - theme.spacing_sm * 2.0, THUMBNAIL_HEIGHT),
        );
        draw_thumbnail(ui, theme, thumb, peaks, slice, accent);
    }

    // The badges say, at a glance, how this cell differs from plain playback.
    let painter = ui.painter();
    painter.text(
        pos2(rect.min.x + theme.spacing_md, rect.max.y - theme.spacing_md),
        Align2::LEFT_BOTTOM,
        badges(cell),
        FontId::proportional(theme.font_sm),
        theme.text_dim,
    );

    if sounding {
        painter.rect_stroke(
            rect,
            theme.radius_sm,
            Stroke::new(theme.stroke_thick, theme.active),
            nih_plug_egui::egui::StrokeKind::Inside,
        );
    }

    PadAction {
        trigger: response.clicked(),
        clear: response.clicked_by(PointerButton::Secondary),
    }
}

/// A short description of everything the cell changes.
///
/// Empty when the cell plays its slice as recorded, so an untouched pad stays
/// visually quiet and the changed ones stand out.
fn badges(cell: &PerformanceCell) -> String {
    let playback = &cell.playback;
    let mut parts: Vec<String> = Vec::new();

    if playback.reverse {
        parts.push("REV".to_owned());
    }
    if (playback.speed - 1.0).abs() > 1e-3 {
        parts.push(format!("{:.2}×", playback.speed));
    }
    if playback.pitch_semitones.abs() > 1e-3 {
        parts.push(format!("{:+.0}st", playback.pitch_semitones));
    }

    parts.join("  ")
}

/// Draw the slice's shape, scaled to the pad.
fn draw_thumbnail(
    ui: &Ui,
    theme: &Theme,
    rect: Rect,
    peaks: &PeakCache,
    slice: &Slice,
    color: nih_plug_egui::egui::Color32,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, theme.radius_sm, theme.waveform_bg);

    let frames = slice.len_frames();
    if frames == 0 || rect.width() < 1.0 {
        return;
    }

    let columns = rect.width().floor().max(1.0) as u32;
    let level = peaks.level_for(frames, columns as f32);
    let centre = rect.center().y;
    let half_height = rect.height() * 0.5 - 1.0;

    for column in 0..columns {
        let start = slice.start_frame + frames * u64::from(column) / u64::from(columns);
        let end = (slice.start_frame + frames * u64::from(column + 1) / u64::from(columns))
            .max(start + 1);
        let peak = peaks.peak_in(level, start, end);

        let x = rect.min.x + column as f32 + 0.5;
        let top = centre - peak.max.clamp(-1.0, 1.0) * half_height;
        let bottom = centre - peak.min.clamp(-1.0, 1.0) * half_height;
        let (top, bottom) = if (bottom - top).abs() < 1.0 {
            (centre - 0.5, centre + 0.5)
        } else {
            (top, bottom)
        };

        painter.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.0, color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saempler_model::{CellId, PlaybackSettings, SliceId};

    fn cell(playback: PlaybackSettings) -> PerformanceCell {
        PerformanceCell {
            id: CellId(0),
            midi_note: 60,
            slice: SliceId(0),
            playback,
        }
    }

    #[test]
    fn a_plain_cell_shows_no_badges() {
        assert_eq!(badges(&cell(PlaybackSettings::default())), "");
    }

    #[test]
    fn reverse_is_called_out() {
        let badges = badges(&cell(PlaybackSettings {
            reverse: true,
            ..Default::default()
        }));

        assert_eq!(badges, "REV");
    }

    #[test]
    fn speed_and_pitch_appear_together() {
        let badges = badges(&cell(PlaybackSettings {
            speed: 0.5,
            pitch_semitones: 7.0,
            ..Default::default()
        }));

        assert!(badges.contains("0.50"));
        assert!(badges.contains("+7st"));
    }

    #[test]
    fn a_downward_transposition_keeps_its_sign() {
        let badges = badges(&cell(PlaybackSettings {
            pitch_semitones: -5.0,
            ..Default::default()
        }));

        assert!(badges.contains("-5st"), "{badges}");
    }
}
