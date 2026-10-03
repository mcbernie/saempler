//! Custom audio widgets drawn with `egui::Painter`.
//!
//! Default egui widgets are only used for layout; everything the user reads a
//! value from or performs with is drawn here.

mod knob;
mod meter;
mod pad;
mod segmented;
mod surface;
mod value_knob;
mod view_range;
pub(crate) mod waveform;

pub use knob::knob;
pub use meter::{readout, stereo_meter};
pub use pad::{performance_pad, PadAction, PadView, PAD_SIZE};
pub use segmented::{button, segmented};
pub use value_knob::{value_knob, Taper};
pub use view_range::{ViewRange, MIN_VISIBLE_FRAMES};
pub use waveform::{slice_color, waveform, WaveformAction, WaveformSource};
