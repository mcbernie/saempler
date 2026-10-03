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
}

impl Modifier {
    /// Every modifier, in the order they are laid out on the keyboard.
    pub const ALL: [Modifier; 5] = [
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

/// Lowest note of the default modifier layout, C1 with C3 at 60.
pub const MODIFIER_BASE_NOTE: u8 = 36;

/// The modifier layout a new project starts with.
///
/// White keys of the C1 octave, below anything the performance cells use, so
/// that both hands can play at once on one keyboard.
pub fn default_layout() -> Vec<ModifierAssignment> {
    // C1, D1, E1, F1, G1.
    const OFFSETS: [u8; MODIFIER_COUNT] = [0, 2, 4, 5, 7];

    Modifier::ALL
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
    fn the_default_layout_covers_every_modifier_once() {
        let layout = default_layout();

        assert_eq!(layout.len(), MODIFIER_COUNT);
        let modifiers: HashSet<Modifier> = layout.iter().map(|a| a.modifier).collect();
        assert_eq!(modifiers.len(), MODIFIER_COUNT);
    }

    #[test]
    fn the_default_layout_uses_distinct_notes_below_the_cells() {
        let layout = default_layout();

        let notes: HashSet<u8> = layout.iter().map(|a| a.note).collect();
        assert_eq!(notes.len(), MODIFIER_COUNT, "two modifiers share a note");
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
