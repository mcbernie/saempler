/// Longest delay the line can be asked for, in seconds.
///
/// Two seconds covers a whole bar well below 120 bpm, which is as long as a
/// chop delay stays musical rather than becoming a looper.
pub const MAX_DELAY_SECONDS: f32 = 2.0;

/// A delay line with a fractional read position.
///
/// The buffer is allocated once in [`DelayLine::prepare`], which runs when the
/// host sets the sample rate, never inside the callback. Reading and writing
/// allocate nothing and never panic: the read position wraps rather than
/// indexing out of range, so a nonsense delay time produces the wrong sound
/// rather than a crash.
#[derive(Debug, Default, Clone)]
pub struct DelayLine {
    buffer: Vec<f32>,
    write: usize,
}

impl DelayLine {
    /// Make room for the longest delay at this sample rate.
    ///
    /// Allocates, so it belongs with the rest of the preparation and not in
    /// the audio callback.
    pub fn prepare(&mut self, sample_rate: f32) {
        let rate = if sample_rate.is_finite() && sample_rate > 1.0 {
            sample_rate
        } else {
            48_000.0
        };
        let frames = (rate * MAX_DELAY_SECONDS).ceil() as usize + 4;

        self.buffer.clear();
        self.buffer.resize(frames, 0.0);
        self.write = 0;
    }

    /// Clear the line without giving up its buffer.
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    /// Frames the line can hold.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Read what was written `delay` frames ago, interpolating between the
    /// two samples either side of a fractional position.
    ///
    /// Interpolated because the delay time is modulated: stepping between
    /// whole frames would click on every change, which is what a chorus is
    /// and what a delay is not.
    pub fn read(&self, delay: f32) -> f32 {
        if self.buffer.is_empty() {
            return 0.0;
        }

        let length = self.buffer.len();
        let delay = finite_or(delay, 0.0).clamp(1.0, (length - 2) as f32);
        let whole = delay.floor();
        let fraction = delay - whole;

        // Backwards from the write head, wrapped into the buffer.
        let first = (self.write + length - whole as usize) % length;
        let second = (first + length - 1) % length;

        self.buffer[first] + (self.buffer[second] - self.buffer[first]) * fraction
    }

    /// Write one sample and step the head on.
    pub fn write(&mut self, value: f32) {
        if self.buffer.is_empty() {
            return;
        }

        // A state that has gone wrong would otherwise circulate forever.
        self.buffer[self.write] = if value.is_finite() { value } else { 0.0 };
        self.write = (self.write + 1) % self.buffer.len();
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

    #[test]
    fn a_prepared_line_holds_the_longest_delay() {
        let mut line = DelayLine::default();
        line.prepare(RATE);

        assert!(line.capacity() >= (RATE * MAX_DELAY_SECONDS) as usize);
    }

    #[test]
    fn what_goes_in_comes_back_out_after_the_delay() {
        let mut line = DelayLine::default();
        line.prepare(RATE);

        line.write(1.0);
        for _ in 0..99 {
            line.write(0.0);
        }

        // A hundred frames were written, so the impulse is a hundred back.
        assert!(
            (line.read(100.0) - 1.0).abs() < 1e-5,
            "{}",
            line.read(100.0)
        );
        assert!(line.read(50.0).abs() < 1e-5);
    }

    #[test]
    fn a_fractional_delay_lands_between_the_two_samples() {
        let mut line = DelayLine::default();
        line.prepare(RATE);

        line.write(0.0);
        line.write(1.0);
        line.write(0.0);

        // Halfway between the impulse and the silence beside it.
        let half = line.read(1.5);

        assert!((half - 0.5).abs() < 1e-5, "{half}");
    }

    #[test]
    fn an_empty_line_reads_silence_rather_than_panicking() {
        let line = DelayLine::default();

        assert_eq!(line.read(100.0), 0.0);
    }

    #[test]
    fn a_nonsense_delay_time_is_clamped_rather_than_indexed() {
        let mut line = DelayLine::default();
        line.prepare(RATE);
        line.write(1.0);

        // Longer than the buffer, shorter than a frame, and not a number.
        for delay in [-10.0, 0.0, f32::NAN, f32::INFINITY, 1e9] {
            assert!(line.read(delay).is_finite(), "{delay}");
        }
    }

    #[test]
    fn a_wrong_sample_does_not_circulate() {
        let mut line = DelayLine::default();
        line.prepare(RATE);

        line.write(f32::NAN);
        for _ in 0..9 {
            line.write(0.0);
        }

        assert_eq!(line.read(10.0), 0.0);
    }

    #[test]
    fn the_head_wraps_rather_than_running_off_the_end() {
        let mut line = DelayLine::default();
        line.prepare(1_000.0);

        for index in 0..10_000 {
            line.write(index as f32 * 0.001);
            assert!(line.read(500.0).is_finite());
        }
    }

    #[test]
    fn a_reset_line_is_silent_but_keeps_its_buffer() {
        let mut line = DelayLine::default();
        line.prepare(RATE);
        let capacity = line.capacity();
        for _ in 0..1_000 {
            line.write(1.0);
        }

        line.reset();

        assert_eq!(line.capacity(), capacity);
        assert_eq!(line.read(100.0), 0.0);
    }
}
