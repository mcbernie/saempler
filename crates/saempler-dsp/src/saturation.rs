/// Most a saturator may be driven by, as linear gain.
///
/// About 30 dB: past that every curve here is a square wave and more drive
/// only changes the level.
pub const MAX_DRIVE: f32 = 32.0;

/// The shape a saturator bends its input by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SaturationKind {
    /// A gentle cubic knee. Adds mostly third harmonic and keeps transients.
    #[default]
    Soft,
    /// Clipped flat. Harsh, and what a chop wants when it should sound broken.
    Hard,
    /// Asymmetric, so even harmonics appear as well as odd ones. The warmer
    /// of the three, and the one that changes a vocal chop most audibly.
    Tube,
}

/// A waveshaper with drive and output compensation.
///
/// Stateless: the whole point of a waveshaper is that the output depends on
/// this sample alone, which also means it can be dropped into a voice without
/// anything to reset between notes.
#[derive(Debug, Clone, Copy)]
pub struct Saturator {
    kind: SaturationKind,
    drive: f32,
    /// Applied after the curve, so turning the drive up does not simply make
    /// everything louder and sound better for the wrong reason.
    compensation: f32,
}

impl Default for Saturator {
    fn default() -> Self {
        let mut saturator = Self {
            kind: SaturationKind::Soft,
            drive: 1.0,
            compensation: 1.0,
        };
        saturator.set(SaturationKind::Soft, 1.0);
        saturator
    }
}

impl Saturator {
    pub fn set(&mut self, kind: SaturationKind, drive: f32) {
        self.kind = kind;
        self.drive = if drive.is_finite() {
            drive.clamp(1.0, MAX_DRIVE)
        } else {
            1.0
        };
        // What the curve does to a full scale input, undone. A unit input
        // then stays about a unit however hard it is driven, and the drive
        // control changes the sound rather than the level.
        let peak = shape(self.kind, self.drive);
        self.compensation = if peak > 1e-6 { 1.0 / peak } else { 1.0 };
    }

    /// Shape one sample.
    pub fn process(&self, input: f32) -> f32 {
        shape(self.kind, input * self.drive) * self.compensation
    }

    pub fn drive(&self) -> f32 {
        self.drive
    }
}

/// The curves themselves, each passing through the origin and bounded by one.
///
/// Bounded on purpose: a shaper that can exceed its input turns a loud chop
/// into a louder one, and the compensation above would then be undoing the
/// wrong thing.
fn shape(kind: SaturationKind, value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }

    match kind {
        SaturationKind::Soft => {
            // A cubic knee up to the point where it would fold back, flat
            // after it. Cheaper than tanh and close enough to it by ear.
            let clamped = value.clamp(-1.5, 1.5);
            clamped - clamped * clamped * clamped / 6.75
        }
        SaturationKind::Hard => value.clamp(-1.0, 1.0),
        SaturationKind::Tube => {
            // Asymmetric: the positive half reaches its ceiling sooner than
            // the negative one, which is where the even harmonics come from.
            // Both halves still stop at one, so the asymmetry is in the knee
            // and not in how loud each side is allowed to get.
            let knee = if value >= 0.0 { 0.25 } else { 0.6 };
            let ceiling = 1.0 / (1.0 - knee);
            let magnitude = value.abs().min(ceiling);
            let shaped = magnitude / (1.0 + knee * magnitude);
            shaped.copysign(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_stays_silent() {
        for kind in [
            SaturationKind::Soft,
            SaturationKind::Hard,
            SaturationKind::Tube,
        ] {
            let mut saturator = Saturator::default();
            saturator.set(kind, 8.0);

            assert_eq!(saturator.process(0.0), 0.0, "{kind:?}");
        }
    }

    #[test]
    fn at_the_lowest_drive_a_soft_curve_barely_bends() {
        let mut saturator = Saturator::default();
        saturator.set(SaturationKind::Soft, 1.0);

        for step in 0..=20 {
            let input = step as f32 / 20.0;
            let output = saturator.process(input);
            assert!((output - input).abs() < 0.2, "{input} became {output}");
        }
    }

    #[test]
    fn driving_harder_squashes_the_peaks_rather_than_raising_them() {
        let mut gentle = Saturator::default();
        gentle.set(SaturationKind::Soft, 1.0);
        let mut hard = Saturator::default();
        hard.set(SaturationKind::Soft, 16.0);

        // A quiet input comes out louder, a loud one no louder: that is what
        // compression by saturation is.
        assert!(hard.process(0.1) > gentle.process(0.1));
        assert!(hard.process(1.0) <= gentle.process(1.0) + 0.05);
    }

    #[test]
    fn nothing_leaves_the_usable_range() {
        for kind in [
            SaturationKind::Soft,
            SaturationKind::Hard,
            SaturationKind::Tube,
        ] {
            let mut saturator = Saturator::default();
            saturator.set(kind, MAX_DRIVE);

            for step in -40..=40 {
                let output = saturator.process(step as f32 / 10.0);
                assert!(output.is_finite(), "{kind:?} at {step}");
                assert!(output.abs() <= 1.5, "{kind:?} at {step}: {output}");
            }
        }
    }

    #[test]
    fn the_tube_curve_is_not_symmetric() {
        let mut saturator = Saturator::default();
        saturator.set(SaturationKind::Tube, 4.0);

        let up = saturator.process(0.5);
        let down = saturator.process(-0.5);

        assert!(
            (up + down).abs() > 0.02,
            "an asymmetric curve is where the even harmonics come from: {up} vs {down}"
        );
    }

    #[test]
    fn the_other_curves_are_symmetric() {
        for kind in [SaturationKind::Soft, SaturationKind::Hard] {
            let mut saturator = Saturator::default();
            saturator.set(kind, 4.0);

            let up = saturator.process(0.5);
            let down = saturator.process(-0.5);

            assert!((up + down).abs() < 1e-5, "{kind:?}: {up} vs {down}");
        }
    }

    #[test]
    fn a_broken_drive_is_repaired_rather_than_obeyed() {
        let mut saturator = Saturator::default();
        saturator.set(SaturationKind::Soft, f32::NAN);

        assert_eq!(saturator.drive(), 1.0);
        assert!(saturator.process(f32::INFINITY).is_finite());
    }
}
