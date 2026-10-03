# How to build, run and test Sämpler

Everything below is driven by [`just`](https://just.systems). Run `just` for
the recipe list.

## Prerequisites

```
Rust stable (1.80 or newer)
just
Git
```

On Windows the recipes run in the POSIX shell from Git for Windows. The
justfile addresses it by full path:

```just
set windows-shell := ["C:/Program Files/Git/bin/sh.exe", "-cu"]
```

This is deliberate. `sh` is normally not on the PATH in PowerShell, and a bare
`bash` resolves to `C:\Windows\System32\bash.exe`, the WSL launcher, which
would run the recipes inside a Linux system with a different toolchain. If Git
is installed somewhere else, change that one line.

## Local configuration

Audio device settings differ per machine and are not committed. Put them in a
`.env` file next to the justfile:

```
SAEMPLER_BACKEND=wasapi
SAEMPLER_PERIOD_SIZE=1056
SAEMPLER_MIDI_INPUT="Arturia MINILAB"
```

Values containing spaces must be quoted, otherwise `just` refuses to parse the
file. `.env` is in `.gitignore`.

Find the right values with:

```bash
just devices
```

That prints the available audio outputs and MIDI inputs. Both commands in the
recipe are expected to fail; the device list is what you are after.

Every setting can also be overridden for a single run, as a variable:

```bash
just period=512 run
just midi="MIDI4x4" run
```

Override them as variables rather than as extra arguments — the standalone
rejects a repeated flag. Extra arguments are for the remaining options:

```bash
just run --tempo 140
```

## Checking your work

```bash
just verify
```

That is formatting, compilation, Clippy with warnings as errors, and the
tests — the chain to run before every commit. The individual steps
are `just fmt-check`, `just check`, `just clippy` and `just test`.

## Running the standalone

```bash
just run
```

This opens the editor window and connects to the configured audio device and
MIDI input.

### The period size has to match your device

NIH-plug's standalone wrapper asserts that the audio backend delivers exactly
the configured number of frames, and panics otherwise:

```
thread 'cpal_wasapi_out' panicked at
'Received 1056 samples, while the configured buffer size is 512'
```

The number in the message is the value your device actually uses. Put it in
`.env` as `SAEMPLER_PERIOD_SIZE`. On this project's development machine (RODECaster
Pro II) that is 1056.

### Do not use the `auto` backend to probe devices

With `--backend auto`, an unknown device name does not produce an error. The
wrapper falls back to the dummy backend and keeps running with no audio. That
is why `just devices` forces the platform's native backend.

### What to try

The editor shows a waveform selector, an "All Notes Off" button, a master gain
knob, a stereo peak meter and the voice count.

```
play notes on the MIDI controller   voice count rises, meter moves, you hear the oscillator
switch the waveform                 the sound changes      (UI -> command queue -> engine)
drag the knob                       level changes          (host parameter -> engine)
shift + drag                        fine adjustment
double click the knob               back to the default
```

### Checking realtime safety

```bash
just run-dummy
```

This runs the debug build, which has NIH-plug's `assert_process_allocs`
feature active: any allocation inside the audio callback aborts the process.
The dummy backend calls the processing function on a timer with silent
buffers, so no audio device is needed. If it keeps running, the audio path did
not allocate.

## Running in a DAW

```bash
just bundle
```

This writes `target/bundled/Sämpler.vst3` and
`target/bundled/Sämpler.clap`.

### Ableton Live (VST3)

Rather than copying after every build, point Live at the build output:

```
Preferences -> Plug-Ins -> VST3 Plug-In Custom Folder
  <repository>/target/bundled
```

Then press Rescan. "Sämpler" appears under **Instruments** — it is an
instrument with MIDI input and stereo output, not an audio effect.

Three things to know:

- **Live must not have the plugin loaded while you rebuild.** Windows locks the
  loaded DLL and `just bundle` fails. Remove the plugin from the Set, or close
  Live, then rebuild and rescan.
- `just clean` deletes `target/bundled`. Bundle again and rescan afterwards.
- Whether Live scans CLAP has not been verified here. Use VST3 for Live; the
  CLAP build is there for hosts that support it.

### Installing system-wide

```bash
just install
```

Copies both bundles into this platform's standard folders:

```
Windows   C:\Program Files\Common Files\{VST3,CLAP}
macOS     ~/Library/Audio/Plug-Ins/{VST3,CLAP}
Linux     ~/.vst3 and ~/.clap
```

On Windows this writes to `Program Files` and therefore needs an elevated
shell. The custom-folder approach above needs no elevation and is the better
loop while developing. `just uninstall` removes the bundles again.

### Checking that state survives

Set the waveform to Square, save the Live Set, restart Live, reload the Set.
The selector must still show Square. That exercises the serialized project
state, including the version field and the migration step that runs when the
plugin initializes.

## What you are actually testing right now

This is phase 1: the technical foundation, with a polyphonic test oscillator
standing in for the sample engine. The point of running it is to confirm that
the foundation holds — plugin loads in the host, MIDI arrives, UI and audio
communicate without locks, state is saved and restored. Sample loading,
slicing and performance cells come in later phases.
