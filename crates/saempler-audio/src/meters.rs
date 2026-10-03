use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Published for an idle slot, so that frame zero stays a valid position.
pub const NO_PLAYHEAD: u64 = u64::MAX;

/// Number of playback positions published at once.
///
/// One per voice: with several notes sounding together the interface shows a
/// playhead for each, not just for the newest.
pub const PLAYHEAD_SLOTS: usize = 16;

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
    playheads: [AtomicU64; PLAYHEAD_SLOTS],
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            peak_left: AtomicU32::new(0),
            peak_right: AtomicU32::new(0),
            active_voices: AtomicU32::new(0),
            playheads: std::array::from_fn(|_| AtomicU64::new(NO_PLAYHEAD)),
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

    /// Publish where one voice is reading, or `None` while its slot is idle.
    ///
    /// Slots beyond [`PLAYHEAD_SLOTS`] are ignored rather than wrapping, so a
    /// larger voice count cannot silently overwrite another voice's position.
    pub fn store_playhead(&self, slot: usize, frame: Option<u64>) {
        if let Some(cell) = self.playheads.get(slot) {
            cell.store(frame.unwrap_or(NO_PLAYHEAD), Ordering::Relaxed);
        }
    }

    /// Clear every published position.
    pub fn clear_playheads(&self) {
        for cell in &self.playheads {
            cell.store(NO_PLAYHEAD, Ordering::Relaxed);
        }
    }

    /// Where each sounding voice is reading.
    ///
    /// Borrows rather than collecting, so a caller that only wants to know
    /// whether any voice is inside a region pays nothing.
    pub fn playheads(&self) -> impl Iterator<Item = u64> + '_ {
        self.playheads
            .iter()
            .map(|cell| cell.load(Ordering::Relaxed))
            .filter(|frame| *frame != NO_PLAYHEAD)
    }

    /// Whether any voice is sounding, without reading every slot twice.
    pub fn any_playhead(&self) -> bool {
        self.playheads().next().is_some()
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
        assert_eq!(meters.playheads().count(), 0);
        assert!(!meters.any_playhead());
    }

    #[test]
    fn a_playhead_distinguishes_frame_zero_from_silence() {
        let meters = Meters::new();

        meters.store_playhead(0, Some(0));
        assert_eq!(meters.playheads().collect::<Vec<_>>(), vec![0]);

        meters.store_playhead(0, None);
        assert_eq!(meters.playheads().count(), 0);
    }

    #[test]
    fn every_sounding_voice_gets_its_own_position() {
        let meters = Meters::new();

        meters.store_playhead(0, Some(100));
        meters.store_playhead(1, Some(50_000));
        meters.store_playhead(3, Some(7));

        let mut positions: Vec<u64> = meters.playheads().collect();
        positions.sort_unstable();

        assert_eq!(positions, vec![7, 100, 50_000]);
    }

    #[test]
    fn clearing_removes_every_position() {
        let meters = Meters::new();
        for slot in 0..PLAYHEAD_SLOTS {
            meters.store_playhead(slot, Some(slot as u64));
        }
        assert_eq!(meters.playheads().count(), PLAYHEAD_SLOTS);

        meters.clear_playheads();

        assert_eq!(meters.playheads().count(), 0);
    }

    #[test]
    fn a_slot_beyond_the_published_range_is_ignored() {
        let meters = Meters::new();

        meters.store_playhead(PLAYHEAD_SLOTS + 5, Some(42));

        assert_eq!(meters.playheads().count(), 0);
    }
}
