use saempler_model::{ModDestination, PlaybackMode};

use crate::command::CellSpec;
use crate::modulation::{Modulation, PITCH_RANGE_SEMITONES};
use crate::sample::SampleBuffer;

/// Output frames a released brake takes to reach full speed again.
const SPIN_UP_FRAMES: f64 = 6_000.0;

/// Shortest loop any mode or modulation may produce, in source frames.
///
/// A collapse would otherwise shrink its loop without limit, and a loop of a
/// few frames is a read position that never moves rather than a sound.
const MIN_LOOP_FRAMES: f64 = 32.0;

/// How far a full-amount route to the loop length reaches, in octaves.
const LOOP_RANGE_OCTAVES: f32 = 2.0;

/// A single sounding note.
///
/// Voices are preallocated in a fixed array and reused, so this type contains
/// no owned allocations and never needs to be dropped on the audio thread.
///
/// The envelopes and LFOs live here rather than in the cell, so two voices of
/// the same cell run independently: pressing a key twice gives one voice in
/// sustain and one still in its attack.
#[derive(Debug, Clone, Copy)]
pub struct Voice {
    active: bool,
    /// Whether the key is still down. A released voice is finishing.
    held: bool,
    note: u8,
    /// Monotonic counter used to pick the oldest voice when stealing.
    age: u64,
    /// Fractional read position in the sample, in frames.
    ///
    /// Fractional because speed and pitch change how fast the slice is read;
    /// the sample values either side are interpolated.
    position: f64,
    /// Frames advanced per output frame before modulation. Always positive;
    /// direction is carried by `reverse`.
    rate: f64,
    /// Multiplier a tape stop applies to the rate, falling from 1 to 0.
    rate_scale: f64,
    /// Change in `rate_scale` per output frame. Positive brakes, negative
    /// winds back up to speed, zero holds.
    rate_decay: f64,
    reverse: bool,
    /// Lower bound of the region the loop reads, in source frames.
    ///
    /// The loop is an interval rather than a point plus a direction, so that
    /// turning a voice round inside a loop reverses it instead of sending the
    /// playhead away from its anchor and out of the region.
    loop_low: u64,
    /// Length of the loop the playback mode runs, in source frames. Zero means
    /// the mode is not looping, either because it never does or because a
    /// release trigger has not fired yet. A collapse shrinks this per pass.
    mode_loop: u64,
    /// Factor the loop length destination applies, carried over from the
    /// previous frame because the loop is wrapped before this frame's
    /// modulation has been evaluated.
    loop_scale: f32,
    sample_rate: f32,
    tempo: f64,
    modulation: Modulation,
    /// The cell as the keyboard maps it, before any modifier.
    ///
    /// Kept so that a modifier engaged or released mid-note can be applied to
    /// the untouched cell rather than to whatever the last one left behind.
    base: CellSpec,
    spec: CellSpec,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            held: false,
            note: 0,
            age: 0,
            position: 0.0,
            rate: 1.0,
            rate_scale: 1.0,
            rate_decay: 0.0,
            reverse: false,
            loop_low: 0,
            mode_loop: 0,
            loop_scale: 1.0,
            sample_rate: 48_000.0,
            tempo: 120.0,
            modulation: Modulation::default(),
            base: CellSpec::default(),
            spec: CellSpec::default(),
        }
    }
}

impl Voice {
    /// Whether this voice currently contributes to the output.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether this voice is sounding the given note and has not been released.
    pub fn is_playing_note(&self, note: u8) -> bool {
        self.active && self.held && self.note == note
    }

    /// Age counter, used by the engine to find the oldest voice.
    pub fn age(&self) -> u64 {
        self.age
    }

    /// Frame this voice is reading, for the playhead display.
    pub fn position(&self) -> u64 {
        self.position.max(0.0) as u64
    }

    /// Where this voice is reading, or `None` while it is idle.
    pub fn active_position(&self) -> Option<u64> {
        self.active.then(|| self.position())
    }

    /// The cell this voice plays, before modifiers.
    pub fn base(&self) -> CellSpec {
        self.base
    }

    /// Start this voice, replacing whatever it was playing before.
    ///
    /// `base` is the cell as mapped, `spec` the same cell with the modifiers
    /// that were engaged at the moment of the trigger.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        note: u8,
        velocity: f32,
        age: u64,
        base: CellSpec,
        spec: CellSpec,
        sample_rate: f32,
        tempo: f64,
    ) {
        self.active = true;
        self.held = true;
        self.note = note;
        self.age = age;
        self.sample_rate = sample_rate;
        self.tempo = tempo;
        self.base = base;
        self.spec = spec;

        self.rate = spec.rate.max(f32::MIN_POSITIVE) as f64;
        self.rate_scale = 1.0;
        self.rate_decay = if spec.tape_stop_frames > 0 {
            1.0 / spec.tape_stop_frames as f64
        } else {
            0.0
        };
        self.reverse = spec.reverse;
        self.loop_scale = 1.0;

        // Backwards playback starts at the last frame of the slice.
        self.position = if spec.reverse {
            spec.bounds.end_frame.saturating_sub(1) as f64
        } else {
            spec.bounds.start_frame as f64
        };
        self.loop_low = self.position.max(0.0) as u64;
        self.mode_loop = 0;
        if !spec.release_trigger {
            self.engage_mode_loop();
        }

        // An envelope that outlasts the slice is scaled down rather than
        // truncated, so a long release on a short chop fades across all of it
        // instead of silencing the voice on its first frame.
        let available = (spec.bounds.len_frames() as f64 / self.rate) as f32;
        let wanted = longest_envelope_frames(&spec, sample_rate);
        let scale = if wanted > available && wanted > 0.0 {
            (available / wanted).max(0.001)
        } else {
            1.0
        };

        self.modulation.start(
            spec.modulation,
            velocity,
            sample_rate,
            tempo,
            scale,
            (age as u32) ^ (note as u32) ^ 0x5bf0_3635,
        );
    }

    /// Apply a changed set of modifiers to a voice that is already sounding.
    ///
    /// The changes take effect from where the voice is, not from the start of
    /// the slice: engaging a stutter loops the audio under the playhead, and
    /// engaging reverse turns the voice round on the spot. That is what makes
    /// modifiers playable rather than merely configurable.
    pub fn retune(&mut self, spec: CellSpec) {
        if !self.active {
            return;
        }

        self.rate = spec.rate.max(f32::MIN_POSITIVE) as f64;
        self.reverse = spec.reverse;

        // A loop that has just been engaged starts under the playhead; one
        // that was already running keeps its region, so neither the rhythm
        // jumps nor does reversing throw the playhead out of it.
        let had_loop = self.loop_length() > 0.0;
        self.spec = spec;
        if self.loop_length() > 0.0 && !had_loop {
            self.anchor_loop();
        }

        if spec.tape_stop_frames > 0 {
            if self.rate_decay <= 0.0 {
                self.rate_decay = 1.0 / spec.tape_stop_frames as f64;
            }
        } else if self.rate_scale < 1.0 {
            // Letting go of the brake winds the tape back up to speed.
            self.rate_decay = -(1.0 / SPIN_UP_FRAMES);
        } else {
            self.rate_decay = 0.0;
        }
    }

    /// Follow a tempo change without restarting anything.
    ///
    /// A repeat or collapse already under way keeps the length it started
    /// with: changing it halfway would move the loop point under a playing
    /// note.
    pub fn set_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
        self.modulation.retune(self.sample_rate, tempo);
    }

    /// Handle the key coming up.
    ///
    /// What that means is the playback mode's business: one shot ignores it,
    /// a release trigger treats it as the start of the effect rather than the
    /// end of the note, and everything else fades out.
    pub fn release(&mut self) {
        if !self.active {
            return;
        }
        if self.spec.mode == PlaybackMode::OneShot {
            // The note plays out whatever the key does; the fade before the
            // slice edge still ends it.
            self.held = false;
            return;
        }
        if self.spec.release_trigger && self.mode_loop == 0 {
            self.engage_mode_loop();
        }
        self.fade_out();
    }

    /// Move the envelopes into their release stage.
    fn fade_out(&mut self) {
        self.held = false;
        self.modulation.release();
    }

    /// Start the loop this playback mode runs, under the current playhead.
    ///
    /// Only the length is set up here; [`Voice::loop_length`] decides whether
    /// it or a modifier loop is the one in force.
    fn engage_mode_loop(&mut self) {
        if !self.spec.mode.loops() {
            return;
        }

        let length = if self.spec.mode.uses_division() {
            self.cycle_frames()
        } else {
            self.spec.bounds.len_frames() as f64
        };

        self.mode_loop = length.max(MIN_LOOP_FRAMES) as u64;
        self.anchor_loop();
    }

    /// Lay the loop region out around the playhead.
    ///
    /// Playing forwards the region runs from here on; playing backwards it
    /// runs up to here, so in both cases the audio under the playhead is the
    /// first thing the loop repeats.
    fn anchor_loop(&mut self) {
        let position = self.position.max(0.0) as u64;
        let length = self.raw_loop() as u64;
        let low = if self.reverse {
            position.saturating_sub(length)
        } else {
            position
        };

        self.loop_low = low.clamp(
            self.spec.bounds.start_frame,
            self.spec.bounds.end_frame.saturating_sub(1),
        );
    }

    /// Loop length before it is clamped to what is left of the slice.
    fn raw_loop(&self) -> f64 {
        let base = match usable_loop(self.spec) {
            0 => self.mode_loop as f64,
            modifier => modifier as f64,
        };
        if base <= 0.0 {
            return 0.0;
        }

        (base * self.loop_scale as f64).max(MIN_LOOP_FRAMES)
    }

    /// Length of one repeat or collapse pass, in source frames.
    ///
    /// The note value is a length in real time, so it is multiplied by the
    /// read rate: a repeat stays a sixteenth however far the cell is
    /// transposed.
    fn cycle_frames(&self) -> f64 {
        // Four beats to a whole note, and sixty seconds to that many beats.
        let seconds = self.spec.cycle_whole_notes as f64 * 240.0 / self.tempo.max(1.0);
        seconds * self.sample_rate as f64 * self.rate
    }

    /// Length of the loop in force, in source frames. Zero means no loop.
    ///
    /// A modifier loop wins over the mode's own: the modifier is a gesture
    /// made while playing, and it should be heard over a setting.
    fn loop_length(&self) -> f64 {
        let base = self.raw_loop();
        if base <= 0.0 {
            return 0.0;
        }

        // The region has to stay inside the slice: reading past the edge would
        // mix the neighbouring chop into the tail.
        let available = self.spec.bounds.end_frame.saturating_sub(self.loop_low) as f64;

        base.min(available)
    }

    /// Silence the voice immediately.
    pub fn kill(&mut self) {
        self.active = false;
        self.held = false;
        self.position = 0.0;
        self.modulation.kill();
    }

    /// Render the next frame, advancing playback, envelopes and LFOs.
    ///
    /// The fade out is started early enough to finish at the slice edge. A
    /// voice therefore never reads past its own slice: doing so would mix the
    /// neighbouring chop into the tail, and would show a playhead running on
    /// past the region it is playing.
    pub fn next_frame(&mut self, sample: &SampleBuffer) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        if self.rate_decay != 0.0 {
            self.rate_scale -= self.rate_decay;
            if self.rate_scale <= 0.0 {
                // The tape has come to rest; there is nothing left to read.
                self.kill();
                return (0.0, 0.0);
            }
            if self.rate_scale >= 1.0 {
                self.rate_scale = 1.0;
                self.rate_decay = 0.0;
            }
        }

        // A looping voice never approaches the slice edge, so the fade out is
        // left to the note off.
        let mut release_floor = 0.0;
        if !self.wrap_loop() {
            let remaining = self.frames_to_edge();
            if remaining <= 0.0 {
                self.kill();
                return (0.0, 0.0);
            }
            if self.held && remaining <= self.modulation.release_frames() {
                self.fade_out();
            }
            if !self.held {
                release_floor = (1.0 / remaining.max(1.0)) as f32;
            }
        }

        let modulation = self.modulation.next(release_floor);
        if !self.modulation.is_active() {
            self.kill();
            return (0.0, 0.0);
        }

        let (left, right) = interpolated(sample, self.position);

        // Pitch and rate both scale the read speed: pitch in semitones, rate
        // as a plain multiplier, exactly as the destinations are named.
        let pitch = modulation.get(ModDestination::Pitch) * PITCH_RANGE_SEMITONES;
        let rate_mod = (1.0 + modulation.get(ModDestination::PlaybackRate)).max(0.01);
        let step = self.rate * self.rate_scale * (rate_mod * semitones(pitch)) as f64;

        if self.reverse {
            self.position -= step;
        } else {
            self.position += step;
        }

        // Volume is modulated rather than fixed: with nothing routed to it the
        // voice is silent, which is what an empty matrix means.
        self.loop_scale =
            2.0f32.powf(modulation.get(ModDestination::LoopLength) * LOOP_RANGE_OCTAVES);

        let level = (modulation.get(ModDestination::Volume) * self.spec.gain).clamp(0.0, 4.0);
        let pan = modulation.get(ModDestination::Pan).clamp(-1.0, 1.0);
        let (left_gain, right_gain) = pan_gains(pan);

        (left * level * left_gain, right * level * right_gain)
    }

    /// Send the playhead back to the far end when it leaves the loop region.
    ///
    /// Returns whether this voice is looping at all, which decides whether the
    /// slice edge is something it can ever reach.
    fn wrap_loop(&mut self) -> bool {
        let length = self.loop_length();
        if length <= 0.0 {
            return false;
        }

        // Half open: forwards the region is read from `low` up to but not
        // including `high`, so backwards it starts one frame below `high`.
        let low = self.loop_low as f64;
        let high = low + length;
        let wrapped = if self.reverse {
            (self.position <= low).then_some(high - 1.0)
        } else {
            (self.position >= high).then_some(low)
        };

        if let Some(position) = wrapped {
            self.position = position;

            // A collapse shortens its own loop with every pass. Only its own:
            // a modifier loop keeps the length the gesture asked for.
            if usable_loop(self.spec) == 0 && self.spec.mode == PlaybackMode::Collapse {
                let next = self.mode_loop as f64 * self.spec.collapse as f64;
                self.mode_loop = next.max(MIN_LOOP_FRAMES) as u64;
                // Backwards the region keeps its upper end, so the collapse is
                // heard at the point the loop was taken from either way.
                if self.reverse {
                    self.loop_low = (high - self.raw_loop()).max(0.0) as u64;
                    self.position = high - 1.0;
                }
            }
        }

        true
    }

    /// Output frames left before the read position leaves the slice.
    fn frames_to_edge(&self) -> f64 {
        let remaining_source = if self.reverse {
            self.position - self.spec.bounds.start_frame as f64
        } else {
            self.spec.bounds.end_frame as f64 - self.position
        };

        // The scaled rate, so a braking voice is not told it has moments left
        // when in truth it has almost stopped moving.
        let rate = (self.rate * self.rate_scale).max(1e-9);
        (remaining_source / rate).max(0.0)
    }
}

/// The loop length a spec actually imposes.
///
/// A loop as long as the slice, or longer, is no loop at all: there is nothing
/// to come back to before the end arrives.
fn usable_loop(spec: CellSpec) -> u64 {
    if spec.loop_frames > 0 && spec.loop_frames < spec.bounds.len_frames() {
        spec.loop_frames
    } else {
        0
    }
}

/// Rate multiplier for a transposition in semitones.
fn semitones(value: f32) -> f32 {
    2.0f32.powf(value / 12.0)
}

/// Left and right gain for a pan position from -1 to 1.
///
/// Constant power, so a sound does not jump in level as it moves across. The
/// curve is normalized to unity in the centre rather than to unity at the
/// edges: pan defaults to centre, and a cell must not lose 3 dB merely
/// because the destination exists.
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
    (
        angle.cos() * std::f32::consts::SQRT_2,
        angle.sin() * std::f32::consts::SQRT_2,
    )
}

/// Longest any envelope of this cell runs, in output frames.
fn longest_envelope_frames(spec: &CellSpec, sample_rate: f32) -> f32 {
    spec.modulation
        .envelopes
        .iter()
        .map(|envelope| {
            (envelope.attack_ms + envelope.decay_ms + envelope.release_ms) / 1000.0 * sample_rate
        })
        .fold(0.0, f32::max)
}

/// Read a frame between two samples, weighting them by the fraction.
///
/// Linear interpolation. It adds some high-frequency dullness and a little
/// distortion at large transpositions, which is the accepted cost until a
/// proper interpolator exists.
fn interpolated(sample: &SampleBuffer, position: f64) -> (f32, f32) {
    if position < 0.0 {
        return (0.0, 0.0);
    }

    let index = position as usize;
    let fraction = (position - index as f64) as f32;
    let (left_a, right_a) = sample.frame(index);
    let (left_b, right_b) = sample.frame(index + 1);

    (
        left_a + (left_b - left_a) * fraction,
        right_a + (right_b - right_a) * fraction,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::SliceBounds;
    use crate::modulation::ModulationSpec;
    use saempler_model::{
        default_routes, Division, EnvelopeDefinition, LfoDefinition, LfoShape, ModSource,
        ModulationRoute, ENVELOPE_COUNT, LFO_COUNT,
    };

    const SAMPLE_RATE: f32 = 48_000.0;
    const TEMPO: f64 = 120.0;

    /// A buffer of constant full-scale samples, so the measured output is the
    /// envelope level alone.
    fn dc_buffer(frames: usize) -> SampleBuffer {
        SampleBuffer::new(vec![vec![1.0; frames], vec![1.0; frames]], 48_000)
    }

    /// A buffer whose sample value equals its frame index divided by `frames`,
    /// so the output says which frame was read.
    fn ramp_buffer(frames: usize) -> SampleBuffer {
        let data: Vec<f32> = (0..frames)
            .map(|index| index as f32 / frames as f32)
            .collect();
        SampleBuffer::new(vec![data], 48_000)
    }

    fn modulation(routes: &[ModulationRoute]) -> ModulationSpec {
        ModulationSpec::new(
            [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            [LfoDefinition::default(); LFO_COUNT],
            routes,
        )
    }

    fn spec(start: u64, end: u64) -> CellSpec {
        CellSpec {
            bounds: SliceBounds {
                start_frame: start,
                end_frame: end,
            },
            modulation: modulation(&default_routes()),
            ..CellSpec::default()
        }
    }

    fn start(voice: &mut Voice, spec: CellSpec) {
        voice.start(60, 1.0, 0, spec, spec, SAMPLE_RATE, TEMPO);
    }

    #[test]
    fn idle_voice_is_silent() {
        let buffer = dc_buffer(100);
        let mut voice = Voice::default();

        assert!(!voice.is_active());
        assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
    }

    #[test]
    fn playback_starts_at_the_slice_start() {
        let mut data = vec![0.0f32; 1_000];
        data[500..].fill(1.0);
        let buffer = SampleBuffer::new(vec![data], 48_000);

        let mut inside = Voice::default();
        start(&mut inside, spec(500, 700));
        let heard = (0..100)
            .map(|_| inside.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        let mut before = Voice::default();
        start(&mut before, spec(0, 200));
        let silent = (0..100)
            .map(|_| before.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert!(heard > 0.0, "a slice over audio must sound");
        assert_eq!(silent, 0.0, "a slice over silence must stay silent");
    }

    #[test]
    fn a_cell_with_nothing_routed_to_the_volume_is_silent() {
        let buffer = dc_buffer(10_000);
        let mut voice = Voice::default();
        start(
            &mut voice,
            CellSpec {
                modulation: modulation(&[]),
                ..spec(0, 10_000)
            },
        );

        let peak = (0..5_000)
            .map(|_| voice.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert_eq!(peak, 0.0, "an empty matrix means no amplitude");
    }

    #[test]
    fn the_envelope_shapes_the_level() {
        let buffer = dc_buffer(300_000);
        let mut spec = spec(0, 300_000);
        spec.modulation = ModulationSpec::new(
            [
                EnvelopeDefinition {
                    attack_ms: 100.0,
                    decay_ms: 100.0,
                    sustain: 0.25,
                    release_ms: 50.0,
                },
                EnvelopeDefinition::default(),
            ],
            [LfoDefinition::default(); LFO_COUNT],
            &default_routes(),
        );

        let mut voice = Voice::default();
        start(&mut voice, spec);

        for _ in 0..4_800 {
            voice.next_frame(&buffer);
        }
        let peak = voice.next_frame(&buffer).0;

        for _ in 0..19_200 {
            voice.next_frame(&buffer);
        }
        let sustain = voice.next_frame(&buffer).0;

        assert!(peak > 0.9, "the attack should have opened: {peak}");
        assert!(
            (sustain - 0.25).abs() < 0.05,
            "the decay should rest at the sustain: {sustain}"
        );
    }

    #[test]
    fn velocity_reaches_the_level_through_the_matrix() {
        let buffer = dc_buffer(10_000);
        let routes = [ModulationRoute {
            source: ModSource::Velocity,
            destination: ModDestination::Volume,
            amount: 1.0,
        }];
        let spec = CellSpec {
            modulation: modulation(&routes),
            ..spec(0, 10_000)
        };

        let mut quiet = Voice::default();
        quiet.start(60, 0.25, 0, spec, spec, SAMPLE_RATE, TEMPO);

        let peak = (0..1_000)
            .map(|_| quiet.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert!((peak - 0.25).abs() < 0.05, "{peak}");
    }

    #[test]
    fn an_lfo_on_the_pitch_moves_the_read_position() {
        let buffer = ramp_buffer(400_000);
        let mut routes = default_routes();
        routes.push(ModulationRoute {
            source: ModSource::Lfo1,
            destination: ModDestination::Pitch,
            amount: 0.5,
        });

        let mut modulated_spec = spec(0, 400_000);
        modulated_spec.modulation = ModulationSpec::new(
            [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            [
                LfoDefinition {
                    shape: LfoShape::Square,
                    rate_hz: 2.0,
                    ..Default::default()
                },
                LfoDefinition::default(),
            ],
            &routes,
        );

        let mut modulated = Voice::default();
        start(&mut modulated, modulated_spec);
        let mut plain = Voice::default();
        start(&mut plain, spec(0, 400_000));

        // A square wave at full amount spends the first half cycle up, so the
        // modulated voice reads further in the same time.
        for _ in 0..6_000 {
            modulated.next_frame(&buffer);
            plain.next_frame(&buffer);
        }

        assert!(
            modulated.position() > plain.position(),
            "{} vs {}",
            modulated.position(),
            plain.position()
        );
    }

    #[test]
    fn a_pan_route_moves_the_sound_across() {
        let buffer = dc_buffer(10_000);
        let mut routes = default_routes();
        routes.push(ModulationRoute {
            source: ModSource::Velocity,
            destination: ModDestination::Pan,
            amount: 1.0,
        });
        let spec = CellSpec {
            modulation: modulation(&routes),
            ..spec(0, 10_000)
        };

        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, spec, spec, SAMPLE_RATE, TEMPO);
        for _ in 0..1_000 {
            voice.next_frame(&buffer);
        }
        let (left, right) = voice.next_frame(&buffer);

        assert!(right > left * 4.0, "full right expected: {left} vs {right}");
    }

    #[test]
    fn a_centred_sound_keeps_its_level() {
        let (left, right) = pan_gains(0.0);

        assert!((left - 1.0).abs() < 1e-5, "{left}");
        assert!((right - 1.0).abs() < 1e-5, "{right}");
    }

    #[test]
    fn the_power_stays_the_same_wherever_a_sound_sits() {
        let centre = {
            let (left, right) = pan_gains(0.0);
            left * left + right * right
        };

        for step in -10..=10 {
            let (left, right) = pan_gains(step as f32 / 10.0);
            let power = left * left + right * right;

            assert!((power - centre).abs() < 1e-5, "{step}: {power}");
        }
    }

    #[test]
    fn a_gated_voice_stops_at_the_end_of_its_slice() {
        let buffer = dc_buffer(10_000);
        let mut voice = Voice::default();
        start(&mut voice, spec(0, 1_000));

        for _ in 0..2_000 {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active());
    }

    #[test]
    fn a_looping_voice_keeps_going_until_the_key_comes_up() {
        let buffer = dc_buffer(10_000);
        let spec = CellSpec {
            mode: PlaybackMode::Loop,
            ..spec(0, 1_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        for _ in 0..10_000 {
            voice.next_frame(&buffer);
        }

        assert!(voice.is_active(), "the loop should still be running");
        assert!(
            voice.position() <= 1_000,
            "it should be inside the slice: {}",
            voice.position()
        );

        voice.release();
        for _ in 0..10_000 {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active(), "the release should have ended it");
    }

    #[test]
    fn a_reversed_loop_keeps_running() {
        let buffer = dc_buffer(10_000);
        let spec = CellSpec {
            mode: PlaybackMode::Loop,
            reverse: true,
            ..spec(1_000, 3_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        let mut heard = 0.0f32;
        for _ in 0..20_000 {
            heard = heard.max(voice.next_frame(&buffer).0.abs());
            assert!(
                (1_000..3_000).contains(&voice.position()),
                "left the slice: {}",
                voice.position()
            );
        }

        assert!(voice.is_active(), "a held loop should not end by itself");
        assert!(heard > 0.5, "it fell silent: {heard}");
    }

    #[test]
    fn turning_a_loop_round_keeps_it_inside_its_region() {
        let buffer = dc_buffer(10_000);
        let forwards = CellSpec {
            mode: PlaybackMode::Repeat,
            cycle_whole_notes: Division::Sixteenth.whole_notes(),
            ..spec(0, 48_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, forwards);
        for _ in 0..3_000 {
            voice.next_frame(&buffer);
        }

        // The reverse modifier, engaged while the loop is already running.
        voice.retune(CellSpec {
            reverse: true,
            ..forwards
        });

        let mut heard = 0.0f32;
        for _ in 0..20_000 {
            heard = heard.max(voice.next_frame(&buffer).0.abs());
        }

        assert!(voice.is_active(), "reversing should not end the voice");
        assert!(heard > 0.5, "reversing silenced the loop: {heard}");
    }

    #[test]
    fn a_reversed_collapse_still_shortens_its_loop() {
        let buffer = dc_buffer(200_000);
        let spec = CellSpec {
            mode: PlaybackMode::Collapse,
            cycle_whole_notes: Division::Sixteenth.whole_notes(),
            collapse: 0.5,
            reverse: true,
            ..spec(0, 100_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        let mut passes = Vec::new();
        let mut lowest = u64::MAX;
        let mut previous = voice.position();
        for _ in 0..40_000 {
            voice.next_frame(&buffer);
            let position = voice.position();
            if position > previous {
                passes.push(previous);
                lowest = u64::MAX;
            }
            lowest = lowest.min(position);
            previous = position;
        }

        // Backwards the region keeps its upper end, so a shrinking loop shows
        // up as a lower bound climbing towards it and then holding at the
        // floor.
        assert!(passes.len() >= 3, "expected several passes: {passes:?}");
        for pair in passes.windows(2) {
            assert!(
                pair[1] >= pair[0],
                "the region grew instead of shrinking: {passes:?}"
            );
        }
        let last = *passes.last().expect("the list was checked above");
        assert!(passes[0] < last, "the loop never shortened: {passes:?}");
        assert!(
            (100_000.0 - last as f64) <= MIN_LOOP_FRAMES + 1.0,
            "it never reached the floor: {passes:?}"
        );
    }

    #[test]
    fn a_one_shot_voice_ignores_the_key_coming_up() {
        let buffer = dc_buffer(10_000);
        let spec = CellSpec {
            mode: PlaybackMode::OneShot,
            ..spec(0, 4_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        voice.next_frame(&buffer);
        voice.release();
        for _ in 0..1_000 {
            voice.next_frame(&buffer);
        }

        assert!(voice.is_active(), "it should play on");
        assert!(voice.position() > 900);

        // It still ends at the slice edge rather than running on.
        for _ in 0..5_000 {
            voice.next_frame(&buffer);
        }
        assert!(!voice.is_active());
    }

    #[test]
    fn a_repeat_loops_the_note_value_rather_than_the_slice() {
        let buffer = dc_buffer(200_000);
        // A sixteenth at 120 bpm is 125 ms, which is 6000 frames at 48 kHz.
        let spec = CellSpec {
            mode: PlaybackMode::Repeat,
            cycle_whole_notes: Division::Sixteenth.whole_notes(),
            ..spec(0, 100_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        let mut highest = 0;
        for _ in 0..30_000 {
            voice.next_frame(&buffer);
            highest = highest.max(voice.position());
        }

        assert!(
            (5_900..=6_100).contains(&highest),
            "the loop should be one sixteenth long: {highest}"
        );
    }

    #[test]
    fn a_collapse_shortens_its_loop_with_every_pass() {
        let buffer = dc_buffer(200_000);
        let spec = CellSpec {
            mode: PlaybackMode::Collapse,
            cycle_whole_notes: Division::Sixteenth.whole_notes(),
            collapse: 0.5,
            ..spec(0, 100_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        let mut passes = Vec::new();
        let mut highest = 0;
        let mut previous = 0;
        for _ in 0..40_000 {
            voice.next_frame(&buffer);
            let position = voice.position();
            if position < previous {
                passes.push(highest);
                highest = 0;
            }
            highest = highest.max(position);
            previous = position;
        }

        assert!(passes.len() >= 4, "expected several passes: {passes:?}");
        // Each pass is shorter than the one before until the floor is reached,
        // where the collapse holds rather than shrinking to nothing.
        for pair in passes.windows(2) {
            assert!(
                pair[1] < pair[0] || pair[1] as f64 <= MIN_LOOP_FRAMES,
                "a pass grew: {passes:?}"
            );
        }
        assert!(
            (passes[1] as f64 - passes[0] as f64 * 0.5).abs() < 2.0,
            "a factor of a half should halve the pass: {passes:?}"
        );
    }

    #[test]
    fn a_collapse_does_not_shrink_to_nothing() {
        let buffer = dc_buffer(200_000);
        let spec = CellSpec {
            mode: PlaybackMode::Collapse,
            cycle_whole_notes: Division::Sixteenth.whole_notes(),
            collapse: 0.25,
            ..spec(0, 100_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        for _ in 0..200_000 {
            voice.next_frame(&buffer);
        }

        assert!(voice.is_active(), "it should still be looping");
        assert!(
            voice.position() as f64 >= MIN_LOOP_FRAMES - 1.0 || voice.position() == 0,
            "the loop should have a floor: {}",
            voice.position()
        );
    }

    #[test]
    fn a_release_trigger_waits_for_the_key_to_come_up() {
        let buffer = dc_buffer(200_000);
        let spec = CellSpec {
            mode: PlaybackMode::Repeat,
            cycle_whole_notes: Division::Sixteenth.whole_notes(),
            release_trigger: true,
            ..spec(0, 100_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        // While held it plays straight through, past the loop length.
        for _ in 0..20_000 {
            voice.next_frame(&buffer);
        }
        assert!(
            voice.position() > 10_000,
            "it should not be looping yet: {}",
            voice.position()
        );

        let before = voice.position();
        voice.release();
        let mut highest = 0;
        for _ in 0..5_000 {
            voice.next_frame(&buffer);
            highest = highest.max(voice.position());
        }

        assert!(
            highest < before + 7_000,
            "the loop should have taken hold: {before} -> {highest}"
        );
    }

    #[test]
    fn a_loop_never_leaves_the_slice() {
        // The ramp makes the output say which frame was read, so this measures
        // the reads themselves rather than the playhead between two frames.
        let frames = 200_000;
        let buffer = ramp_buffer(frames);
        let spec = CellSpec {
            mode: PlaybackMode::Repeat,
            // A whole bar is far longer than the slice this is given.
            cycle_whole_notes: Division::OneBar.whole_notes(),
            ..spec(1_000, 4_000)
        };
        let mut voice = Voice::default();
        start(&mut voice, spec);

        let edge = 4_000.0 / frames as f32;
        for _ in 0..50_000 {
            let (left, _) = voice.next_frame(&buffer);
            assert!(left <= edge, "read past the slice end: {left} > {edge}");
        }
    }

    #[test]
    fn the_loop_length_destination_shortens_the_loop() {
        let buffer = dc_buffer(200_000);
        let mut routes = default_routes();
        routes.push(ModulationRoute {
            source: ModSource::Velocity,
            destination: ModDestination::LoopLength,
            amount: -1.0,
        });
        let spec = CellSpec {
            mode: PlaybackMode::Repeat,
            cycle_whole_notes: Division::Sixteenth.whole_notes(),
            modulation: modulation(&routes),
            ..spec(0, 100_000)
        };

        let mut voice = Voice::default();
        voice.start(60, 1.0, 0, spec, spec, SAMPLE_RATE, TEMPO);

        let mut highest = 0;
        for _ in 0..30_000 {
            voice.next_frame(&buffer);
            highest = highest.max(voice.position());
        }

        // Full negative amount is two octaves down: a quarter of 6000 frames.
        assert!(
            (1_400..=1_700).contains(&highest),
            "the loop should have been shortened: {highest}"
        );
    }

    #[test]
    fn reverse_reads_the_slice_from_the_other_end() {
        let buffer = ramp_buffer(1_000);
        let bounds = spec(100, 900);

        let mut forward = Voice::default();
        start(&mut forward, bounds);
        let mut backward = Voice::default();
        start(
            &mut backward,
            CellSpec {
                reverse: true,
                ..bounds
            },
        );

        for _ in 0..200 {
            forward.next_frame(&buffer);
            backward.next_frame(&buffer);
        }

        assert_eq!(forward.position(), 300);
        assert_eq!(backward.position(), 699);
    }

    #[test]
    fn a_voice_reaching_the_slice_end_stops_on_its_own() {
        let buffer = dc_buffer(10_000);
        let mut voice = Voice::default();
        start(&mut voice, spec(0, 500));

        for _ in 0..10_000 {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active());
    }

    #[test]
    fn the_fade_out_never_reads_the_next_slice() {
        let mut data = vec![0.0f32; 20_000];
        data[10_000..].fill(1.0);
        let buffer = SampleBuffer::new(vec![data], 48_000);

        let mut voice = Voice::default();
        start(&mut voice, spec(0, 10_000));

        let peak = (0..20_000)
            .map(|_| voice.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert_eq!(peak, 0.0, "the voice read past its own slice");
    }

    #[test]
    fn a_long_release_stays_inside_the_slice() {
        let mut data = vec![0.0f32; 20_000];
        data[10_000..].fill(1.0);
        let buffer = SampleBuffer::new(vec![data], 48_000);

        let mut long = spec(0, 10_000);
        long.modulation = ModulationSpec::new(
            [
                EnvelopeDefinition {
                    release_ms: 2_000.0,
                    ..Default::default()
                },
                EnvelopeDefinition::default(),
            ],
            [LfoDefinition::default(); LFO_COUNT],
            &default_routes(),
        );

        let mut voice = Voice::default();
        start(&mut voice, long);
        let peak = (0..40_000)
            .map(|_| voice.next_frame(&buffer).0.abs())
            .fold(0.0f32, f32::max);

        assert_eq!(peak, 0.0, "a long release must not borrow the next slice");
        assert!(!voice.is_active(), "and it must still end");
    }

    #[test]
    fn the_playhead_never_leaves_the_slice() {
        let buffer = dc_buffer(100_000);
        let mut voice = Voice::default();
        start(&mut voice, spec(20_000, 30_000));

        while voice.is_active() {
            voice.next_frame(&buffer);
            if !voice.is_active() {
                break;
            }
            assert!(
                (20_000..=30_000).contains(&voice.position()),
                "playhead left the slice at {}",
                voice.position()
            );
        }
    }

    #[test]
    fn a_loop_keeps_returning_to_the_trigger_point() {
        let buffer = ramp_buffer(100_000);
        let mut voice = Voice::default();
        start(
            &mut voice,
            CellSpec {
                loop_frames: 1_000,
                ..spec(0, 50_000)
            },
        );

        for _ in 0..10_000 {
            voice.next_frame(&buffer);
            assert!(
                voice.position() <= 1_000,
                "a looping voice left its loop at {}",
                voice.position()
            );
        }
        assert!(voice.is_active(), "a loop plays until the key is released");
    }

    #[test]
    fn a_loop_longer_than_the_slice_is_no_loop() {
        let buffer = dc_buffer(100_000);
        let mut voice = Voice::default();
        start(
            &mut voice,
            CellSpec {
                loop_frames: 1_000_000,
                ..spec(0, 5_000)
            },
        );

        for _ in 0..50_000 {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active(), "it must still end at the slice");
    }

    #[test]
    fn a_looping_voice_still_stops_on_note_off() {
        let buffer = dc_buffer(100_000);
        let mut voice = Voice::default();
        start(
            &mut voice,
            CellSpec {
                loop_frames: 1_000,
                ..spec(0, 50_000)
            },
        );
        for _ in 0..5_000 {
            voice.next_frame(&buffer);
        }

        voice.release();
        for _ in 0..10_000 {
            voice.next_frame(&buffer);
        }

        assert!(!voice.is_active());
    }

    #[test]
    fn a_tape_stop_slows_down_and_ends() {
        let buffer = ramp_buffer(400_000);
        let mut voice = Voice::default();
        start(
            &mut voice,
            CellSpec {
                tape_stop_frames: 4_800,
                ..spec(0, 400_000)
            },
        );

        let mut positions = Vec::new();
        for frame in 0..4_800 {
            voice.next_frame(&buffer);
            if frame % 800 == 0 {
                positions.push(voice.position());
            }
        }

        let steps: Vec<u64> = positions.windows(2).map(|p| p[1] - p[0]).collect();
        for pair in steps.windows(2) {
            assert!(pair[1] < pair[0], "the brake did not slow down: {steps:?}");
        }
        assert!(!voice.is_active(), "the tape must come to rest");
    }

    #[test]
    fn two_voices_of_one_cell_run_independently() {
        let buffer = dc_buffer(300_000);
        let mut slow_attack = spec(0, 300_000);
        slow_attack.modulation = ModulationSpec::new(
            [
                EnvelopeDefinition {
                    attack_ms: 200.0,
                    ..Default::default()
                },
                EnvelopeDefinition::default(),
            ],
            [LfoDefinition::default(); LFO_COUNT],
            &default_routes(),
        );

        let mut first = Voice::default();
        start(&mut first, slow_attack);
        for _ in 0..8_000 {
            first.next_frame(&buffer);
        }

        let mut second = Voice::default();
        second.start(60, 1.0, 1, slow_attack, slow_attack, SAMPLE_RATE, TEMPO);

        let early = first.next_frame(&buffer).0;
        let late = second.next_frame(&buffer).0;

        assert!(
            early > late * 4.0,
            "the second voice must start from zero: {early} vs {late}"
        );
    }

    #[test]
    fn interpolation_lands_between_the_neighbouring_samples() {
        let buffer = SampleBuffer::new(vec![vec![0.0, 1.0]], 48_000);

        let (left, _) = interpolated(&buffer, 0.5);

        assert!((left - 0.5).abs() < 1e-6);
    }

    #[test]
    fn output_stays_finite_across_the_rate_range() {
        let buffer = ramp_buffer(10_000);

        for rate in [0.0625f32, 0.5, 1.0, 2.0, 16.0] {
            let mut voice = Voice::default();
            start(
                &mut voice,
                CellSpec {
                    rate,
                    ..spec(0, 10_000)
                },
            );

            for _ in 0..5_000 {
                let (left, right) = voice.next_frame(&buffer);
                assert!(left.is_finite() && right.is_finite(), "rate {rate}");
                assert!(left.abs() <= 1.5, "rate {rate} produced {left}");
            }
        }
    }

    #[test]
    fn kill_silences_without_release() {
        let buffer = dc_buffer(100);
        let mut voice = Voice::default();
        start(&mut voice, spec(0, 100));

        voice.kill();

        assert!(!voice.is_active());
        assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
    }

    #[test]
    fn reaching_the_end_of_the_buffer_is_silent_rather_than_a_panic() {
        let buffer = dc_buffer(10);
        let mut voice = Voice::default();
        start(&mut voice, spec(1_000, 2_000));

        for _ in 0..64 {
            assert_eq!(voice.next_frame(&buffer), (0.0, 0.0));
        }
    }
}
