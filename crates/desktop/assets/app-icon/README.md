# Probe application icon

The source artwork is `source/probe-app-icon-1024.png`. Derive platform assets
from that file rather than resizing an already reduced image.

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

The current artwork is a flattened sRGB icon. Apple accepts flattened app icons,
but an Icon Composer project with independently reactive Liquid Glass layers would
require separate background, cable, probe, and collar artwork.

## Design source

The selected concept was generated with the built-in image tool and refined as a
clean, softly dimensional cartoon illustration: a compact dark cabled test probe
with a sturdy blunt steel sensor, a peach collar, recessed grip details on its
cylindrical graphite housing, a cable continuing beyond the frame, and a warm-white
background. It intentionally contains no text, monogram, abstract emblem, sharp tip,
or weapon-like detail.
