//! The hardware surfaces cut from the design renders.
//!
//! Plates, knob caps, switches and pads are pictures rather than shapes: the
//! light and the material in the renders are what make the instrument read as
//! hardware, and no amount of gradients gets there. Everything that changes -
//! text, scales, values, waveforms, lamps - is still drawn on top.
//!
//! The files are produced by `tools/design/cut_textures.py` as raw RGBA with a
//! small header, so no image decoder has to ship with the plugin.

use std::sync::Arc;

use nih_plug_egui::egui::{
    epaint::{Mesh, Vertex},
    pos2, vec2, Color32, ColorImage, Context, Id, Pos2, Rect, Shape, TextureFilter, TextureHandle,
    TextureId, TextureOptions, TextureWrapMode, Vec2,
};

/// Texels per point the textures were cut at: twice their size on screen, so
/// they stay sharp on a HiDPI display.
pub const TEXELS_PER_POINT: f32 = 2.0;

/// Width of the nine slice corners of the dark keys, in texels.
pub const KEY_CORNER: f32 = 9.0;
/// Width of the nine slice corners of the small selector keys and their tray.
pub const SMALL_KEY_CORNER: f32 = 6.0;
/// Radius of the selector's nickel ring, as a fraction of the texture's side.
pub const SELECTOR_RING_RADIUS: f32 = 0.4828;
/// Width of the nine slice corners of the panel texture, in texels.
pub const PANEL_CORNER: f32 = 40.0;
/// Width of the nine slice corners of the pad bezel, in texels.
pub const PAD_BEZEL_CORNER: f32 = 24.0;
/// Width of the nine slice corners of the pad cap, in texels.
pub const PAD_CAP_CORNER: f32 = 15.0;
/// Where the pointer of the knob turns, as a fraction of the texture's side.
///
/// Not the middle: the render looks at the knob slightly from the front, so
/// the flat top sits above the centre of the knurled skirt.
pub const KNOB_PIVOT: Vec2 = Vec2::new(0.5044, 0.4254);
/// Radius of the knob's flat top, as a fraction of the texture's side.
pub const KNOB_CAP_RADIUS: f32 = 0.4013;

/// Every texture the interface draws with, uploaded once per context.
pub struct Textures {
    pub panel: TextureHandle,
    pub panel_grain: TextureHandle,
    pub screw: TextureHandle,
    pub knob: TextureHandle,
    pub switch_up: TextureHandle,
    pub switch_down: TextureHandle,
    pub pad_bezel: TextureHandle,
    pub pad_cap: TextureHandle,
    pub pad_grain: TextureHandle,
    pub key: TextureHandle,
    pub key_pressed: TextureHandle,
    pub key_ivory: TextureHandle,
    pub key_teal: TextureHandle,
    pub key_tray: TextureHandle,
    pub led_off: TextureHandle,
    pub led_on: TextureHandle,
    pub selector_ring: TextureHandle,
    pub selector_head: TextureHandle,
}

/// Upload an embedded texture under its file name.
macro_rules! embedded {
    ($ctx:expr, $name:literal, $options:expr) => {
        upload(
            $ctx,
            $name,
            include_bytes!(concat!("../../assets/", $name, ".rgba")),
            $options,
        )
    };
}

/// The textures for this context, uploaded on first use.
///
/// Kept in the context's memory rather than in a static: a plugin window can
/// be closed and opened again with a new context, whose renderer knows none of
/// the textures the old one was given.
pub fn textures(ctx: &Context) -> Arc<Textures> {
    let id = Id::new("saempler-textures");
    if let Some(textures) = ctx.data(|data| data.get_temp::<Arc<Textures>>(id)) {
        return textures;
    }

    let textures = Arc::new(Textures {
        panel: embedded!(ctx, "panel", crisp()),
        panel_grain: embedded!(ctx, "panel_grain", tiled()),
        screw: embedded!(ctx, "screw", smooth()),
        knob: embedded!(ctx, "knob", smooth()),
        switch_up: embedded!(ctx, "switch_up", smooth()),
        switch_down: embedded!(ctx, "switch_down", smooth()),
        pad_bezel: embedded!(ctx, "pad_bezel", crisp()),
        pad_cap: embedded!(ctx, "pad_cap", crisp()),
        pad_grain: embedded!(ctx, "pad_grain", tiled()),
        key: embedded!(ctx, "key", crisp()),
        key_pressed: embedded!(ctx, "key_pressed", crisp()),
        key_ivory: embedded!(ctx, "key_ivory", crisp()),
        key_teal: embedded!(ctx, "key_teal", crisp()),
        key_tray: embedded!(ctx, "key_tray", crisp()),
        led_off: embedded!(ctx, "led_off", smooth()),
        led_on: embedded!(ctx, "led_on", smooth()),
        selector_ring: embedded!(ctx, "selector_ring", smooth()),
        selector_head: embedded!(ctx, "selector_head", smooth()),
    });
    ctx.data_mut(|data| data.insert_temp(id, Arc::clone(&textures)));
    textures
}

/// Filtering for nine slices, which are only ever stretched.
fn crisp() -> TextureOptions {
    TextureOptions::LINEAR
}

/// Filtering for parts drawn smaller than they were cut, such as a knob at
/// half its texture size: mipmaps keep the knurling from shimmering.
fn smooth() -> TextureOptions {
    TextureOptions {
        mipmap_mode: Some(TextureFilter::Linear),
        ..TextureOptions::LINEAR
    }
}

/// Filtering for surface grain laid across a whole plate. Mirrored, so the
/// tile needs no seamless edges of its own.
fn tiled() -> TextureOptions {
    TextureOptions {
        wrap_mode: TextureWrapMode::MirroredRepeat,
        ..TextureOptions::LINEAR
    }
}

/// Upload one embedded texture.
///
/// A file that does not decode becomes a single transparent texel: a missing
/// surface is a cosmetic fault and must not take the editor down with it.
fn upload(ctx: &Context, name: &str, bytes: &[u8], options: TextureOptions) -> TextureHandle {
    let image = decode(bytes).unwrap_or_else(|| ColorImage::new([1, 1], Color32::TRANSPARENT));
    ctx.load_texture(name, image, options)
}

/// Read the raw RGBA format written by the cutting script.
///
/// Eight bytes of header, width and height as little-endian `u32`, then the
/// pixels unpremultiplied, row by row.
pub fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let header = bytes.get(..8)?;
    let width = u32::from_le_bytes(header[..4].try_into().ok()?) as usize;
    let height = u32::from_le_bytes(header[4..].try_into().ok()?) as usize;
    let pixels = bytes.get(8..)?;
    if width == 0 || height == 0 || pixels.len() != width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied([width, height], pixels))
}

/// Size of a texture on screen, in points.
pub fn size_of(texture: &TextureHandle) -> Vec2 {
    let [width, height] = texture.size();
    vec2(width as f32, height as f32) / TEXELS_PER_POINT
}

/// A texture stretched over a rectangle.
pub fn image(texture: &TextureHandle, rect: Rect, tint: Color32) -> Shape {
    let mut mesh = Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
    Shape::mesh(mesh)
}

/// A grain texture laid across a rectangle at its natural size, repeating.
pub fn grain(texture: &TextureHandle, rect: Rect, tint: Color32) -> Shape {
    let tile = size_of(texture);
    let uv = Rect::from_min_max(
        Pos2::ZERO,
        pos2(rect.width() / tile.x, rect.height() / tile.y),
    );
    let mut mesh = Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(rect, uv, tint);
    Shape::mesh(mesh)
}

/// A nine slice: corners at their natural size, edges and face stretched.
///
/// `corner` is the width of a corner in texels. On a rectangle too small for
/// two whole corners they shrink together, so the outline stays closed.
pub fn nine_slice(texture: &TextureHandle, rect: Rect, corner: f32, tint: Color32) -> Shape {
    Shape::mesh(nine_slice_mesh(
        texture.id(),
        texture.size(),
        rect,
        corner,
        tint,
    ))
}

/// The mesh behind [`nine_slice`], kept apart so its geometry can be tested
/// without a renderer.
fn nine_slice_mesh(
    texture: TextureId,
    size: [usize; 2],
    rect: Rect,
    corner: f32,
    tint: Color32,
) -> Mesh {
    let [width, height] = [size[0] as f32, size[1] as f32];
    let points = (corner / TEXELS_PER_POINT)
        .min(rect.width() * 0.5)
        .min(rect.height() * 0.5)
        .max(0.0);

    let xs = [
        rect.min.x,
        rect.min.x + points,
        rect.max.x - points,
        rect.max.x,
    ];
    let ys = [
        rect.min.y,
        rect.min.y + points,
        rect.max.y - points,
        rect.max.y,
    ];
    let us = [0.0, corner / width, 1.0 - corner / width, 1.0];
    let vs = [0.0, corner / height, 1.0 - corner / height, 1.0];

    let mut mesh = Mesh::with_texture(texture);
    for row in 0..4 {
        for column in 0..4 {
            mesh.vertices.push(Vertex {
                pos: pos2(xs[column], ys[row]),
                uv: pos2(us[column], vs[row]),
                color: tint,
            });
        }
    }
    for row in 0..3_u32 {
        for column in 0..3_u32 {
            let top_left = row * 4 + column;
            mesh.add_triangle(top_left, top_left + 1, top_left + 5);
            mesh.add_triangle(top_left, top_left + 5, top_left + 4);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(pixels);
        bytes
    }

    #[test]
    fn a_well_formed_file_decodes() {
        let bytes = encoded(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]);

        let image = decode(&bytes).expect("decodes");

        assert_eq!(image.size, [2, 1]);
        assert_eq!(image.pixels[0], Color32::from_rgb(255, 0, 0));
        assert_eq!(image.pixels[1], Color32::from_rgb(0, 0, 255));
    }

    #[test]
    fn a_short_or_empty_file_is_refused() {
        assert!(decode(&[]).is_none());
        assert!(decode(&encoded(2, 2, &[0; 12])).is_none());
        assert!(decode(&encoded(0, 4, &[])).is_none());
    }

    #[test]
    fn every_embedded_texture_decodes() {
        for bytes in [
            &include_bytes!("../../assets/panel.rgba")[..],
            &include_bytes!("../../assets/panel_grain.rgba")[..],
            &include_bytes!("../../assets/screw.rgba")[..],
            &include_bytes!("../../assets/knob.rgba")[..],
            &include_bytes!("../../assets/switch_up.rgba")[..],
            &include_bytes!("../../assets/switch_down.rgba")[..],
            &include_bytes!("../../assets/pad_bezel.rgba")[..],
            &include_bytes!("../../assets/pad_cap.rgba")[..],
            &include_bytes!("../../assets/pad_grain.rgba")[..],
            &include_bytes!("../../assets/key.rgba")[..],
            &include_bytes!("../../assets/key_pressed.rgba")[..],
            &include_bytes!("../../assets/key_ivory.rgba")[..],
            &include_bytes!("../../assets/key_teal.rgba")[..],
            &include_bytes!("../../assets/key_tray.rgba")[..],
            &include_bytes!("../../assets/led_off.rgba")[..],
            &include_bytes!("../../assets/led_on.rgba")[..],
            &include_bytes!("../../assets/selector_ring.rgba")[..],
            &include_bytes!("../../assets/selector_head.rgba")[..],
        ] {
            assert!(decode(bytes).is_some());
        }
    }

    #[test]
    fn a_nine_slice_keeps_its_corners_at_their_natural_size() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(200.0, 100.0));

        let mesh = nine_slice_mesh(TextureId::default(), [81, 81], rect, 40.0, Color32::WHITE);

        assert_eq!(mesh.vertices.len(), 16);
        assert_eq!(mesh.indices.len(), 9 * 6);
        // The second column sits one corner in: 40 texels are 20 points.
        assert_eq!(mesh.vertices[1].pos, pos2(30.0, 20.0));
        assert_eq!(mesh.vertices[15].pos, rect.max);
        assert!((mesh.vertices[1].uv.x - 40.0 / 81.0).abs() < 1e-6);
    }

    #[test]
    fn corners_shrink_on_a_rectangle_too_small_for_them() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(30.0, 100.0));

        let mesh = nine_slice_mesh(TextureId::default(), [81, 81], rect, 40.0, Color32::WHITE);

        // Half the width each, so the left and right corners meet.
        assert_eq!(mesh.vertices[1].pos.x, 15.0);
        assert_eq!(mesh.vertices[2].pos.x, 15.0);
    }
}
