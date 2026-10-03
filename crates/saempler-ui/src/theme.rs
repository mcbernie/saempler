use nih_plug_egui::egui::{Color32, CornerRadius, Stroke};

/// Every visual constant used by the interface.
///
/// Widgets take a `&Theme` instead of reaching for literals so that spacing
/// and colour stay consistent across screens and can be changed in one place.
pub struct Theme {
    pub spacing_sm: f32,
    pub spacing_md: f32,
    pub spacing_lg: f32,

    pub radius_sm: CornerRadius,
    pub radius_md: CornerRadius,

    pub stroke_thin: f32,
    pub stroke_thick: f32,

    pub font_sm: f32,
    pub font_md: f32,
    pub font_lg: f32,

    /// Window background, the darkest surface in the hierarchy.
    pub window_bg: Color32,
    /// Background of a framed section.
    pub panel_bg: Color32,
    /// Lighter upper edge of a panel, the lit side of its bevel.
    pub panel_top: Color32,
    /// Background of an interactive control at rest.
    pub control_bg: Color32,
    /// Lighter upper half of a control at rest.
    pub control_top: Color32,
    /// Background of an interactive control under the pointer.
    pub control_hover_bg: Color32,
    /// Lighter upper half of a control under the pointer.
    pub control_hover_top: Color32,
    /// Background of a control that is held down, or of a recessed track.
    pub control_pressed_bg: Color32,
    /// Background of a control carrying the current value.
    pub control_selected_bg: Color32,
    /// Lighter upper half of a control carrying the current value.
    pub control_selected_top: Color32,
    /// The lit pixel along the top edge that gives a control its relief.
    pub control_highlight: Color32,
    /// Border between surfaces.
    pub outline: Color32,

    pub text: Color32,
    pub text_dim: Color32,

    /// Selection and primary interaction colour.
    pub accent: Color32,
    /// Something armed and waiting, as opposed to in effect.
    pub armed: Color32,
    /// Running/active state.
    pub active: Color32,
    /// Reserved for clipping and errors only.
    pub danger: Color32,

    /// Background behind the waveform.
    pub waveform_bg: Color32,
    /// The waveform trace itself.
    pub waveform: Color32,
    /// Zero line through the middle of the waveform.
    pub waveform_axis: Color32,
    /// A slice boundary marker at rest.
    pub marker: Color32,
    /// The position the engine is playing.
    pub playhead: Color32,
    /// Shading of an unselected slice.
    pub slice_fill: Color32,
    /// Shading of the neighbouring slice, so divisions read without markers.
    pub slice_fill_alternate: Color32,
    /// Shading of the selected slice.
    pub slice_selected_fill: Color32,
    /// Colours cycled through to tell slices apart in lists and pads.
    pub slice_palette: [Color32; 6],
}

impl Theme {
    /// The product's dark theme.
    ///
    /// The greys carry a little warmth rather than being neutral or blue, and
    /// every surface has a lit top edge over a dark outline. That reads as a
    /// panel with depth, the way a performance instrument should, while the
    /// accent colours stay flat and modern.
    pub const fn dark() -> Self {
        Self {
            spacing_sm: 4.0,
            spacing_md: 8.0,
            spacing_lg: 14.0,

            radius_sm: CornerRadius::same(3),
            radius_md: CornerRadius::same(5),

            stroke_thin: 1.0,
            stroke_thick: 2.0,

            font_sm: 11.0,
            font_md: 13.0,
            font_lg: 18.0,

            window_bg: Color32::from_rgb(0x14, 0x13, 0x11),
            panel_bg: Color32::from_rgb(0x21, 0x1f, 0x1c),
            panel_top: Color32::from_rgb(0x28, 0x26, 0x22),
            control_bg: Color32::from_rgb(0x2b, 0x28, 0x24),
            control_top: Color32::from_rgb(0x34, 0x31, 0x2c),
            control_hover_bg: Color32::from_rgb(0x38, 0x34, 0x2e),
            control_hover_top: Color32::from_rgb(0x43, 0x3e, 0x37),
            control_pressed_bg: Color32::from_rgb(0x16, 0x15, 0x13),
            control_selected_bg: Color32::from_rgb(0x1d, 0x32, 0x33),
            control_selected_top: Color32::from_rgb(0x24, 0x3d, 0x3e),
            control_highlight: Color32::from_rgb(0x4e, 0x48, 0x40),
            outline: Color32::from_rgb(0x0d, 0x0c, 0x0b),

            text: Color32::from_rgb(0xe8, 0xe3, 0xd8),
            text_dim: Color32::from_rgb(0x95, 0x8d, 0x80),

            accent: Color32::from_rgb(0x3a, 0xd9, 0xc4),
            armed: Color32::from_rgb(0xf5, 0xa5, 0x24),
            active: Color32::from_rgb(0x63, 0xe2, 0x8a),
            danger: Color32::from_rgb(0xf0, 0x5c, 0x4a),

            waveform_bg: Color32::from_rgb(0x0f, 0x0e, 0x0d),
            waveform: Color32::from_rgb(0x5e, 0xea, 0xd4),
            waveform_axis: Color32::from_rgb(0x33, 0x30, 0x2b),
            marker: Color32::from_rgb(0x6e, 0x67, 0x5c),
            playhead: Color32::from_rgb(0xf5, 0xa5, 0x24),
            // Slice shading sits behind the trace, so it stays very low
            // contrast; the markers carry the actual division.
            slice_fill: Color32::from_rgb(0x16, 0x15, 0x13),
            slice_fill_alternate: Color32::from_rgb(0x1c, 0x1a, 0x18),
            slice_selected_fill: Color32::from_rgb(0x1c, 0x2d, 0x2c),
            slice_palette: [
                Color32::from_rgb(0x3a, 0xd9, 0xc4),
                Color32::from_rgb(0xf4, 0x72, 0xb6),
                Color32::from_rgb(0xf5, 0xa5, 0x24),
                Color32::from_rgb(0x60, 0xa5, 0xfa),
                Color32::from_rgb(0xa7, 0x8b, 0xfa),
                Color32::from_rgb(0x63, 0xe2, 0x8a),
            ],
        }
    }

    /// Outline stroke for a surface at rest.
    pub fn outline_stroke(&self) -> Stroke {
        Stroke::new(self.stroke_thin, self.outline)
    }

    /// Outline stroke for a selected or focused surface.
    pub fn accent_stroke(&self) -> Stroke {
        Stroke::new(self.stroke_thick, self.accent)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
