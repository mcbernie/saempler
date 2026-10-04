use serde::{Deserialize, Serialize};

use crate::modulation::Division;

/// Slowest a half-time modifier may read.
pub const MIN_HALF_TIME_RATE: f32 = 0.125;
/// Fastest it may read. Above one it is no longer half time.
pub const MAX_HALF_TIME_RATE: f32 = 1.0;

/// How hard each playback modifier hits.
///
/// The gestures themselves are fixed - a brake slows to a stop, a stutter
/// loops - but how far and how fast is a matter of taste and of tempo. A brake
/// over a whole bar is a tape stop; over a sixteenth it is a stumble, and both
/// are wanted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModifierSettings {
    /// Length of the loop a stutter imposes.
    pub stutter_division: Division,
    /// Length of the loop a repeat imposes.
    pub repeat_division: Division,
    /// What the read rate is multiplied by while half time is held.
    pub half_time_rate: f32,
    /// How long a brake takes to reach a standstill.
    pub brake_division: Division,
}

impl Default for ModifierSettings {
    fn default() -> Self {
        Self {
            stutter_division: Division::Sixteenth,
            repeat_division: Division::Eighth,
            half_time_rate: 0.5,
            brake_division: Division::Quarter,
        }
    }
}

impl ModifierSettings {
    pub fn sanitized(mut self) -> Self {
        self.half_time_rate = if self.half_time_rate.is_finite() {
            self.half_time_rate
                .clamp(MIN_HALF_TIME_RATE, MAX_HALF_TIME_RATE)
        } else {
            0.5
        };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_what_the_modifiers_always_did() {
        // Changing these would change how every existing project plays, so
        // they are pinned rather than left to whatever looks reasonable.
        let settings = ModifierSettings::default();

        assert_eq!(settings.stutter_division, Division::Sixteenth);
        assert_eq!(settings.repeat_division, Division::Eighth);
        assert_eq!(settings.half_time_rate, 0.5);
        assert_eq!(settings.brake_division, Division::Quarter);
    }

    #[test]
    fn a_broken_rate_is_repaired() {
        assert_eq!(
            ModifierSettings {
                half_time_rate: f32::NAN,
                ..Default::default()
            }
            .sanitized()
            .half_time_rate,
            0.5
        );
        assert_eq!(
            ModifierSettings {
                half_time_rate: 8.0,
                ..Default::default()
            }
            .sanitized()
            .half_time_rate,
            MAX_HALF_TIME_RATE
        );
    }

    #[test]
    fn a_stutter_can_be_set_shorter_than_a_repeat() {
        // The two are one mechanism at two lengths, and the shorter one wins
        // when both are held. That only makes sense while they can differ.
        let settings = ModifierSettings {
            stutter_division: Division::SixtyFourth,
            repeat_division: Division::Quarter,
            ..Default::default()
        };

        assert!(settings.stutter_division.whole_notes() < settings.repeat_division.whole_notes());
    }
}
