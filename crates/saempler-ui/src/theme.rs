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

    /// Height of a key, a list or a button, so a row of mixed controls lines
    /// up.
    pub control_height: f32,

    /// Window background, seen only at the very edge of the chassis.
    pub window_bg: Color32,
    /// Lit top of the brushed metal a panel is milled from.
    pub chassis_top: Color32,
    /// The metal across the middle of a panel, where the light falls off.
    pub chassis_mid: Color32,
    /// Shaded bottom of that metal.
    pub chassis_bottom: Color32,
    /// Light bounced back up into the very bottom edge.
    pub chassis_foot: Color32,
    /// Hairline along the top edge of a panel, where the light catches it.
    pub chassis_edge: Color32,
    /// Shadow under a panel, which gives it its thickness.
    pub chassis_shadow: Color32,
    /// Body of the screws that hold a panel down.
    pub screw: Color32,
    /// Lit side of a screw head.
    pub screw_highlight: Color32,
    /// Dark text, for the panel legends printed onto the metal.
    pub title: Color32,
    /// Dark text for the smaller labels printed onto the metal, such as the
    /// name under a knob. Light text would vanish on a light panel.
    pub label: Color32,
    /// The value a control currently reads, printed onto the metal.
    pub value: Color32,
    /// An indicator lamp that is not lit.
    pub led_off: Color32,
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
    /// Night city: the chassis is dark anodized metal lit from above, and the
    /// light in the room is neon. The panels keep their gradients and bevels,
    /// so the depth is still there, but nothing on them is bright by itself.
    /// Everything that glows is something the instrument is doing - a lamp, a
    /// value arc, a chop's colour - which is what makes the state readable
    /// across a dark stage.
    pub const fn dark() -> Self {
        Self {
            spacing_sm: 4.0,
            spacing_md: 8.0,
            spacing_lg: 14.0,

            radius_sm: CornerRadius::same(4),
            radius_md: CornerRadius::same(9),

            stroke_thin: 1.0,
            stroke_thick: 2.0,

            font_sm: 11.0,
            font_md: 13.0,
            font_lg: 18.0,

            control_height: 24.0,

            window_bg: Color32::from_rgb(0x07, 0x08, 0x0e),
            // Dark anodized metal. The gradient is what carries the shape, so
            // the span from top to bottom stays wide even though none of it
            // is bright.
            chassis_top: Color32::from_rgb(0x2b, 0x30, 0x44),
            chassis_mid: Color32::from_rgb(0x1e, 0x22, 0x33),
            chassis_bottom: Color32::from_rgb(0x12, 0x15, 0x22),
            chassis_foot: Color32::from_rgb(0x19, 0x1d, 0x2d),
            chassis_edge: Color32::from_rgb(0x46, 0x4f, 0x6d),
            chassis_shadow: Color32::from_rgb(0x05, 0x06, 0x0b),
            screw: Color32::from_rgb(0x2a, 0x2f, 0x40),
            screw_highlight: Color32::from_rgb(0x4a, 0x52, 0x6a),
            // The legends are silkscreened on in a pale cyan rather than
            // printed dark: on an unlit panel dark text is unreadable.
            title: Color32::from_rgb(0xc8, 0xf4, 0xf0),
            label: Color32::from_rgb(0x7f, 0x95, 0xb4),
            value: Color32::from_rgb(0xe6, 0xfb, 0xff),
            led_off: Color32::from_rgb(0x17, 0x1b, 0x28),
            panel_bg: Color32::from_rgb(0x12, 0x15, 0x22),
            panel_top: Color32::from_rgb(0x1c, 0x20, 0x30),
            control_bg: Color32::from_rgb(0x0d, 0x10, 0x1c),
            control_top: Color32::from_rgb(0x25, 0x2b, 0x3e),
            control_hover_bg: Color32::from_rgb(0x15, 0x1a, 0x2a),
            control_hover_top: Color32::from_rgb(0x32, 0x3a, 0x52),
            control_pressed_bg: Color32::from_rgb(0x06, 0x08, 0x10),
            // What carries the current value glows rather than merely
            // changing shade: on a dark panel a shade is not enough.
            control_selected_bg: Color32::from_rgb(0x07, 0x2c, 0x33),
            control_selected_top: Color32::from_rgb(0x0d, 0x4b, 0x56),
            control_highlight: Color32::from_rgb(0x3e, 0x48, 0x64),
            outline: Color32::from_rgb(0x04, 0x05, 0x09),

            text: Color32::from_rgb(0xdd, 0xf2, 0xff),
            text_dim: Color32::from_rgb(0x72, 0x87, 0xa6),

            accent: Color32::from_rgb(0x1a, 0xf0, 0xe6),
            armed: Color32::from_rgb(0xff, 0xc4, 0x1f),
            active: Color32::from_rgb(0x4d, 0xff, 0xb0),
            danger: Color32::from_rgb(0xff, 0x2e, 0x7e),

            waveform_bg: Color32::from_rgb(0x05, 0x07, 0x0f),
            waveform: Color32::from_rgb(0x2d, 0xf7, 0xe8),
            waveform_axis: Color32::from_rgb(0x15, 0x20, 0x33),
            marker: Color32::from_rgb(0x44, 0x5a, 0x7a),
            playhead: Color32::from_rgb(0xff, 0x2e, 0x7e),
            // Slice shading sits behind the trace, so it stays very low
            // contrast; the markers carry the actual division.
            slice_fill: Color32::from_rgb(0x08, 0x0b, 0x16),
            slice_fill_alternate: Color32::from_rgb(0x0c, 0x10, 0x1e),
            slice_selected_fill: Color32::from_rgb(0x08, 0x22, 0x2c),
            // Neon signs: every one of these reads at a glance against the
            // dark chassis, and no two of them are close enough to confuse.
            slice_palette: [
                Color32::from_rgb(0x1a, 0xf0, 0xe6),
                Color32::from_rgb(0xff, 0x3d, 0x9a),
                Color32::from_rgb(0xff, 0xc4, 0x1f),
                Color32::from_rgb(0x4d, 0x9f, 0xff),
                Color32::from_rgb(0xc2, 0x6c, 0xff),
                Color32::from_rgb(0x4d, 0xff, 0xb0),
            ],
        }
    }

    /// The product's light theme: warm ivory Eurorack front plates.
    ///
    /// Plates are matte, finely textured metal lit from the top left. The
    /// controls on them are dark - ribbed knobs, black keys - and the displays
    /// are sunk in, dark green with a pale trace. Colour is kept for what the
    /// instrument is doing and for telling the slices apart.
    pub const fn ivory() -> Self {
        Self {
            spacing_sm: 4.0,
            spacing_md: 8.0,
            spacing_lg: 14.0,

            radius_sm: CornerRadius::same(4),
            radius_md: CornerRadius::same(8),

            stroke_thin: 1.0,
            stroke_thick: 2.0,

            font_sm: 11.0,
            font_md: 13.0,
            font_lg: 18.0,

            control_height: 24.0,

            // A shade under the plates, so the gaps between them read as the
            // rack the modules are screwed into.
            window_bg: Color32::from_rgb(0xd6, 0xd0, 0xc4),
            chassis_top: Color32::from_rgb(0xf5, 0xf1, 0xe8),
            chassis_mid: Color32::from_rgb(0xe8, 0xe4, 0xda),
            chassis_bottom: Color32::from_rgb(0xdd, 0xd8, 0xcd),
            chassis_foot: Color32::from_rgb(0xe4, 0xdf, 0xd4),
            chassis_edge: Color32::from_rgb(0xf5, 0xf1, 0xe8),
            chassis_shadow: Color32::from_rgb(0x8c, 0x85, 0x78),
            screw: Color32::from_rgb(0x9a, 0x94, 0x8a),
            screw_highlight: Color32::from_rgb(0xf5, 0xf1, 0xe8),
            title: Color32::from_rgb(0x30, 0x36, 0x33),
            label: Color32::from_rgb(0x30, 0x36, 0x33),
            value: Color32::from_rgb(0x4a, 0x50, 0x4d),
            led_off: Color32::from_rgb(0x3a, 0x3f, 0x3d),
            // The surface of the editor windows, which open over the plates
            // like a separate dark display.
            panel_bg: Color32::from_rgb(0x26, 0x2b, 0x29),
            panel_top: Color32::from_rgb(0x30, 0x36, 0x33),
            control_bg: Color32::from_rgb(0x29, 0x2d, 0x2c),
            control_top: Color32::from_rgb(0x3c, 0x41, 0x3f),
            control_hover_bg: Color32::from_rgb(0x30, 0x35, 0x33),
            control_hover_top: Color32::from_rgb(0x48, 0x4e, 0x4b),
            control_pressed_bg: Color32::from_rgb(0x1b, 0x1e, 0x1d),
            control_selected_bg: Color32::from_rgb(0x2e, 0x74, 0x6c),
            control_selected_top: Color32::from_rgb(0x3d, 0x91, 0x88),
            control_highlight: Color32::from_rgb(0x55, 0x5b, 0x58),
            outline: Color32::from_rgb(0x14, 0x16, 0x15),

            // Text on the dark keys and displays; text on the plates is
            // `title` and `label`.
            text: Color32::from_rgb(0xe8, 0xe4, 0xda),
            text_dim: Color32::from_rgb(0x8e, 0x99, 0x93),

            accent: Color32::from_rgb(0x4f, 0xb8, 0xa8),
            armed: Color32::from_rgb(0xc6, 0xa0, 0x4d),
            active: Color32::from_rgb(0x5d, 0xd6, 0xa4),
            danger: Color32::from_rgb(0xd4, 0x5a, 0x45),

            waveform_bg: Color32::from_rgb(0x20, 0x2d, 0x2a),
            waveform: Color32::from_rgb(0x9b, 0xc9, 0xb6),
            waveform_axis: Color32::from_rgb(0x34, 0x45, 0x40),
            marker: Color32::from_rgb(0x6d, 0x86, 0x7e),
            playhead: Color32::from_rgb(0xe8, 0x84, 0x6c),
            slice_fill: Color32::from_rgb(0x20, 0x2d, 0x2a),
            slice_fill_alternate: Color32::from_rgb(0x24, 0x33, 0x2f),
            slice_selected_fill: Color32::from_rgb(0x2b, 0x40, 0x3b),
            // The four from the design, then two more of the same weight.
            slice_palette: [
                Color32::from_rgb(0x3d, 0x91, 0x88),
                Color32::from_rgb(0xc9, 0x77, 0x63),
                Color32::from_rgb(0xc6, 0xa0, 0x4d),
                Color32::from_rgb(0x63, 0x8f, 0xa5),
                Color32::from_rgb(0x8d, 0x7b, 0xa6),
                Color32::from_rgb(0x7f, 0x9c, 0x5c),
            ],
        }
    }

    /// Stroke around a recessed area cut into the metal.
    pub fn inset_stroke(&self) -> Stroke {
        Stroke::new(self.stroke_thin, self.chassis_shadow)
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
        Self::ivory()
    }
}
