use nih_plug_egui::egui::{
    pos2, vec2, Align2, Color32, FontId, PointerButton, Rect, Sense, Stroke, StrokeKind, Ui,
};
use saempler_core::PeakCache;
use saempler_model::{note_name, PerformanceCell, Slice};

use crate::theme::Theme;
use crate::widgets::texture::{self, PAD_BEZEL_CORNER, PAD_CAP_CORNER};
use crate::widgets::waveform::slice_color;

/// Size a pad is drawn at when there is room for it.
pub const PAD_SIZE: f32 = 104.0;
/// Smallest a pad may shrink to; below this the grid scrolls instead.
pub const MIN_PAD_SIZE: f32 = 68.0;
/// Width of the rubber bezel around the cap.
const CAP_INSET: f32 = 5.0;
/// Face value of the cap texture, which the slice colour is lifted by so the
/// face comes out in exactly that colour.
const CAP_FACE: f32 = 200.0 / 255.0;

/// What the user did on a pad.
#[derive(Debug)]
pub struct PadAction {
    /// The pad was clicked: select the cell and play it.
    pub trigger: bool,
    /// The pad was right clicked: take the cell off its note.
    pub clear: bool,
    /// A drag began on this pad.
    pub drag_started: bool,
    /// A drag that began somewhere ended with the pointer released.
    pub drag_released: bool,
    /// Where the pad was drawn, so the caller can work out what a drag was
    /// let go over.
    pub rect: Rect,
}

impl Default for PadAction {
    fn default() -> Self {
        Self {
            trigger: false,
            clear: false,
            drag_started: false,
            drag_released: false,
            rect: Rect::NOTHING,
        }
    }
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
    /// Side length to draw at. The grid shrinks its pads rather than
    /// scrolling, so that every key stays visible however many are mapped.
    pub size: f32,
    /// Whether this pad is the one being dragged to another key.
    pub dragging: bool,
    /// Whether a dragged pad is hovering over this one, which is where it
    /// would land.
    pub drop_target: bool,
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
        size,
        dragging,
        drop_target,
    } = *view;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let pressed = response.is_pointer_button_down_on();
    let accent = slice_color(theme, slice_index);
    let painter = ui.painter();
    let textures = texture::textures(ui.ctx());

    // The rubber bezel, with a short shadow on the plate below it.
    painter.rect_filled(
        rect.translate(vec2(0.8, 1.5)),
        theme.radius_md,
        Color32::from_black_alpha(60),
    );
    painter.add(texture::nine_slice(
        &textures.pad_bezel,
        rect,
        PAD_BEZEL_CORNER,
        Color32::WHITE,
    ));

    // The cap in the chop's colour. Pressed, it sinks into the bezel: a
    // point lower and a shade darker, the light no longer reaching its top.
    let cap = if pressed {
        rect.shrink(CAP_INSET).translate(vec2(0.0, 1.0))
    } else {
        rect.shrink(CAP_INSET)
    };
    let tint = if pressed {
        lift(accent).gamma_multiply(0.82)
    } else if response.hovered() {
        lift(accent.lerp_to_gamma(Color32::WHITE, 0.08))
    } else {
        lift(accent)
    };
    let tint = Color32::from_rgb(tint.r(), tint.g(), tint.b());
    painter.add(texture::nine_slice(
        &textures.pad_cap,
        cap,
        PAD_CAP_CORNER,
        tint,
    ));
    painter.add(texture::grain(
        &textures.pad_grain,
        cap.shrink(3.0),
        Color32::WHITE.gamma_multiply(0.4),
    ));

    let ink = theme.title;
    let pad = (size * 0.06).clamp(4.0, 8.0);

    // The key on a chip a shade deeper than the cap, as on the waveform
    // above: the colour is what ties a pad to its chop.
    let name = note_name(cell.midi_note);
    let chip = Rect::from_min_size(
        pos2(cap.min.x + pad, cap.min.y + pad),
        vec2(
            name.chars().count() as f32 * theme.font_sm * 0.68 + theme.spacing_sm * 2.5,
            theme.font_sm + 6.0,
        ),
    );
    painter.rect_filled(
        chip,
        theme.radius_sm,
        accent.lerp_to_gamma(Color32::BLACK, 0.25),
    );
    painter.text(
        chip.center(),
        Align2::CENTER_CENTER,
        name,
        FontId::proportional(theme.font_sm),
        theme.chassis_top,
    );
    // Its lamp: lit while the chop sounds.
    crate::widgets::panel::lamp(
        painter,
        theme,
        pos2(cap.max.x - pad - 5.0, chip.center().y),
        sounding.then_some(theme.active),
    );

    // The chop's shape on a small display set into the cap.
    let footer = theme.font_sm + pad;
    let display = Rect::from_min_max(
        pos2(cap.min.x + pad, chip.max.y + pad * 0.75),
        pos2(cap.max.x - pad, cap.max.y - footer - pad * 0.5),
    );
    if display.height() > 8.0 {
        crate::widgets::panel::inset(painter, theme, display, theme.waveform_bg);
        if let Some(slice) = slice {
            draw_thumbnail(ui, display.shrink(2.0), peaks, slice, theme.waveform);
        }
    }

    // Below it: the chop's number on the left, and the badges that say how
    // this cell differs from plain playback on the right.
    let painter = ui.painter();
    painter.text(
        pos2(cap.min.x + pad, cap.max.y - pad * 0.6),
        Align2::LEFT_BOTTOM,
        format!("S{}", slice_index + 1),
        FontId::proportional(theme.font_sm),
        ink,
    );
    painter.text(
        pos2(cap.max.x - pad, cap.max.y - pad * 0.6),
        Align2::RIGHT_BOTTOM,
        badges(cell),
        FontId::proportional(theme.font_sm),
        ink.gamma_multiply(0.75),
    );

    // The pad that is up in the editor has its bezel lit along the inside,
    // like an engaged key; so do the one being dragged and the one it would
    // land on. On the bezel itself, so it costs no room around the pad.
    if selected || dragging || drop_target {
        let color = if drop_target {
            theme.armed
        } else {
            theme.accent
        };
        painter.rect_stroke(
            rect.shrink(1.0),
            theme.radius_md,
            Stroke::new(theme.stroke_thick, color),
            StrokeKind::Inside,
        );
    }

    PadAction {
        trigger: response.clicked(),
        clear: response.clicked_by(PointerButton::Secondary),
        drag_started: response.drag_started(),
        drag_released: response.drag_stopped(),
        rect,
    }
}

/// The tint that turns the grey cap texture into `color`.
///
/// The texture's face sits below white so its lit edges have room above it;
/// the tint is raised by the same factor to land the face on the colour.
fn lift(color: Color32) -> Color32 {
    let up = |channel: u8| (f32::from(channel) / CAP_FACE).round().min(255.0) as u8;
    Color32::from_rgb(up(color.r()), up(color.g()), up(color.b()))
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
fn draw_thumbnail(ui: &Ui, rect: Rect, peaks: &PeakCache, slice: &Slice, color: Color32) {
    let painter = ui.painter();

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

        painter.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.0_f32, color));
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
            ..PerformanceCell::placeholder()
        }
    }

    #[test]
    fn the_cap_tint_lands_the_face_on_the_slice_colour() {
        let color = Color32::from_rgb(0x3d, 0x91, 0x88);

        let tint = lift(color);

        // Multiplied by the face value of the texture, the tint gives back
        // the colour itself, to within rounding.
        for (tinted, wanted) in [
            (tint.r(), color.r()),
            (tint.g(), color.g()),
            (tint.b(), color.b()),
        ] {
            let face = (f32::from(tinted) * CAP_FACE).round() as i32;
            assert!((face - i32::from(wanted)).abs() <= 1, "{tinted} -> {face}");
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
