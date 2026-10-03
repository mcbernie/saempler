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

Phase 1 of the plan: the technical foundation. There is no sample engine yet.
The plugin currently plays a polyphonic test oscillator so that the audio path,
MIDI input, editor and state handling can be verified end to end.

What works today:

- VST3, CLAP and standalone builds
- egui editor with the product theme and custom widgets
- MIDI note on/off/choke start and release voices
- UI edits reach the engine through a wait-free queue
- the engine publishes peak levels and voice count back to the UI
- master gain as a host parameter, project state as versioned plugin state

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
crates/saempler-audio    realtime engine, voices, command queue, meters
crates/saempler-ui       theme, custom widgets, screens
crates/saempler-plugin   host parameters and the VST3/CLAP/standalone exports
xtask                    bundler entry point
```

## Licensing

NIH-plug's VST3 bindings are GPLv3, so any VST3 build of this plugin has to
comply with the GPLv3. The manifests declare `GPL-3.0-or-later`; a `LICENSE`
file still needs to be added.
