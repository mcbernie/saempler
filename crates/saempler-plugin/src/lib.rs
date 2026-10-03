//! Host integration for Sämpler.
//!
//! This crate contains no DSP and no drawing. It owns the parameters, wires
//! the realtime engine to the host's audio callback, hands the editor to
//! [`saempler_ui`], and exports the VST3, CLAP and standalone targets.

use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use nih_plug_egui::create_egui_editor;
use saempler_audio::{command_queue, CommandProducer, Engine, Meters};
use saempler_ui::ViewState;

mod params;

pub use params::SaemplerParams;

/// Upper bound for a processing sub-block.
///
/// Blocks are also cut short at note events, so this only caps how long the
/// master gain ramp may run before it is recomputed.
const MAX_BLOCK_SIZE: usize = 64;

pub struct Saempler {
    params: Arc<SaemplerParams>,
    engine: Engine,
    meters: Arc<Meters>,
    /// Shared with the editor. Locked on the UI thread only; the audio thread
    /// holds the consuming end and never waits on this mutex.
    commands: Arc<Mutex<CommandProducer>>,
}

impl Default for Saempler {
    fn default() -> Self {
        let (producer, consumer) = command_queue();
        let meters = Arc::new(Meters::new());

        Self {
            params: Arc::new(SaemplerParams::default()),
            engine: Engine::new(consumer, Arc::clone(&meters)),
            meters,
            commands: Arc::new(Mutex::new(producer)),
        }
    }
}

impl Plugin for Saempler {
    const NAME: &'static str = "Sämpler";
    const VENDOR: &'static str = "mcbernie";
    const URL: &'static str = env!("CARGO_PKG_REPOSITORY");
    const EMAIL: &'static str = "";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let meters = Arc::clone(&self.meters);
        let commands = Arc::clone(&self.commands);

        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                saempler_ui::draw(
                    egui_ctx,
                    setter,
                    &ViewState {
                        project: &params.project,
                        commands: &commands,
                        meters: &meters,
                        gain: &params.gain,
                    },
                );
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // Migration happens here rather than during deserialization because
        // this is the first point at which a load failure can be reported to
        // the host by refusing to initialize.
        let waveform = match self.params.project.lock() {
            Ok(mut project) => match project.migrate() {
                Ok(()) => project.project.waveform,
                Err(error) => {
                    nih_error!("Projekt konnte nicht geladen werden: {error}");
                    return false;
                }
            },
            Err(_) => {
                nih_error!("Projektzustand ist nicht lesbar");
                return false;
            }
        };

        self.engine.prepare(buffer_config.sample_rate);
        self.engine.set_waveform(waveform);

        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.engine.apply_commands();

        let num_samples = buffer.samples();
        let channels = buffer.as_slice();
        let Some((left, rest)) = channels.split_first_mut() else {
            return ProcessStatus::Normal;
        };
        let Some(right) = rest.first_mut() else {
            return ProcessStatus::Normal;
        };

        let mut next_event = context.next_event();
        let mut block_start = 0usize;
        while block_start < num_samples {
            // Handle every event scheduled for the current position, then cut
            // the block short at the next one so notes land sample-accurately.
            let mut block_end = (block_start + MAX_BLOCK_SIZE).min(num_samples);
            while let Some(event) = next_event {
                let timing = event.timing() as usize;
                if timing > block_start {
                    block_end = block_end.min(timing);
                    break;
                }

                apply_note_event(&mut self.engine, &event);
                next_event = context.next_event();
            }

            let frames = block_end - block_start;
            let gain_start = self.params.gain.smoothed.next();
            let gain_end = if frames > 1 {
                self.params.gain.smoothed.next_step(frames as u32 - 1)
            } else {
                gain_start
            };

            self.engine.render(
                &mut left[block_start..block_end],
                &mut right[block_start..block_end],
                gain_start,
                gain_end,
            );

            block_start = block_end;
        }

        ProcessStatus::Normal
    }
}

/// Translate a host note event into an engine action.
///
/// Kept separate from [`Plugin::process`] so the mapping can be tested without
/// a host. Events the engine does not react to are ignored on purpose.
fn apply_note_event(engine: &mut Engine, event: &NoteEvent<()>) {
    match *event {
        NoteEvent::NoteOn { note, velocity, .. } => engine.note_on(note, velocity),
        // A choke ends the note as hard as the engine currently can, which is
        // the same release path as a note off until voices gain a fast mute.
        NoteEvent::NoteOff { note, .. } | NoteEvent::Choke { note, .. } => engine.note_off(note),
        _ => {}
    }
}

impl ClapPlugin for Saempler {
    const CLAP_ID: &'static str = "de.mcbernie.saempler";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Playable remix and vocal chop instrument");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Sampler,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for Saempler {
    const VST3_CLASS_ID: [u8; 16] = *b"Saempler00000001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Sampler];
}

nih_export_clap!(Saempler);
nih_export_vst3!(Saempler);

#[cfg(test)]
mod tests {
    use super::*;
    use saempler_audio::command_queue;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn engine() -> Engine {
        let (_producer, consumer) = command_queue();
        let mut engine = Engine::new(consumer, Arc::new(Meters::new()));
        engine.prepare(SAMPLE_RATE);
        // The producer is dropped here on purpose: this test only covers the
        // note event mapping, not the command queue.
        engine
    }

    fn render(engine: &mut Engine, frames: usize) {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        engine.render(&mut left, &mut right, 1.0, 1.0);
    }

    #[test]
    fn note_on_starts_a_voice() {
        let mut engine = engine();

        apply_note_event(
            &mut engine,
            &NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 60,
                velocity: 1.0,
            },
        );

        assert_eq!(engine.active_voices(), 1);
    }

    #[test]
    fn note_off_releases_the_matching_note_only() {
        let mut engine = engine();
        for note in [60, 64] {
            apply_note_event(
                &mut engine,
                &NoteEvent::NoteOn {
                    timing: 0,
                    voice_id: None,
                    channel: 0,
                    note,
                    velocity: 1.0,
                },
            );
        }

        apply_note_event(
            &mut engine,
            &NoteEvent::NoteOff {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 60,
                velocity: 0.0,
            },
        );
        render(&mut engine, 4_800);

        assert_eq!(engine.active_voices(), 1);
    }

    #[test]
    fn choke_ends_the_note() {
        let mut engine = engine();
        apply_note_event(
            &mut engine,
            &NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 72,
                velocity: 1.0,
            },
        );

        apply_note_event(
            &mut engine,
            &NoteEvent::Choke {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 72,
            },
        );
        render(&mut engine, 4_800);

        assert_eq!(engine.active_voices(), 0);
    }

    #[test]
    fn unhandled_events_do_not_disturb_sounding_voices() {
        let mut engine = engine();
        apply_note_event(
            &mut engine,
            &NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 60,
                velocity: 1.0,
            },
        );

        apply_note_event(
            &mut engine,
            &NoteEvent::PolyPressure {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 60,
                pressure: 0.5,
            },
        );

        assert_eq!(engine.active_voices(), 1);
    }
}
