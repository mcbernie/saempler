//! Reusable DSP primitives.
//!
//! Everything here is realtime safe once prepared: no allocation, no locking
//! and no panic path inside a `process` call. Buffers are taken in `prepare`,
//! which the host calls with the sample rate before playback starts.
//!
//! The crate knows nothing about slices, cells or the project. It sits below
//! the engine so that a filter can be tested on its own numbers rather than by
//! ear through a voice.

mod delay;
mod effect;
mod eq;
mod saturation;
mod svf;

pub use delay::{DelayLine, MAX_DELAY_SECONDS};
pub use effect::{Delay, Phaser, Reverb, MAX_FEEDBACK};
pub use eq::{BandKind, BandSetting, Biquad, Equalizer, EQ_BANDS, MAX_BAND_GAIN_DB, MAX_Q, MIN_Q};
pub use saturation::{SaturationKind, Saturator, MAX_DRIVE};
pub use svf::{FilterMode, Svf, MAX_CUTOFF, MAX_RESONANCE, MIN_CUTOFF, MIN_RESONANCE};
