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
    /// Background of an interactive control at rest.
    pub control_bg: Color32,
    /// Background of an interactive control under the pointer.
    pub control_hover_bg: Color32,
    /// Border between surfaces.
    pub outline: Color32,

    pub text: Color32,
    pub text_dim: Color32,

    /// Selection and primary interaction colour.
    pub accent: Color32,
    /// Running/active state.
    pub active: Color32,
    /// Reserved for clipping and errors only.
    pub danger: Color32,
}

impl Theme {
    /// The product's dark theme.
    ///
    /// Surfaces are layered greys rather than pure black so that panels stay
    /// distinguishable on the kind of displays these plugins are used on.
    pub const fn dark() -> Self {
        Self {
            spacing_sm: 4.0,
            spacing_md: 8.0,
            spacing_lg: 16.0,

            radius_sm: CornerRadius::same(3),
            radius_md: CornerRadius::same(6),

            stroke_thin: 1.0,
            stroke_thick: 2.0,

            font_sm: 11.0,
            font_md: 13.0,
            font_lg: 17.0,

            window_bg: Color32::from_rgb(0x15, 0x17, 0x1a),
            panel_bg: Color32::from_rgb(0x1d, 0x20, 0x24),
            control_bg: Color32::from_rgb(0x2a, 0x2e, 0x34),
            control_hover_bg: Color32::from_rgb(0x36, 0x3b, 0x43),
            outline: Color32::from_rgb(0x3a, 0x3f, 0x47),

            text: Color32::from_rgb(0xe2, 0xe6, 0xeb),
            text_dim: Color32::from_rgb(0x8c, 0x95, 0xa1),

            accent: Color32::from_rgb(0x2d, 0xd4, 0xbf),
            active: Color32::from_rgb(0x4a, 0xde, 0x80),
            danger: Color32::from_rgb(0xef, 0x44, 0x44),
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
