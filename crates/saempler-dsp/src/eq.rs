/// Bands an equalizer carries.
///
/// Eight, like the desk equalizers this follows. A cell that only needs three
/// switches the rest off rather than having a second, smaller type.
pub const EQ_BANDS: usize = 8;

/// Range of a band's gain, in decibels.
pub const MAX_BAND_GAIN_DB: f32 = 18.0;
/// Range of a band's width.
pub const MIN_Q: f32 = 0.1;
pub const MAX_Q: f32 = 18.0;

/// What one band of an equalizer does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BandKind {
    /// A bell: a lift or a cut around its frequency.
    #[default]
    Peak,
    /// Everything below its frequency, lifted or cut.
    LowShelf,
    /// Everything above it.
    HighShelf,
    /// Everything below it, removed.
    HighPass,
    /// Everything above it, removed.
    LowPass,
}

/// One biquad section in direct form one.
///
/// A biquad rather than the state variable filter used in the voices: an
/// equalizer band is set when the user turns a knob, not per frame, so there
/// is nothing to gain from a topology built for being modulated, and the
/// cookbook shelves and bells have no state variable equivalent as simple.
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

impl Biquad {
    /// Point the band at a frequency, a gain and a width.
    ///
    /// Recomputing the coefficients clears nothing: the state is the last two
    /// samples in and out, which stay meaningful across a change. Clearing
    /// them would click on every knob movement.
    pub fn set(&mut self, kind: BandKind, frequency: f32, gain_db: f32, q: f32, sample_rate: f32) {
        let rate = if sample_rate.is_finite() && sample_rate > 1.0 {
            sample_rate
        } else {
            48_000.0
        };
        let frequency = finite_or(frequency, 1_000.0).clamp(20.0, rate * 0.45);
        let gain_db = finite_or(gain_db, 0.0).clamp(-MAX_BAND_GAIN_DB, MAX_BAND_GAIN_DB);
        let q = finite_or(q, 0.707).clamp(MIN_Q, MAX_Q);

        let omega = std::f32::consts::TAU * frequency / rate;
        let (sin, cos) = omega.sin_cos();
        let alpha = sin / (2.0 * q);
        // Amplitude, which the shelves need as its square root.
        let amplitude = 10.0f32.powf(gain_db / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            BandKind::Peak => (
                1.0 + alpha * amplitude,
                -2.0 * cos,
                1.0 - alpha * amplitude,
                1.0 + alpha / amplitude,
                -2.0 * cos,
                1.0 - alpha / amplitude,
            ),
            BandKind::LowShelf => {
                let shared = 2.0 * amplitude.sqrt() * alpha;
                (
                    amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cos + shared),
                    2.0 * amplitude * ((amplitude - 1.0) - (amplitude + 1.0) * cos),
                    amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cos - shared),
                    (amplitude + 1.0) + (amplitude - 1.0) * cos + shared,
                    -2.0 * ((amplitude - 1.0) + (amplitude + 1.0) * cos),
                    (amplitude + 1.0) + (amplitude - 1.0) * cos - shared,
                )
            }
            BandKind::HighShelf => {
                let shared = 2.0 * amplitude.sqrt() * alpha;
                (
                    amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cos + shared),
                    -2.0 * amplitude * ((amplitude - 1.0) + (amplitude + 1.0) * cos),
                    amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cos - shared),
                    (amplitude + 1.0) - (amplitude - 1.0) * cos + shared,
                    2.0 * ((amplitude - 1.0) - (amplitude + 1.0) * cos),
                    (amplitude + 1.0) - (amplitude - 1.0) * cos - shared,
                )
            }
            BandKind::HighPass => (
                (1.0 + cos) * 0.5,
                -(1.0 + cos),
                (1.0 + cos) * 0.5,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BandKind::LowPass => (
                (1.0 - cos) * 0.5,
                1.0 - cos,
                (1.0 - cos) * 0.5,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
        };

        // A zero leading coefficient would make every later sample infinite.
        if a0.abs() < 1e-12 {
            *self = Self {
                x1: self.x1,
                x2: self.x2,
                y1: self.y1,
                y2: self.y2,
                ..Self::default()
            };
            return;
        }

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// Make the band do nothing at all.
    pub fn bypass(&mut self) {
        self.b0 = 1.0;
        self.b1 = 0.0;
        self.b2 = 0.0;
        self.a1 = 0.0;
        self.a2 = 0.0;
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    /// Filter one sample.
    pub fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;

        if !output.is_finite() {
            self.reset();
            return 0.0;
        }

        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

/// How one band of an equalizer is set.
#[derive(Debug, Clone, Copy)]
pub struct BandSetting {
    pub kind: BandKind,
    pub frequency: f32,
    pub gain_db: f32,
    pub q: f32,
    pub enabled: bool,
}

impl Default for BandSetting {
    fn default() -> Self {
        Self {
            kind: BandKind::Peak,
            frequency: 1_000.0,
            gain_db: 0.0,
            q: 0.707,
            enabled: false,
        }
    }
}

/// A stereo equalizer of [`EQ_BANDS`] biquads in series.
///
/// Both channels take the same settings and keep their own state, so the
/// equalizer colours the sound without moving it across the stereo picture.
#[derive(Debug, Default, Clone)]
pub struct Equalizer {
    bands: [[Biquad; EQ_BANDS]; 2],
    active: [bool; EQ_BANDS],
}

impl Equalizer {
    pub fn reset(&mut self) {
        for channel in &mut self.bands {
            for band in channel {
                band.reset();
            }
        }
    }

    /// Apply a full set of band settings.
    pub fn set(&mut self, settings: &[BandSetting; EQ_BANDS], sample_rate: f32) {
        for (index, setting) in settings.iter().enumerate() {
            // A band at unity gain still costs its arithmetic, and a bell at
            // 0 dB is the common case while someone is only using three of
            // the eight.
            let silent_bell = matches!(
                setting.kind,
                BandKind::Peak | BandKind::LowShelf | BandKind::HighShelf
            ) && setting.gain_db.abs() < 0.01;
            self.active[index] = setting.enabled && !silent_bell;

            for channel in 0..2 {
                if self.active[index] {
                    self.bands[channel][index].set(
                        setting.kind,
                        setting.frequency,
                        setting.gain_db,
                        setting.q,
                        sample_rate,
                    );
                } else {
                    self.bands[channel][index].bypass();
                }
            }
        }
    }

    /// Filter one frame.
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let mut output = [left, right];
        for (channel, value) in output.iter_mut().enumerate() {
            for index in 0..EQ_BANDS {
                if self.active[index] {
                    *value = self.bands[channel][index].process(*value);
                }
            }
        }
        (output[0], output[1])
    }

    /// Whether any band is doing anything, so a caller can skip the whole
    /// equalizer rather than running eight bypassed biquads.
    pub fn is_active(&self) -> bool {
        self.active.iter().any(|active| *active)
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

    /// Gain at `frequency` once the band has settled, relative to its input.
    ///
    /// Relative on purpose: a sine sampled at six points per cycle peaks at 0.87,
    /// not at 1, so an absolute measurement would report a band's passband as a
    /// cut that is not there.
    fn response(band: &mut Biquad, frequency: f32) -> f32 {
        band.reset();
        let step = std::f32::consts::TAU * frequency / RATE;
        let sample = |index: u32| (index as f32 * step).sin();

        // A tenth of a second of settling before anything is measured.
        for index in 0..4_800 {
            band.process(sample(index));
        }

        let mut output = 0.0f32;
        let mut input = 0.0f32;
        for index in 4_800..9_600 {
            let value = sample(index);
            input = input.max(value.abs());
            output = output.max(band.process(value).abs());
        }

        output / input.max(1e-9)
    }

    #[test]
    fn silence_stays_silent() {
        let mut band = Biquad::default();
        band.set(BandKind::Peak, 1_000.0, 12.0, 2.0, RATE);

        for _ in 0..1_000 {
            assert_eq!(band.process(0.0), 0.0);
        }
    }

    #[test]
    fn a_bell_lifts_its_own_frequency_and_leaves_the_rest() {
        let mut band = Biquad::default();
        band.set(BandKind::Peak, 1_000.0, 12.0, 4.0, RATE);

        let at = response(&mut band, 1_000.0);
        let away = response(&mut band, 60.0);

        // Twelve decibels is a factor of four.
        assert!((at - 4.0).abs() < 0.4, "{at}");
        assert!((away - 1.0).abs() < 0.1, "{away}");
    }

    #[test]
    fn a_bell_cuts_as_well_as_it_lifts() {
        let mut band = Biquad::default();
        band.set(BandKind::Peak, 1_000.0, -12.0, 4.0, RATE);

        let at = response(&mut band, 1_000.0);

        assert!((at - 0.25).abs() < 0.05, "{at}");
    }

    #[test]
    fn a_low_shelf_lifts_below_and_not_above() {
        let mut band = Biquad::default();
        band.set(BandKind::LowShelf, 500.0, 12.0, 0.707, RATE);

        let below = response(&mut band, 60.0);
        let above = response(&mut band, 8_000.0);

        assert!((below - 4.0).abs() < 0.4, "{below}");
        assert!((above - 1.0).abs() < 0.1, "{above}");
    }

    #[test]
    fn a_high_shelf_lifts_above_and_not_below() {
        let mut band = Biquad::default();
        band.set(BandKind::HighShelf, 4_000.0, 12.0, 0.707, RATE);

        let below = response(&mut band, 200.0);
        let above = response(&mut band, 12_000.0);

        assert!((below - 1.0).abs() < 0.1, "{below}");
        assert!((above - 4.0).abs() < 0.4, "{above}");
    }

    #[test]
    fn the_cut_bands_cut() {
        let mut high_pass = Biquad::default();
        high_pass.set(BandKind::HighPass, 1_000.0, 0.0, 0.707, RATE);
        let mut low_pass = Biquad::default();
        low_pass.set(BandKind::LowPass, 1_000.0, 0.0, 0.707, RATE);

        assert!(response(&mut high_pass, 60.0) < 0.02);
        assert!(response(&mut high_pass, 10_000.0) > 0.9);
        assert!(response(&mut low_pass, 60.0) > 0.9);
        assert!(response(&mut low_pass, 10_000.0) < 0.05);
    }

    #[test]
    fn a_bypassed_band_passes_everything_through_untouched() {
        let mut band = Biquad::default();
        band.set(BandKind::Peak, 1_000.0, 12.0, 4.0, RATE);
        band.bypass();
        band.reset();

        for step in 0..100 {
            let input = (step as f32 * 0.1).sin();
            assert!((band.process(input) - input).abs() < 1e-6);
        }
    }

    #[test]
    fn a_broken_setting_is_repaired_rather_than_obeyed() {
        let mut band = Biquad::default();
        band.set(BandKind::Peak, f32::NAN, f32::INFINITY, -5.0, RATE);

        for step in 0..1_000 {
            assert!(band.process((step as f32 * 0.1).sin()).is_finite());
        }
    }

    #[test]
    fn an_equalizer_with_nothing_switched_on_changes_nothing() {
        let mut equalizer = Equalizer::default();
        equalizer.set(&[BandSetting::default(); EQ_BANDS], RATE);

        assert!(!equalizer.is_active());
        for step in 0..100 {
            let input = (step as f32 * 0.1).sin();
            assert_eq!(equalizer.process(input, -input), (input, -input));
        }
    }

    #[test]
    fn a_band_at_unity_gain_is_left_switched_off() {
        let mut settings = [BandSetting::default(); EQ_BANDS];
        settings[0] = BandSetting {
            gain_db: 0.0,
            enabled: true,
            ..BandSetting::default()
        };
        let mut equalizer = Equalizer::default();
        equalizer.set(&settings, RATE);

        assert!(
            !equalizer.is_active(),
            "a bell doing nothing should not cost anything"
        );
    }

    #[test]
    fn the_bands_of_an_equalizer_add_up() {
        let mut settings = [BandSetting::default(); EQ_BANDS];
        settings[0] = BandSetting {
            kind: BandKind::Peak,
            frequency: 1_000.0,
            gain_db: 6.0,
            q: 4.0,
            enabled: true,
        };
        settings[1] = BandSetting {
            kind: BandKind::Peak,
            frequency: 1_000.0,
            gain_db: 6.0,
            q: 4.0,
            enabled: true,
        };
        let mut equalizer = Equalizer::default();
        equalizer.set(&settings, RATE);

        let step = std::f32::consts::TAU * 1_000.0 / RATE;
        for index in 0..4_800 {
            equalizer.process((index as f32 * step).sin(), 0.0);
        }
        let peak = (4_800..9_600)
            .map(|index| equalizer.process((index as f32 * step).sin(), 0.0).0.abs())
            .fold(0.0f32, f32::max);

        // Two lifts of six decibels make twelve, which is a factor of four.
        assert!((peak - 4.0).abs() < 0.5, "{peak}");
    }

    #[test]
    fn the_two_channels_are_filtered_the_same() {
        let mut settings = [BandSetting::default(); EQ_BANDS];
        settings[0] = BandSetting {
            frequency: 800.0,
            gain_db: 9.0,
            q: 2.0,
            enabled: true,
            ..BandSetting::default()
        };
        let mut equalizer = Equalizer::default();
        equalizer.set(&settings, RATE);

        for index in 0..4_800 {
            let input = (index as f32 * 0.1).sin();
            let (left, right) = equalizer.process(input, input);
            assert!((left - right).abs() < 1e-5, "{left} vs {right} at {index}");
        }
    }
}
