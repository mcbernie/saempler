use nih_plug_egui::egui::{epaint::PathShape, pos2, vec2, Sense, Stroke, StrokeKind, Ui};
use saempler_model::{EnvelopeDefinition, LfoShape};

use crate::theme::Theme;

/// Number of line segments a drawn curve is built from.
const SEGMENTS: usize = 96;

/// Draw the shape of an envelope.
///
/// The stages are laid out in proportion to their lengths, with a fixed
/// stretch of sustain between decay and release, so the picture says what the
/// numbers mean without having to read them.
pub fn envelope_display(
    ui: &mut Ui,
    theme: &Theme,
    envelope: EnvelopeDefinition,
    size: (f32, f32),
) {
    let (rect, _response) = ui.allocate_exact_size(vec2(size.0, size.1), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, theme.radius_sm, theme.waveform_bg);
    painter.rect_stroke(
        rect,
        theme.radius_sm,
        theme.outline_stroke(),
        StrokeKind::Inside,
    );

    let inner = rect.shrink(theme.spacing_sm);
    // A quarter of the width is reserved for the sustain stretch, so a patch
    // with no decay and no release still reads as a shape.
    let sustain_share = 0.25;
    let total = (envelope.attack_ms + envelope.decay_ms + envelope.release_ms).max(1.0);
    let span = inner.width() * (1.0 - sustain_share);

    let attack = inner.min.x + span * (envelope.attack_ms / total);
    let decay = attack + span * (envelope.decay_ms / total);
    let sustain_end = decay + inner.width() * sustain_share;
    let release = sustain_end + span * (envelope.release_ms / total);

    let level_y = |level: f32| inner.max.y - level.clamp(0.0, 1.0) * inner.height();
    let points = vec![
        pos2(inner.min.x, inner.max.y),
        pos2(attack, inner.min.y),
        pos2(decay, level_y(envelope.sustain)),
        pos2(sustain_end, level_y(envelope.sustain)),
        pos2(release.min(inner.max.x), inner.max.y),
    ];

    painter.add(PathShape::line(
        points,
        Stroke::new(theme.stroke_thick, theme.accent),
    ));
}

/// Draw one cycle of an LFO shape.
pub fn lfo_display(ui: &mut Ui, theme: &Theme, shape: LfoShape, size: (f32, f32)) {
    let (rect, _response) = ui.allocate_exact_size(vec2(size.0, size.1), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, theme.radius_sm, theme.waveform_bg);
    painter.rect_stroke(
        rect,
        theme.radius_sm,
        theme.outline_stroke(),
        StrokeKind::Inside,
    );

    let inner = rect.shrink(theme.spacing_sm);
    painter.line_segment(
        [
            pos2(inner.min.x, inner.center().y),
            pos2(inner.max.x, inner.center().y),
        ],
        Stroke::new(1.0, theme.waveform_axis),
    );

    let half = inner.height() * 0.5;
    let points = (0..=SEGMENTS)
        .map(|step| {
            let phase = step as f32 / SEGMENTS as f32;
            let value = shape_value(shape, phase);
            pos2(
                inner.min.x + inner.width() * phase,
                inner.center().y - value * half,
            )
        })
        .collect();

    painter.add(PathShape::line(
        points,
        Stroke::new(theme.stroke_thick, theme.accent),
    ));
}

/// One cycle of a shape, from -1 to 1.
///
/// Sample and hold is drawn as a fixed staircase: the real one is random, and
/// a different picture every frame would be unreadable.
fn shape_value(shape: LfoShape, phase: f32) -> f32 {
    match shape {
        LfoShape::Sine => (phase * std::f32::consts::TAU).sin(),
        LfoShape::Triangle => 1.0 - (phase * 4.0 - 1.0).abs().min(3.0 - phase * 4.0),
        LfoShape::Saw => phase * 2.0 - 1.0,
        LfoShape::ReverseSaw => 1.0 - phase * 2.0,
        LfoShape::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        LfoShape::SampleHold => match (phase * 4.0) as u32 {
            0 => 0.6,
            1 => -0.3,
            2 => 0.9,
            _ => -0.7,
        },
    }
    .clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_stays_inside_its_range() {
        for shape in LfoShape::ALL {
            for step in 0..=100 {
                let value = shape_value(shape, step as f32 / 100.0);
                assert!(
                    (-1.0..=1.0).contains(&value),
                    "{shape:?} drew {value} at {step}"
                );
            }
        }
    }

    #[test]
    fn a_saw_runs_from_bottom_to_top() {
        assert!((shape_value(LfoShape::Saw, 0.0) + 1.0).abs() < 1e-6);
        assert!((shape_value(LfoShape::Saw, 1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_reverse_saw_runs_the_other_way() {
        assert!(shape_value(LfoShape::ReverseSaw, 0.0) > shape_value(LfoShape::ReverseSaw, 1.0));
    }

    #[test]
    fn a_triangle_peaks_in_the_middle_of_its_rise() {
        let peak = shape_value(LfoShape::Triangle, 0.25);

        assert!((peak - 1.0).abs() < 1e-5, "{peak}");
    }

    #[test]
    fn the_drawn_sample_and_hold_is_the_same_every_frame() {
        let first: Vec<f32> = (0..20)
            .map(|step| shape_value(LfoShape::SampleHold, step as f32 / 20.0))
            .collect();
        let second: Vec<f32> = (0..20)
            .map(|step| shape_value(LfoShape::SampleHold, step as f32 / 20.0))
            .collect();

        assert_eq!(first, second);
    }
}
