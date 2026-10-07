#!/usr/bin/env python3
"""Render SVG sources and rebuild platform icons (requires CairoSVG and Pillow)."""

from io import BytesIO
from pathlib import Path
import struct

import cairosvg
from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "crates/desktop/assets/app-icon"
LIGHT_SOURCE = ASSETS / "source/probe-app-icon.svg"
DARK_SOURCE = ASSETS / "source/probe-app-icon-dark.svg"
WINDOWS_SIZES = (16, 24, 32, 48, 64, 128, 256)
MACOS_IMAGES = (
    (b"icp4", "icon_16x16.png", 16),
    (b"icp5", "icon_32x32.png", 32),
    (b"icp6", "icon_32x32@2x.png", 64),
    (b"ic07", "icon_128x128.png", 128),
    (b"ic08", "icon_256x256.png", 256),
    (b"ic09", "icon_512x512.png", 512),
    (b"ic10", "icon_512x512@2x.png", 1024),
)


def render_svg(path, size):
    data = cairosvg.svg2png(url=str(path), output_width=size, output_height=size)
    with Image.open(BytesIO(data)) as source:
        if source.convert("RGBA").getchannel("A").getextrema() != (255, 255):
            raise ValueError(f"The rendered icon must be fully opaque: {path}")
        return source.convert("RGB")


def main():
    icons = {1024: render_svg(LIGHT_SOURCE, 1024)}
    icons[1024].save(ASSETS / "source/probe-app-icon-1024.png")
    render_svg(DARK_SOURCE, 1024).save(ASSETS / "source/probe-app-icon-dark-1024.png")

    def png(path, size):
        if size not in icons:
            icons[size] = render_svg(LIGHT_SOURCE, size)
        icons[size].save(path)
        return path.read_bytes()

    iconset = ASSETS / "macos/Probe.iconset"
    chunks = []
    for tag, name, size in MACOS_IMAGES:
        data = png(iconset / name, size)
        chunks.append(tag + struct.pack(">I", len(data) + 8) + data)
    for name, size in (
        ("icon_16x16@2x.png", 32),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256@2x.png", 512),
    ):
        png(iconset / name, size)
    body = b"".join(chunks)
    (ASSETS / "macos/Probe.icns").write_bytes(
        b"icns" + struct.pack(">I", len(body) + 8) + body
    )

    chunks = []
    entries = []
    offset = 6 + 16 * len(WINDOWS_SIZES)
    for size in WINDOWS_SIZES:
        data = png(ASSETS / f"windows/png/probe-{size}.png", size)
        dimension = size if size < 256 else 0
        entries.append(
            struct.pack("<BBBBHHII", dimension, dimension, 0, 0, 1, 24, len(data), offset)
        )
        chunks.append(data)
        offset += len(data)
    (ASSETS / "windows/Probe.ico").write_bytes(
        struct.pack("<HHH", 0, 1, len(WINDOWS_SIZES))
        + b"".join(entries)
        + b"".join(chunks)
    )

    for size in (*WINDOWS_SIZES, 512):
        png(ASSETS / f"linux/hicolor/{size}x{size}/apps/dev.probe.desktop.png", size)

    # Supersample the corner mask for smooth edges in the 128px README display.
    preview = icons[512].convert("RGBA")
    mask = Image.new("L", (2048, 2048))
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, 2047, 2047), radius=448, fill=255)
    preview.putalpha(mask.resize(preview.size, Image.Resampling.LANCZOS))
    preview.save(ROOT / "docs/assets/probe-app-icon.png")


if __name__ == "__main__":
    main()
