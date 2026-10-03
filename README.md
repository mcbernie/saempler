# Sämpler

A playable remix and vocal chop instrument, written in Rust.

The idea is a separation that most slicers do not make:

```
Source Sample -> Slice -> Performance Cell -> Transforms / Effects
```

A slice can be used any number of times. Several MIDI notes may point at the
same slice while playing it back differently:

```
C3  -> Slice 4 -> normal
C#3 -> Slice 4 -> reverse
D3  -> Slice 4 -> half-time
D#3 -> Slice 4 -> stutter 1/16
```

Modifier notes change how the next trigger behaves rather than producing sound
themselves.

## Status

Phase 5 of the plan: every cell carries its own modulation.

What works today:

- VST3, CLAP and standalone builds
- sample import for WAV, AIFF, FLAC, MP3 and OGG, decoded off the audio thread
- waveform with zoom, panning and a detailed view that reads the audio itself
- slices: equal divisions, split by double click, drag or remove the markers
- performance cells: slices laid out across the keyboard, several notes able to
  share one slice, each with its own reverse, speed, pitch and gain
- playback modes per cell: gate, one shot, loop, tempo-synced repeat and a
  collapse whose loop shrinks on every pass, each able to start at the note off
  instead of the note on
- one screen: the sample with the key every chop plays on across the top, the
  pads down the left, and the editor beside them, so what is being played and
  what is being edited are never on separate pages
- panel lamps, lit pads and a readout of the notes and slices currently
  sounding, so the state of a performance is readable at a glance
- the modulation shown running: markers riding the envelope and LFO curves and
  a second arc on every knob the engine is moving
- two envelopes and two LFOs per cell, free running or locked to the host
  tempo, none of them wired to a fixed parameter
- a modulation matrix of up to eight routes, from envelopes, LFOs or velocity
  to volume, pan, pitch, playback rate or loop length; the amplitude envelope
  is one of those routes rather than a special case
- modifier keys below the playing range: reverse, stutter, repeat, half-time
  and brake, each in hold, toggle or one-shot mode, on freely chosen notes
- modifiers take effect on notes already sounding, so a phrase can be turned
  round, stuttered and braked while it plays
- polyphonic playback with a playhead per voice on the waveform and the pads
- master gain as a host parameter; sample reference, slices, cells and the
  modifier layout as versioned plugin state, reloaded when a project is opened

The effect primitives exist and are tested on their own numbers; wiring them
into the voices and the mixer is the next increment.

Not there yet: regions, host automation of the cell controls, presets and
gestures.

See `docs/architecture.md` for the decisions behind this and
`docs/realtime.md` for the rules the audio thread follows.

## Building

Tasks run through [`just`](https://just.systems); `just` on its own lists every
recipe.

```bash
just verify     # format, compile, lint and test
just run        # start the standalone build
just devices    # list the audio and MIDI devices it can use
just bundle     # write the VST3 and CLAP bundles to target/bundled
just install    # copy them into this platform's plug-in folders
```

Audio device settings differ per machine and live in a local `.env` file; copy
`.env.example` and adjust it. [docs/howto.md](docs/howto.md) covers the setup,
the device quirks and how to load the plugin in a DAW.

## Layout

```
crates/saempler-model    serializable project state
crates/saempler-dsp      filters, saturation, delay, phaser, reverb, equalizer
crates/saempler-audio    realtime engine, voices, sample buffers, queues
crates/saempler-core     sample import and the waveform peak cache
crates/saempler-ui       theme, custom widgets, the four editor pages
crates/saempler-plugin   host parameters and the VST3/CLAP/standalone exports
xtask                    bundler entry point
```

## Licensing

NIH-plug's VST3 bindings are GPLv3, so any VST3 build of this plugin has to
comply with the GPLv3. The manifests declare `GPL-3.0-or-later`; a `LICENSE`
file still needs to be added.
