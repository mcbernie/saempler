//! Custom audio widgets drawn with `egui::Painter`.
//!
//! Default egui widgets are only used for layout; everything the user reads a
//! value from or performs with is drawn here.

mod curves;
mod dropdown;
mod knob;
mod meter;
mod pad;
mod segmented;
pub(crate) mod surface;
mod tabs;
mod value_knob;
mod value_slider;
mod view_range;
pub(crate) mod waveform;

pub use curves::{envelope_display, lfo_display};
pub use dropdown::dropdown;
pub use knob::knob;
pub use meter::{readout, stereo_meter};
pub use pad::{performance_pad, PadAction, PadView, PAD_SIZE};
pub use segmented::{button, segmented, toggle};
pub use tabs::{led, tab_bar};
pub use value_knob::{value_knob, KnobSpec, Taper, Unit};
pub use value_slider::{value_slider, SliderSpec};
pub use view_range::{ViewRange, MIN_VISIBLE_FRAMES};
pub use waveform::{slice_color, slice_map, waveform, WaveformAction, WaveformSource};
