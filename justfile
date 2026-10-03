# Sämpler development tasks.
#
# Run `just` for the recipe list.
#
# On Windows the recipes run in the POSIX shell that ships with Git for
# Windows. It is addressed by its full path on purpose: `sh` is usually not on
# the PATH in PowerShell, and a bare `bash` resolves to the WSL launcher in
# C:\Windows\System32, which would run the recipes in a different system.
# Adjust the path below if Git is installed elsewhere.
set windows-shell := ["C:/Program Files/Git/bin/sh.exe", "-cu"]

# Machine-specific audio settings belong in a local `.env` file, which is not
# committed. See docs/howto.md.
set dotenv-load := true
set positional-arguments := true

plugin := "saempler-plugin"
binary := "saempler"

# Standalone audio settings. Override in `.env` or as a variable on the command
# line, for example `just period=1056 run`.
backend := env_var_or_default("SAEMPLER_BACKEND", "auto")
period := env_var_or_default("SAEMPLER_PERIOD_SIZE", "512")
rate := env_var_or_default("SAEMPLER_SAMPLE_RATE", "48000")
midi := env_var_or_default("SAEMPLER_MIDI_INPUT", "")

# `--midi-input` is only passed when a device is configured; the standalone
# treats an empty value as a request to list the available devices.
midi_arg := if midi == "" { "" } else { "--midi-input " + quote(midi) }

# This platform's native backend. `devices` must not use `auto`: on an unknown
# device name `auto` silently falls back to the dummy backend and keeps
# running, instead of reporting the error and exiting.
native_backend := if os() == "windows" { "wasapi" } else if os() == "macos" { "core-audio" } else { "alsa" }

# Standard plug-in folders of this platform.
vst3_dir := if os() == "windows" { "/c/Program Files/Common Files/VST3" } else if os() == "macos" { home_directory() / "Library/Audio/Plug-Ins/VST3" } else { home_directory() / ".vst3" }
clap_dir := if os() == "windows" { "/c/Program Files/Common Files/CLAP" } else if os() == "macos" { home_directory() / "Library/Audio/Plug-Ins/CLAP" } else { home_directory() / ".clap" }

# Show the available recipes.
default:
    @just --list

# --- checks -----------------------------------------------------------------

# The full check chain. Run this before every commit.
verify: fmt-check check clippy test

# Format the workspace.
fmt:
    cargo fmt --all

# Fail if anything is unformatted.
fmt-check:
    cargo fmt --all --check

# Type check the workspace.
check:
    cargo check --workspace

# Lint with warnings treated as errors.
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run all tests.
test:
    cargo test --workspace

# --- running ----------------------------------------------------------------

# Backend, sample rate, period size and MIDI input come from the variables
# above. Override them as variables rather than as arguments, because the
# standalone rejects a repeated flag:
#
#     just midi="MIDI4x4" run
#     just period=512 run
#
# Extra arguments cover the remaining options, e.g. `just run --tempo 140`.
[doc("Run the standalone build; extra arguments are passed through")]
run *ARGS:
    cargo run --release -p {{ plugin }} --bin {{ binary }} -- --backend {{ backend }} --sample-rate {{ rate }} --period-size {{ period }} {{ midi_arg }} "$@"

# No audio device is needed, and NIH-plug's allocation assertions are active in
# this build, so any allocation in the audio callback aborts the process.
[doc("Run the debug standalone with no audio device and allocation assertions on")]
run-dummy *ARGS:
    cargo run -p {{ plugin }} --bin {{ binary }} -- --backend dummy "$@"

# Both commands are expected to fail; the printed device list is the point.
[doc("List the audio output and MIDI input devices the standalone can use")]
devices:
    -@cargo run --release -q -p {{ plugin }} --bin {{ binary }} -- --backend {{ native_backend }} --output-device "?"
    -@cargo run --release -q -p {{ plugin }} --bin {{ binary }} -- --backend {{ native_backend }} --midi-input "?"

# --- bundling ---------------------------------------------------------------

# Build the VST3 and CLAP bundles into target/bundled.
bundle:
    cargo xtask bundle {{ plugin }} --release

[doc("Bundle without optimisations, with allocation assertions active")]
bundle-debug:
    cargo xtask bundle {{ plugin }}

# On Windows this writes to "Program Files" and needs an elevated shell. The
# alternative that needs no elevation is to point the host at target/bundled
# directly; see docs/howto.md.
[doc("Copy the bundles into this platform's plug-in folders")]
install: bundle
    mkdir -p "{{ vst3_dir }}" "{{ clap_dir }}"
    rm -rf "{{ vst3_dir }}/Sämpler.vst3" "{{ clap_dir }}/Sämpler.clap"
    cp -r "target/bundled/Sämpler.vst3" "{{ vst3_dir }}/"
    cp -r "target/bundled/Sämpler.clap" "{{ clap_dir }}/"
    @echo "Installiert nach {{ vst3_dir }} und {{ clap_dir }}"

# Remove the installed bundles again.
uninstall:
    rm -rf "{{ vst3_dir }}/Sämpler.vst3" "{{ clap_dir }}/Sämpler.clap"
    @echo "Entfernt aus {{ vst3_dir }} und {{ clap_dir }}"

# --- housekeeping -----------------------------------------------------------

# Remove all build artifacts, including target/bundled.
clean:
    cargo clean
