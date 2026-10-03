use nih_plug_egui::egui::{pos2, vec2, Align2, FontId, Rect, Sense, StrokeKind, Ui};

use crate::theme::Theme;

/// Lowest level shown by the meter.
const FLOOR_DB: f32 = -60.0;
/// Level above which the bar switches to the clipping colour.
const CLIP_DB: f32 = -0.1;

/// A horizontal stereo peak meter.
///
/// `peaks` are linear gain values as published by the engine.
pub fn stereo_meter(ui: &mut Ui, theme: &Theme, peaks: (f32, f32), width: f32) {
    let bar_height = theme.font_sm;
    let height = bar_height * 2.0 + theme.spacing_sm;
    let (rect, _response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());

    let left_rect = Rect::from_min_size(rect.min, vec2(width, bar_height));
    let right_rect = Rect::from_min_size(
        pos2(rect.min.x, rect.min.y + bar_height + theme.spacing_sm),
        vec2(width, bar_height),
    );

    bar(ui, theme, left_rect, peaks.0);
    bar(ui, theme, right_rect, peaks.1);
}

/// Draw a single meter bar.
fn bar(ui: &mut Ui, theme: &Theme, rect: Rect, peak: f32) {
    let painter = ui.painter();
    painter.rect_filled(rect, theme.radius_sm, theme.control_bg);

    let db = gain_to_db(peak);
    let filled = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
    if filled > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, vec2(rect.width() * filled, rect.height()));
        let color = if db >= CLIP_DB {
            theme.danger
        } else {
            theme.accent
        };
        painter.rect_filled(fill_rect, theme.radius_sm, color);
    }

    painter.rect_stroke(
        rect,
        theme.radius_sm,
        theme.outline_stroke(),
        StrokeKind::Inside,
    );
}

/// Convert linear gain to dBFS, clamped at the meter floor.
fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 {
        return FLOOR_DB;
    }
    (20.0 * gain.log10()).max(FLOOR_DB)
}

/// A read-only text readout, used for values the engine publishes.
pub fn readout(ui: &mut Ui, theme: &Theme, label: &str, value: &str, width: f32) {
    let height = theme.font_md + theme.spacing_md;
    let (rect, _response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, theme.radius_sm, theme.panel_bg);
    painter.text(
        pos2(rect.min.x + theme.spacing_md, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme.font_sm),
        theme.text_dim,
    );
    painter.text(
        pos2(rect.max.x - theme.spacing_md, rect.center().y),
        Align2::RIGHT_CENTER,
        value,
        FontId::monospace(theme.font_sm),
        theme.text,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_maps_to_the_meter_floor() {
        assert_eq!(gain_to_db(0.0), FLOOR_DB);
        assert_eq!(gain_to_db(-0.5), FLOOR_DB);
    }

    #[test]
    fn unity_gain_maps_to_zero_db() {
        assert!(gain_to_db(1.0).abs() < 1e-5);
    }

    #[test]
    fn half_gain_maps_to_roughly_minus_six_db() {
        assert!((gain_to_db(0.5) + 6.02).abs() < 0.01);
    }
}
