/// Lowest and highest cutoff a filter may be set to, in hertz.
///
/// The top stops short of Nyquist at 44.1 kHz, below which the bilinear
/// transform's frequency warping runs away and the filter stops behaving like
/// the one that was asked for.
pub const MIN_CUTOFF: f32 = 20.0;
pub const MAX_CUTOFF: f32 = 18_000.0;

/// Lowest and highest resonance.
///
/// Below a half the filter is broader than one pole and there is no point;
/// above twenty it self-oscillates loudly enough to be a hazard.
pub const MIN_RESONANCE: f32 = 0.5;
pub const MAX_RESONANCE: f32 = 20.0;

/// What a filter lets through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FilterMode {
    #[default]
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

/// A two pole state variable filter in topology preserving transform form.
///
/// This form rather than a biquad because the cutoff is a modulation
/// destination: a biquad's coefficients have to be recomputed from scratch to
/// be changed, and changing them per frame makes it ring and blow up. Here the
/// state is the two integrators, and moving the cutoff under them is what the
/// structure is for.
///
/// Holds no allocation and never panics, so it is safe inside the callback.
#[derive(Debug, Clone, Copy)]
pub struct Svf {
    mode: FilterMode,
    /// Prewarped cutoff.
    g: f32,
    /// Damping, the reciprocal of resonance.
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    /// The two integrator states.
    ic1: f32,
    ic2: f32,
}

impl Default for Svf {
    fn default() -> Self {
        let mut filter = Self {
            mode: FilterMode::LowPass,
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        filter.set(FilterMode::LowPass, 1_000.0, 0.707, 48_000.0);
        filter
    }
}

impl Svf {
    /// Point the filter at a cutoff and resonance.
    ///
    /// Cheap enough to call per frame, which is what makes the cutoff usable
    /// as a modulation destination.
    pub fn set(&mut self, mode: FilterMode, cutoff: f32, resonance: f32, sample_rate: f32) {
        self.mode = mode;

        let sample_rate = if sample_rate.is_finite() && sample_rate > 1.0 {
            sample_rate
        } else {
            48_000.0
        };
        // Kept below a quarter of the sample rate as well as below the fixed
        // ceiling: at a low rate the ceiling alone would sit past Nyquist.
        let cutoff = finite_or(cutoff, 1_000.0)
            .clamp(MIN_CUTOFF, MAX_CUTOFF)
            .min(sample_rate * 0.45);
        let resonance = finite_or(resonance, 0.707).clamp(MIN_RESONANCE, MAX_RESONANCE);

        self.g = (std::f32::consts::PI * cutoff / sample_rate).tan();
        self.k = 1.0 / resonance;
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    /// Forget what has been through the filter.
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// Filter one sample.
    pub fn process(&mut self, input: f32) -> f32 {
        let v3 = input - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;

        let output = match self.mode {
            FilterMode::LowPass => v2,
            FilterMode::HighPass => input - self.k * v1 - v2,
            FilterMode::BandPass => v1,
            FilterMode::Notch => input - self.k * v1,
        };

        // A denormal state costs more than the filter itself on some hosts,
        // and a state that has gone wrong must not be allowed to stay wrong.
        if !self.ic1.is_finite() || !self.ic2.is_finite() {
            self.reset();
            return 0.0;
        }
        output
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

    const RATE: f32 = 48_000.0;

    /// Gain at `frequency` once the filter has settled, relative to its input.
    ///
    /// Relative on purpose: a sine sampled at six points per cycle peaks at 0.87,
    /// not at 1, so an absolute measurement would report a filter's passband as a
    /// cut that is not there.
    fn response(filter: &mut Svf, frequency: f32) -> f32 {
        filter.reset();
        let step = std::f32::consts::TAU * frequency / RATE;
        let sample = |index: u32| (index as f32 * step).sin();

        // A tenth of a second of settling before anything is measured.
        for index in 0..4_800 {
            filter.process(sample(index));
        }

        let mut output = 0.0f32;
        let mut input = 0.0f32;
        for index in 4_800..9_600 {
            let value = sample(index);
            input = input.max(value.abs());
            output = output.max(filter.process(value).abs());
        }

        output / input.max(1e-9)
    }

    #[test]
    fn silence_stays_silent() {
        let mut filter = Svf::default();
        filter.set(FilterMode::LowPass, 800.0, 4.0, RATE);

        for _ in 0..1_000 {
            assert_eq!(filter.process(0.0), 0.0);
        }
    }

    #[test]
    fn a_low_pass_keeps_the_low_and_drops_the_high() {
        let mut filter = Svf::default();
        filter.set(FilterMode::LowPass, 1_000.0, 0.707, RATE);

        let low = response(&mut filter, 100.0);
        let high = response(&mut filter, 10_000.0);

        assert!(low > 0.9, "the passband should come through: {low}");
        assert!(high < 0.05, "the stopband should be gone: {high}");
    }

    #[test]
    fn a_high_pass_does_the_opposite() {
        let mut filter = Svf::default();
        filter.set(FilterMode::HighPass, 1_000.0, 0.707, RATE);

        let low = response(&mut filter, 100.0);
        let high = response(&mut filter, 10_000.0);

        assert!(low < 0.05, "{low}");
        assert!(high > 0.9, "{high}");
    }

    #[test]
    fn a_band_pass_keeps_its_middle() {
        let mut filter = Svf::default();
        filter.set(FilterMode::BandPass, 1_000.0, 4.0, RATE);

        let below = response(&mut filter, 100.0);
        let centre = response(&mut filter, 1_000.0);
        let above = response(&mut filter, 10_000.0);

        assert!(centre > below * 4.0, "{below} -> {centre}");
        assert!(centre > above * 4.0, "{above} <- {centre}");
    }

    #[test]
    fn a_notch_cuts_its_middle() {
        let mut filter = Svf::default();
        filter.set(FilterMode::Notch, 1_000.0, 4.0, RATE);

        let centre = response(&mut filter, 1_000.0);
        let away = response(&mut filter, 100.0);

        assert!(centre < 0.2, "the notch should bite: {centre}");
        assert!(away > 0.8, "away from it nothing should change: {away}");
    }

    #[test]
    fn sweeping_the_cutoff_every_frame_stays_finite() {
        // The reason for this topology: a biquad fed new coefficients per
        // frame rings and eventually blows up.
        let mut filter = Svf::default();
        let mut highest = 0.0f32;

        for index in 0..48_000 {
            let sweep = 100.0 + (index as f32 / 48_000.0) * 15_000.0;
            filter.set(FilterMode::LowPass, sweep, 8.0, RATE);
            let output = filter.process((index as f32 * 0.05).sin());
            assert!(output.is_finite(), "went wild at {index}");
            highest = highest.max(output.abs());
        }

        assert!(highest < 20.0, "resonance ran away: {highest}");
    }

    #[test]
    fn a_broken_setting_is_repaired_rather_than_obeyed() {
        let mut filter = Svf::default();
        filter.set(FilterMode::LowPass, f32::NAN, f32::INFINITY, RATE);

        let output = filter.process(0.5);

        assert!(output.is_finite(), "{output}");
    }

    #[test]
    fn the_cutoff_stays_below_nyquist_at_any_rate() {
        // At 8 kHz the fixed ceiling alone would sit well past Nyquist.
        let mut filter = Svf::default();
        filter.set(FilterMode::LowPass, MAX_CUTOFF, 0.707, 8_000.0);

        for index in 0..1_000 {
            let output = filter.process((index as f32 * 0.3).sin());
            assert!(output.is_finite(), "went wild at {index}");
        }
    }

    #[test]
    fn a_reset_filter_forgets_what_went_through_it() {
        let mut filter = Svf::default();
        filter.set(FilterMode::LowPass, 500.0, 2.0, RATE);
        for _ in 0..100 {
            filter.process(1.0);
        }

        filter.reset();

        assert_eq!(filter.process(0.0), 0.0);
    }
}
