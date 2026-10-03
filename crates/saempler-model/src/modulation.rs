use serde::{Deserialize, Serialize};

/// Number of envelopes a cell has.
pub const ENVELOPE_COUNT: usize = 2;
/// Number of LFOs a cell has.
pub const LFO_COUNT: usize = 2;
/// Most routes a cell may carry.
///
/// Fixed so that the engine can hold a cell in a `Copy` value with no
/// allocation. Eight is more than a readable matrix holds; raise it when a
/// patch actually runs out.
pub const MAX_ROUTES: usize = 8;

/// A classic four stage envelope.
///
/// Not tied to volume: it is a modulation source like any other, and only
/// becomes an amplitude envelope by being routed to [`ModDestination::Volume`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvelopeDefinition {
    pub attack_ms: f32,
    pub decay_ms: f32,
    /// Level held while the key is down, from 0 to 1.
    pub sustain: f32,
    pub release_ms: f32,
}

impl Default for EnvelopeDefinition {
    fn default() -> Self {
        Self {
            attack_ms: 3.0,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms: 30.0,
        }
    }
}

impl EnvelopeDefinition {
    /// Pull every stage into a usable range.
    pub fn sanitized(mut self) -> Self {
        self.attack_ms = finite_or(self.attack_ms, 3.0).clamp(0.0, 10_000.0);
        self.decay_ms = finite_or(self.decay_ms, 0.0).clamp(0.0, 10_000.0);
        self.sustain = finite_or(self.sustain, 1.0).clamp(0.0, 1.0);
        self.release_ms = finite_or(self.release_ms, 30.0).clamp(0.0, 10_000.0);
        self
    }
}

/// Wave an LFO runs through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Saw,
    ReverseSaw,
    Square,
    /// A new random value each cycle, held until the next one.
    SampleHold,
}

impl LfoShape {
    pub const ALL: [LfoShape; 6] = [
        LfoShape::Sine,
        LfoShape::Triangle,
        LfoShape::Saw,
        LfoShape::ReverseSaw,
        LfoShape::Square,
        LfoShape::SampleHold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LfoShape::Sine => "Sine",
            LfoShape::Triangle => "Tri",
            LfoShape::Saw => "Saw",
            LfoShape::ReverseSaw => "Rev Saw",
            LfoShape::Square => "Square",
            LfoShape::SampleHold => "S&H",
        }
    }
}

/// Note values an LFO or a timed length can be locked to.
///
/// The number is the denominator over a whole note, so 4 is a quarter and 16 a
/// sixteenth. Bars are expressed the same way, as fractions below one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Division {
    FourBars,
    TwoBars,
    OneBar,
    Half,
    #[default]
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    SixtyFourth,
}

impl Division {
    pub const ALL: [Division; 9] = [
        Division::FourBars,
        Division::TwoBars,
        Division::OneBar,
        Division::Half,
        Division::Quarter,
        Division::Eighth,
        Division::Sixteenth,
        Division::ThirtySecond,
        Division::SixtyFourth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Division::FourBars => "4 Bars",
            Division::TwoBars => "2 Bars",
            Division::OneBar => "1 Bar",
            Division::Half => "1/2",
            Division::Quarter => "1/4",
            Division::Eighth => "1/8",
            Division::Sixteenth => "1/16",
            Division::ThirtySecond => "1/32",
            Division::SixtyFourth => "1/64",
        }
    }

    /// Length in whole notes. A bar is one whole note in common time.
    pub fn whole_notes(self) -> f32 {
        match self {
            Division::FourBars => 4.0,
            Division::TwoBars => 2.0,
            Division::OneBar => 1.0,
            Division::Half => 0.5,
            Division::Quarter => 0.25,
            Division::Eighth => 0.125,
            Division::Sixteenth => 0.0625,
            Division::ThirtySecond => 0.031_25,
            Division::SixtyFourth => 0.015_625,
        }
    }
}

/// A low frequency oscillator, free running or locked to the tempo.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LfoDefinition {
    pub shape: LfoShape,
    /// Rate in hertz, used when `sync` is off.
    pub rate_hz: f32,
    pub sync: bool,
    /// Note value one cycle takes, used when `sync` is on.
    pub division: Division,
    /// Whether the phase restarts with every note.
    pub retrigger: bool,
}

impl Default for LfoDefinition {
    fn default() -> Self {
        Self {
            shape: LfoShape::Sine,
            rate_hz: 2.0,
            sync: false,
            division: Division::Quarter,
            retrigger: true,
        }
    }
}

impl LfoDefinition {
    pub fn sanitized(mut self) -> Self {
        self.rate_hz = finite_or(self.rate_hz, 2.0).clamp(0.01, 50.0);
        self
    }
}

/// Where a modulation comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModSource {
    #[default]
    EnvelopeA,
    EnvelopeB,
    Lfo1,
    Lfo2,
    Velocity,
}

impl ModSource {
    pub const ALL: [ModSource; 5] = [
        ModSource::EnvelopeA,
        ModSource::EnvelopeB,
        ModSource::Lfo1,
        ModSource::Lfo2,
        ModSource::Velocity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModSource::EnvelopeA => "ENV A",
            ModSource::EnvelopeB => "ENV B",
            ModSource::Lfo1 => "LFO 1",
            ModSource::Lfo2 => "LFO 2",
            ModSource::Velocity => "Velocity",
        }
    }

    /// Whether the source swings either side of zero.
    ///
    /// Envelopes and velocity run from 0 to 1; LFOs run from -1 to 1. The
    /// difference decides how a route reads at the same amount.
    pub fn is_bipolar(self) -> bool {
        matches!(self, ModSource::Lfo1 | ModSource::Lfo2)
    }
}

/// What a modulation changes.
///
/// Only destinations the engine actually acts on are listed. The list grows
/// when a parameter exists to be modulated, not before.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModDestination {
    #[default]
    Volume,
    Pan,
    /// Transposition, in semitones at full amount.
    Pitch,
    /// Multiplier on the read rate.
    PlaybackRate,
    /// Length of the loop a stutter imposes.
    LoopLength,
}

impl ModDestination {
    pub const ALL: [ModDestination; 5] = [
        ModDestination::Volume,
        ModDestination::Pan,
        ModDestination::Pitch,
        ModDestination::PlaybackRate,
        ModDestination::LoopLength,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModDestination::Volume => "Volume",
            ModDestination::Pan => "Pan",
            ModDestination::Pitch => "Pitch",
            ModDestination::PlaybackRate => "Rate",
            ModDestination::LoopLength => "Loop Length",
        }
    }

    pub fn index(self) -> usize {
        match self {
            ModDestination::Volume => 0,
            ModDestination::Pan => 1,
            ModDestination::Pitch => 2,
            ModDestination::PlaybackRate => 3,
            ModDestination::LoopLength => 4,
        }
    }
}

/// Number of distinct destinations.
pub const DESTINATION_COUNT: usize = ModDestination::ALL.len();

/// One line of the modulation matrix.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModulationRoute {
    pub source: ModSource,
    pub destination: ModDestination,
    /// How much of the source reaches the destination, from -1 to 1.
    pub amount: f32,
}

impl Default for ModulationRoute {
    fn default() -> Self {
        Self {
            source: ModSource::EnvelopeA,
            destination: ModDestination::Volume,
            amount: 1.0,
        }
    }
}

impl ModulationRoute {
    pub fn sanitized(mut self) -> Self {
        self.amount = finite_or(self.amount, 0.0).clamp(-1.0, 1.0);
        self
    }
}

/// The routes a new cell starts with.
///
/// Envelope A to volume: without it a cell would have no amplitude envelope
/// and would click at both ends. It is a route rather than a fixed connection
/// so that it can be changed or taken out like any other.
pub fn default_routes() -> Vec<ModulationRoute> {
    vec![ModulationRoute {
        source: ModSource::EnvelopeA,
        destination: ModDestination::Volume,
        amount: 1.0,
    }]
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
    fn a_new_envelope_opens_and_holds() {
        let envelope = EnvelopeDefinition::default();

        assert_eq!(envelope.sustain, 1.0);
        assert!(envelope.attack_ms > 0.0);
    }

    #[test]
    fn envelope_values_are_repaired() {
        let broken = EnvelopeDefinition {
            attack_ms: f32::NAN,
            decay_ms: -5.0,
            sustain: 9.0,
            release_ms: f32::INFINITY,
        }
        .sanitized();

        assert_eq!(broken.attack_ms, 3.0);
        assert_eq!(broken.decay_ms, 0.0);
        assert_eq!(broken.sustain, 1.0);
        assert_eq!(broken.release_ms, 30.0);
    }

    #[test]
    fn divisions_halve_as_they_go_down() {
        assert_eq!(Division::OneBar.whole_notes(), 1.0);
        assert_eq!(Division::Half.whole_notes(), 0.5);
        assert_eq!(Division::Quarter.whole_notes(), 0.25);
        assert_eq!(Division::FourBars.whole_notes(), 4.0);

        for pair in Division::ALL.windows(2) {
            assert!(
                pair[0].whole_notes() > pair[1].whole_notes(),
                "{:?} should be longer than {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn only_the_lfos_swing_either_side_of_zero() {
        assert!(ModSource::Lfo1.is_bipolar());
        assert!(ModSource::Lfo2.is_bipolar());
        assert!(!ModSource::EnvelopeA.is_bipolar());
        assert!(!ModSource::Velocity.is_bipolar());
    }

    #[test]
    fn every_destination_has_its_own_index() {
        let mut seen = [false; DESTINATION_COUNT];
        for destination in ModDestination::ALL {
            let index = destination.index();
            assert!(!seen[index], "{destination:?} shares an index");
            seen[index] = true;
        }
    }

    #[test]
    fn a_new_cell_has_an_amplitude_envelope() {
        let routes = default_routes();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].source, ModSource::EnvelopeA);
        assert_eq!(routes[0].destination, ModDestination::Volume);
        assert_eq!(routes[0].amount, 1.0);
    }

    #[test]
    fn a_route_amount_stays_within_its_range() {
        let route = ModulationRoute {
            amount: 5.0,
            ..Default::default()
        }
        .sanitized();

        assert_eq!(route.amount, 1.0);
    }

    #[test]
    fn an_lfo_rate_stays_usable() {
        let fast = LfoDefinition {
            rate_hz: 1_000.0,
            ..Default::default()
        }
        .sanitized();
        let broken = LfoDefinition {
            rate_hz: f32::NAN,
            ..Default::default()
        }
        .sanitized();

        assert_eq!(fast.rate_hz, 50.0);
        assert_eq!(broken.rate_hz, 2.0);
    }
}
