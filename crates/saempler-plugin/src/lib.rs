//! Host integration for Sämpler.
//!
//! This crate contains no DSP and no drawing. It owns the parameters, wires
//! the realtime engine to the host's audio callback, hands the editor to
//! [`saempler_ui`], and exports the VST3, CLAP and standalone targets.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use nih_plug_egui::create_egui_editor;
use saempler_audio::{
    command_queue, disposal_queue, CellSpec, CommandProducer, DisposalConsumer, Engine,
    EngineCommand, Meters,
};
use saempler_core::{cell_spec, load_sample};
use saempler_model::{Modifier, ModifierMode};
use saempler_ui::{SampleView, ViewState};

mod params;

pub use params::SaemplerParams;

/// Upper bound for a processing sub-block.
///
/// Blocks are also cut short at note events, so this only caps how long the
/// master gain ramp may run before it is recomputed.
const MAX_BLOCK_SIZE: usize = 64;

/// File types offered by the import dialog.
const AUDIO_EXTENSIONS: [&str; 7] = ["wav", "flac", "mp3", "ogg", "oga", "aiff", "aif"];

/// Work that must not happen on the audio or GUI thread.
pub enum Task {
    /// Ask the user for a file and import it, starting a fresh set of slices.
    ///
    /// The dialog is opened here rather than from the editor on purpose. A
    /// modal dialog runs its own message loop, and opening one from inside the
    /// editor callback re-enters the window procedure while the window handler
    /// is still borrowed, which aborts the process.
    PickAndImport,
    /// Restore the sample a saved project refers to, keeping its slices.
    Restore(PathBuf),
}

pub struct Saempler {
    params: Arc<SaemplerParams>,
    engine: Engine,
    meters: Arc<Meters>,
    /// Shared with the editor and the import task. Locked off the audio
    /// thread only; the audio thread holds the consuming end and never waits
    /// on this mutex.
    commands: Arc<Mutex<CommandProducer>>,
    /// Buffers the engine has retired. Drained from the editor and after an
    /// import, so that freeing them never happens on the audio thread.
    disposal: Arc<Mutex<DisposalConsumer>>,
    /// Peaks and import status for the interface.
    sample_view: Arc<Mutex<SampleView>>,
}

impl Default for Saempler {
    fn default() -> Self {
        let (command_producer, command_consumer) = command_queue();
        let (disposal_producer, disposal_consumer) = disposal_queue();
        let meters = Arc::new(Meters::new());

        Self {
            params: Arc::new(SaemplerParams::default()),
            engine: Engine::new(command_consumer, disposal_producer, Arc::clone(&meters)),
            meters,
            commands: Arc::new(Mutex::new(command_producer)),
            disposal: Arc::new(Mutex::new(disposal_consumer)),
            sample_view: Arc::new(Mutex::new(SampleView::default())),
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
    type BackgroundTask = Task;

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn task_executor(&mut self) -> TaskExecutor<Self> {
        let project = self.params.project.clone();
        let sample_view = Arc::clone(&self.sample_view);
        let commands = Arc::clone(&self.commands);
        let disposal = Arc::clone(&self.disposal);

        Box::new(move |task| {
            let (path, keep_slices) = match task {
                Task::PickAndImport => match pick_audio_file() {
                    Some(path) => (path, false),
                    None => {
                        // Cancelled: release the button again and change nothing.
                        if let Ok(mut view) = sample_view.lock() {
                            view.loading = false;
                        }
                        return;
                    }
                },
                Task::Restore(path) => (path, true),
            };

            let result = load_sample(&path);

            // Freeing whatever the engine retired for the previous sample
            // belongs here, on a thread that is allowed to call the allocator.
            drain_disposal(&disposal);

            let mut view = match sample_view.lock() {
                Ok(view) => view,
                Err(_) => return,
            };
            view.loading = false;

            let loaded = match result {
                Ok(loaded) => loaded,
                Err(error) => {
                    nih_error!("Sample konnte nicht geladen werden: {error}");
                    view.status = Some(error.to_string());
                    return;
                }
            };
            view.status = None;
            view.peaks = loaded.peaks;
            view.buffer = Some(Arc::clone(&loaded.buffer));
            view.reset_view();

            // The keyboard mapping is rebuilt from the project: a restored
            // project brings its own cells, a fresh import has none yet.
            let mut specs: Vec<(u8, CellSpec)> = Vec::new();
            let mut modifiers: Vec<(u8, (Modifier, ModifierMode))> = Vec::new();
            {
                let mut project = match project.lock() {
                    Ok(project) => project,
                    Err(_) => return,
                };
                if keep_slices {
                    project.project.sample = Some(loaded.source);
                } else {
                    project.project.set_sample(Some(loaded.source));
                }
                for cell in project.project.cells() {
                    if let Some(spec) = cell_spec(&project.project, cell) {
                        specs.push((cell.midi_note, spec));
                    }
                }
                for entry in project.project.modifiers() {
                    modifiers.push((entry.note, (entry.modifier, entry.mode)));
                }
            }

            if let Ok(mut commands) = commands.lock() {
                let _ = commands.push(EngineCommand::SetSample(loaded.buffer));
                let _ = commands.push(EngineCommand::ClearCells);
                for (note, spec) in specs {
                    let _ = commands.push(EngineCommand::SetCell {
                        note,
                        spec: Some(spec),
                    });
                }
                let _ = commands.push(EngineCommand::ClearModifiers);
                for (note, assignment) in modifiers {
                    let _ = commands.push(EngineCommand::SetModifier {
                        note,
                        assignment: Some(assignment),
                    });
                }
            }
        })
    }

    fn editor(&mut self, async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let meters = Arc::clone(&self.meters);
        let commands = Arc::clone(&self.commands);
        let disposal = Arc::clone(&self.disposal);
        let sample_view = Arc::clone(&self.sample_view);

        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                // The GUI thread is the one place that reliably runs while the
                // plugin is in use, so retired buffers are freed from here.
                drain_disposal(&disposal);

                let import_requested = saempler_ui::draw(
                    egui_ctx,
                    setter,
                    &ViewState {
                        project: &params.project,
                        sample: &sample_view,
                        commands: &commands,
                        meters: &meters,
                        gain: &params.gain,
                    },
                );

                if import_requested {
                    // The flag is set before the task starts so the button
                    // stays disabled while the dialog is open, which prevents
                    // a second dialog from being requested.
                    if let Ok(mut view) = sample_view.lock() {
                        view.loading = true;
                        view.status = None;
                    }
                    async_executor.execute_background(Task::PickAndImport);
                }
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        // Migration happens here rather than during deserialization because
        // this is the first point at which a load failure can be reported to
        // the host by refusing to initialize.
        let restore = match self.params.project.lock() {
            Ok(mut project) => match project.migrate() {
                Ok(()) => project.project.sample.as_ref().map(|s| s.path.clone()),
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

        // The modifier layout exists before any sample does, so it is pushed
        // here rather than from the import task alone.
        if let Ok(project) = self.params.project.lock() {
            if let Ok(mut commands) = self.commands.lock() {
                let _ = commands.push(EngineCommand::ClearModifiers);
                for entry in project.project.modifiers() {
                    let _ = commands.push(EngineCommand::SetModifier {
                        note: entry.note,
                        assignment: Some((entry.modifier, entry.mode)),
                    });
                }
            }
        }

        // A saved project only stores the path to its sample, so the audio has
        // to be decoded again. `execute` runs the task on this thread and
        // returns when it is done, which keeps offline rendering correct.
        // `initialize` may run more than once, so an already loaded buffer is
        // not decoded a second time.
        if let Some(path) = restore {
            if !self.engine.has_sample() {
                context.execute(Task::Restore(path));
            }
        }

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
        // Stutter and brake work in musical lengths, so the engine needs to
        // know the tempo. A plain field write, cheap enough for every block.
        if let Some(tempo) = context.transport().tempo {
            self.engine.set_tempo(tempo);
        }

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
///
/// Every note currently triggers the selected slice. Mapping individual notes
/// to their own slices is what performance cells introduce.
fn apply_note_event(engine: &mut Engine, event: &NoteEvent<()>) {
    match *event {
        NoteEvent::NoteOn { note, velocity, .. } => engine.note_on(note, velocity),
        // A choke ends the note as hard as the engine currently can, which is
        // the same release path as a note off until voices gain a fast mute.
        NoteEvent::NoteOff { note, .. } | NoteEvent::Choke { note, .. } => engine.note_off(note),
        _ => {}
    }
}

/// Drop every buffer the engine has handed back.
///
/// Must only be called from a thread that may block and allocate.
fn drain_disposal(disposal: &Mutex<DisposalConsumer>) {
    let Ok(mut disposal) = disposal.lock() else {
        return;
    };
    while disposal.pop().is_ok() {}
}

/// Ask the user for an audio file.
///
/// Called from the background task thread, never from the editor callback;
/// see [`Task::PickAndImport`].
fn pick_audio_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Sample laden")
        .add_filter("Audio", &AUDIO_EXTENSIONS)
        .pick_file()
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
    use saempler_audio::{SampleBuffer, SliceBounds};

    const SAMPLE_RATE: f32 = 48_000.0;

    /// An engine with a one second sample and the whole of it selected.
    fn loaded_engine() -> Engine {
        let (mut commands, command_consumer) = command_queue();
        let (disposal_producer, _disposal) = disposal_queue();
        let mut engine = Engine::new(command_consumer, disposal_producer, Arc::new(Meters::new()));
        engine.prepare(SAMPLE_RATE);

        let frames = 48_000;
        commands
            .push(EngineCommand::SetSample(Arc::new(SampleBuffer::new(
                vec![vec![1.0; frames], vec![1.0; frames]],
                48_000,
            ))))
            .expect("the queue has capacity");
        // The notes these tests play all need a cell, otherwise a note on
        // would be silent for a reason that has nothing to do with mapping.
        for note in [60u8, 64, 72] {
            commands
                .push(EngineCommand::SetCell {
                    note,
                    spec: Some(CellSpec {
                        bounds: SliceBounds {
                            start_frame: 0,
                            end_frame: frames as u64,
                        },
                        ..CellSpec::default()
                    }),
                })
                .expect("the queue has capacity");
        }
        engine.apply_commands();
        // The producer is dropped here on purpose: these tests only cover the
        // note event mapping.
        engine
    }

    fn note_on(note: u8) -> NoteEvent<()> {
        NoteEvent::NoteOn {
            timing: 0,
            voice_id: None,
            channel: 0,
            note,
            velocity: 1.0,
        }
    }

    fn render(engine: &mut Engine, frames: usize) {
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        engine.render(&mut left, &mut right, 1.0, 1.0);
    }

    #[test]
    fn note_on_starts_a_voice() {
        let mut engine = loaded_engine();

        apply_note_event(&mut engine, &note_on(60));

        assert_eq!(engine.active_voices(), 1);
    }

    #[test]
    fn note_off_releases_the_matching_note_only() {
        let mut engine = loaded_engine();
        apply_note_event(&mut engine, &note_on(60));
        apply_note_event(&mut engine, &note_on(64));

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
        let mut engine = loaded_engine();
        apply_note_event(&mut engine, &note_on(72));

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
        let mut engine = loaded_engine();
        apply_note_event(&mut engine, &note_on(60));

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

    /// Write a minimal 16-bit stereo PCM WAV and return its path.
    ///
    /// The same helper exists in `saempler-core`'s loader tests. Sharing it
    /// would mean a test-only crate for twenty lines, so both copies stay.
    fn write_test_wav(name: &str, frames: usize) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("saempler-plugin-{name}-{unique}.wav"));

        let data_len = (frames * 4) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for _ in 0..frames {
            bytes.extend_from_slice(&i16::MAX.to_le_bytes());
            bytes.extend_from_slice(&i16::MAX.to_le_bytes());
        }

        std::fs::write(&path, &bytes).expect("the temp directory must be writable");
        path
    }

    struct TempFile(PathBuf);

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn restoring_a_project_reloads_its_sample_and_keeps_the_slices() {
        let path = write_test_wav("restore", 48_000);
        let _cleanup = TempFile(path.clone());

        let mut plugin = Saempler::default();
        // Stand in for state the host restored: a sample reference, the slices
        // the user had made, and the notes they were mapped to.
        {
            let mut project = plugin.params.project.lock().expect("fresh mutex");
            project.project.sample = Some(saempler_model::SampleRef {
                path: path.clone(),
                frames: 48_000,
                sample_rate: 48_000,
                channels: 2,
            });
            let first = project.project.add_slice(0, 24_000);
            let second = project.project.add_slice(24_000, 48_000);
            assert_ne!(first, second);
            project.project.assign(60, first);
            let cell = project.project.assign(62, second).expect("slice exists");
            project.project.set_playback(
                cell,
                saempler_model::PlaybackSettings {
                    reverse: true,
                    ..Default::default()
                },
            );
        }

        let executor = plugin.task_executor();
        executor(Task::Restore(path.clone()));

        // The interface received peaks to draw.
        {
            let view = plugin.sample_view.lock().expect("fresh mutex");
            assert_eq!(view.status, None, "a valid file must not report an error");
            assert!(!view.loading);
            assert_eq!(view.peaks.frames(), 48_000);
        }

        // The project kept its slices and its note mapping.
        {
            let project = plugin.params.project.lock().expect("fresh mutex");
            assert_eq!(project.project.slices().len(), 2);
            assert_eq!(project.project.cells().len(), 2);
        }

        // The engine received the audio and the whole keyboard mapping.
        plugin.engine.apply_commands();
        assert!(plugin.engine.has_sample());
        assert_eq!(plugin.engine.mapped_notes(), 2);

        let first = plugin.engine.cell(60).expect("note 60 was restored");
        assert_eq!(first.bounds.start_frame, 0);
        assert_eq!(first.bounds.end_frame, 24_000);
        assert!(!first.reverse);

        let second = plugin.engine.cell(62).expect("note 62 was restored");
        assert_eq!(second.bounds.start_frame, 24_000);
        assert!(second.reverse, "the cell settings came back too");

        plugin.engine.note_on(60, 1.0);
        assert_eq!(plugin.engine.active_voices(), 1);
        plugin.engine.note_on(61, 1.0);
        assert_eq!(
            plugin.engine.active_voices(),
            1,
            "an unmapped note must stay silent"
        );
    }

    #[test]
    fn a_failed_import_reports_the_error_and_changes_nothing() {
        let mut plugin = Saempler::default();

        let executor = plugin.task_executor();
        executor(Task::Restore(PathBuf::from("gibt-es-nicht.wav")));

        let view = plugin.sample_view.lock().expect("fresh mutex");
        assert!(view.status.is_some(), "the failure must be shown");
        assert!(!view.loading);
        assert_eq!(view.peaks.frames(), 0);

        drop(view);
        plugin.engine.apply_commands();
        assert!(!plugin.engine.has_sample());
    }

    #[test]
    fn every_offered_extension_is_lowercase_and_without_a_dot() {
        for extension in AUDIO_EXTENSIONS {
            assert!(
                !extension.starts_with('.'),
                "{extension} must not have a dot"
            );
            assert_eq!(extension, extension.to_lowercase());
        }
    }
}
