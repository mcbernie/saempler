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

`Meters` stores peak levels, the voice count and one playback position per
voice in plain atomics. Floats are stored as bit patterns in `AtomicU32`.
Readers may see a value one block stale, which is acceptable for metering and
avoids any synchronisation on the audio thread.

There is a position per voice rather than one for the newest, so that notes
played together each show their own playhead.

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

Zoomed in closer than the cache's finest level, the widget reads the decoded
buffer directly instead. The interface therefore holds its own `Arc` to the
sample; that reference is what keeps a detailed view possible without asking
the audio thread for anything.

## Slice boundaries are shared

A boundary is identified by its frame position, not by a slice and an edge.
Moving it moves every slice that starts or ends there, so dragging a marker in
an evenly divided sample keeps the divisions adjacent instead of opening a gap.

The target is clamped so that no affected slice can collapse to nothing.

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

## Performance cells

A cell is a slice on a MIDI note together with how that note plays it. Several
cells may reference one slice, which is the whole point: one chop, performed
many ways.

A note holds exactly one cell, so assigning to an occupied note takes that cell
over. Removing a slice removes the cells that played it, because a cell without
a slice is a note that silently does nothing.

The engine holds a fixed `[Option<CellSpec>; 128]` indexed by note, so a note on
is one index rather than a lookup. The interface pushes the whole mapping again
after any edit: 128 notes plus the clearing command fit in one queue, and the
mapping only changes on a user action, so tracking differences would buy
nothing.

`CellSpec` is the flattened form: the slice is already resolved to frame
bounds, and speed and pitch are already folded into one read rate. The audio
thread does no arithmetic that could have been done ahead of it.

## Speed and pitch are the same control underneath

Both change how fast the slice is read, so transposing a cell up also shortens
it. Separating them needs time stretching, which does not exist yet.

The read position is fractional and the samples either side are mixed linearly.
Linear interpolation dulls the top end and adds a little distortion at large
transpositions; a better interpolator is the first thing `saempler-dsp` will
hold when it exists.

## Modifier notes

A modifier key makes no sound. It changes how the *next* performance note
behaves, which is what turns the keyboard into a performance surface rather
than a set of fixed playback buttons.

A note is either a modifier or a performance cell, never both: the engine
checks the modifier table first, so a mistaken mapping cannot make a modifier
audible, and the project refuses to put a cell on a modifier key. Laying slices
out across the keyboard steps over the modifier keys rather than losing those
slices to keys they could never be played from.

The layout is editable: any modifier on any free note, several notes carrying
the same modifier in different modes, and the starting layout a button away.

The three modes differ only in when the state clears:

```text
Hold      while the key is down
Toggle    until the key is pressed again
OneShot   until a performance note has actually used it
```

One shot is consumed by the trigger rather than by the key release. That is
what makes it mean "the next note" however long that takes.

## Modifiers reach notes already sounding

Pressing a modifier retunes every voice that is playing, not only the next one
to start. Engaging a stutter loops the audio under the playhead, engaging
reverse turns the voice round on the spot, and engaging a brake slows that
voice to a stop. A brake let go of winds back up to speed rather than snapping.

Each voice keeps the cell as the keyboard maps it alongside its live
parameters, so a modifier engaged and released mid-note is applied to the
untouched cell rather than to whatever the previous one left behind.

An armed one shot is explicitly excluded here: it belongs to a note that has
not been played yet and must not reach into notes already sounding. That is the
difference between `ModifierState::applied` and `applied_live`.

Modifiers are applied to the flattened `CellSpec` at trigger time, so a cell
is never modified in place and the same cell can be played plainly and
modified in the same breath. The state itself is two fixed arrays of booleans,
so reading it on the audio thread costs an index.

## Musical lengths

Stutter, repeat and brake work in note values, so they need the host tempo.
`Plugin::process` writes it into the engine once per block, which is a plain
field store.

Stutter and repeat are deliberately one mechanism — a loop taken from the
trigger point — at two lengths, a sixteenth and an eighth. Two separate
implementations of the same loop would be two places to get it wrong.

## Voice model

Voices are a fixed array of 16 preallocated `Voice` values. A note that finds
no free slot steals the oldest voice. Nothing in the voice path allocates or
drops owned data, so the audio thread never touches the allocator.

A voice never reads past its own slice. The fade out is started early enough
to finish at the boundary, because reading on would mix the neighbouring chop
into the tail and would show a playhead running past the region it plays.

An envelope longer than its slice is scaled down in proportion rather than
truncated, so a long release on a short chop fades across all of it instead of
silencing the voice on its first frame.

## Loop regions

A loop is an interval in source frames, not a point plus a direction. Turning a
voice round inside a loop has to reverse it; anchored to a point, the playhead
would travel away from the anchor, never meet the wrap condition and read
itself out of the slice into silence.

An audition has no key to let go of, so the engine plays it in gate mode
whatever the cell says. A looping cell previewed while it is being set up would
otherwise sound until something else stopped it.

## Playback modes

Gate, one shot, loop, repeat and collapse are one mechanism at heart: a loop
taken from a point, at a length. What differs is where the length comes from
and whether it shrinks.

A modifier loop wins over the cell's own. The modifier is a gesture made while
playing and should be heard over a setting.

The loop is always clamped to what is left of the slice from its start point,
because reading past the slice edge would mix the neighbouring chop into the
tail. A collapse has a floor as well: without one it would shrink until the
read position no longer moves.

A release trigger turns the key coming up into the start of the effect rather
than the end of the note. The loop then runs for as long as the release stage
of whatever is routed to volume.

## Effect routing

Filter and saturation run per voice. They are cheap, and they belong to the
chop rather than to the mix: two cells on the same slice should be able to
sound different, and that only works if the filter is part of the voice.

Delay, reverb, phaser and flanger are sends behind the mixer, one set shared
by every voice. Sixteen voices would otherwise mean sixteen reverbs, which is
the cost of the whole instrument again for a difference nobody can hear under
a beat. Each voice contributes to a send in proportion to its own send amount,
so one chop can be soaked while the next stays dry.

The sends run every frame whether or not anything is feeding them, because a
reverb tail has to carry on after the last voice has stopped.

## Editing while playing

An edit to a cell reaches the voices already sounding it, not only the next
note. A chop instrument is played and adjusted at the same time, and a change
that is only heard on the next note is not a change anyone can make by ear.

Nothing restarts to do it. Envelopes take new stage lengths and carry on from
where they are, LFOs keep their phase, filters keep their state, and the
playhead does not move. Resetting any of those would click on every knob
movement.

## DSP primitives

`saempler-dsp` sits below the engine and knows nothing about slices, cells or
the project. A filter can therefore be tested on its own numbers rather than
by ear through a voice, which is the whole reason for the separation.

Everything is realtime safe once prepared: buffers are taken in `prepare`,
which runs when the host sets the sample rate, and no `process` call
allocates, locks or panics. A setting that arrives as NaN is repaired rather
than obeyed, and any state that has gone wrong is cleared rather than left to
circulate.

Two filter topologies, for two different jobs. The voices use a state variable
filter because their cutoff is a modulation destination and changing a
biquad's coefficients per frame makes it ring and blow up. The equalizer uses
biquads because its bands are set when a knob moves, and the cookbook shelves
have no state variable equivalent as simple.

The planned routing: filter and saturation per voice, where they are cheap and
belong to the chop; delay, phaser and reverb as sends behind the mixer, so
sixteen voices do not mean sixteen reverbs; the equalizer once on the sum and
once per cell.

## Window layout

Every band of the window is given its rectangle before anything is drawn in
it, and a panel fills the rectangle it was handed. Laid out by flow instead, a
panel that measures a few points too tall pushes the ones after it off the
window, and an empty project lays out differently from a full one; both were
visible as panels clipped at the right and bottom edges.

The minimum window size is therefore the size the layout actually needs, not a
guess. Extra height goes to the two middle columns. The pad grid shrinks its
pads to fit rather than scrolling: a key that is mapped but out of sight is
worse than a small one.

## Showing the engine running

The meters publish where the newest voice's modulation stands: both envelope
levels, both LFO outputs and the amount reaching each destination. The audio
thread stores them as atomics once per block, not per frame, because the
interface redraws far more slowly than the audio runs.

One voice rather than all of them. Averaging several would produce a reading
that matches none of the notes being played, and the newest is the one the
player just triggered.

With that, a panel lamp lights while its module is doing something, a marker
rides the envelope and LFO curves, and a knob whose destination is being
modulated shows a second arc running out from where it is set. None of it is
recomputed in the interface: a second copy of the modulation would be a second
place for it to be wrong.

An audition is held for two seconds and then released, so a loop, a repeat and
a collapse can all be heard doing what they do from a click on a pad, and
still end.

## Envelope stages

Each stage covers the distance it actually travels, so a stage takes the time
it was given. Scaling every step by the full range instead made a decay to a
high sustain finish early, and a release from a low level finish early too.

With the decay at zero there is no stage for a peak to fall from, so the
attack rises to the sustain level directly. Otherwise the envelope spends one
frame at full scale on its way down, which is a click rather than a stage, and
the drawn shape shows it as a spike that should not be there.

The release step is worked out when the key comes up rather than at the start,
because a key let go during the attack releases from wherever the level had
got to and should still take the time it was given.

## Modulation

A cell owns two envelopes, two LFOs and a list of routes. None of the sources
is tied to a parameter; a route names a source, a destination and an amount,
and the engine sums the routes that share a destination.

Volume is a destination like any other. A new cell is given one route, envelope
A to volume, so it has an amplitude envelope, but that route can be edited or
removed like the rest. Nothing reaching volume means a silent cell, which the
interface says out loud rather than leaving to be discovered.

The engine holds the whole thing in a `Copy` value: fixed arrays of envelope
and LFO definitions and `[Option<RouteSpec>; MAX_ROUTES]`. A cell therefore
crosses to the audio thread without allocating, and a voice evaluates its
modulation per frame out of preallocated state.

A looping voice never approaches the slice edge, so it plays until the key is
released. A braking voice ends when its rate reaches zero.

## Time representation

Positions are frames (`u64`) everywhere: slice bounds, playback position,
sample length. Seconds exist only where a value is shown to the user.
