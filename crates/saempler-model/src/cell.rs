use serde::{Deserialize, Serialize};

use crate::slice::SliceId;

/// Identifies a performance cell within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CellId(pub u32);

/// Lowest and highest playback rate a cell may be set to.
///
/// Four octaves either way is more than the material stays usable over, and
/// keeps the read position from advancing so fast that interpolation becomes
/// meaningless.
pub const MIN_SPEED: f32 = 0.0625;
pub const MAX_SPEED: f32 = 16.0;

/// Range of the pitch control, in semitones.
pub const MAX_PITCH_SEMITONES: f32 = 24.0;

/// How a cell plays its slice.
///
/// Pitch and speed both change how fast the slice is read, so raising the
/// pitch also shortens the slice. Separating them needs time stretching,
/// which does not exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSettings {
    /// Play the slice backwards.
    pub reverse: bool,
    /// Playback rate multiplier. 1.0 plays at the recorded speed.
    pub speed: f32,
    /// Transposition in semitones, applied on top of `speed`.
    pub pitch_semitones: f32,
    /// Level of this cell, as linear gain.
    pub gain: f32,
    /// Fade in at the start of the slice.
    pub attack_ms: f32,
    /// Fade out after the key is released or the slice ends.
    pub release_ms: f32,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            reverse: false,
            speed: 1.0,
            pitch_semitones: 0.0,
            gain: 1.0,
            attack_ms: 3.0,
            release_ms: 30.0,
        }
    }
}

impl PlaybackSettings {
    /// Frames advanced per output frame, combining speed and pitch.
    pub fn rate(&self) -> f32 {
        let pitch = 2.0f32.powf(self.pitch_semitones / 12.0);
        (self.speed * pitch).clamp(MIN_SPEED, MAX_SPEED)
    }

    /// Pull every field back into the range the controls allow.
    ///
    /// Applied after loading, so that hand-edited or future state cannot feed
    /// the engine a rate of zero or a negative envelope.
    pub fn sanitized(mut self) -> Self {
        if !self.speed.is_finite() {
            self.speed = 1.0;
        }
        if !self.pitch_semitones.is_finite() {
            self.pitch_semitones = 0.0;
        }
        if !self.gain.is_finite() {
            self.gain = 1.0;
        }
        self.speed = self.speed.clamp(MIN_SPEED, MAX_SPEED);
        self.pitch_semitones = self
            .pitch_semitones
            .clamp(-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES);
        self.gain = self.gain.clamp(0.0, 4.0);
        self.attack_ms = if self.attack_ms.is_finite() {
            self.attack_ms.clamp(0.0, 5_000.0)
        } else {
            3.0
        };
        self.release_ms = if self.release_ms.is_finite() {
            self.release_ms.clamp(0.0, 5_000.0)
        } else {
            30.0
        };
        self
    }
}

/// A slice assigned to a MIDI note, with its own way of playing it.
///
/// Several cells may reference the same slice. That is the point of the
/// design: one chop, many performances of it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PerformanceCell {
    pub id: CellId,
    pub midi_note: u8,
    pub slice: SliceId,
    pub playback: PlaybackSettings,
}

/// Name of a MIDI note, with C3 at note 60.
///
/// Octave numbering differs between makers; this follows the convention most
/// of the hardware this is played from uses.
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = note as i32 / 12 - 2;
    format!("{}{}", NAMES[note as usize % 12], octave)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_cell_plays_the_slice_as_recorded() {
        let settings = PlaybackSettings::default();

        assert!(!settings.reverse);
        assert!((settings.rate() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn an_octave_up_doubles_the_rate() {
        let settings = PlaybackSettings {
            pitch_semitones: 12.0,
            ..Default::default()
        };

        assert!((settings.rate() - 2.0).abs() < 1e-5);
    }

    #[test]
    fn an_octave_down_halves_the_rate() {
        let settings = PlaybackSettings {
            pitch_semitones: -12.0,
            ..Default::default()
        };

        assert!((settings.rate() - 0.5).abs() < 1e-5);
    }

    #[test]
    fn speed_and_pitch_multiply() {
        let settings = PlaybackSettings {
            speed: 0.5,
            pitch_semitones: 12.0,
            ..Default::default()
        };

        assert!((settings.rate() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_rate_stays_inside_its_limits() {
        let fast = PlaybackSettings {
            speed: MAX_SPEED,
            pitch_semitones: MAX_PITCH_SEMITONES,
            ..Default::default()
        };
        let slow = PlaybackSettings {
            speed: MIN_SPEED,
            pitch_semitones: -MAX_PITCH_SEMITONES,
            ..Default::default()
        };

        assert_eq!(fast.rate(), MAX_SPEED);
        assert_eq!(slow.rate(), MIN_SPEED);
    }

    #[test]
    fn sanitizing_repairs_values_that_would_stall_or_explode() {
        let broken = PlaybackSettings {
            reverse: false,
            speed: 0.0,
            pitch_semitones: f32::NAN,
            gain: f32::INFINITY,
            attack_ms: -5.0,
            release_ms: f32::NAN,
        }
        .sanitized();

        assert!(broken.speed >= MIN_SPEED);
        assert_eq!(broken.pitch_semitones, 0.0);
        assert!(broken.gain.is_finite());
        assert_eq!(broken.attack_ms, 0.0);
        assert_eq!(broken.release_ms, 30.0);
        assert!(broken.rate() > 0.0);
    }

    #[test]
    fn sanitizing_leaves_usable_values_alone() {
        let settings = PlaybackSettings {
            reverse: true,
            speed: 0.5,
            pitch_semitones: 7.0,
            gain: 0.8,
            attack_ms: 10.0,
            release_ms: 200.0,
        };

        assert_eq!(settings.sanitized(), settings);
    }

    #[test]
    fn note_names_follow_the_c3_is_60_convention() {
        assert_eq!(note_name(60), "C3");
        assert_eq!(note_name(61), "C#3");
        assert_eq!(note_name(72), "C4");
        assert_eq!(note_name(48), "C2");
        assert_eq!(note_name(69), "A3");
        assert_eq!(note_name(0), "C-2");
        assert_eq!(note_name(127), "G8");
    }
}
