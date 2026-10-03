use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Published when no voice is sounding, so that zero stays a valid frame.
pub const NO_PLAYHEAD: u64 = u64::MAX;

/// Values published by the audio thread for display in the user interface.
///
/// Floats are stored as their bit patterns in [`AtomicU32`] so that no lock is
/// needed. Readers may observe a slightly stale value, which is acceptable for
/// metering.
#[derive(Debug)]
pub struct Meters {
    peak_left: AtomicU32,
    peak_right: AtomicU32,
    active_voices: AtomicU32,
    playhead: AtomicU64,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            peak_left: AtomicU32::new(0),
            peak_right: AtomicU32::new(0),
            active_voices: AtomicU32::new(0),
            playhead: AtomicU64::new(NO_PLAYHEAD),
        }
    }
}

impl Meters {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish the peak levels of the block that was just rendered.
    pub fn store_peaks(&self, left: f32, right: f32) {
        self.peak_left.store(left.to_bits(), Ordering::Relaxed);
        self.peak_right.store(right.to_bits(), Ordering::Relaxed);
    }

    /// Most recently published peak levels as linear gain.
    pub fn peaks(&self) -> (f32, f32) {
        (
            f32::from_bits(self.peak_left.load(Ordering::Relaxed)),
            f32::from_bits(self.peak_right.load(Ordering::Relaxed)),
        )
    }

    /// Publish how many voices are currently sounding.
    pub fn store_active_voices(&self, count: u32) {
        self.active_voices.store(count, Ordering::Relaxed);
    }

    /// Most recently published voice count.
    pub fn active_voices(&self) -> u32 {
        self.active_voices.load(Ordering::Relaxed)
    }

    /// Publish the frame the newest sounding voice is reading.
    pub fn store_playhead(&self, frame: Option<u64>) {
        self.playhead
            .store(frame.unwrap_or(NO_PLAYHEAD), Ordering::Relaxed);
    }

    /// Frame of the newest sounding voice, or `None` while nothing sounds.
    pub fn playhead(&self) -> Option<u64> {
        match self.playhead.load(Ordering::Relaxed) {
            NO_PLAYHEAD => None,
            frame => Some(frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_round_trip_through_bit_patterns() {
        let meters = Meters::new();
        meters.store_peaks(0.25, 0.75);

        assert_eq!(meters.peaks(), (0.25, 0.75));
    }

    #[test]
    fn fresh_meters_report_silence() {
        let meters = Meters::new();

        assert_eq!(meters.peaks(), (0.0, 0.0));
        assert_eq!(meters.active_voices(), 0);
        assert_eq!(meters.playhead(), None);
    }

    #[test]
    fn the_playhead_distinguishes_frame_zero_from_silence() {
        let meters = Meters::new();

        meters.store_playhead(Some(0));
        assert_eq!(meters.playhead(), Some(0));

        meters.store_playhead(None);
        assert_eq!(meters.playhead(), None);
    }
}
