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

```
Sample laden …                load a WAV, AIFF, FLAC, MP3 or OGG file
4 / 8 / 16 / 32               divide the sample into that many equal slices
click a slice                 select it; notes play the selection
double click inside a slice   split it there, placing a marker by hand
drag a marker                 move that boundary, taking the neighbour with it
Slice löschen                 remove the selected slice
play notes                    the selected slice sounds, polyphonically
```

Navigating the waveform:

```
mouse wheel                   zoom around the pointer
shift + wheel                 shift the view sideways
hold right button and drag    shift the view sideways
```

Zoomed in past about 256 frames per pixel the waveform stops reading the peak
cache and draws the audio itself; closer still, individual samples get a dot.
A pink line marks the frame the engine is playing.

The pads:

```
lay out                     put every chop across the keyboard from C3
click a pad                 select it and play it once
drag a pad onto another     the two swap keys
drag a pad onto a gap       it moves there
right click a pad           take the cell off that key
duplicate                   copy the selected cell to the first free key
play that key               the cell sounds, polyphonically
```

The key a cell plays on is also a list on the PLAYBACK panel, for when the
key you want is not on screen.

The sample lives on the panel at the top, loading and slicing included, and
stays there whatever else is open. Every chop carries a chip with the key that
plays it, in the chop's own colour; the same colour marks its pad and the
editor's header, so one glance connects all three. Clicking a chop or a pad
selects the same thing everywhere and plays it once.

The editor's header row steps through the chops with its arrows and gives the
selected one's position in the sample in seconds.

The CELL page edits the selected pad. Playback holds reverse, speed, pitch and
gain; speed and pitch both change the read rate, so a transposed cell is also
shorter.

```
Gate        plays while the key is held
One Shot    plays the slice to its end, whatever the key does
Loop        repeats the whole slice
Repeat      repeats the chosen note value, locked to the host tempo
Collapse    like repeat, with the loop shrinking on every pass
```

Release Trigger starts the loop when the key comes up rather than when it goes
down, so a phrase plays through and then collapses as it fades. Duplicating a cell and changing only the copy is the quickest way to
the idea the instrument is built on.

Below that sit two envelopes, two LFOs and the modulation matrix. None of the
four modules is wired to anything by itself: a route in the matrix decides what
it changes and by how much. A new cell starts with one route, ENV A to volume,
which is the amplitude envelope. Take it out and the cell falls silent, and the
page says so.

```
cycle a selector    left click steps forwards, right click steps back
drag an amount      the bar fills from zero, either way for an LFO
Sync                locks an LFO to the host tempo and swaps hertz for notes
Retrigger           restarts the LFO phase with every note
```

Nothing scrolls. Every band of the window has a fixed height and the two
middle columns take what is left, so the picture holds still whether the
project is empty or full. The window cannot be dragged below the size the
layout needs; making it bigger hands the extra room to the pads and the
editor. Too many keys to fit are drawn smaller rather than hidden.

The toolbars carry drawn marks rather than words. Resting the pointer on one
says what it does.

Everything is on one screen: the sample across the top, the pads on the
left, the editor on the right, and a footer with the modifier keys and the
output strip. The modifier cards in the footer only show what each key does;
the button beside their legend opens the window they are configured in.

The modifier keys, by default an octave below the pads:

```
C1   Reverse      play the slice backwards
D1   Stutter      loop a sixteenth from the trigger point
E1   Repeat       loop an eighth from the trigger point
F1   Half-Time    read at half speed
G1   Brake        slow to a stop over one whole note
```

Click a modifier pad to cycle its mode:

```
Hold       in effect while the key is down
Toggle     in effect until the key is pressed again
One Shot   in effect for the next performance note, then cleared
```

Modifiers work on notes that are already sounding. Hold a chop, press reverse
and it turns round where it is; press stutter and it loops under the playhead;
press brake and it slows to a stop. Let the brake go and it winds back up.

On the MODIFIERS page every key can be changed: drag a note to move it, right
click it to remove the row, pick what it does and how it responds, or add
another row. The same modifier may sit on several keys with different modes —
stutter held on one and armed as a one shot on the next.

Stutter, repeat and brake follow the host tempo. In the standalone that is the
`--tempo` option, which defaults to 120.

The gain knob:

```
drag                          level changes          (host parameter -> engine)
shift + drag                  fine adjustment
double click                  back to the default
```

The line under the waveform shows the slice count, the selected slice with its
length in frames, and the frame under the pointer. Import errors replace it.

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

Load a sample, divide it into 8 slices, select one, save the Live Set, restart
Live and reload the Set. The waveform, the slices and the selection must come
back.

Only the *path* of the sample is stored, not the audio: the file is decoded
again when the project loads. Moving or deleting the file afterwards therefore
breaks the restore, and the error appears under the waveform.

## What you are actually testing right now

This is phase 5: every cell carries two envelopes, two LFOs and a matrix that
decides what they reach. Try an LFO on the playback rate of one cell while the
cell next to it, on the same slice, has none.

The modifier keys still change the performance as it happens, both for the next
note and for the notes already ringing. Play a chop, and while it sounds press
reverse, then stutter, then brake. That sequence is the instrument.

The advanced playback modes, regions and effects come next.
