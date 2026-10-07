#!/usr/bin/env python3
"""Rebuild platform icons and the rounded README preview (requires Pillow)."""

from pathlib import Path
import struct

from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "crates/desktop/assets/app-icon"
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


def main():
    with Image.open(ASSETS / "source/probe-app-icon-1024.png") as source:
        if source.size != (1024, 1024):
            raise ValueError("The source icon must be 1024 x 1024 pixels")
        if source.convert("RGBA").getchannel("A").getextrema() != (255, 255):
            raise ValueError("The source icon must be fully opaque")
        icon = source.convert("RGB")

    def png(path, size):
        icon.resize((size, size), Image.Resampling.LANCZOS).save(path)
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
    preview = icon.resize((512, 512), Image.Resampling.LANCZOS).convert("RGBA")
    mask = Image.new("L", (2048, 2048))
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, 2047, 2047), radius=448, fill=255)
    preview.putalpha(mask.resize(preview.size, Image.Resampling.LANCZOS))
    preview.save(ROOT / "docs/assets/probe-app-icon.png")


if __name__ == "__main__":
    main()
