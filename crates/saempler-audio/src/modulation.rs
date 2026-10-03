use saempler_model::{
    Division, EnvelopeDefinition, LfoDefinition, LfoShape, ModDestination, ModSource,
    ModulationRoute, DESTINATION_COUNT, ENVELOPE_COUNT, LFO_COUNT, MAX_ROUTES,
};

/// How far a full-amount route to pitch transposes, in semitones.
pub const PITCH_RANGE_SEMITONES: f32 = 24.0;

/// A route in the form the engine reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteSpec {
    pub source: ModSource,
    pub destination: ModDestination,
    pub amount: f32,
}

/// The modulation of one cell, flattened into fixed storage.
///
/// `Copy` and allocation free, so it can travel through the command queue and
/// sit in a voice without the audio thread ever touching the allocator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModulationSpec {
    pub envelopes: [EnvelopeDefinition; ENVELOPE_COUNT],
    pub lfos: [LfoDefinition; LFO_COUNT],
    pub routes: [Option<RouteSpec>; MAX_ROUTES],
}

impl Default for ModulationSpec {
    fn default() -> Self {
        // The amplitude route is part of the default, mirroring the model.
        // Volume reaches a voice only through the matrix, so a spec built by
        // hand - an audition of a region, for instance - would otherwise be
        // silent.
        let mut routes = [None; MAX_ROUTES];
        routes[0] = Some(RouteSpec {
            source: ModSource::EnvelopeA,
            destination: ModDestination::Volume,
            amount: 1.0,
        });

        Self {
            envelopes: [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            lfos: [LfoDefinition::default(); LFO_COUNT],
            routes,
        }
    }
}

impl ModulationSpec {
    /// Build from the definitions a cell carries.
    ///
    /// Routes beyond [`MAX_ROUTES`] are dropped rather than reaching the audio
    /// thread as a variable-length list.
    pub fn new(
        envelopes: [EnvelopeDefinition; ENVELOPE_COUNT],
        lfos: [LfoDefinition; LFO_COUNT],
        routes: &[ModulationRoute],
    ) -> Self {
        let mut spec = Self {
            envelopes,
            lfos,
            routes: [None; MAX_ROUTES],
        };
        for (slot, route) in spec.routes.iter_mut().zip(routes) {
            *slot = Some(RouteSpec {
                source: route.source,
                destination: route.destination,
                amount: route.amount,
            });
        }
        spec
    }
}

/// Stage of a four stage envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// A running envelope.
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeState {
    stage: Stage,
    level: f32,
    attack_step: f32,
    decay_step: f32,
    sustain: f32,
    release_step: f32,
}

impl Default for EnvelopeState {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            attack_step: 1.0,
            decay_step: 1.0,
            sustain: 1.0,
            release_step: 1.0,
        }
    }
}

impl EnvelopeState {
    /// Start from zero with the stage lengths `definition` asks for.
    ///
    /// `scale` shortens every stage by the same factor, used when the envelope
    /// would otherwise outlast the audio it is shaping.
    pub fn start(&mut self, definition: EnvelopeDefinition, sample_rate: f32, scale: f32) {
        let frames = |ms: f32| (ms / 1000.0 * sample_rate * scale).max(0.0);

        self.stage = Stage::Attack;
        self.level = 0.0;
        self.sustain = definition.sustain.clamp(0.0, 1.0);
        self.attack_step = step(frames(definition.attack_ms));
        self.decay_step = step(frames(definition.decay_ms));
        self.release_step = step(frames(definition.release_ms));
    }

    /// Move into the release stage.
    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Stop at once.
    pub fn kill(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    /// Whether the envelope still has anything to give.
    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Advance by one frame and return the new level.
    ///
    /// `release_floor` forces a steeper release when the audio is about to run
    /// out, so a fade can always finish where it has to.
    pub fn next(&mut self, release_floor: f32) -> f32 {
        match self.stage {
            Stage::Idle => return 0.0,
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level -= self.decay_step;
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level -= self.release_step.max(release_floor);
                if self.level <= 0.0 {
                    self.kill();
                    return 0.0;
                }
            }
        }

        self.level
    }

    /// Output frames a full release would take.
    pub fn release_frames(&self) -> f64 {
        if self.release_step <= 0.0 {
            0.0
        } else {
            (1.0 / self.release_step) as f64
        }
    }
}

/// Increment that traverses the whole range in `frames`.
fn step(frames: f32) -> f32 {
    if frames >= 1.0 {
        1.0 / frames
    } else {
        1.0
    }
}

/// A running low frequency oscillator.
#[derive(Debug, Clone, Copy, Default)]
pub struct LfoState {
    /// Normalized phase in `[0, 1)`.
    phase: f32,
    phase_delta: f32,
    /// Value held by sample and hold until the next cycle.
    held: f32,
    /// Simple deterministic noise, so a rendered take is reproducible.
    seed: u32,
}

impl LfoState {
    /// Prepare for a new note.
    pub fn start(&mut self, definition: LfoDefinition, sample_rate: f32, tempo: f64, seed: u32) {
        if definition.retrigger {
            self.phase = 0.0;
        }
        self.seed = seed | 1;
        self.held = self.next_random();
        self.phase_delta = phase_delta(definition, sample_rate, tempo);
    }

    /// Keep the rate current without disturbing the phase.
    pub fn retune(&mut self, definition: LfoDefinition, sample_rate: f32, tempo: f64) {
        self.phase_delta = phase_delta(definition, sample_rate, tempo);
    }

    /// Advance by one frame and return the output, from -1 to 1.
    pub fn next(&mut self, shape: LfoShape) -> f32 {
        let value = match shape {
            LfoShape::Sine => (self.phase * std::f32::consts::TAU).sin(),
            LfoShape::Triangle => 1.0 - (self.phase * 4.0 - 1.0).abs().min(3.0 - self.phase * 4.0),
            LfoShape::Saw => self.phase * 2.0 - 1.0,
            LfoShape::ReverseSaw => 1.0 - self.phase * 2.0,
            LfoShape::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SampleHold => self.held,
        };

        self.phase += self.phase_delta;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            // A new value at the top of every cycle.
            self.held = self.next_random();
        }

        value.clamp(-1.0, 1.0)
    }

    /// Next value of a small deterministic generator, from -1 to 1.
    fn next_random(&mut self) -> f32 {
        // Xorshift: cheap, allocation free and repeatable, which matters when
        // the same project is rendered twice.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Phase advance per frame for an LFO.
fn phase_delta(definition: LfoDefinition, sample_rate: f32, tempo: f64) -> f32 {
    let hz = if definition.sync {
        division_hz(definition.division, tempo)
    } else {
        definition.rate_hz.clamp(0.01, 50.0)
    };

    (hz / sample_rate.max(1.0)).clamp(0.0, 0.5)
}

/// Cycles per second of one `division` at `tempo`.
pub fn division_hz(division: Division, tempo: f64) -> f32 {
    let tempo = if tempo.is_finite() && tempo > 1.0 {
        tempo
    } else {
        120.0
    };
    // A whole note is four beats in common time.
    let seconds = (240.0 / tempo) as f32 * division.whole_notes();
    if seconds <= 0.0 {
        1.0
    } else {
        1.0 / seconds
    }
}

/// How much each destination is being modulated this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModulationFrame {
    amounts: [f32; DESTINATION_COUNT],
}

impl ModulationFrame {
    pub fn get(&self, destination: ModDestination) -> f32 {
        self.amounts[destination.index()]
    }

    fn add(&mut self, destination: ModDestination, value: f32) {
        self.amounts[destination.index()] += value;
    }
}

/// Everything modulating one voice.
#[derive(Debug, Clone, Copy, Default)]
pub struct Modulation {
    spec: ModulationSpec,
    envelopes: [EnvelopeState; ENVELOPE_COUNT],
    lfos: [LfoState; LFO_COUNT],
    velocity: f32,
}

impl Modulation {
    /// Start every source for a new note.
    pub fn start(
        &mut self,
        spec: ModulationSpec,
        velocity: f32,
        sample_rate: f32,
        tempo: f64,
        envelope_scale: f32,
        seed: u32,
    ) {
        self.spec = spec;
        self.velocity = velocity.clamp(0.0, 1.0);

        for (index, envelope) in self.envelopes.iter_mut().enumerate() {
            envelope.start(spec.envelopes[index], sample_rate, envelope_scale);
        }
        for (index, lfo) in self.lfos.iter_mut().enumerate() {
            lfo.start(
                spec.lfos[index],
                sample_rate,
                tempo,
                seed.wrapping_mul(index as u32 + 1)
                    .wrapping_add(0x9E37_79B9),
            );
        }
    }

    /// Follow a tempo change without restarting anything.
    pub fn retune(&mut self, sample_rate: f32, tempo: f64) {
        for (index, lfo) in self.lfos.iter_mut().enumerate() {
            lfo.retune(self.spec.lfos[index], sample_rate, tempo);
        }
    }

    /// Release every envelope.
    pub fn release(&mut self) {
        for envelope in &mut self.envelopes {
            envelope.release();
        }
    }

    pub fn kill(&mut self) {
        for envelope in &mut self.envelopes {
            envelope.kill();
        }
    }

    /// Whether any envelope is still running.
    pub fn is_active(&self) -> bool {
        self.envelopes.iter().any(EnvelopeState::is_active)
    }

    /// Output frames the longest release would take.
    pub fn release_frames(&self) -> f64 {
        self.envelopes
            .iter()
            .map(EnvelopeState::release_frames)
            .fold(0.0, f64::max)
    }

    /// Advance every source by a frame and sum the routes.
    pub fn next(&mut self, release_floor: f32) -> ModulationFrame {
        let mut sources = [0.0f32; 5];
        sources[source_index(ModSource::EnvelopeA)] = self.envelopes[0].next(release_floor);
        sources[source_index(ModSource::EnvelopeB)] = self.envelopes[1].next(release_floor);
        sources[source_index(ModSource::Lfo1)] = self.lfos[0].next(self.spec.lfos[0].shape);
        sources[source_index(ModSource::Lfo2)] = self.lfos[1].next(self.spec.lfos[1].shape);
        sources[source_index(ModSource::Velocity)] = self.velocity;

        let mut frame = ModulationFrame::default();
        for route in self.spec.routes.iter().flatten() {
            frame.add(
                route.destination,
                sources[source_index(route.source)] * route.amount,
            );
        }

        frame
    }
}

fn source_index(source: ModSource) -> usize {
    match source {
        ModSource::EnvelopeA => 0,
        ModSource::EnvelopeB => 1,
        ModSource::Lfo1 => 2,
        ModSource::Lfo2 => 3,
        ModSource::Velocity => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn amp_spec() -> ModulationSpec {
        ModulationSpec::new(
            [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            [LfoDefinition::default(); LFO_COUNT],
            &saempler_model::default_routes(),
        )
    }

    #[test]
    fn an_envelope_rises_holds_and_falls() {
        let mut envelope = EnvelopeState::default();
        envelope.start(
            EnvelopeDefinition {
                attack_ms: 10.0,
                decay_ms: 10.0,
                sustain: 0.5,
                release_ms: 10.0,
            },
            SAMPLE_RATE,
            1.0,
        );

        // Attack: rising from zero.
        let early = envelope.next(0.0);
        for _ in 0..480 {
            envelope.next(0.0);
        }
        assert!(envelope.level > early);

        // Decay into sustain.
        for _ in 0..1_000 {
            envelope.next(0.0);
        }
        assert!((envelope.level - 0.5).abs() < 1e-3, "{}", envelope.level);

        envelope.release();
        for _ in 0..1_000 {
            envelope.next(0.0);
        }
        assert!(!envelope.is_active());
    }

    #[test]
    fn a_full_sustain_skips_straight_past_the_decay() {
        let mut envelope = EnvelopeState::default();
        envelope.start(
            EnvelopeDefinition {
                attack_ms: 1.0,
                decay_ms: 500.0,
                sustain: 1.0,
                release_ms: 10.0,
            },
            SAMPLE_RATE,
            1.0,
        );

        for _ in 0..100 {
            envelope.next(0.0);
        }

        assert!((envelope.level - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_release_floor_forces_a_faster_fade() {
        let mut slow = EnvelopeState::default();
        slow.start(
            EnvelopeDefinition {
                attack_ms: 0.0,
                release_ms: 10_000.0,
                ..Default::default()
            },
            SAMPLE_RATE,
            1.0,
        );
        for _ in 0..10 {
            slow.next(0.0);
        }
        slow.release();

        for _ in 0..200 {
            slow.next(0.01);
        }

        assert!(!slow.is_active(), "the floor must be able to cut it short");
    }

    #[test]
    fn scaling_shortens_every_stage() {
        let mut envelope = EnvelopeState::default();
        envelope.start(
            EnvelopeDefinition {
                attack_ms: 100.0,
                ..Default::default()
            },
            SAMPLE_RATE,
            0.1,
        );

        // A tenth of 100 ms is 10 ms, so 480 frames open it fully.
        for _ in 0..600 {
            envelope.next(0.0);
        }

        assert!((envelope.level - 1.0).abs() < 1e-3);
    }

    #[test]
    fn every_lfo_shape_stays_inside_its_range() {
        for shape in LfoShape::ALL {
            let mut lfo = LfoState::default();
            lfo.start(
                LfoDefinition {
                    shape,
                    rate_hz: 7.0,
                    ..Default::default()
                },
                SAMPLE_RATE,
                120.0,
                42,
            );

            for _ in 0..48_000 {
                let value = lfo.next(shape);
                assert!(value.is_finite(), "{shape:?} produced {value}");
                assert!((-1.0..=1.0).contains(&value), "{shape:?} produced {value}");
            }
        }
    }

    #[test]
    fn a_sine_completes_the_cycles_its_rate_asks_for() {
        let mut lfo = LfoState::default();
        lfo.start(
            LfoDefinition {
                shape: LfoShape::Sine,
                rate_hz: 1.0,
                ..Default::default()
            },
            SAMPLE_RATE,
            120.0,
            1,
        );

        let mut crossings = 0;
        let mut previous = lfo.next(LfoShape::Sine);
        for _ in 0..48_000 {
            let current = lfo.next(LfoShape::Sine);
            if previous < 0.0 && current >= 0.0 {
                crossings += 1;
            }
            previous = current;
        }

        assert_eq!(crossings, 1, "one hertz is one cycle per second");
    }

    #[test]
    fn a_synced_lfo_follows_the_tempo() {
        let slow = division_hz(Division::OneBar, 120.0);
        let fast = division_hz(Division::Sixteenth, 120.0);

        // A bar at 120 bpm is two seconds.
        assert!((slow - 0.5).abs() < 1e-4, "{slow}");
        assert!(fast > slow);
        assert!(division_hz(Division::Quarter, 240.0) > division_hz(Division::Quarter, 120.0));
    }

    #[test]
    fn sample_and_hold_changes_only_at_the_cycle() {
        let mut lfo = LfoState::default();
        lfo.start(
            LfoDefinition {
                shape: LfoShape::SampleHold,
                rate_hz: 1.0,
                ..Default::default()
            },
            SAMPLE_RATE,
            120.0,
            7,
        );

        let first = lfo.next(LfoShape::SampleHold);
        for _ in 0..1_000 {
            assert_eq!(lfo.next(LfoShape::SampleHold), first);
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_sequence() {
        let mut a = LfoState::default();
        let mut b = LfoState::default();
        let definition = LfoDefinition {
            shape: LfoShape::SampleHold,
            rate_hz: 50.0,
            ..Default::default()
        };
        a.start(definition, SAMPLE_RATE, 120.0, 99);
        b.start(definition, SAMPLE_RATE, 120.0, 99);

        for _ in 0..10_000 {
            assert_eq!(
                a.next(LfoShape::SampleHold),
                b.next(LfoShape::SampleHold),
                "a rendered take must come out the same twice"
            );
        }
    }

    #[test]
    fn the_default_matrix_sends_envelope_a_to_the_volume() {
        let mut modulation = Modulation::default();
        modulation.start(amp_spec(), 1.0, SAMPLE_RATE, 120.0, 1.0, 1);

        for _ in 0..1_000 {
            modulation.next(0.0);
        }
        let frame = modulation.next(0.0);

        assert!(frame.get(ModDestination::Volume) > 0.9);
        assert_eq!(frame.get(ModDestination::Pitch), 0.0);
    }

    #[test]
    fn several_routes_to_one_destination_add_up() {
        let spec = ModulationSpec::new(
            [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            [LfoDefinition::default(); LFO_COUNT],
            &[
                ModulationRoute {
                    source: ModSource::Velocity,
                    destination: ModDestination::Volume,
                    amount: 0.5,
                },
                ModulationRoute {
                    source: ModSource::EnvelopeA,
                    destination: ModDestination::Volume,
                    amount: 0.5,
                },
            ],
        );
        let mut modulation = Modulation::default();
        modulation.start(spec, 1.0, SAMPLE_RATE, 120.0, 1.0, 1);

        for _ in 0..1_000 {
            modulation.next(0.0);
        }
        let frame = modulation.next(0.0);

        assert!((frame.get(ModDestination::Volume) - 1.0).abs() < 0.01);
    }

    #[test]
    fn velocity_reaches_the_matrix_unchanged() {
        let spec = ModulationSpec::new(
            [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            [LfoDefinition::default(); LFO_COUNT],
            &[ModulationRoute {
                source: ModSource::Velocity,
                destination: ModDestination::Pan,
                amount: 1.0,
            }],
        );
        let mut modulation = Modulation::default();
        modulation.start(spec, 0.25, SAMPLE_RATE, 120.0, 1.0, 1);

        let frame = modulation.next(0.0);

        assert!((frame.get(ModDestination::Pan) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn routes_beyond_the_ceiling_are_dropped_rather_than_allocated() {
        let many: Vec<ModulationRoute> = (0..MAX_ROUTES + 4)
            .map(|_| ModulationRoute::default())
            .collect();

        let spec = ModulationSpec::new(
            [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            [LfoDefinition::default(); LFO_COUNT],
            &many,
        );

        assert_eq!(spec.routes.iter().flatten().count(), MAX_ROUTES);
    }

    #[test]
    fn an_empty_matrix_modulates_nothing() {
        let spec = ModulationSpec::new(
            [EnvelopeDefinition::default(); ENVELOPE_COUNT],
            [LfoDefinition::default(); LFO_COUNT],
            &[],
        );
        let mut modulation = Modulation::default();
        modulation.start(spec, 1.0, SAMPLE_RATE, 120.0, 1.0, 1);

        let frame = modulation.next(0.0);

        for destination in ModDestination::ALL {
            assert_eq!(frame.get(destination), 0.0);
        }
    }
}
