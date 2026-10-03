use std::f32::consts::TAU;

use saempler_model::Waveform;

/// Naive (non band-limited) oscillator used as the development test signal.
///
/// Aliasing is accepted here: this exists only to prove that MIDI reaches the
/// engine and that audio leaves it. It will be replaced by sample playback.
#[derive(Debug, Default, Clone, Copy)]
pub struct Oscillator {
    /// Normalized phase in `[0, 1)`.
    phase: f32,
    phase_delta: f32,
}

impl Oscillator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restart the oscillator at the beginning of its period.
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Set the oscillator frequency in Hz for the given sample rate.
    pub fn set_frequency(&mut self, frequency_hz: f32, sample_rate: f32) {
        self.phase_delta = if sample_rate > 0.0 {
            frequency_hz / sample_rate
        } else {
            0.0
        };
    }

    /// Produce the next sample in `[-1, 1]` and advance the phase.
    pub fn next_sample(&mut self, waveform: Waveform) -> f32 {
        let value = match waveform {
            Waveform::Sine => (self.phase * TAU).sin(),
            Waveform::Saw => (self.phase * 2.0) - 1.0,
            Waveform::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        };

        self.phase += self.phase_delta;
        // Wrapping by subtraction keeps the phase exact for small deltas and
        // avoids a modulo in the per-sample path.
        while self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        value
    }
}

/// Frequency of a MIDI note number in 12-TET with A4 = 440 Hz.
pub fn note_frequency(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_maps_to_440_hz() {
        assert!((note_frequency(69) - 440.0).abs() < 1e-3);
        assert!((note_frequency(81) - 880.0).abs() < 1e-3);
    }

    #[test]
    fn output_stays_within_unit_range() {
        for waveform in Waveform::ALL {
            let mut osc = Oscillator::new();
            osc.set_frequency(997.0, 44_100.0);

            for _ in 0..44_100 {
                let sample = osc.next_sample(waveform);
                assert!(sample.is_finite());
                assert!((-1.0..=1.0).contains(&sample));
            }
        }
    }

    #[test]
    fn sine_repeats_exactly_once_per_period() {
        let sample_rate = 48_000.0;
        let mut osc = Oscillator::new();
        osc.set_frequency(100.0, sample_rate);

        // 480 samples is exactly one period at 100 Hz / 48 kHz.
        let first: Vec<f32> = (0..480).map(|_| osc.next_sample(Waveform::Sine)).collect();
        let second: Vec<f32> = (0..480).map(|_| osc.next_sample(Waveform::Sine)).collect();

        for (index, (a, b)) in first.iter().zip(&second).enumerate() {
            assert!(
                (a - b).abs() < 1e-4,
                "sample {index} differs between periods: {a} vs {b}"
            );
        }
        assert!(first.iter().any(|sample| *sample > 0.9));
        assert!(first.iter().any(|sample| *sample < -0.9));
    }

    #[test]
    fn zero_sample_rate_does_not_produce_nan() {
        let mut osc = Oscillator::new();
        osc.set_frequency(440.0, 0.0);

        assert!(osc.next_sample(Waveform::Sine).is_finite());
    }
}
