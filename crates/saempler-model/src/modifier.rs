use serde::{Deserialize, Serialize};

/// A change a modifier note makes to the next performance trigger.
///
/// Modifiers do not make sound themselves. They change how a performance note
/// behaves while they are in effect, which is what makes the keyboard a
/// performance surface rather than a set of fixed playback buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    /// Play the slice backwards.
    Reverse,
    /// Loop a sixteenth note from the trigger point.
    Stutter,
    /// Loop an eighth note from the trigger point.
    Repeat,
    /// Read at half speed, so the slice lasts twice as long an octave down.
    HalfTime,
    /// Slow playback to a stop over one beat.
    Brake,
    /// Throw everything into the delay, driven hard.
    Delay,
    /// Throw everything into the reverb, driven hard.
    Reverb,
    /// Throw everything into the phaser, driven hard.
    Phaser,
    /// Throw everything into the flanger, driven hard.
    Flanger,
}

impl Modifier {
    /// Every modifier, in the order they are laid out on the keyboard.
    pub const ALL: [Modifier; 9] = [
        Modifier::Reverse,
        Modifier::Stutter,
        Modifier::Repeat,
        Modifier::HalfTime,
        Modifier::Brake,
        Modifier::Delay,
        Modifier::Reverb,
        Modifier::Phaser,
        Modifier::Flanger,
    ];

    /// The ones a new project puts on the keyboard.
    ///
    /// The playback modifiers only. The effect ones are there to be reached
    /// for deliberately, and five keys taken by default is already as much of
    /// the bottom octave as anyone wants gone.
    pub const DEFAULT_LAYOUT: [Modifier; 5] = [
        Modifier::Reverse,
        Modifier::Stutter,
        Modifier::Repeat,
        Modifier::HalfTime,
        Modifier::Brake,
    ];

    /// Short label for display.
    pub fn label(self) -> &'static str {
        match self {
            Modifier::Reverse => "Reverse",
            Modifier::Stutter => "Stutter",
            Modifier::Repeat => "Repeat",
            Modifier::HalfTime => "Half-Time",
            Modifier::Brake => "Brake",
            Modifier::Delay => "Delay",
            Modifier::Reverb => "Reverb",
            Modifier::Phaser => "Phaser",
            Modifier::Flanger => "Flanger",
        }
    }

    /// Which send this modifier throws the sound into, if it is one of those.
    ///
    /// The index is the engine's send order, so the two cannot drift apart
    /// without this failing to compile.
    pub fn send(self) -> Option<usize> {
        match self {
            Modifier::Delay => Some(0),
            Modifier::Reverb => Some(1),
            Modifier::Phaser => Some(2),
            Modifier::Flanger => Some(3),
            _ => None,
        }
    }

    /// Position in a fixed table, so the engine can hold modifier state in an
    /// array rather than a map.
    pub fn index(self) -> usize {
        match self {
            Modifier::Reverse => 0,
            Modifier::Stutter => 1,
            Modifier::Repeat => 2,
            Modifier::HalfTime => 3,
            Modifier::Brake => 4,
            Modifier::Delay => 5,
            Modifier::Reverb => 6,
            Modifier::Phaser => 7,
            Modifier::Flanger => 8,
        }
    }
}

/// Number of distinct modifiers.
pub const MODIFIER_COUNT: usize = Modifier::ALL.len();

/// How holding or pressing a modifier note behaves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModifierMode {
    /// In effect while the key is down.
    #[default]
    Hold,
    /// A press switches it on, the next press switches it off.
    Toggle,
    /// A press arms it; the next performance note uses it and clears it.
    OneShot,
}

impl ModifierMode {
    pub const ALL: [ModifierMode; 3] = [
        ModifierMode::Hold,
        ModifierMode::Toggle,
        ModifierMode::OneShot,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModifierMode::Hold => "Hold",
            ModifierMode::Toggle => "Toggle",
            ModifierMode::OneShot => "One Shot",
        }
    }

    /// The mode after this one, for a control that cycles through them.
    pub fn next(self) -> ModifierMode {
        match self {
            ModifierMode::Hold => ModifierMode::Toggle,
            ModifierMode::Toggle => ModifierMode::OneShot,
            ModifierMode::OneShot => ModifierMode::Hold,
        }
    }
}

/// A modifier sitting on a MIDI note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModifierAssignment {
    pub note: u8,
    pub modifier: Modifier,
    pub mode: ModifierMode,
}

/// Lowest note of the default modifier layout, C2 with C3 at 60.
///
/// An octave below where chops are usually mapped, and high enough to be
/// reachable on a short keyboard without shifting octaves.
pub const MODIFIER_BASE_NOTE: u8 = 48;

/// The modifier layout a new project starts with.
///
/// White keys of the C1 octave, below anything the performance cells use, so
/// that both hands can play at once on one keyboard.
pub fn default_layout() -> Vec<ModifierAssignment> {
    // C1, D1, E1, F1, G1.
    // The white keys of the octave, so the layout sits under the hand.
    const OFFSETS: [u8; Modifier::DEFAULT_LAYOUT.len()] = [0, 2, 4, 5, 7];

    Modifier::DEFAULT_LAYOUT
        .iter()
        .zip(OFFSETS)
        .map(|(modifier, offset)| ModifierAssignment {
            note: MODIFIER_BASE_NOTE + offset,
            modifier: *modifier,
            mode: ModifierMode::Hold,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_modifier_has_its_own_index() {
        let indices: HashSet<usize> = Modifier::ALL.iter().map(|m| m.index()).collect();

        assert_eq!(indices.len(), MODIFIER_COUNT);
        assert!(indices.iter().all(|index| *index < MODIFIER_COUNT));
    }

    #[test]
    fn cycling_the_mode_returns_to_the_start() {
        let mut mode = ModifierMode::Hold;

        for _ in 0..ModifierMode::ALL.len() {
            mode = mode.next();
        }

        assert_eq!(mode, ModifierMode::Hold);
    }

    #[test]
    fn the_default_layout_covers_every_playback_modifier_once() {
        let layout = default_layout();

        assert_eq!(layout.len(), Modifier::DEFAULT_LAYOUT.len());
        let modifiers: HashSet<Modifier> = layout.iter().map(|a| a.modifier).collect();
        assert_eq!(modifiers.len(), Modifier::DEFAULT_LAYOUT.len());
    }

    #[test]
    fn the_effect_modifiers_are_not_mapped_by_default() {
        // They take a key each and are reached for deliberately; five keys of
        // the bottom octave gone by default is already as much as anyone
        // wants.
        let layout = default_layout();

        for modifier in Modifier::ALL {
            let mapped = layout.iter().any(|entry| entry.modifier == modifier);
            assert_eq!(mapped, modifier.send().is_none(), "{modifier:?}");
        }
    }

    #[test]
    fn every_modifier_has_its_own_slot() {
        let mut seen = [false; MODIFIER_COUNT];
        for modifier in Modifier::ALL {
            let index = modifier.index();
            assert!(!seen[index], "{modifier:?} shares a slot");
            seen[index] = true;
        }
    }

    #[test]
    fn only_the_effect_modifiers_name_a_send() {
        let sends: HashSet<usize> = Modifier::ALL.iter().filter_map(|m| m.send()).collect();

        assert_eq!(sends.len(), 4, "the four sends should each have one key");
        assert!(Modifier::Reverse.send().is_none());
    }

    #[test]
    fn the_default_layout_uses_distinct_notes_below_the_cells() {
        let layout = default_layout();

        let notes: HashSet<u8> = layout.iter().map(|a| a.note).collect();
        assert_eq!(
            notes.len(),
            Modifier::DEFAULT_LAYOUT.len(),
            "two modifiers share a note"
        );
        // Performance cells are laid out from C3 upwards.
        assert!(notes.iter().all(|note| *note < 60));
    }

    #[test]
    fn the_default_layout_starts_on_c1() {
        let layout = default_layout();

        assert_eq!(layout[0].note, MODIFIER_BASE_NOTE);
        assert_eq!(layout[0].modifier, Modifier::Reverse);
        assert_eq!(layout[0].mode, ModifierMode::Hold);
    }
}
