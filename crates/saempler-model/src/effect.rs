use serde::{Deserialize, Serialize};

use crate::modulation::Division;

/// What a cell's own filter lets through.
///
/// A copy of the DSP crate's mode rather than a re-export: the model is the
/// serialized shape of a project and must not change because a DSP type was
/// rearranged. The engine translates between them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterShape {
    #[default]
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

impl FilterShape {
    pub const ALL: [FilterShape; 4] = [
        FilterShape::LowPass,
        FilterShape::HighPass,
        FilterShape::BandPass,
        FilterShape::Notch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterShape::LowPass => "Low Pass",
            FilterShape::HighPass => "High Pass",
            FilterShape::BandPass => "Band Pass",
            FilterShape::Notch => "Notch",
        }
    }
}

/// The curve a cell's saturator bends by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveShape {
    #[default]
    Soft,
    Hard,
    Tube,
}

impl DriveShape {
    pub const ALL: [DriveShape; 3] = [DriveShape::Soft, DriveShape::Hard, DriveShape::Tube];

    pub fn label(self) -> &'static str {
        match self {
            DriveShape::Soft => "Soft",
            DriveShape::Hard => "Hard",
            DriveShape::Tube => "Tube",
        }
    }
}

/// Limits of the per-cell filter and drive.
pub const MIN_CUTOFF_HZ: f32 = 20.0;
pub const MAX_CUTOFF_HZ: f32 = 18_000.0;
pub const MIN_RESONANCE: f32 = 0.5;
pub const MAX_RESONANCE: f32 = 20.0;
pub const MAX_DRIVE: f32 = 32.0;

/// The effects a cell carries on its own voice.
///
/// Filter and drive only. They are cheap, they belong to the chop rather than
/// to the mix, and a voice can afford one of each; a delay or a reverb per
/// voice would mean sixteen of them, which is why those are sends.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellEffects {
    pub filter_on: bool,
    pub filter_shape: FilterShape,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive_on: bool,
    pub drive_shape: DriveShape,
    pub drive: f32,
    /// How much of this cell is fed to each send.
    pub delay_send: f32,
    pub reverb_send: f32,
    pub phaser_send: f32,
    pub flanger_send: f32,
}

impl Default for CellEffects {
    fn default() -> Self {
        Self {
            filter_on: false,
            filter_shape: FilterShape::LowPass,
            cutoff_hz: 8_000.0,
            resonance: 0.707,
            drive_on: false,
            drive_shape: DriveShape::Soft,
            drive: 1.0,
            delay_send: 0.0,
            reverb_send: 0.0,
            phaser_send: 0.0,
            flanger_send: 0.0,
        }
    }
}

impl CellEffects {
    /// Pull every value back into the range the controls allow.
    pub fn sanitized(mut self) -> Self {
        self.cutoff_hz = finite_or(self.cutoff_hz, 8_000.0).clamp(MIN_CUTOFF_HZ, MAX_CUTOFF_HZ);
        self.resonance = finite_or(self.resonance, 0.707).clamp(MIN_RESONANCE, MAX_RESONANCE);
        self.drive = finite_or(self.drive, 1.0).clamp(1.0, MAX_DRIVE);
        for send in [
            &mut self.delay_send,
            &mut self.reverb_send,
            &mut self.phaser_send,
            &mut self.flanger_send,
        ] {
            *send = finite_or(*send, 0.0).clamp(0.0, 1.0);
        }
        self
    }

    /// Whether anything is switched on, so the engine can skip the chain.
    pub fn is_active(&self) -> bool {
        self.filter_on
            || self.drive_on
            || self.delay_send > 0.0
            || self.reverb_send > 0.0
            || self.phaser_send > 0.0
            || self.flanger_send > 0.0
    }
}

/// The sends, shared by every voice, sitting behind the mixer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SendEffects {
    /// Repeat time, as a note value when synced and in seconds otherwise.
    pub delay_sync: bool,
    pub delay_division: Division,
    pub delay_seconds: f32,
    pub delay_feedback: f32,
    pub delay_damping_hz: f32,

    pub reverb_size: f32,
    pub reverb_damping: f32,

    pub phaser_rate_hz: f32,
    pub phaser_depth: f32,
    pub phaser_feedback: f32,

    pub flanger_rate_hz: f32,
    pub flanger_depth: f32,
    pub flanger_feedback: f32,

    /// How much of each send comes back into the mix.
    ///
    /// A send is added on top of the dry signal, so without a return level
    /// the only way to make an effect quieter was to feed it less, which
    /// changes how it sounds as well as how loud it is. These start below
    /// unity because an effect at full return is as loud as the sound it was
    /// made from.
    pub delay_level: f32,
    pub reverb_level: f32,
    pub phaser_level: f32,
    pub flanger_level: f32,
}

impl Default for SendEffects {
    fn default() -> Self {
        Self {
            delay_sync: true,
            delay_division: Division::Eighth,
            delay_seconds: 0.25,
            delay_feedback: 0.35,
            delay_damping_hz: 6_000.0,

            reverb_size: 0.6,
            reverb_damping: 0.4,

            phaser_rate_hz: 0.5,
            phaser_depth: 0.7,
            phaser_feedback: 0.4,

            flanger_rate_hz: 0.3,
            flanger_depth: 0.8,
            flanger_feedback: 0.5,

            delay_level: 0.45,
            reverb_level: 0.35,
            phaser_level: 0.5,
            flanger_level: 0.5,
        }
    }
}

impl SendEffects {
    pub fn sanitized(mut self) -> Self {
        self.delay_seconds = finite_or(self.delay_seconds, 0.25).clamp(0.001, 2.0);
        self.delay_feedback = finite_or(self.delay_feedback, 0.35).clamp(0.0, 0.95);
        self.delay_damping_hz = finite_or(self.delay_damping_hz, 6_000.0).clamp(200.0, 18_000.0);
        self.reverb_size = finite_or(self.reverb_size, 0.6).clamp(0.0, 1.0);
        self.reverb_damping = finite_or(self.reverb_damping, 0.4).clamp(0.0, 0.95);
        self.phaser_rate_hz = finite_or(self.phaser_rate_hz, 0.5).clamp(0.01, 10.0);
        self.phaser_depth = finite_or(self.phaser_depth, 0.7).clamp(0.0, 1.0);
        self.phaser_feedback = finite_or(self.phaser_feedback, 0.4).clamp(0.0, 0.9);
        self.flanger_rate_hz = finite_or(self.flanger_rate_hz, 0.3).clamp(0.01, 10.0);
        self.flanger_depth = finite_or(self.flanger_depth, 0.8).clamp(0.0, 1.0);
        self.flanger_feedback = finite_or(self.flanger_feedback, 0.5).clamp(-0.95, 0.95);
        for level in [
            &mut self.delay_level,
            &mut self.reverb_level,
            &mut self.phaser_level,
            &mut self.flanger_level,
        ] {
            *level = finite_or(*level, 0.4).clamp(0.0, MAX_SEND_LEVEL);
        }
        self
    }

    /// Return level of each send, in the engine's send order.
    pub fn levels(&self) -> [f32; 4] {
        [
            self.delay_level,
            self.reverb_level,
            self.phaser_level,
            self.flanger_level,
        ]
    }

    /// Repeat time in seconds at the given tempo.
    pub fn delay_time(&self, tempo: f64) -> f32 {
        if self.delay_sync {
            // Four beats to a whole note, and sixty seconds to that many.
            let seconds = self.delay_division.whole_notes() as f64 * 240.0 / tempo.max(1.0);
            (seconds as f32).clamp(0.001, 2.0)
        } else {
            self.delay_seconds
        }
    }
}

/// Loudest a send may come back at.
///
/// One is as loud as the sound that fed it, which is already more effect than
/// dry in the mix. There is no reason to go past that.
pub const MAX_SEND_LEVEL: f32 = 1.0;
/// Hardest the driven sends may be saturated.
pub const MAX_SEND_DRIVE: f32 = 16.0;

/// Both settings of the shared sends, and what switches between them.
///
/// An effect modifier does not add an effect: it throws the whole mix into
/// one that is already there, set up differently. Keeping the driven settings
/// as data rather than as constants in the engine is what makes that second
/// sound something to dial in rather than something to accept.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SendRack {
    /// How the sends sound normally.
    pub normal: SendEffects,
    /// How they sound while an effect modifier is held.
    pub driven: SendEffects,
    /// Saturation on a driven send, which is most of what makes it driven.
    pub drive: f32,
    pub drive_shape: DriveShape,
}

impl Default for SendRack {
    fn default() -> Self {
        Self {
            normal: SendEffects::default(),
            driven: SendEffects {
                // Short, long-tailed and bright: a throw rather than an echo.
                delay_sync: true,
                delay_division: Division::Sixteenth,
                delay_feedback: 0.85,
                delay_damping_hz: 16_000.0,
                delay_level: 0.8,

                // The biggest room, barely damped.
                reverb_size: 1.0,
                reverb_damping: 0.05,
                reverb_level: 0.7,

                phaser_rate_hz: 4.0,
                phaser_depth: 1.0,
                phaser_feedback: 0.9,
                phaser_level: 0.8,

                flanger_rate_hz: 2.0,
                flanger_depth: 1.0,
                flanger_feedback: 0.92,
                flanger_level: 0.8,

                ..SendEffects::default()
            },
            drive: 6.0,
            drive_shape: DriveShape::Tube,
        }
    }
}

impl SendRack {
    pub fn sanitized(mut self) -> Self {
        self.normal = self.normal.sanitized();
        self.driven = self.driven.sanitized();
        self.drive = finite_or(self.drive, 6.0).clamp(1.0, MAX_SEND_DRIVE);
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
    fn a_new_cell_has_nothing_switched_on() {
        let effects = CellEffects::default();

        assert!(!effects.is_active());
    }

    #[test]
    fn switching_anything_on_makes_the_chain_worth_running() {
        for effects in [
            CellEffects {
                filter_on: true,
                ..Default::default()
            },
            CellEffects {
                drive_on: true,
                ..Default::default()
            },
            CellEffects {
                reverb_send: 0.3,
                ..Default::default()
            },
        ] {
            assert!(effects.is_active(), "{effects:?}");
        }
    }

    #[test]
    fn broken_cell_values_are_repaired() {
        let repaired = CellEffects {
            // Not a number falls back to the default; a number out of range
            // is clamped to the end it ran past.
            cutoff_hz: f32::NAN,
            resonance: 500.0,
            drive: -5.0,
            reverb_send: 9.0,
            ..Default::default()
        }
        .sanitized();

        assert_eq!(repaired.cutoff_hz, 8_000.0);
        assert_eq!(repaired.resonance, MAX_RESONANCE);
        assert_eq!(repaired.drive, 1.0);
        assert_eq!(repaired.reverb_send, 1.0);
    }

    #[test]
    fn broken_send_values_are_repaired() {
        let repaired = SendEffects {
            delay_seconds: f32::NAN,
            delay_feedback: 5.0,
            reverb_size: -1.0,
            flanger_feedback: f32::INFINITY,
            ..Default::default()
        }
        .sanitized();

        assert_eq!(repaired.delay_seconds, 0.25);
        assert_eq!(repaired.delay_feedback, 0.95);
        assert_eq!(repaired.reverb_size, 0.0);
        assert_eq!(repaired.flanger_feedback, 0.5);
    }

    #[test]
    fn a_send_comes_back_below_the_sound_that_fed_it() {
        // A send is added on top of the dry signal. At unity the effect is as
        // loud as the chop, which is why the four of them together were
        // drowning everything.
        let sends = SendEffects::default();

        for level in sends.levels() {
            assert!(level > 0.0 && level < 1.0, "{level}");
        }
    }

    #[test]
    fn a_broken_level_is_repaired() {
        let repaired = SendEffects {
            delay_level: f32::NAN,
            reverb_level: 40.0,
            phaser_level: -1.0,
            ..Default::default()
        }
        .sanitized();

        assert_eq!(repaired.delay_level, 0.4);
        assert_eq!(repaired.reverb_level, MAX_SEND_LEVEL);
        assert_eq!(repaired.phaser_level, 0.0);
    }

    #[test]
    fn the_driven_settings_are_harder_than_the_normal_ones() {
        let rack = SendRack::default();

        assert!(rack.driven.delay_feedback > rack.normal.delay_feedback);
        assert!(rack.driven.reverb_size > rack.normal.reverb_size);
        assert!(rack.driven.phaser_feedback > rack.normal.phaser_feedback);
        assert!(rack.driven.flanger_feedback > rack.normal.flanger_feedback);
        assert!(rack.drive > 1.0);
    }

    #[test]
    fn a_broken_rack_is_repaired_on_both_sides() {
        let repaired = SendRack {
            normal: SendEffects {
                reverb_size: f32::NAN,
                ..Default::default()
            },
            driven: SendEffects {
                phaser_level: 9.0,
                ..Default::default()
            },
            drive: f32::INFINITY,
            ..Default::default()
        }
        .sanitized();

        assert_eq!(repaired.normal.reverb_size, 0.6);
        assert_eq!(repaired.driven.phaser_level, MAX_SEND_LEVEL);
        assert_eq!(repaired.drive, 6.0);
    }

    #[test]
    fn a_synced_delay_follows_the_tempo() {
        let effects = SendEffects {
            delay_sync: true,
            delay_division: Division::Quarter,
            ..Default::default()
        };

        // A quarter note at 120 bpm is half a second.
        assert!((effects.delay_time(120.0) - 0.5).abs() < 1e-4);
        // At twice the tempo it is half as long.
        assert!((effects.delay_time(240.0) - 0.25).abs() < 1e-4);
    }

    #[test]
    fn a_free_delay_ignores_the_tempo() {
        let effects = SendEffects {
            delay_sync: false,
            delay_seconds: 0.4,
            ..Default::default()
        };

        assert_eq!(effects.delay_time(120.0), 0.4);
        assert_eq!(effects.delay_time(240.0), 0.4);
    }

    #[test]
    fn a_synced_delay_stays_inside_the_line() {
        // Four bars at a very slow tempo would want more than the line holds.
        let effects = SendEffects {
            delay_sync: true,
            delay_division: Division::FourBars,
            ..Default::default()
        };

        assert!(effects.delay_time(20.0) <= 2.0);
    }
}
