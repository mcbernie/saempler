//! Verifies the communication between the UI/main thread and the audio thread.
//!
//! The unit tests drive the queues from a single thread. This test moves the
//! producer to another thread and the engine to a third one, so that the `Send`
//! bounds, the atomic handover and the buffer disposal path are exercised the
//! way they run in the plugin.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use saempler_audio::{
    command_queue, disposal_queue, Engine, EngineCommand, Meters, SampleBuffer, SliceBounds,
};

const BLOCK_SIZE: usize = 128;
const SAMPLE_FRAMES: usize = 48_000;

fn dc_sample() -> Arc<SampleBuffer> {
    Arc::new(SampleBuffer::new(
        vec![vec![1.0; SAMPLE_FRAMES], vec![1.0; SAMPLE_FRAMES]],
        48_000,
    ))
}

#[test]
fn commands_meters_and_disposal_cross_the_thread_boundary() {
    let (mut commands, command_consumer) = command_queue();
    let (disposal_producer, mut disposal) = disposal_queue();
    let meters = Arc::new(Meters::new());
    let mut engine = Engine::new(command_consumer, disposal_producer, Arc::clone(&meters));
    engine.prepare(48_000.0);

    let loaded = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));

    // Stands in for the UI thread: sends a sample, then a replacement, and
    // takes retired buffers back so they are freed off the audio thread.
    let editor = {
        let loaded = Arc::clone(&loaded);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            commands
                .push(EngineCommand::SetSample(dc_sample()))
                .expect("the queue has capacity");
            commands
                .push(EngineCommand::SetSlice(SliceBounds {
                    start_frame: 0,
                    end_frame: SAMPLE_FRAMES as u64,
                }))
                .expect("the queue has capacity");
            loaded.store(true, Ordering::Release);

            while !stop.load(Ordering::Acquire) {
                thread::yield_now();
            }

            commands
                .push(EngineCommand::SetSample(dc_sample()))
                .expect("the queue has capacity");

            // The replacement retires the first buffer; receiving it here is
            // what keeps the audio thread out of the allocator.
            let mut retired = 0;
            for _ in 0..1_000_000 {
                if disposal.pop().is_ok() {
                    retired += 1;
                    break;
                }
                thread::yield_now();
            }
            retired
        })
    };

    // Stands in for the audio thread.
    let audio = {
        let meters = Arc::clone(&meters);
        let loaded = Arc::clone(&loaded);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut left = [0.0f32; BLOCK_SIZE];
            let mut right = [0.0f32; BLOCK_SIZE];

            while !loaded.load(Ordering::Acquire) {
                engine.apply_commands();
                engine.render(&mut left, &mut right, 1.0, 1.0);
                thread::yield_now();
            }
            engine.apply_commands();

            engine.note_on(60, 1.0);
            for _ in 0..100 {
                engine.render(&mut left, &mut right, 1.0, 1.0);
                if meters.peaks().0 > 0.5 {
                    break;
                }
            }

            assert!(
                meters.peaks().0 > 0.5,
                "the engine should publish a level once a slice is playing"
            );
            assert_eq!(meters.active_voices(), 1);

            stop.store(true, Ordering::Release);

            // Keep processing so the replacement command is picked up and the
            // retired buffer is handed over.
            for _ in 0..100_000 {
                engine.apply_commands();
                engine.render(&mut left, &mut right, 1.0, 1.0);
            }

            assert!(engine.has_sample());
        })
    };

    audio.join().expect("audio thread must not panic");
    let retired = editor.join().expect("editor thread must not panic");

    assert_eq!(
        retired, 1,
        "the replaced buffer must come back for disposal"
    );
}
