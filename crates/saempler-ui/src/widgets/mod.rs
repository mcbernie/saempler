//! Custom audio widgets drawn with `egui::Painter`.
//!
//! Default egui widgets are only used for layout; everything the user reads a
//! value from or performs with is drawn here.

mod knob;
mod meter;
mod segmented;

pub use knob::knob;
pub use meter::{readout, stereo_meter};
pub use segmented::{button, segmented};
