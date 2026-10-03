use saempler_model::{Modifier, ModifierMode, MODIFIER_COUNT};

use crate::command::CellSpec;

/// Default tempo used until the host reports one.
pub const DEFAULT_TEMPO: f64 = 120.0;

/// Which modifiers are in effect, and which are waiting for the next note.
///
/// Held entirely in fixed arrays so that reading and updating it from the
/// audio thread costs an index and nothing else.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModifierState {
    /// In effect right now, from a held key or a toggle.
    active: [bool; MODIFIER_COUNT],
    /// Armed by a one shot, to be used and cleared by the next trigger.
    armed: [bool; MODIFIER_COUNT],
}

impl ModifierState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a modifier will affect the next performance note.
    pub fn is_engaged(&self, modifier: Modifier) -> bool {
        let index = modifier.index();
        self.active[index] || self.armed[index]
    }

    /// Whether a modifier is in effect right now.
    ///
    /// A one shot is armed but not in effect: it belongs to a note that has
    /// not been played yet, and must not reach into notes already sounding.
    pub fn is_live(&self, modifier: Modifier) -> bool {
        self.active[modifier.index()]
    }

    /// Bit per modifier, for publishing the state to the interface.
    pub fn bits(&self) -> u32 {
        let mut bits = 0;
        for modifier in Modifier::ALL {
            if self.is_engaged(modifier) {
                bits |= 1 << modifier.index();
            }
        }
        bits
    }

    /// Handle the key for a modifier going down.
    pub fn press(&mut self, modifier: Modifier, mode: ModifierMode) {
        let index = modifier.index();
        match mode {
            ModifierMode::Hold => self.active[index] = true,
            ModifierMode::Toggle => self.active[index] = !self.active[index],
            // Pressing again before the next note re-arms rather than
            // cancelling, so a nervous finger cannot disarm the gesture.
            ModifierMode::OneShot => self.armed[index] = true,
        }
    }

    /// Handle the key for a modifier coming up.
    pub fn release(&mut self, modifier: Modifier, mode: ModifierMode) {
        if mode == ModifierMode::Hold {
            self.active[modifier.index()] = false;
        }
    }

    /// Clear everything, used when the engine is reset.
    pub fn clear(&mut self) {
        self.active = [false; MODIFIER_COUNT];
        self.armed = [false; MODIFIER_COUNT];
    }

    /// Apply the engaged modifiers to a cell and consume the armed ones.
    ///
    /// Consuming here rather than on the key press is what makes one shot mean
    /// "the next note": the gesture survives until a note actually uses it.
    pub fn apply(&mut self, spec: CellSpec, tempo: f64, sample_rate: f32) -> CellSpec {
        let spec = self.applied(spec, tempo, sample_rate);
        self.armed = [false; MODIFIER_COUNT];
        spec
    }

    /// The result of applying the engaged modifiers, without consuming them.
    ///
    /// Used when a note is triggered, so a one shot counts.
    pub fn applied(&self, spec: CellSpec, tempo: f64, sample_rate: f32) -> CellSpec {
        self.apply_with(spec, tempo, sample_rate, |state, modifier| {
            state.is_engaged(modifier)
        })
    }

    /// The result of applying only the modifiers in effect right now.
    ///
    /// Used for notes that are already sounding, so an armed one shot stays
    /// waiting for the note it was meant for.
    pub fn applied_live(&self, spec: CellSpec, tempo: f64, sample_rate: f32) -> CellSpec {
        self.apply_with(spec, tempo, sample_rate, |state, modifier| {
            state.is_live(modifier)
        })
    }

    fn apply_with(
        &self,
        mut spec: CellSpec,
        tempo: f64,
        sample_rate: f32,
        engaged: impl Fn(&Self, Modifier) -> bool,
    ) -> CellSpec {
        if engaged(self, Modifier::Reverse) {
            spec.reverse = !spec.reverse;
        }
        if engaged(self, Modifier::HalfTime) {
            spec.rate *= 0.5;
        }
        // Stutter and repeat are the same mechanism at different lengths: a
        // loop taken from the trigger point. Only the shorter one survives if
        // both are engaged, because that is the one you can still hear.
        if engaged(self, Modifier::Stutter) {
            spec.loop_frames = division_frames(tempo, sample_rate, 16);
        } else if engaged(self, Modifier::Repeat) {
            spec.loop_frames = division_frames(tempo, sample_rate, 8);
        }
        if engaged(self, Modifier::Brake) {
            spec.tape_stop_frames = division_frames(tempo, sample_rate, 1);
        }

        spec
    }
}

/// Frames in one note of `division` per whole note, at `tempo`.
///
/// A division of 4 is a quarter note, 16 a sixteenth. Clamped so that a
/// missing or absurd tempo cannot produce a loop of zero frames, which would
/// leave a voice reading the same sample forever.
pub fn division_frames(tempo: f64, sample_rate: f32, division: u32) -> u64 {
    let tempo = if tempo.is_finite() && tempo > 1.0 {
        tempo
    } else {
        DEFAULT_TEMPO
    };
    let division = division.max(1) as f64;

    let beats_per_whole = 4.0;
    let seconds = (60.0 / tempo) * (beats_per_whole / division);
    ((seconds * sample_rate.max(1.0) as f64) as u64).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::SliceBounds;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn spec() -> CellSpec {
        CellSpec {
            bounds: SliceBounds {
                start_frame: 0,
                end_frame: 48_000,
            },
            ..CellSpec::default()
        }
    }

    #[test]
    fn nothing_engaged_leaves_the_cell_alone() {
        let mut state = ModifierState::new();

        assert_eq!(state.apply(spec(), 120.0, SAMPLE_RATE), spec());
        assert_eq!(state.bits(), 0);
    }

    #[test]
    fn hold_lasts_while_the_key_is_down() {
        let mut state = ModifierState::new();

        state.press(Modifier::Reverse, ModifierMode::Hold);
        assert!(state.apply(spec(), 120.0, SAMPLE_RATE).reverse);
        assert!(
            state.apply(spec(), 120.0, SAMPLE_RATE).reverse,
            "a held modifier survives the note that used it"
        );

        state.release(Modifier::Reverse, ModifierMode::Hold);
        assert!(!state.apply(spec(), 120.0, SAMPLE_RATE).reverse);
    }

    #[test]
    fn toggle_stays_until_pressed_again() {
        let mut state = ModifierState::new();

        state.press(Modifier::Reverse, ModifierMode::Toggle);
        state.release(Modifier::Reverse, ModifierMode::Toggle);
        assert!(
            state.apply(spec(), 120.0, SAMPLE_RATE).reverse,
            "releasing the key must not switch a toggle off"
        );

        state.press(Modifier::Reverse, ModifierMode::Toggle);
        assert!(!state.apply(spec(), 120.0, SAMPLE_RATE).reverse);
    }

    #[test]
    fn one_shot_applies_to_the_next_note_only() {
        let mut state = ModifierState::new();

        state.press(Modifier::Reverse, ModifierMode::OneShot);
        state.release(Modifier::Reverse, ModifierMode::OneShot);

        assert!(state.apply(spec(), 120.0, SAMPLE_RATE).reverse);
        assert!(
            !state.apply(spec(), 120.0, SAMPLE_RATE).reverse,
            "a one shot must clear itself after being used"
        );
    }

    #[test]
    fn one_shot_waits_however_long_it_takes() {
        let mut state = ModifierState::new();
        state.press(Modifier::Reverse, ModifierMode::OneShot);

        // Time passing, other modifiers moving: the gesture still stands.
        state.press(Modifier::HalfTime, ModifierMode::Hold);
        state.release(Modifier::HalfTime, ModifierMode::Hold);

        assert!(state.apply(spec(), 120.0, SAMPLE_RATE).reverse);
    }

    #[test]
    fn pressing_a_one_shot_twice_keeps_it_armed() {
        let mut state = ModifierState::new();

        state.press(Modifier::Reverse, ModifierMode::OneShot);
        state.press(Modifier::Reverse, ModifierMode::OneShot);

        assert!(state.apply(spec(), 120.0, SAMPLE_RATE).reverse);
    }

    #[test]
    fn reverse_flips_a_cell_that_is_already_reversed() {
        let mut state = ModifierState::new();
        state.press(Modifier::Reverse, ModifierMode::Hold);

        let reversed_cell = CellSpec {
            reverse: true,
            ..spec()
        };

        assert!(!state.apply(reversed_cell, 120.0, SAMPLE_RATE).reverse);
    }

    #[test]
    fn half_time_halves_the_rate() {
        let mut state = ModifierState::new();
        state.press(Modifier::HalfTime, ModifierMode::Hold);

        assert_eq!(state.apply(spec(), 120.0, SAMPLE_RATE).rate, 0.5);
    }

    #[test]
    fn modifiers_combine() {
        let mut state = ModifierState::new();
        state.press(Modifier::Reverse, ModifierMode::Hold);
        state.press(Modifier::HalfTime, ModifierMode::Hold);
        state.press(Modifier::Stutter, ModifierMode::Hold);

        let result = state.apply(spec(), 120.0, SAMPLE_RATE);

        assert!(result.reverse);
        assert_eq!(result.rate, 0.5);
        assert!(result.loop_frames > 0);
    }

    #[test]
    fn stutter_is_shorter_than_repeat() {
        let mut stutter = ModifierState::new();
        stutter.press(Modifier::Stutter, ModifierMode::Hold);
        let mut repeat = ModifierState::new();
        repeat.press(Modifier::Repeat, ModifierMode::Hold);

        let short = stutter.apply(spec(), 120.0, SAMPLE_RATE).loop_frames;
        let long = repeat.apply(spec(), 120.0, SAMPLE_RATE).loop_frames;

        assert!(short < long, "{short} should be shorter than {long}");
    }

    #[test]
    fn both_loop_modifiers_at_once_take_the_shorter_one() {
        let mut state = ModifierState::new();
        state.press(Modifier::Stutter, ModifierMode::Hold);
        state.press(Modifier::Repeat, ModifierMode::Hold);

        let both = state.applied(spec(), 120.0, SAMPLE_RATE).loop_frames;
        let mut only_stutter = ModifierState::new();
        only_stutter.press(Modifier::Stutter, ModifierMode::Hold);

        assert_eq!(
            both,
            only_stutter.apply(spec(), 120.0, SAMPLE_RATE).loop_frames
        );
    }

    #[test]
    fn brake_sets_a_stop_over_one_whole_note() {
        let mut state = ModifierState::new();
        state.press(Modifier::Brake, ModifierMode::Hold);

        let frames = state.apply(spec(), 120.0, SAMPLE_RATE).tape_stop_frames;

        // One whole note at 120 bpm is two seconds.
        assert!((frames as i64 - 96_000).abs() < 100, "{frames}");
    }

    #[test]
    fn a_sixteenth_at_120_bpm_is_an_eighth_of_a_second() {
        let frames = division_frames(120.0, SAMPLE_RATE, 16);

        assert!((frames as i64 - 6_000).abs() < 10, "{frames}");
    }

    #[test]
    fn a_missing_tempo_falls_back_rather_than_producing_nothing() {
        for tempo in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let frames = division_frames(tempo, SAMPLE_RATE, 16);
            assert!(frames > 0, "tempo {tempo} produced {frames}");
        }
    }

    #[test]
    fn an_armed_one_shot_does_not_reach_notes_already_sounding() {
        let mut state = ModifierState::new();
        state.press(Modifier::Reverse, ModifierMode::OneShot);

        assert!(
            !state.applied_live(spec(), 120.0, SAMPLE_RATE).reverse,
            "an armed one shot belongs to the next note, not this one"
        );
        assert!(
            state.applied(spec(), 120.0, SAMPLE_RATE).reverse,
            "but it must still apply when that note arrives"
        );
    }

    #[test]
    fn a_held_modifier_reaches_notes_already_sounding() {
        let mut state = ModifierState::new();
        state.press(Modifier::Stutter, ModifierMode::Hold);

        assert!(state.applied_live(spec(), 120.0, SAMPLE_RATE).loop_frames > 0);
    }

    #[test]
    fn the_published_bits_name_the_engaged_modifiers() {
        let mut state = ModifierState::new();
        state.press(Modifier::Stutter, ModifierMode::Hold);

        let bits = state.bits();

        assert_eq!(bits, 1 << Modifier::Stutter.index());
        assert!(!state.is_engaged(Modifier::Reverse));
    }

    #[test]
    fn clearing_releases_everything() {
        let mut state = ModifierState::new();
        state.press(Modifier::Reverse, ModifierMode::Toggle);
        state.press(Modifier::Stutter, ModifierMode::OneShot);

        state.clear();

        assert_eq!(state.bits(), 0);
    }
}
