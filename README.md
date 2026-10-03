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

Phase 3 of the plan: slices sit on notes, and each note plays its slice its
own way.

What works today:

- VST3, CLAP and standalone builds
- sample import for WAV, AIFF, FLAC, MP3 and OGG, decoded off the audio thread
- waveform with zoom, panning and a detailed view that reads the audio itself
- slices: equal divisions, split by double click, drag or remove the markers
- performance cells: slices laid out across the keyboard, several notes able to
  share one slice, each with its own reverse, speed, pitch, gain and envelope
- polyphonic playback with a playhead shown on the waveform and the pads
- master gain as a host parameter; sample reference, slices and cells as
  versioned plugin state, reloaded when a project is opened

Not there yet: modifier notes, stutter, tape stop and effects.

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
crates/saempler-audio    realtime engine, voices, sample buffers, queues
crates/saempler-core     sample import and the waveform peak cache
crates/saempler-ui       theme, custom widgets, screens
crates/saempler-plugin   host parameters and the VST3/CLAP/standalone exports
xtask                    bundler entry point
```

## Licensing

NIH-plug's VST3 bindings are GPLv3, so any VST3 build of this plugin has to
comply with the GPLv3. The manifests declare `GPL-3.0-or-later`; a `LICENSE`
file still needs to be added.
