/// The section of the sample the waveform is showing.
///
/// Pure arithmetic, kept apart from the drawing so that zooming and panning
/// can be tested without a user interface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewRange {
    pub start_frame: u64,
    pub end_frame: u64,
}

/// Closest zoom, in frames across the whole widget.
///
/// Below this the display would show fewer than a handful of samples, which
/// stops being useful for editing.
pub const MIN_VISIBLE_FRAMES: u64 = 32;

impl ViewRange {
    /// The whole sample.
    pub fn full(total_frames: u64) -> Self {
        Self {
            start_frame: 0,
            end_frame: total_frames,
        }
    }

    /// Number of frames on screen.
    pub fn len_frames(&self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }

    pub fn is_empty(&self) -> bool {
        self.len_frames() == 0
    }

    /// Zoom by `factor` around `anchor`, keeping that frame under the pointer.
    ///
    /// A factor below one zooms in. The result stays inside the sample and
    /// never gets closer than [`MIN_VISIBLE_FRAMES`].
    pub fn zoomed(self, anchor: u64, factor: f32, total_frames: u64) -> Self {
        if total_frames == 0 || !factor.is_finite() || factor <= 0.0 {
            return self;
        }

        let current = self.len_frames().max(1);
        let target = ((current as f64) * factor as f64).round() as u64;
        let target = target.clamp(MIN_VISIBLE_FRAMES.min(total_frames).max(1), total_frames);
        if target == current {
            return self;
        }

        // Keep the anchor at the same relative position in the view.
        let anchor = anchor.clamp(self.start_frame, self.end_frame);
        let offset = anchor - self.start_frame;
        let ratio = offset as f64 / current as f64;
        let new_offset = (ratio * target as f64).round() as u64;
        let start = anchor.saturating_sub(new_offset);

        Self {
            start_frame: start,
            end_frame: start.saturating_add(target),
        }
        .clamped(total_frames)
    }

    /// Shift the view by `delta` frames without changing the zoom.
    pub fn panned(self, delta: i64, total_frames: u64) -> Self {
        let len = self.len_frames();
        if len == 0 {
            return self;
        }

        let start = if delta >= 0 {
            self.start_frame.saturating_add(delta as u64)
        } else {
            self.start_frame.saturating_sub(delta.unsigned_abs())
        };

        Self {
            start_frame: start,
            end_frame: start.saturating_add(len),
        }
        .clamped(total_frames)
    }

    /// Pull the range back inside `0..total_frames`, keeping its length.
    pub fn clamped(self, total_frames: u64) -> Self {
        if total_frames == 0 {
            return Self::default();
        }

        let len = self.len_frames().clamp(1, total_frames);
        let start = self.start_frame.min(total_frames - len);

        Self {
            start_frame: start,
            end_frame: start + len,
        }
    }

    /// Whether the range covers the whole sample.
    pub fn is_full(&self, total_frames: u64) -> bool {
        self.start_frame == 0 && self.end_frame >= total_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOTAL: u64 = 100_000;

    #[test]
    fn a_full_view_covers_the_whole_sample() {
        let view = ViewRange::full(TOTAL);

        assert_eq!(view.len_frames(), TOTAL);
        assert!(view.is_full(TOTAL));
    }

    #[test]
    fn zooming_in_keeps_the_anchor_under_the_pointer() {
        let view = ViewRange::full(TOTAL);
        let anchor = 60_000;

        let zoomed = view.zoomed(anchor, 0.5, TOTAL);

        assert_eq!(zoomed.len_frames(), TOTAL / 2);
        // The anchor sat at 60% of the view and must still sit at 60%.
        let before = (anchor - view.start_frame) as f64 / view.len_frames() as f64;
        let after = (anchor - zoomed.start_frame) as f64 / zoomed.len_frames() as f64;
        assert!((before - after).abs() < 0.01, "{before} vs {after}");
    }

    #[test]
    fn zooming_out_stops_at_the_whole_sample() {
        let view = ViewRange {
            start_frame: 40_000,
            end_frame: 60_000,
        };

        let zoomed = view.zoomed(50_000, 100.0, TOTAL);

        assert_eq!(zoomed, ViewRange::full(TOTAL));
        assert!(zoomed.is_full(TOTAL));
    }

    #[test]
    fn zooming_in_stops_at_the_minimum() {
        let mut view = ViewRange::full(TOTAL);

        for _ in 0..100 {
            view = view.zoomed(50_000, 0.5, TOTAL);
        }

        assert_eq!(view.len_frames(), MIN_VISIBLE_FRAMES);
    }

    #[test]
    fn zooming_at_the_left_edge_does_not_underflow() {
        let view = ViewRange::full(TOTAL);

        let zoomed = view.zoomed(0, 0.25, TOTAL);

        assert_eq!(zoomed.start_frame, 0);
        assert_eq!(zoomed.len_frames(), TOTAL / 4);
    }

    #[test]
    fn zooming_at_the_right_edge_stays_inside_the_sample() {
        let view = ViewRange::full(TOTAL);

        let zoomed = view.zoomed(TOTAL, 0.25, TOTAL);

        assert_eq!(zoomed.end_frame, TOTAL);
        assert_eq!(zoomed.len_frames(), TOTAL / 4);
    }

    #[test]
    fn panning_keeps_the_zoom_level() {
        let view = ViewRange {
            start_frame: 40_000,
            end_frame: 50_000,
        };

        let panned = view.panned(5_000, TOTAL);

        assert_eq!(panned.len_frames(), 10_000);
        assert_eq!(panned.start_frame, 45_000);
    }

    #[test]
    fn panning_stops_at_both_ends() {
        let view = ViewRange {
            start_frame: 40_000,
            end_frame: 50_000,
        };

        assert_eq!(view.panned(-1_000_000, TOTAL).start_frame, 0);
        assert_eq!(view.panned(1_000_000, TOTAL).end_frame, TOTAL);
        assert_eq!(view.panned(1_000_000, TOTAL).len_frames(), 10_000);
    }

    #[test]
    fn a_view_wider_than_the_sample_is_pulled_back_in() {
        let view = ViewRange {
            start_frame: 90_000,
            end_frame: 500_000,
        };

        let clamped = view.clamped(TOTAL);

        assert_eq!(clamped, ViewRange::full(TOTAL));
    }

    #[test]
    fn an_empty_sample_produces_an_empty_view() {
        assert!(ViewRange::full(0).is_empty());
        assert!(ViewRange::full(TOTAL).clamped(0).is_empty());
        assert_eq!(ViewRange::full(TOTAL).zoomed(0, 0.5, 0).len_frames(), TOTAL);
    }

    #[test]
    fn a_degenerate_zoom_factor_changes_nothing() {
        let view = ViewRange::full(TOTAL);

        assert_eq!(view.zoomed(0, 0.0, TOTAL), view);
        assert_eq!(view.zoomed(0, -1.0, TOTAL), view);
        assert_eq!(view.zoomed(0, f32::NAN, TOTAL), view);
    }
}
