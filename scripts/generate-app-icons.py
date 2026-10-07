#!/usr/bin/env python3
"""Render SVG sources and rebuild platform icons (requires CairoSVG and Pillow)."""

import argparse
from copy import deepcopy
from io import BytesIO
from pathlib import Path
import struct
import xml.etree.ElementTree as ET

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


def composer_layers():
    """Extract depth planes from #106 without redrawing or recoloring artwork.

    Effects/highlights stay with the object they describe; separating those into
    floating planes would change occlusion and the original geometry.
    """
    namespace = "{http://www.w3.org/2000/svg}"
    ET.register_namespace("", namespace[1:-1])
    destination = ASSETS / "macos/Probe.icon/Assets"
    destination.mkdir(parents=True, exist_ok=True)
    for appearance, source in (("default", LIGHT_SOURCE), ("dark", DARK_SOURCE)):
        root = ET.parse(source).getroot()
        shapes = list(root)[3:]  # title, description, defs precede the artwork.
        if [node.tag.removeprefix(namespace) for node in shapes] != [
            "rect", "rect", *(["path"] * 5),
            "g", "g", "path", "path", "g", "path", *(["circle"] * 4),
        ]:
            raise ValueError(f"Update the depth split for changed SVG structure: {source}")
        for name, nodes in (
            ("01-background-grid", shapes[:2]),
            ("02-cable", shapes[2:7]),
            ("03-probe", shapes[7:-4]),
            ("04-target", shapes[-4:]),
        ):
            layer = ET.Element(namespace + "svg", {
                "width": "1024", "height": "1024", "viewBox": "0 0 1024 1024",
            })
            definitions = deepcopy(root.find(namespace + "defs"))
            # Unused pattern definitions also break Apple's SVG importer.
            required = {"grid"} if name == "01-background-grid" else (
                {"metal", "guard-fill", "handle-clip", "bands-clip", "guard-clip"}
                if name == "03-probe" else set()
            )
            for definition in list(definitions):
                if definition.get("id") not in required:
                    definitions.remove(definition)
            if len(definitions):
                layer.append(definitions)
            layer.extend(deepcopy(nodes))
            path = destination / f"{name}-{appearance}.svg"
            ET.indent(layer, space="  ")
            ET.ElementTree(layer).write(path, encoding="utf-8", xml_declaration=True)
            if name == "01-background-grid":
                # Icon Composer's SVG importer does not support pattern fills.
                # Rasterize only this depth plane, directly from its SVG.
                cairosvg.svg2png(url=str(path), write_to=str(path.with_suffix(".png")))
                path.unlink()


def render_svg(path, size):
    data = cairosvg.svg2png(url=str(path), output_width=size, output_height=size)
    with Image.open(BytesIO(data)) as source:
        if source.convert("RGBA").getchannel("A").getextrema() != (255, 255):
            raise ValueError(f"The rendered icon must be fully opaque: {path}")
        return source.convert("RGB")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--composer-only", action="store_true",
                        help="rebuild only the Icon Composer depth artwork")
    args = parser.parse_args()
    composer_layers()
    if args.composer_only:
        return
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
