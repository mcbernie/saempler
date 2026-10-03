# Realtime rules

The audio callback must stay deterministic. These rules are correctness
requirements, not optimisations.

## Forbidden inside `Plugin::process` and everything it calls

```
locks of any kind
filesystem or network access
logging, println!, format!, String construction
allocation, Vec growth, collection resizing
drops of owned heap data
sleeping or blocking system calls
panics
```

## How the current code satisfies them

| Concern | Mechanism |
| --- | --- |
| Commands from the UI | `rtrb` SPSC queue, drained with `Consumer::pop` |
| Values to the UI | `AtomicU32` in `Meters`, `Relaxed` ordering |
| Voice storage | `[Voice; MAX_VOICES]`, `Copy`, allocated at construction |
| Sample access | borrowed `Arc<SampleBuffer>`, read-only, never resized |
| Retiring a buffer | handed back through a second queue, dropped elsewhere |
| Parameter smoothing | nih-plug smoother advanced in `process`, passed as two endpoints |
| Channel access | `Buffer::as_slice` plus `split_first_mut`, no indexing that can panic |

`Engine::render` writes through `&mut [f32]` slices the host owns. It never
grows anything and never drops an owned value.

## Never drop a sample buffer on the audio thread

Replacing the loaded sample would release the previous `Arc<SampleBuffer>` in
the processing callback, freeing megabytes through the allocator.

`Engine::apply_commands` therefore peeks at each command first. A command that
would retire the current buffer is only popped when the disposal queue has a
free slot; otherwise it stays queued and is retried in a later block. The
retired buffer is pushed to that queue and dropped by the GUI thread or the
import task.

## Verification

`saempler-plugin` enables nih-plug's `assert_process_allocs` feature. In debug
builds this aborts when the processing function allocates. Running the
standalone exercises it:

```bash
cargo run -p saempler-plugin --bin saempler -- --backend dummy
```

The dummy backend calls the processing callback on a timer with silent
buffers, so the assertion is active even with no audio device present.

For cross-thread behaviour, `crates/saempler-audio/tests/thread_handover.rs`
moves the queue producer and the engine onto separate threads and checks that
commands arrive and meters are published.

## Where work belongs instead

```
sample decoding        background thread
waveform peak building background thread
project loading        UI/main thread
slice editing          UI/main thread
```

The engine only ever receives data that is already decoded and preallocated.
