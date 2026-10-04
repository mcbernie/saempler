//! What the host is automating, per slice.
//!
//! These are offsets on top of what a cell is set to, never replacements. A
//! slot at its defaults changes nothing, so a project that is never automated
//! sounds exactly as it was built, and an automation lane reads as a move away
//! from the sound rather than as the sound itself.

use saempler_model::MAX_SLICES;

/// One automation slot per slice.
pub const AUTOMATION_SLOTS: usize = MAX_SLICES;

/// Slot number meaning "this cell has none".
///
/// A cell whose slice sits past the bank, which cannot happen while the model
/// enforces its own limit, but the voice must still have an answer for.
pub const NO_SLOT: u8 = u8::MAX;

/// How far a full-scale cutoff automation sweeps, in octaves.
pub const AUTOMATION_CUTOFF_OCTAVES: f32 = 4.0;
/// Widest transposition the pitch automation reaches, in semitones.
pub const AUTOMATION_PITCH_SEMITONES: f32 = 24.0;

/// What the host is doing to one slice right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliceAutomation {
    /// Multiplies the cell's own gain.
    pub gain: f32,
    /// Added to the cell's pitch, in semitones.
    pub pitch_semitones: f32,
    /// Multiplies the cell's read speed.
    pub speed: f32,
    /// Shifts the cell's filter corner, in octaves.
    pub cutoff_octaves: f32,
    /// Added to how much of the cell reaches the reverb send.
    pub reverb_send: f32,
}

impl Default for SliceAutomation {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pitch_semitones: 0.0,
            speed: 1.0,
            cutoff_octaves: 0.0,
            reverb_send: 0.0,
        }
    }
}

impl SliceAutomation {
    /// Whether this slot is doing anything at all.
    ///
    /// A voice whose slot is idle skips the whole path, which is what keeps an
    /// unautomated project costing exactly what it did before the bank existed.
    pub fn is_idle(&self) -> bool {
        self.gain == 1.0
            && self.pitch_semitones == 0.0
            && self.speed == 1.0
            && self.cutoff_octaves == 0.0
            && self.reverb_send == 0.0
    }

    /// Whether this slot moves the filter, which costs a coefficient update.
    pub fn moves_the_filter(&self) -> bool {
        self.cutoff_octaves != 0.0
    }

    /// Repair anything the host sent that cannot be used.
    ///
    /// Hosts write parameter values from automation lanes and from their own
    /// restored state, and a denormal or a stale value must not reach the
    /// filter as a coefficient.
    pub fn sanitized(mut self) -> Self {
        self.gain = finite_or(self.gain, 1.0).clamp(0.0, 2.0);
        self.pitch_semitones = finite_or(self.pitch_semitones, 0.0)
            .clamp(-AUTOMATION_PITCH_SEMITONES, AUTOMATION_PITCH_SEMITONES);
        self.speed = finite_or(self.speed, 1.0).clamp(0.05, 4.0);
        self.cutoff_octaves = finite_or(self.cutoff_octaves, 0.0)
            .clamp(-AUTOMATION_CUTOFF_OCTAVES, AUTOMATION_CUTOFF_OCTAVES);
        self.reverb_send = finite_or(self.reverb_send, 0.0).clamp(0.0, 1.0);
        self
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_slot_changes_nothing() {
        let slot = SliceAutomation::default();

        assert!(slot.is_idle());
        assert!(!slot.moves_the_filter());
    }

    #[test]
    fn moving_any_one_of_them_takes_the_slot_out_of_idle() {
        for slot in [
            SliceAutomation {
                gain: 0.5,
                ..Default::default()
            },
            SliceAutomation {
                pitch_semitones: 1.0,
                ..Default::default()
            },
            SliceAutomation {
                speed: 2.0,
                ..Default::default()
            },
            SliceAutomation {
                cutoff_octaves: -1.0,
                ..Default::default()
            },
            SliceAutomation {
                reverb_send: 0.2,
                ..Default::default()
            },
        ] {
            assert!(!slot.is_idle(), "{slot:?}");
        }
    }

    #[test]
    fn a_broken_value_from_the_host_is_repaired() {
        let repaired = SliceAutomation {
            gain: f32::NAN,
            pitch_semitones: 400.0,
            speed: 0.0,
            cutoff_octaves: f32::INFINITY,
            reverb_send: -3.0,
        }
        .sanitized();

        assert_eq!(repaired.gain, 1.0);
        assert_eq!(repaired.pitch_semitones, AUTOMATION_PITCH_SEMITONES);
        assert!(repaired.speed > 0.0);
        assert_eq!(repaired.cutoff_octaves, 0.0);
        assert_eq!(repaired.reverb_send, 0.0);
    }

    #[test]
    fn there_is_one_slot_for_every_slice_a_project_may_hold() {
        assert_eq!(AUTOMATION_SLOTS, MAX_SLICES);
    }
}
