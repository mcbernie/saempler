use serde::{Deserialize, Serialize};

use crate::modulation::{
    default_routes, EnvelopeDefinition, LfoDefinition, ModulationRoute, ENVELOPE_COUNT, LFO_COUNT,
    MAX_ROUTES,
};
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
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            reverse: false,
            speed: 1.0,
            pitch_semitones: 0.0,
            gain: 1.0,
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
        self
    }
}

/// A slice assigned to a MIDI note, with its own way of playing it.
///
/// Several cells may reference the same slice. That is the point of the
/// design: one chop, many performances of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default = "PerformanceCell::placeholder")]
pub struct PerformanceCell {
    pub id: CellId,
    pub midi_note: u8,
    pub slice: SliceId,
    pub playback: PlaybackSettings,
    /// Two envelopes, neither tied to a particular parameter.
    pub envelopes: [EnvelopeDefinition; ENVELOPE_COUNT],
    pub lfos: [LfoDefinition; LFO_COUNT],
    /// What modulates what. Capped at [`MAX_ROUTES`] so the engine can hold a
    /// cell without allocating.
    pub routes: Vec<ModulationRoute>,
}

impl PerformanceCell {
    /// A cell with the default modulation, waiting to be given a slice.
    ///
    /// Only used to fill in fields that serde finds missing; a real cell is
    /// always created through the project.
    pub fn placeholder() -> Self {
        Self {
            id: CellId(0),
            midi_note: 60,
            slice: SliceId(0),
            playback: PlaybackSettings::default(),
            envelopes: [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            lfos: [LfoDefinition::default(); LFO_COUNT],
            routes: default_routes(),
        }
    }

    /// Add a route, if there is room.
    pub fn add_route(&mut self, route: ModulationRoute) -> bool {
        if self.routes.len() >= MAX_ROUTES {
            return false;
        }
        self.routes.push(route.sanitized());
        true
    }

    /// Remove the route at `index`.
    pub fn remove_route(&mut self, index: usize) -> bool {
        if index >= self.routes.len() {
            return false;
        }
        self.routes.remove(index);
        true
    }

    /// Replace the route at `index`.
    pub fn set_route(&mut self, index: usize, route: ModulationRoute) -> bool {
        match self.routes.get_mut(index) {
            Some(slot) => {
                *slot = route.sanitized();
                true
            }
            None => false,
        }
    }

    /// Whether anything reaches the volume.
    ///
    /// Nothing does when the amplitude route has been taken out, and the cell
    /// is then silent. Worth saying out loud in the interface.
    pub fn has_amplitude(&self) -> bool {
        self.routes
            .iter()
            .any(|route| route.destination == crate::ModDestination::Volume && route.amount != 0.0)
    }

    /// Pull every definition back into a usable range.
    pub fn sanitized(mut self) -> Self {
        self.playback = self.playback.sanitized();
        for envelope in &mut self.envelopes {
            *envelope = envelope.sanitized();
        }
        for lfo in &mut self.lfos {
            *lfo = lfo.sanitized();
        }
        self.routes.truncate(MAX_ROUTES);
        for route in &mut self.routes {
            *route = route.sanitized();
        }
        self
    }
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
        }
        .sanitized();

        assert!(broken.speed >= MIN_SPEED);
        assert_eq!(broken.pitch_semitones, 0.0);
        assert!(broken.gain.is_finite());
        assert!(broken.rate() > 0.0);
    }

    #[test]
    fn sanitizing_leaves_usable_values_alone() {
        let settings = PlaybackSettings {
            reverse: true,
            speed: 0.5,
            pitch_semitones: 7.0,
            gain: 0.8,
        };

        assert_eq!(settings.sanitized(), settings);
    }

    #[test]
    fn a_cell_is_repaired_whole() {
        let mut cell = PerformanceCell::placeholder();
        cell.playback.speed = 0.0;
        cell.envelopes[0].sustain = 9.0;
        cell.lfos[0].rate_hz = f32::NAN;
        cell.routes.push(ModulationRoute {
            amount: 100.0,
            ..Default::default()
        });

        let cell = cell.sanitized();

        assert!(cell.playback.rate() > 0.0);
        assert_eq!(cell.envelopes[0].sustain, 1.0);
        assert!(cell.lfos[0].rate_hz.is_finite());
        assert!(cell.routes.iter().all(|route| route.amount.abs() <= 1.0));
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
