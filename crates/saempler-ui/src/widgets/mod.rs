//! Custom audio widgets drawn with `egui::Painter`.
//!
//! Default egui widgets are only used for layout; everything the user reads a
//! value from or performs with is drawn here.

mod curves;
pub(crate) mod dial;
mod dropdown;
mod icon;
mod knob;
mod meter;
mod pad;
pub(crate) mod panel;
mod segmented;
mod selector;
pub(crate) mod surface;
mod switch;
pub(crate) mod texture;
mod value_knob;
mod value_slider;
mod view_range;
pub(crate) mod waveform;

pub use curves::{envelope_display, lfo_display};
pub use dial::{dial, DialState, Placement};
pub use dropdown::{dropdown, labelled_dropdown};
pub use icon::{icon_button, icon_label_button, Icon};
pub use knob::knob;
pub use meter::{readout, stereo_meter};
pub use pad::{performance_pad, PadAction, PadView, MIN_PAD_SIZE, PAD_SIZE};
pub use panel::{
    divider, engraved, header_rule, inset, lamp, legend, metal_panel, panel_header, raised_body,
    screw, vertical_gradient, HEADER_HEIGHT,
};
pub use segmented::{button, segmented, toggle};
pub use selector::rotary_selector;
pub use switch::toggle_switch;
pub use texture::textures;
pub use value_knob::{value_knob, value_knob_beside, KnobSpec, Taper, Unit};
pub use value_slider::{value_slider, SliderSpec};
pub use view_range::{ViewRange, MIN_VISIBLE_FRAMES};
pub use waveform::{slice_color, waveform, WaveformAction, WaveformSource};
