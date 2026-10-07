# Probe application icon

The canonical artwork is editable SVG: `source/probe-app-icon.svg` (light) and
`source/probe-app-icon-dark.svg` (dark). Both variants share the same geometry,
including the aligned needle and collar, with different color palettes. Platform
assets are rendered directly from the light SVG at each required size. The dark
variant is retained for a future appearance switch.

The main orange fills match the app's accent colors: `#E7821B` for Porcelain Honey
and `#D98E26` for Graphite Honey. Highlights and shadows use shades of those accents
and the theme's hover/pressed colors. The needle and lower collar share the grip's
exact -45° centerline (`x + y = 1082.5`), which also passes through the contact
target's center.

The `source/probe-app-icon-1024.png` and `source/probe-app-icon-dark-1024.png` files
are generated previews of the SVGs; edit the SVGs and regenerate rather than
editing or resizing those PNGs.

## Platform assets

- `macos/Probe.icns` contains 16, 32, 64, 128, 256, 512, and 1024 pixel PNG
  representations.
- `macos/Probe.iconset/` contains the conventional macOS 1x and 2x source PNGs.
- `windows/Probe.ico` contains 16, 24, 32, 48, 64, 128, and 256 pixel PNG
  representations.
- `windows/png/` contains the individual Windows source PNGs.
- `linux/hicolor/` follows the freedesktop hicolor directory layout and uses
  the desktop application identifier `dev.probe.desktop`.

The desktop crate's bundle metadata references these files for macOS, Windows,
and Linux packages. Its build script also embeds `windows/Probe.ico` into the
Windows executable, while the GPUI application and Linux desktop integration
share the stable `dev.probe.desktop` application identifier.

Build and launch a local macOS app from the workspace root with:

```sh
cargo bundle --release -p probe-desktop --format osx
codesign --force --deep --sign - target/release/bundle/osx/Probe.app
open target/release/bundle/osx/Probe.app
```

The ad-hoc signature is suitable for local development only. Distribution builds
need the project's Developer ID signing and notarization workflow. The icon paths
in Cargo metadata are workspace-root-relative because cargo-bundle 0.11 expands
its resource globs from the process working directory.

The light artwork has an opaque, edge-to-edge porcelain grid background; the dark
variant uses a graphite grid. Platform exports are fully opaque square raster
icons. Apple accepts flattened app icons,
but an Icon Composer project with independently reactive Liquid Glass layers would
require separate background, cable, probe, and collar artwork.

## Documentation preview

`docs/assets/probe-app-icon.png` (relative to the workspace root) is a 512px
README-only preview with rounded corners and transparent corner cutouts.
Packaged icons remain fully opaque.

## Regeneration

Install [CairoSVG](https://cairosvg.org/documentation/) and Pillow. CairoSVG also
requires the system Cairo library (for example, `brew install cairo` on macOS or
`sudo apt-get install libcairo2` on Debian/Ubuntu). From the workspace root, run:

```sh
python3 -m pip install CairoSVG Pillow
python3 scripts/generate-app-icons.py
```

The script renders both SVGs to 1024px source previews, validates that rendered
icons are opaque, and rebuilds all platform assets from the light SVG at their
native sizes. It applies an antialiased rounded-corner mask only to the README
preview. The preview's corner radius is 112px at 512px resolution.

## Design source

The artwork is a vector redraw of the original golden-orange test probe and
contact rings on a subtle grid background. Clean color faces replace the original
raster texture while retaining the composition in light and dark variants.
