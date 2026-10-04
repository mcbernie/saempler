# Sämpler

[![CI](https://github.com/mcbernie/saempler/actions/workflows/ci.yml/badge.svg)](https://github.com/mcbernie/saempler/actions/workflows/ci.yml)
[![Release](https://github.com/mcbernie/saempler/actions/workflows/release.yml/badge.svg)](https://github.com/mcbernie/saempler/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/mcbernie/saempler?sort=semver&label=release)](https://github.com/mcbernie/saempler/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/mcbernie/saempler/total?label=downloads)](https://github.com/mcbernie/saempler/releases)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Formats](https://img.shields.io/badge/formats-VST3%20%7C%20CLAP%20%7C%20standalone-1af0e6)](#building)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-c8f4f0)](#building)
[![Sponsor](https://img.shields.io/github/sponsors/mcbernie?label=sponsor&logo=githubsponsors&color=ff2e7e)](https://github.com/sponsors/mcbernie)

A playable remix and vocal chop instrument, written in Rust.

> **The design is not settled yet.** How it looks and how it plays are both
> still open, and feedback is wanted while changing them is still cheap.
> [Open an issue](https://github.com/mcbernie/saempler/issues/new).

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

![The Sämpler interface: a waveform cut into sixteen slices, the chops laid out
as pads on the left, and the selected cell with its playback, modulation and
effect settings on the right.](website/screenshot-main.png)

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
- modifier keys from C2: reverse, stutter, repeat, half-time and brake, plus
  four that throw everything into a send driven far past its own settings,
  each in hold, toggle or one-shot mode, on freely chosen notes
- an option to keep chops off the black keys, so a run of them lines up with
  the scale under the hand
- modifiers take effect on notes already sounding, so a phrase can be turned
  round, stuttered and braked while it plays
- polyphonic playback with a playhead per voice on the waveform and the pads
- master gain as a host parameter; sample reference, slices, cells and the
  modifier layout as versioned plugin state, reloaded when a project is opened

- a filter and a saturator per cell, and four sends shared by every voice:
  delay with tempo sync, reverb, phaser and flanger. How much of a cell
  reaches each send is the cell's own setting, so one chop can be soaked in
  reverb while the next one beside it stays dry
- every setting editable while a note is sounding, heard on that note

Not there yet: transposition without changing the length, regions, host
automation of the cell controls, presets and gestures.

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

Dual licensed. Pick whichever applies to you:

- **GPL-3.0-or-later** for the source, in [LICENSE](LICENSE). Use it, change
  it, pass it on; a modified copy you distribute has to stay under the GPL.
  Music you make with it is yours, the licence covers the code.
- **A commercial licence** on request, for building it into something whose
  source stays closed.

The GPL is not a preference but a requirement: Steinberg's VST3 SDK is itself
dual licensed, and without an agreement with Steinberg anything shipping VST3
has to be GPL-3. [LICENSING.md](LICENSING.md) has the details and the list of
third-party components.

## Releases

Tagging `v*` builds every platform, assembles the Windows and macOS installers
and opens a draft release. See [docs/releasing.md](docs/releasing.md) for the
version scheme and what signing would still take.

## Website

The page in [website/](website/) is plain HTML and CSS, published to GitHub
Pages by [a workflow](.github/workflows/pages.yml) whenever it changes.
