use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use saempler_model::{ModDestination, DESTINATION_COUNT, ENVELOPE_COUNT, LFO_COUNT};

/// Published for an idle slot, so that frame zero stays a valid position.
pub const NO_PLAYHEAD: u64 = u64::MAX;

/// Number of playback positions published at once.
///
/// One per voice: with several notes sounding together the interface shows a
/// playhead for each, not just for the newest.
pub const PLAYHEAD_SLOTS: usize = 16;

/// Bits a frame position is given inside a published playhead.
///
/// The note rides in the top eight. Fifty-six bits is more audio than any
/// sample will ever hold, so the two never collide, and one atomic per voice
/// stays one atomic per voice.
const FRAME_BITS: u32 = 56;
const FRAME_MASK: u64 = (1 << FRAME_BITS) - 1;

fn pack(note: u8, frame: u64) -> u64 {
    ((note as u64) << FRAME_BITS) | (frame & FRAME_MASK)
}

fn unpack(packed: u64) -> (u8, u64) {
    ((packed >> FRAME_BITS) as u8, packed & FRAME_MASK)
}

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
    /// One bit per modifier that would affect the next performance note.
    modifiers: AtomicU32,
    /// Host tempo in hundredths of a beat per minute.
    tempo: AtomicU32,
    /// Where the modulation of the newest voice stands.
    ///
    /// Published so the interface can show the modules running rather than
    /// only their settings: a knob under an envelope has to be seen to move.
    /// One voice rather than all of them, because several voices would have to
    /// be averaged into something that matches none of them.
    envelopes: [AtomicU32; ENVELOPE_COUNT],
    lfos: [AtomicU32; LFO_COUNT],
    destinations: [AtomicU32; DESTINATION_COUNT],
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            peak_left: AtomicU32::new(0),
            peak_right: AtomicU32::new(0),
            active_voices: AtomicU32::new(0),
            playheads: std::array::from_fn(|_| AtomicU64::new(NO_PLAYHEAD)),
            modifiers: AtomicU32::new(0),
            tempo: AtomicU32::new(12_000),
            envelopes: std::array::from_fn(|_| AtomicU32::new(0)),
            lfos: std::array::from_fn(|_| AtomicU32::new(0)),
            destinations: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}

impl Meters {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish the host tempo, in beats per minute.
    pub fn store_tempo(&self, tempo: f64) {
        let hundredths = (tempo.clamp(1.0, 999.0) * 100.0) as u32;
        self.tempo.store(hundredths, Ordering::Relaxed);
    }

    /// Host tempo in beats per minute.
    pub fn tempo(&self) -> f64 {
        f64::from(self.tempo.load(Ordering::Relaxed)) / 100.0
    }

    /// Publish where the newest voice's modulation stands.
    ///
    /// Called once per block rather than per frame: the interface redraws far
    /// more slowly than the audio runs, and a per-frame store would be work
    /// nobody sees.
    pub fn store_modulation(
        &self,
        envelopes: [f32; ENVELOPE_COUNT],
        lfos: [f32; LFO_COUNT],
        destinations: [f32; DESTINATION_COUNT],
    ) {
        for (slot, value) in self.envelopes.iter().zip(envelopes) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        for (slot, value) in self.lfos.iter().zip(lfos) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        for (slot, value) in self.destinations.iter().zip(destinations) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    /// Clear the published modulation, for when nothing is sounding.
    pub fn clear_modulation(&self) {
        self.store_modulation(
            [0.0; ENVELOPE_COUNT],
            [0.0; LFO_COUNT],
            [0.0; DESTINATION_COUNT],
        );
    }

    /// Level of an envelope of the newest voice, from 0 to 1.
    pub fn envelope(&self, index: usize) -> f32 {
        self.envelopes
            .get(index)
            .map(|slot| f32::from_bits(slot.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }

    /// Output of an LFO of the newest voice, from -1 to 1.
    pub fn lfo(&self, index: usize) -> f32 {
        self.lfos
            .get(index)
            .map(|slot| f32::from_bits(slot.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }

    /// How much modulation is reaching a destination on the newest voice.
    pub fn destination(&self, destination: ModDestination) -> f32 {
        self.destinations
            .get(destination.index())
            .map(|slot| f32::from_bits(slot.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
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
    pub fn store_playhead(&self, slot: usize, position: Option<(u8, u64)>) {
        if let Some(cell) = self.playheads.get(slot) {
            let packed = match position {
                Some((note, frame)) => pack(note, frame),
                None => NO_PLAYHEAD,
            };
            cell.store(packed, Ordering::Relaxed);
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
        self.voices().map(|(_, frame)| frame)
    }

    /// Which note each sounding voice is playing, and where it is reading.
    ///
    /// The note matters wherever one region can be played by more than one
    /// key: two cells may share a slice, and a position alone cannot say which
    /// of them is sounding.
    pub fn voices(&self) -> impl Iterator<Item = (u8, u64)> + '_ {
        self.playheads
            .iter()
            .map(|cell| cell.load(Ordering::Relaxed))
            .filter(|packed| *packed != NO_PLAYHEAD)
            .map(unpack)
    }

    /// Whether any voice is sounding, without reading every slot twice.
    pub fn any_playhead(&self) -> bool {
        self.playheads().next().is_some()
    }

    /// Publish which modifiers are engaged, one bit each.
    pub fn store_modifiers(&self, bits: u32) {
        self.modifiers.store(bits, Ordering::Relaxed);
    }

    /// Which modifiers are engaged, one bit each.
    pub fn modifiers(&self) -> u32 {
        self.modifiers.load(Ordering::Relaxed)
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
        assert_eq!(meters.modifiers(), 0);
    }

    #[test]
    fn engaged_modifiers_round_trip() {
        let meters = Meters::new();

        meters.store_modifiers(0b1010);

        assert_eq!(meters.modifiers(), 0b1010);
    }

    #[test]
    fn a_playhead_distinguishes_frame_zero_from_silence() {
        let meters = Meters::new();

        meters.store_playhead(0, Some((60, 0)));
        assert_eq!(meters.playheads().collect::<Vec<_>>(), vec![0]);

        meters.store_playhead(0, None);
        assert_eq!(meters.playheads().count(), 0);
    }

    #[test]
    fn every_sounding_voice_gets_its_own_position() {
        let meters = Meters::new();

        meters.store_playhead(0, Some((60, 100)));
        meters.store_playhead(1, Some((60, 50_000)));
        meters.store_playhead(3, Some((60, 7)));

        let mut positions: Vec<u64> = meters.playheads().collect();
        positions.sort_unstable();

        assert_eq!(positions, vec![7, 100, 50_000]);
    }

    #[test]
    fn clearing_removes_every_position() {
        let meters = Meters::new();
        for slot in 0..PLAYHEAD_SLOTS {
            meters.store_playhead(slot, Some((60, slot as u64)));
        }
        assert_eq!(meters.playheads().count(), PLAYHEAD_SLOTS);

        meters.clear_playheads();

        assert_eq!(meters.playheads().count(), 0);
    }

    #[test]
    fn two_voices_on_one_region_are_told_apart_by_their_notes() {
        // A copied cell plays the same slice from the same frames. Only the
        // note says which of the two is sounding, so the interface cannot
        // light the right pad without it.
        let meters = Meters::new();

        meters.store_playhead(0, Some((60, 1_000)));
        meters.store_playhead(1, Some((64, 1_000)));

        let mut voices: Vec<(u8, u64)> = meters.voices().collect();
        voices.sort();
        assert_eq!(voices, vec![(60, 1_000), (64, 1_000)]);
    }

    #[test]
    fn a_position_survives_riding_next_to_its_note() {
        // Packing must not cost range: a frame far past anything a real sample
        // holds still has to come back unchanged.
        let meters = Meters::new();
        let far = 1_u64 << 40;

        meters.store_playhead(0, Some((127, far)));

        assert_eq!(meters.voices().next(), Some((127, far)));
        assert_eq!(meters.playheads().next(), Some(far));
    }

    #[test]
    fn a_slot_beyond_the_published_range_is_ignored() {
        let meters = Meters::new();

        meters.store_playhead(PLAYHEAD_SLOTS + 5, Some((60, 42)));

        assert_eq!(meters.playheads().count(), 0);
    }
}
