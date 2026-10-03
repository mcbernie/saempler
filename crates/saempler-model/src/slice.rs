use serde::{Deserialize, Serialize};

/// Identifies a slice within a project.
///
/// A newtype rather than a bare integer so a slice index and a slice identity
/// cannot be confused once cells start referring to slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SliceId(pub u32);

/// A region of the source sample.
///
/// A slice says *which* audio is played and nothing about *how*. Playback
/// settings live in the performance cells that reference it, so the same slice
/// can be used many times without being copied.
///
/// The range is half-open: `start_frame` is included, `end_frame` is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slice {
    pub id: SliceId,
    pub start_frame: u64,
    pub end_frame: u64,
}

impl Slice {
    /// Number of frames this slice covers.
    pub fn len_frames(&self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }

    /// Whether the slice covers at least one frame.
    pub fn is_empty(&self) -> bool {
        self.len_frames() == 0
    }

    /// Whether `frame` lies inside the slice.
    pub fn contains(&self, frame: u64) -> bool {
        frame >= self.start_frame && frame < self.end_frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(start: u64, end: u64) -> Slice {
        Slice {
            id: SliceId(0),
            start_frame: start,
            end_frame: end,
        }
    }

    #[test]
    fn length_is_the_half_open_range() {
        assert_eq!(slice(100, 200).len_frames(), 100);
        assert_eq!(slice(0, 1).len_frames(), 1);
    }

    #[test]
    fn inverted_bounds_report_empty_instead_of_underflowing() {
        let inverted = slice(200, 100);

        assert_eq!(inverted.len_frames(), 0);
        assert!(inverted.is_empty());
    }

    #[test]
    fn contains_excludes_the_end_frame() {
        let slice = slice(10, 20);

        assert!(!slice.contains(9));
        assert!(slice.contains(10));
        assert!(slice.contains(19));
        assert!(!slice.contains(20));
    }
}
