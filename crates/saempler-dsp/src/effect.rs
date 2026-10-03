use crate::delay::DelayLine;
use crate::svf::{FilterMode, Svf};

/// Most of itself a delay may feed back.
///
/// Short of one: at one the line never decays, and the first loud chop stays
/// in it for the rest of the session.
pub const MAX_FEEDBACK: f32 = 0.95;

/// A stereo delay with feedback and a damped repeat path.
///
/// Damped because an undamped delay repeating a vocal chop keeps the sibilance
/// as bright on the twentieth repeat as on the first, which no room does.
#[derive(Debug, Default, Clone)]
pub struct Delay {
    lines: [DelayLine; 2],
    damping: [Svf; 2],
    sample_rate: f32,
    /// Delay in frames, approached a little at a time.
    time: f32,
    target_time: f32,
    feedback: f32,
}

impl Delay {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 1.0 {
            sample_rate
        } else {
            48_000.0
        };
        for line in &mut self.lines {
            line.prepare(self.sample_rate);
        }
        for filter in &mut self.damping {
            filter.set(FilterMode::LowPass, 6_000.0, 0.707, self.sample_rate);
            filter.reset();
        }
        self.time = 0.0;
        self.target_time = 0.0;
    }

    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.reset();
        }
        for filter in &mut self.damping {
            filter.reset();
        }
    }

    /// Set the repeat time in seconds, the feedback and how dark the repeats go.
    pub fn set(&mut self, seconds: f32, feedback: f32, damping_hz: f32) {
        let seconds = finite_or(seconds, 0.25).clamp(0.001, crate::MAX_DELAY_SECONDS);
        self.target_time = seconds * self.sample_rate.max(1.0);
        // The first setting lands straight away; later ones are slid into, so
        // that turning the time knob glides like tape rather than clicking.
        if self.time <= 0.0 {
            self.time = self.target_time;
        }
        self.feedback = finite_or(feedback, 0.3).clamp(0.0, MAX_FEEDBACK);
        for filter in &mut self.damping {
            filter.set(FilterMode::LowPass, damping_hz, 0.707, self.sample_rate);
        }
    }

    /// Process one frame, returning the wet signal alone.
    ///
    /// Wet alone rather than mixed: this is a send, and the caller owns how
    /// much of it reaches the output.
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        // A twentieth of the way per frame is slow enough not to pitch-shift
        // audibly and quick enough to feel immediate.
        self.time += (self.target_time - self.time) * 0.0005;

        let mut output = [0.0f32; 2];
        for (channel, input) in [left, right].into_iter().enumerate() {
            let delayed = self.lines[channel].read(self.time);
            let damped = self.damping[channel].process(delayed);
            self.lines[channel].write(input + damped * self.feedback);
            output[channel] = delayed;
        }

        (output[0], output[1])
    }
}

/// Number of all-pass stages in the phaser.
///
/// Four pairs of notches: enough to be heard sweeping, few enough that the
/// sound keeps its body.
const PHASER_STAGES: usize = 4;

/// A stereo phaser: a chain of first order all-passes swept by its own LFO.
///
/// Its own LFO rather than the voice's, because a send is shared by every
/// voice and there is no one note whose modulation it could follow.
#[derive(Debug, Default, Clone)]
pub struct Phaser {
    /// One all-pass state per stage per channel.
    stages: [[f32; PHASER_STAGES]; 2],
    /// Fed back from the end of the chain into its start.
    feedback_state: [f32; 2],
    phase: f32,
    phase_delta: f32,
    depth: f32,
    feedback: f32,
    sample_rate: f32,
}

impl Phaser {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() && sample_rate > 1.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.reset();
        self.set(0.5, 0.7, 0.4);
    }

    pub fn reset(&mut self) {
        self.stages = [[0.0; PHASER_STAGES]; 2];
        self.feedback_state = [0.0; 2];
        self.phase = 0.0;
    }

    /// Set the sweep rate in hertz, how far it sweeps, and the resonance.
    pub fn set(&mut self, rate_hz: f32, depth: f32, feedback: f32) {
        let rate = finite_or(rate_hz, 0.5).clamp(0.01, 10.0);
        self.phase_delta = rate / self.sample_rate.max(1.0);
        self.depth = finite_or(depth, 0.7).clamp(0.0, 1.0);
        self.feedback = finite_or(feedback, 0.4).clamp(0.0, 0.9);
    }

    /// Process one frame, returning the wet signal alone.
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.phase = (self.phase + self.phase_delta).fract();

        let mut output = [0.0f32; 2];
        for (channel, input) in [left, right].into_iter().enumerate() {
            // The two channels sweep half a cycle apart, which is what makes
            // a phaser widen rather than simply wobble.
            let offset = if channel == 0 { 0.0 } else { 0.5 };
            let sweep = ((self.phase + offset).fract() * std::f32::consts::TAU).sin();
            // From roughly 200 Hz to 2 kHz, as an all-pass coefficient.
            let normalized = 0.5 + 0.5 * sweep * self.depth;
            let coefficient = 0.1 + 0.8 * normalized;

            let mut value = input + self.feedback_state[channel] * self.feedback;
            for stage in 0..PHASER_STAGES {
                let state = self.stages[channel][stage];
                // A first order all-pass: unity magnitude, shifting phase.
                let shaped = -coefficient * value + state;
                self.stages[channel][stage] = value + coefficient * shaped;
                value = shaped;
            }

            self.feedback_state[channel] = if value.is_finite() { value } else { 0.0 };
            output[channel] = self.feedback_state[channel];
        }

        (output[0], output[1])
    }
}

/// Comb filter lengths in frames at 48 kHz, and the all-pass lengths after
/// them. Mutually prime so their repeats do not line up into a flutter.
const COMB_LENGTHS: [usize; 4] = [1_557, 1_617, 1_491, 1_422];
const ALLPASS_LENGTHS: [usize; 2] = [225, 556];
/// How much longer the right channel's combs are, in frames.
const STEREO_SPREAD: usize = 23;

/// A Schroeder reverb: parallel combs into a pair of all-passes.
///
/// Not a convolution and not a feedback delay network: this is a send on a
/// chop instrument, where the job is to put a vocal in a space, and the cost
/// of the better algorithms buys detail nobody will hear under a beat.
#[derive(Debug, Default, Clone)]
pub struct Reverb {
    combs: [[DelayLine; COMB_LENGTHS.len()]; 2],
    comb_damping: [[f32; COMB_LENGTHS.len()]; 2],
    allpasses: [[DelayLine; ALLPASS_LENGTHS.len()]; 2],
    comb_frames: [[f32; COMB_LENGTHS.len()]; 2],
    allpass_frames: [[f32; ALLPASS_LENGTHS.len()]; 2],
    feedback: f32,
    damping: f32,
}

impl Reverb {
    pub fn prepare(&mut self, sample_rate: f32) {
        let rate = if sample_rate.is_finite() && sample_rate > 1.0 {
            sample_rate
        } else {
            48_000.0
        };
        // The lengths above are for 48 kHz; at another rate the room would
        // otherwise change size with the session.
        let scale = rate / 48_000.0;

        for channel in 0..2 {
            let spread = if channel == 0 { 0 } else { STEREO_SPREAD };
            for (index, length) in COMB_LENGTHS.iter().enumerate() {
                self.combs[channel][index].prepare(rate);
                self.comb_frames[channel][index] = ((length + spread) as f32 * scale).max(2.0);
            }
            for (index, length) in ALLPASS_LENGTHS.iter().enumerate() {
                self.allpasses[channel][index].prepare(rate);
                self.allpass_frames[channel][index] = ((length + spread) as f32 * scale).max(2.0);
            }
        }
        self.reset();
        self.set(0.6, 0.4);
    }

    pub fn reset(&mut self) {
        for channel in 0..2 {
            for line in &mut self.combs[channel] {
                line.reset();
            }
            for line in &mut self.allpasses[channel] {
                line.reset();
            }
            self.comb_damping[channel] = [0.0; COMB_LENGTHS.len()];
        }
    }

    /// Set how big the room is and how much of the high end it swallows.
    pub fn set(&mut self, size: f32, damping: f32) {
        let size = finite_or(size, 0.6).clamp(0.0, 1.0);
        // Short of one: at one the combs never decay.
        self.feedback = 0.7 + size * 0.28;
        self.damping = finite_or(damping, 0.4).clamp(0.0, 0.95);
    }

    /// Process one frame, returning the wet signal alone.
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let mut output = [0.0f32; 2];

        for (channel, input) in [left, right].into_iter().enumerate() {
            let mut summed = 0.0;
            for index in 0..COMB_LENGTHS.len() {
                let frames = self.comb_frames[channel][index];
                let delayed = self.combs[channel][index].read(frames);
                // A one pole lowpass inside the loop, so each pass through
                // the comb loses a little more of the top.
                let state = &mut self.comb_damping[channel][index];
                *state = delayed * (1.0 - self.damping) + *state * self.damping;
                self.combs[channel][index].write(input + *state * self.feedback);
                summed += delayed;
            }
            summed /= COMB_LENGTHS.len() as f32;

            for index in 0..ALLPASS_LENGTHS.len() {
                let frames = self.allpass_frames[channel][index];
                let delayed = self.allpasses[channel][index].read(frames);
                // The usual Schroeder all-pass coefficient; it scatters the
                // comb output without colouring it.
                self.allpasses[channel][index].write(summed + delayed * 0.5);
                summed = delayed - summed * 0.5;
            }

            output[channel] = if summed.is_finite() { summed } else { 0.0 };
        }

        (output[0], output[1])
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

    /// Run an effect on silence and report the loudest thing it produced.
    fn silence_through(mut step: impl FnMut() -> (f32, f32), frames: usize) -> f32 {
        (0..frames).fold(0.0f32, |loudest, _| {
            let (left, right) = step();
            loudest.max(left.abs()).max(right.abs())
        })
    }

    #[test]
    fn a_delay_is_silent_until_something_reaches_it() {
        let mut delay = Delay::default();
        delay.prepare(RATE);
        delay.set(0.25, 0.5, 6_000.0);

        assert_eq!(silence_through(|| delay.process(0.0, 0.0), 24_000), 0.0);
    }

    #[test]
    fn a_delay_repeats_after_the_time_it_was_given() {
        let mut delay = Delay::default();
        delay.prepare(RATE);
        delay.set(0.1, 0.0, 18_000.0);

        delay.process(1.0, 1.0);
        // Nothing yet at half the time.
        let early = silence_through(|| delay.process(0.0, 0.0), 2_000);
        // The repeat lands somewhere in the window around 0.1 s.
        let late = silence_through(|| delay.process(0.0, 0.0), 4_000);

        assert!(early < 0.01, "it repeated too soon: {early}");
        assert!(late > 0.5, "the repeat never arrived: {late}");
    }

    #[test]
    fn a_delay_decays_rather_than_building_up() {
        let mut delay = Delay::default();
        delay.prepare(RATE);
        delay.set(0.05, MAX_FEEDBACK, 8_000.0);

        for _ in 0..240 {
            delay.process(1.0, 1.0);
        }
        let loudest = silence_through(|| delay.process(0.0, 0.0), RATE as usize * 20);

        assert!(loudest.is_finite());
        assert!(loudest < 40.0, "the feedback ran away: {loudest}");
    }

    #[test]
    fn a_phaser_is_silent_on_silence() {
        let mut phaser = Phaser::default();
        phaser.prepare(RATE);

        assert_eq!(silence_through(|| phaser.process(0.0, 0.0), 48_000), 0.0);
    }

    #[test]
    fn a_phaser_passes_signal_without_running_away() {
        let mut phaser = Phaser::default();
        phaser.prepare(RATE);
        phaser.set(2.0, 1.0, 0.9);

        let mut loudest = 0.0f32;
        for index in 0..48_000 {
            let input = (index as f32 * 0.05).sin();
            let (left, right) = phaser.process(input, input);
            assert!(left.is_finite() && right.is_finite(), "wild at {index}");
            loudest = loudest.max(left.abs());
        }

        assert!(loudest > 0.1, "it swallowed everything: {loudest}");
        assert!(loudest < 20.0, "the resonance ran away: {loudest}");
    }

    #[test]
    fn a_phaser_moves_the_two_channels_apart() {
        let mut phaser = Phaser::default();
        phaser.prepare(RATE);
        phaser.set(4.0, 1.0, 0.5);

        let mut difference = 0.0f32;
        for index in 0..24_000 {
            let input = (index as f32 * 0.07).sin();
            let (left, right) = phaser.process(input, input);
            difference = difference.max((left - right).abs());
        }

        assert!(
            difference > 0.05,
            "the same signal came out of both sides: {difference}"
        );
    }

    #[test]
    fn a_reverb_is_silent_on_silence() {
        let mut reverb = Reverb::default();
        reverb.prepare(RATE);

        assert_eq!(silence_through(|| reverb.process(0.0, 0.0), 48_000), 0.0);
    }

    #[test]
    fn a_reverb_rings_on_after_the_sound_stops() {
        let mut reverb = Reverb::default();
        reverb.prepare(RATE);
        reverb.set(0.8, 0.3);

        for _ in 0..480 {
            reverb.process(1.0, 1.0);
        }
        // A quarter of a second later there should still be something there.
        let mut tail = 0.0f32;
        for index in 0..24_000 {
            let (left, right) = reverb.process(0.0, 0.0);
            if index > 12_000 {
                tail = tail.max(left.abs()).max(right.abs());
            }
        }

        assert!(tail > 0.001, "the room had no tail: {tail}");
    }

    #[test]
    fn a_reverb_decays_rather_than_building_up() {
        let mut reverb = Reverb::default();
        reverb.prepare(RATE);
        reverb.set(1.0, 0.0);

        for _ in 0..RATE as usize {
            reverb.process(1.0, 1.0);
        }
        let during = silence_through(|| reverb.process(1.0, 1.0), 4_800);
        let after = silence_through(|| reverb.process(0.0, 0.0), RATE as usize * 30);

        assert!(
            during.is_finite() && during < 200.0,
            "it built up: {during}"
        );
        assert!(after.is_finite(), "it went wild once the input stopped");
    }

    #[test]
    fn the_two_reverb_channels_differ() {
        let mut reverb = Reverb::default();
        reverb.prepare(RATE);

        for _ in 0..480 {
            reverb.process(1.0, 1.0);
        }
        let mut difference = 0.0f32;
        for _ in 0..24_000 {
            let (left, right) = reverb.process(0.0, 0.0);
            difference = difference.max((left - right).abs());
        }

        assert!(difference > 1e-4, "it is a mono room: {difference}");
    }

    #[test]
    fn every_effect_survives_a_broken_setting() {
        let mut delay = Delay::default();
        delay.prepare(RATE);
        delay.set(f32::NAN, f32::INFINITY, f32::NAN);

        let mut phaser = Phaser::default();
        phaser.prepare(RATE);
        phaser.set(f32::NAN, f32::INFINITY, f32::NAN);

        let mut reverb = Reverb::default();
        reverb.prepare(RATE);
        reverb.set(f32::NAN, f32::INFINITY);

        for _ in 0..4_800 {
            for (left, right) in [
                delay.process(0.5, -0.5),
                phaser.process(0.5, -0.5),
                reverb.process(0.5, -0.5),
            ] {
                assert!(left.is_finite() && right.is_finite());
            }
        }
    }

    #[test]
    fn an_unprepared_effect_is_silent_rather_than_panicking() {
        let mut delay = Delay::default();
        let mut phaser = Phaser::default();
        let mut reverb = Reverb::default();

        for _ in 0..100 {
            assert_eq!(delay.process(1.0, 1.0), (0.0, 0.0));
            assert!(phaser.process(1.0, 1.0).0.is_finite());
            assert_eq!(reverb.process(1.0, 1.0), (0.0, 0.0));
        }
    }
}
