# Architecture

Short records of decisions that would be expensive to reverse. Each entry
states what was decided, why, and what would make us revisit it.

## Crate layout

```
saempler-model      serializable domain state, no audio, no UI, no host
   ^
saempler-audio      realtime engine and sample buffers, no host, no UI
   ^
saempler-core       sample import and waveform peaks, no realtime work
   ^
saempler-ui         theme, custom widgets, screens
   ^
saempler-plugin     parameters, VST3/CLAP/standalone exports
```

`saempler-dsp` from the original plan does not exist yet. The only shared DSP
would be the voice envelope, which has one caller. It will be created when
resampling, pitch and filters arrive and there is more than one consumer.

## saempler-ui depends on nih_plug

The interface uses nih-plug's `Param` and `ParamSetter` types so the knob can
read, display and write a host parameter directly. The alternative was passing
a callback per control from `saempler-plugin`, which adds an indirection layer
without removing the coupling.

Host parameters stay defined in `saempler-plugin`, and `saempler-ui` receives
individual parameter references. That keeps the dependency acyclic:
`saempler-plugin -> saempler-ui -> nih_plug`.

`saempler-model`, `saempler-audio` and `saempler-core` remain free of nih-plug,
egui, VST3 and CLAP.

## Sample buffer ownership

A decoded sample lives in a `SampleBuffer` behind an `Arc`, built once on a
background thread and never mutated. The audio thread holds one reference and
voices read through it. Slices are frame ranges into that buffer, so assigning
the same audio to many slices copies nothing.

The buffer type lives in `saempler-audio` rather than `saempler-core`, because
the engine is what plays it; `saempler-core` depends on `saempler-audio` to
build one.

## Retiring a sample buffer

Replacing the sample would drop the previous `Arc` on the audio thread, and
with it several megabytes, inside the processing callback.

Instead, retired buffers travel back through a second wait-free queue and are
dropped by the GUI thread or the import task. Before the engine applies a
command that would retire a buffer, it checks that a slot is free; if not, the
command stays in the queue and is retried in a later block. Playback continues
unaffected in the meantime.

This costs one extra queue and one check per command. The alternative — hoping
the UI still holds a reference — depends on timing the engine cannot observe.

## UI to audio communication

A wait-free SPSC queue (`rtrb`) carries `EngineCommand` values from the
UI/main thread to the audio thread. The audio thread drains it once at the
start of every processing block.

The producing end lives behind a `std::sync::Mutex` because the egui editor
closure must be `Sync`, and because the import task writes to it from a
background thread. That mutex is never locked by the audio thread, which holds
the consuming end directly. A full queue drops the command rather than
blocking the interface.

Project state is shared the same way: `Arc<Mutex<ProjectFile>>`.

## Audio to UI communication

`Meters` stores peak levels and the voice count in plain atomics. Floats are
stored as bit patterns in `AtomicU32`. Readers may see a value one block stale,
which is acceptable for metering and avoids any synchronisation on the audio
thread.

## Sample import

Decoding runs in a nih-plug background task, never in `process` or a draw call.
The task decodes the file, builds the peak cache, updates the project and
pushes the new buffer into the command queue.

The file dialog is opened from that same background task rather than from the
editor callback. A modal dialog runs its own message loop; opening one from
inside the editor callback re-enters baseview's window procedure while its
window handler is still mutably borrowed, which aborts the process.

Restoring a saved project reuses the same task through
`InitContext::execute`, which runs it synchronously so that offline rendering
cannot start before the audio is available.

## Waveform peaks

`PeakCache` holds a pyramid of minimum/maximum pairs: 256 frames per peak at
level 0, halving resolution per level. The widget picks the coarsest level that
still has a peak per pixel, so drawing cost scales with the width of the view
rather than the length of the audio.

The cache is built once per import, on the background thread.

## Host parameters versus project state

Only stable global controls are host parameters; right now that is
`master_gain`. Sample reference, slices and selection belong in `ProjectFile`
and are stored as plugin state, so an automation lane can never point at a
slice that no longer exists.

A project stores the *path* of its sample, never the audio. Reloading decodes
the file again. That keeps project state small, at the cost of breaking when
the file moves; a later phase can add a fallback.

`ProjectFile` carries a `version` field from the first release.
`ProjectFile::migrate` runs in `Plugin::initialize`, where a failure can still
be reported to the host by refusing to initialize. Added fields are covered by
serde defaults, removed ones by serde ignoring unknown keys; structural changes
get an explicit migration step.

## Slice identity

Slices carry a `SliceId` newtype, handed out by the project and never reused
within a session, so that a selection — or a future performance cell — refers
to a slice rather than to a position in a list.

There is no `SampleId` yet: a project has exactly one source sample, which is
what the product is built around. It arrives when more than one does.

## Note to slice mapping

Every incoming note currently triggers the selected slice. Mapping individual
notes to their own slices, with their own playback settings, is what
performance cells introduce; a provisional mapping now would be replaced
immediately.

## Voice model

Voices are a fixed array of 16 preallocated `Voice` values. A note that finds
no free slot steals the oldest voice. Nothing in the voice path allocates or
drops owned data, so the audio thread never touches the allocator.

A voice reaching the end of its slice releases rather than stopping, and the
envelope fades out over the audio that follows the slice. Cutting at the
boundary would click.

## Time representation

Positions are frames (`u64`) everywhere: slice bounds, playback position,
sample length. Seconds exist only where a value is shown to the user.
