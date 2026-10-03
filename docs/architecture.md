# Architecture

Short records of decisions that would be expensive to reverse. Each entry
states what was decided, why, and what would make us revisit it.

## Crate layout

```
saempler-model      serializable domain state, no audio, no UI, no host
   ^
saempler-audio      realtime engine, no host and no UI dependencies
   ^
saempler-ui         theme, custom widgets, screens
   ^
saempler-plugin     parameters, VST3/CLAP/standalone exports
```

`saempler-dsp` and `saempler-core` from the original plan do not exist yet. The only
DSP primitive in the project is the test oscillator with a single caller, and
there is no sample import, waveform cache or background processing to
coordinate. Both crates will be created when they have real content:
`saempler-dsp` with the resampling and filter work of phase 2/4, `saempler-core` with
sample import and the waveform cache of phase 2.

## saempler-ui depends on nih_plug

The interface uses nih-plug's `Param` and `ParamSetter` types so the knob can
read, display and write a host parameter directly. The alternative was passing
a callback per control from `saempler-plugin`, which adds an indirection layer
without removing the coupling.

Host parameters stay defined in `saempler-plugin`, and `saempler-ui` receives
individual parameter references. That keeps the dependency acyclic:
`saempler-plugin -> saempler-ui -> nih_plug`.

`saempler-model` and `saempler-audio` remain free of nih-plug, egui, VST3 and CLAP.

## UI to audio communication

A wait-free SPSC queue (`rtrb`) carries `EngineCommand` values from the
UI/main thread to the audio thread. The audio thread drains it once at the
start of every processing block.

The producing end lives behind a `std::sync::Mutex` because the egui editor
closure must be `Sync`. That mutex is only ever locked from the UI thread; the
audio thread holds the consuming end directly and never waits on it. A full
queue drops the command rather than blocking the interface.

Project state is shared the same way: `Arc<Mutex<ProjectFile>>`, locked from
the UI thread, with the derived values reaching the engine through commands.

## Audio to UI communication

`Meters` stores peak levels and the voice count in plain atomics. Floats are
stored as bit patterns in `AtomicU32`. Readers may see a value one block stale,
which is acceptable for metering and avoids any synchronisation on the audio
thread.

## Master gain ramping

The engine does not own parameter smoothers. `Plugin::process` splits the
buffer into sub-blocks, advances nih-plug's smoother over each sub-block, and
passes the start and end gain to `Engine::render`, which interpolates linearly.

This keeps parameter handling in the crate that owns the parameters. It costs
a small deviation from the smoother's exponential curve within a sub-block,
which is capped at 64 samples.

## Host parameters versus project state

Only stable global controls are host parameters; right now that is
`master_gain`. Slice, cell and gesture state belongs in `ProjectFile` and is
stored as plugin state, so an automation lane can never point at a slice that
no longer exists.

`ProjectFile` carries a `version` field from the first release.
`ProjectFile::migrate` runs in `Plugin::initialize`, where a failure can still
be reported to the host by refusing to initialize. Added fields are covered by
serde defaults; structural changes get an explicit migration step.

## Voice model

Voices are a fixed array of 16 preallocated `Voice` values. A note that finds
no free slot steals the oldest voice. Nothing in the voice path allocates or
drops owned data, so the audio thread never touches the allocator.

Per-voice playback state lives in the voice. Shared definitions are never
mutated to represent playback position.

## Time representation

Not yet exercised: the test oscillator has no sample positions. Slice bounds
will use `u64` frame counts when the sample engine lands, with seconds used
only for display.
