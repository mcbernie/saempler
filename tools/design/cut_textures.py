#!/usr/bin/env python3
"""Cut the interface textures out of the design renders in DESIGN/.

Development aid, not shipped. Needs Pillow (`pip install pillow`).

The renders are whole scenes, so every part is cut out with a mask of its
outline, scaled to twice its size on screen (sharp on a HiDPI display) and
written as raw RGBA: an 8 byte header (width and height as little-endian u32)
followed by unpremultiplied pixels. The interface embeds the files with
`include_bytes!`, which keeps an image decoder out of the plugin.

Run from the repository root:

    python3 tools/design/cut_textures.py

Pass `--preview DIR` to also write every texture as a PNG for inspection.
"""

import argparse
import math
import struct
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
DESIGN = ROOT / "DESIGN"
OUT = ROOT / "crates" / "saempler-ui" / "assets"

# Supersampling factor for the masks, so their edges are antialiased.
SS = 4


def load(name):
    return Image.open(DESIGN / name).convert("RGBA")


def write(name, image, preview):
    image = image.convert("RGBA")
    # The renders are not quite opaque everywhere; a plate must be.
    r, g, b, a = image.split()
    image = Image.merge("RGBA", (r, g, b, a.point(lambda v: 255 if v >= 248 else v)))
    width, height = image.size
    data = struct.pack("<II", width, height) + image.tobytes()
    (OUT / f"{name}.rgba").write_bytes(data)
    if preview:
        image.save(preview / f"{name}.png")
    print(f"{name}: {width}x{height}")


def rounded_mask(size, box, radius):
    """Antialiased mask of a rounded rectangle, in image coordinates."""
    big = Image.new("L", (size[0] * SS, size[1] * SS), 0)
    x0, y0, x1, y1 = (v * SS for v in box)
    ImageDraw.Draw(big).rounded_rectangle((x0, y0, x1, y1), radius * SS, fill=255)
    return big.resize(size, Image.LANCZOS)


def polygon_mask(size, shapes):
    """Antialiased mask of a union of polygons and circles."""
    big = Image.new("L", (size[0] * SS, size[1] * SS), 0)
    draw = ImageDraw.Draw(big)
    for kind, data in shapes:
        if kind == "poly":
            draw.polygon([(x * SS, y * SS) for x, y in data], fill=255)
        elif kind == "circle":
            cx, cy, r = data
            draw.ellipse(((cx - r) * SS, (cy - r) * SS, (cx + r) * SS, (cy + r) * SS), fill=255)
        elif kind == "ellipse":
            cx, cy, rx, ry = data
            draw.ellipse(((cx - rx) * SS, (cy - ry) * SS, (cx + rx) * SS, (cy + ry) * SS), fill=255)
    return big.resize(size, Image.LANCZOS)


def masked(image, mask):
    # Resampling leaves the inside a hair short of opaque; snap it back, or
    # every plate would show a trace of what is behind it.
    mask = mask.point(lambda v: 255 if v >= 248 else v)
    out = image.copy()
    out.putalpha(ImageChops.multiply(image.getchannel("A"), mask))
    return out


def scaled(image, factor):
    """Resize with premultiplied alpha, so transparent pixels do not bleed."""
    size = (max(1, round(image.width * factor)), max(1, round(image.height * factor)))
    return image.convert("RGBa").resize(size, Image.LANCZOS).convert("RGBA")


def nine_slice(image, corner):
    """Reduce a cut-out plate to its corners and one averaged row of edge.

    The edges of a plate are the same all along their length, so one pixel of
    each, averaged over the whole edge, stretches without the grain of the
    render turning into streaks. The centre is the average of the face.
    """
    w, h = image.size
    c = corner
    out = Image.new("RGBA", (2 * c + 1, 2 * c + 1))

    def average(box, size):
        return image.crop(box).convert("RGBa").resize(size, Image.BOX).convert("RGBA")

    out.paste(image.crop((0, 0, c, c)), (0, 0))
    out.paste(image.crop((w - c, 0, w, c)), (c + 1, 0))
    out.paste(image.crop((0, h - c, c, h)), (0, c + 1))
    out.paste(image.crop((w - c, h - c, w, h)), (c + 1, c + 1))
    out.paste(average((c, 0, w - c, c), (1, c)), (c, 0))
    out.paste(average((c, h - c, w - c, h), (1, c)), (c, c + 1))
    out.paste(average((0, c, c, h - c), (c, 1)), (0, c))
    out.paste(average((w - c, c, w, h - c), (c, 1)), (c + 1, c))
    out.paste(average((c, c, w - c, h - c), (1, 1)), (c, c))
    return out


def grain(image, box, size, gain, blur_radius=2.5):
    """The fine structure of a surface, as a light and dark overlay.

    Brighter than average becomes white and darker becomes black, each with an
    alpha for how far off it is. Laid over any flat colour, it gives that
    colour the texture of the render without carrying the render's tint.
    """
    gray = image.crop(box).convert("L").resize(size, Image.LANCZOS)
    # Only the fine grain: the slow light falloff across the face is taken out.
    blur = gray.filter(ImageFilter.GaussianBlur(radius=blur_radius))
    out = Image.new("RGBA", size)
    pixels = []
    for value, mean in zip(gray.get_flattened_data(), blur.get_flattened_data()):
        delta = (value - mean) * gain
        alpha = int(min(255, abs(delta)))
        pixels.append((255, 255, 255, alpha) if delta > 0 else (0, 0, 0, alpha))
    out.putdata(pixels)
    return out


def flatten_face(image, box, radius, color=None):
    """Replace the face inside the milled edge with one flat colour.

    A nine slice keeps its corners as they are and stretches one averaged
    pixel between them; if the corners still carried the falloff of the light
    across the face, the stretched middle would meet them at a visible seam.
    The grain overlay puts the texture back on top.
    """
    mask = rounded_mask(image.size, box, radius).filter(ImageFilter.GaussianBlur(2))
    if color is None:
        rgb = image.convert("RGB").crop(tuple(int(v) for v in box)).resize((1, 1), Image.BOX)
        color = rgb.getpixel((0, 0))
    flat = Image.new("RGBA", image.size, tuple(color) + (255,))
    flat.putalpha(image.getchannel("A"))
    return Image.composite(flat, image, mask)


def neutral(image, face_luma):
    """Turn a coloured part grey, so it can be tinted to any slice colour.

    Scaled so that the face, the middle texel of a nine slice, comes out at
    `face_luma`; the lit edges keep the headroom above it.
    """
    alpha = image.getchannel("A")
    luma = image.convert("RGB").convert("L")
    centre = luma.getpixel((luma.width // 2, luma.height // 2))
    factor = face_luma / max(1, centre)
    luma = luma.point(lambda v: max(0, min(255, round(v * factor))))
    return Image.merge("RGBA", (luma, luma, luma, alpha))


def panel(preview):
    """The front plate: rounded ivory with a two-step milled edge."""
    image = load("Leere warme Elfenbein-Frontplatte-2.png")
    box = (16.5, 119.5, 1519.5, 905.5)
    mask = rounded_mask(image.size, box, 17)
    plate = masked(image, mask)
    plate = flatten_face(plate, (box[0] + 16, box[1] + 16, box[2] - 16, box[3] - 16), 6)
    plate = plate.crop((16, 119, 1521, 907))
    # Drawn at half size: 2 texels per point.
    write("panel", nine_slice(plate, 40), preview)
    # Cut at its native size: two texels per point makes the grain half as
    # coarse on screen as in the render, which suits the smaller plates.
    write("panel_grain", grain(image, (300, 300, 556, 556), (256, 256), 1.4), preview)


def screw(preview):
    image = load("Unknown-2.png")
    cx, cy, r = 40.0, 41.0, 15.5
    mask = polygon_mask(image.size, [("circle", (cx, cy, r))])
    part = masked(image, mask).crop((int(cx - 16), int(cy - 16), int(cx + 16), int(cy + 16)))
    write("screw", part, preview)


def knob(preview):
    """A knob cap with its pointer painted out.

    The pointer is drawn by the interface, over a cap whose light stays where
    it is: a turning picture of a knob would turn its highlights with it.
    """
    image = load("Eurorack-Regler und Kippschalter im Raster.png")
    cx, cy = 452.0, 247.0  # centre of the flat top, which the pointer turns on

    # Paint the pointer out with the same strip of cap turned 40 degrees left:
    # the face is matte and the light comes from the top left, so a strip a
    # little way round looks the same.
    # Near the pivot a turned strip still holds the pointer, so the bottom of
    # the strip is taken from beside it instead, where the face is flat.
    turned = image.rotate(-40, resample=Image.BICUBIC, center=(cx, cy))
    beside = ImageChops.offset(image, 70, 0)
    top = polygon_mask(
        image.size,
        [("poly", [(cx - 26, 70), (cx + 26, 70), (cx + 26, cy - 70), (cx - 26, cy - 70)])],
    ).filter(ImageFilter.GaussianBlur(3))
    bottom = polygon_mask(
        image.size,
        [("poly", [(cx - 26, cy - 76), (cx + 26, cy - 76), (cx + 26, cy + 20), (cx - 26, cy + 20)])],
    ).filter(ImageFilter.GaussianBlur(3))
    image = Image.composite(turned, image, top)
    image = Image.composite(beside, image, bottom)

    skirt = (450.0, 281.5, 222.0, 226.0)  # centre and radii of the knurled skirt
    mask = polygon_mask(image.size, [("ellipse", skirt)])
    half = 228
    box = (int(skirt[0] - half), int(skirt[1] - half), int(skirt[0] + half), int(skirt[1] + half))
    part = masked(image, mask).crop(box)
    factor = 128 / part.width
    write("knob", scaled(part, factor), preview)
    side = part.width
    print(
        f"  knob pivot: ({(cx - box[0]) / side:.4f}, {(cy - box[1]) / side:.4f}),"
        f" cap radius {183 / side:.4f} of the side"
    )


def switch(preview):
    """A nickel toggle, lever up and lever down."""
    image = load("Eurorack-Regler und Kippschalter im Raster.png")

    def hexagon(cx, cy, top, bottom):
        return [
            (cx - 80, top), (cx + 88, top), (cx + 162, cy - 5),
            (cx + 88, bottom), (cx - 80, bottom), (cx - 162, cy - 5),
        ]

    up_centre = (440.0, 768.0)
    down_centre = (1098.0, 770.0)
    up = [
        ("poly", hexagon(*up_centre, 632, 906)),
        ("circle", (447.0, 598.0, 55.0)),
        ("poly", [(395, 600), (500, 600), (483, 770), (410, 770)]),
    ]
    down = [
        ("poly", hexagon(*down_centre, 631, 909)),
        ("circle", (1102.0, 878.0, 57.0)),
        ("poly", [(1063, 735), (1140, 735), (1158, 880), (1046, 880)]),
    ]
    for name, (cx, cy), shapes in [("switch_up", up_centre, up), ("switch_down", down_centre, down)]:
        mask = polygon_mask(image.size, shapes)
        part = masked(image, mask).crop((int(cx - 172), int(cy - 228), int(cx + 172), int(cy + 172)))
        write(name, scaled(part, 64 / part.width), preview)


def pad(preview):
    """A performance pad: a dark rubber bezel and a cap tinted per slice."""
    image = load("Unknown-4.png")

    bezel_box = (60.5, 96.5, 1192.5, 1166.5)
    cap_box = (112.5, 149.5, 1141.5, 1125.5)
    bezel = masked(image, rounded_mask(image.size, bezel_box, 78))
    # The cap is drawn on its own over the bezel; underneath it is the dark
    # well the cap sinks into when pressed.
    bezel = flatten_face(bezel, cap_box, 52, (18, 19, 19))
    bezel = bezel.crop((60, 96, 1193, 1167))
    bezel = scaled(bezel, 0.2)
    write("pad_bezel", nine_slice(bezel, 24), preview)

    cap = masked(image, rounded_mask(image.size, cap_box, 52))
    # The face colour comes from a patch with nothing printed on it: the
    # average of the whole face would take in the dark display.
    face = image.convert("RGB").crop((520, 180, 1100, 400)).resize((1, 1), Image.BOX).getpixel((0, 0))
    cap = flatten_face(
        cap, (cap_box[0] + 70, cap_box[1] + 70, cap_box[2] - 70, cap_box[3] - 70), 20, face
    )
    cap = cap.crop((112, 149, 1142, 1126))
    cap = scaled(cap, 0.2)
    write("pad_cap", neutral(nine_slice(cap, 15), 200), preview)
    write("pad_grain", grain(image, (520, 180, 1100, 400), (128, 48), 1.6, 1.5), preview)


def keys(preview):
    """The dark keys at rest, held down and engaged, and the small selector."""
    image = load("Makrostudie taktiler Eurorack-Taster.png")
    for name, (x0, x1) in [("key", (46.5, 570.5)), ("key_pressed", (628.5, 1145.5))]:
        box = (x0, 125.5, x1, 378.5)
        part = masked(image, rounded_mask(image.size, box, 24))
        # The lamp and the legend are drawn by the interface.
        part = flatten_face(part, (x0 + 50, box[1] + 50, x1 - 50, box[3] - 50), 10)
        part = part.crop((int(x0), int(box[1]), int(x1) + 1, int(box[3]) + 1))
        write(name, nine_slice(scaled(part, 0.19), 9), preview)

    for name, box in [("key_ivory", (606.5, 564.5, 735.5, 731.5)), ("key_teal", (893.5, 564.5, 1025.5, 731.5))]:
        part = masked(image, rounded_mask(image.size, box, 12))
        part = flatten_face(part, (box[0] + 28, box[1] + 28, box[2] - 28, box[3] - 28), 6)
        part = part.crop(tuple(int(v) for v in box[:2]) + (int(box[2]) + 1, int(box[3]) + 1))
        write(name, nine_slice(scaled(part, 0.24), 6), preview)

    box = (585.5, 541.5, 1190.5, 752.5)
    tray = masked(image, rounded_mask(image.size, box, 18))
    tray = flatten_face(tray, (box[0] + 16, box[1] + 16, box[2] - 16, box[3] - 16), 8, (22, 24, 23))
    tray = tray.crop((585, 541, 1191, 753))
    write("key_tray", nine_slice(scaled(tray, 0.24), 6), preview)

    # The lamps: dark glass at rest, and lit glass turned grey so the
    # interface can light it in any colour.
    for name, (cx, cy) in [("led_off", (142.0, 228.0)), ("led_on", (730.0, 230.0))]:
        r = 36.0
        part = masked(image, polygon_mask(image.size, [("circle", (cx, cy, r))]))
        part = part.crop((int(cx - r - 1), int(cy - r - 1), int(cx + r + 1), int(cy + r + 1)))
        part = scaled(part, 20 / part.width)
        if name == "led_on":
            part = neutral(part, 200)
        write(name, part, preview)


def selector(preview):
    """A detented selector: a nickel ring and a chicken-head knob on it.

    The two are cut apart so the ring, and the light on it, stays where it is
    while the head turns from position to position.
    """
    image = load("Detentierter LFO-Wellenformwähler.png")
    cx, cy = 687.0, 595.0
    head = [
        (672, 362), (702, 362), (735, 440), (795, 488), (848, 505), (852, 690),
        (825, 725), (785, 775), (768, 812), (740, 820), (635, 820), (608, 812),
        (590, 775), (552, 725), (525, 690), (528, 505), (580, 488), (640, 440),
    ]
    head_mask = polygon_mask(image.size, [("poly", head)])

    # Under the head: the ring turned an eighth, which puts plain ring where
    # the pointer and the lip were, and a dark well in the middle the head
    # never leaves.
    turned = image.rotate(45, resample=Image.BICUBIC, center=(cx, cy))
    ring = Image.composite(turned, image, head_mask.filter(ImageFilter.MaxFilter(9)))
    well = Image.new("RGBA", image.size, (21, 22, 22, 255))
    well_mask = polygon_mask(image.size, [("circle", (cx, cy, 186))]).filter(ImageFilter.GaussianBlur(3))
    ring = Image.composite(well, ring, well_mask)
    ring = masked(ring, polygon_mask(image.size, [("circle", (cx, cy, 224))]))

    knob = masked(image, head_mask)
    half = 232
    box = (int(cx - half), int(cy - half), int(cx + half), int(cy + half))
    factor = 112 / (2 * half)
    write("selector_ring", scaled(ring.crop(box), factor), preview)
    write("selector_head", scaled(knob.crop(box), factor), preview)
    print(f"  selector ring radius {224 / (2 * half):.4f} of the side")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--preview", type=Path)
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    if args.preview:
        args.preview.mkdir(parents=True, exist_ok=True)
    for cut in (panel, screw, knob, switch, pad, keys, selector):
        cut(args.preview)


if __name__ == "__main__":
    main()
