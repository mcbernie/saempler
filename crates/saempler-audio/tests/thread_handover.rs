//! Verifies the communication between the UI/main thread and the audio thread.
//!
//! The unit tests drive the queue and the meters from a single thread. This
//! test moves the producer to another thread and the engine to a third one, so
//! that the `Send` bounds and the atomic handover are actually exercised.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use saempler_audio::{command_queue, Engine, EngineCommand, Meters};
use saempler_model::Waveform;

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK_SIZE: usize = 128;

#[test]
fn commands_and_meters_cross_the_thread_boundary() {
    let (mut producer, consumer) = command_queue();
    let meters = Arc::new(Meters::new());
    let mut engine = Engine::new(consumer, Arc::clone(&meters));
    engine.prepare(SAMPLE_RATE);

    let stop = Arc::new(AtomicBool::new(false));

    // Stands in for the UI thread.
    let editor = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            producer
                .push(EngineCommand::SetWaveform(Waveform::Square))
                .expect("the queue has capacity");
            while !stop.load(Ordering::Acquire) {
                thread::yield_now();
            }
            producer
                .push(EngineCommand::AllNotesOff)
                .expect("the queue has capacity");
        })
    };

    // Stands in for the audio thread.
    let audio = {
        let meters = Arc::clone(&meters);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut left = [0.0f32; BLOCK_SIZE];
            let mut right = [0.0f32; BLOCK_SIZE];

            engine.note_on(69, 1.0);
            // Render until the waveform command has arrived and the note has
            // reached a level the meter can see.
            for _ in 0..100 {
                engine.apply_commands();
                engine.render(&mut left, &mut right, 1.0, 1.0);
                if engine.waveform() == Waveform::Square && meters.peaks().0 > 0.5 {
                    break;
                }
            }

            assert_eq!(engine.waveform(), Waveform::Square);
            assert!(meters.peaks().0 > 0.5, "the engine should publish a level");
            assert_eq!(meters.active_voices(), 1);

            stop.store(true, Ordering::Release);

            // Drain the shutdown command and let the release stage finish.
            for _ in 0..200 {
                engine.apply_commands();
                engine.render(&mut left, &mut right, 1.0, 1.0);
            }

            assert_eq!(engine.active_voices(), 0);
            assert_eq!(meters.active_voices(), 0);
        })
    };

    audio.join().expect("audio thread must not panic");
    editor.join().expect("editor thread must not panic");
}
