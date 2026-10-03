//! Custom audio widgets drawn with `egui::Painter`.
//!
//! Default egui widgets are only used for layout; everything the user reads a
//! value from or performs with is drawn here.

mod knob;
mod meter;
mod segmented;
mod waveform;

pub use knob::knob;
pub use meter::{readout, stereo_meter};
pub use segmented::{button, segmented};
pub use waveform::{slice_color, waveform, MarkerEdge, WaveformAction};
