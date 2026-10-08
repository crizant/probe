# Probe application icon

The canonical artwork is editable SVG: `source/probe-app-icon.svg` (light) and
`source/probe-app-icon-dark.svg` (dark). Both variants share the same geometry,
including the aligned needle and collar, with different color palettes.
The close-up framing shows the tip, contact target, finger guard, and three grip
rings, with equal left and bottom target margins. The rest of the handle and
cable are outside the composition.
Windows, Linux, and loose legacy macOS assets are rendered directly from the
light SVG at each required size. The native macOS icon uses both palettes, selected
by macOS; Probe does not switch its icon in Rust or GPUI.

The orange palette follows the vivid golden-orange reference supplied during
native icon integration: the dark base is saturated `#FF9800`, with `#FFB21A`
and `#FFC43D` highlights and reddish burnt-orange grip rings (`#C96201`).
The light base is a stronger, deeper orange (`#F08000`) with saturated amber
highlights and darker rings for contrast against porcelain.
The grid retains the Porcelain Honey and Graphite
Honey background palettes. The needle and lower collar share the grip's exact
-45° centerline (`x + y = 1082.5`), which also passes through the contact target.
The target is composited above the needle in both source SVGs and native layers.

The `source/probe-app-icon-1024.png` and `source/probe-app-icon-dark-1024.png` files
are generated previews of the SVGs; edit the SVGs and regenerate rather than
editing or resizing those PNGs.

## Platform assets

- `macos/Probe.icon/` is the editable Icon Composer package, with Default and Dark
  artwork derived from the canonical SVGs.
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
python3 scripts/compile-macos-icon.py target/release/bundle/osx/Probe.app
codesign --force --deep --sign - target/release/bundle/osx/Probe.app
python3 scripts/compile-macos-icon.py --verify-only target/release/bundle/osx/Probe.app
codesign --verify --deep --strict target/release/bundle/osx/Probe.app
open target/release/bundle/osx/Probe.app
```

The ad-hoc signature is suitable for local development only. Distribution builds
need Developer ID signing and notarization. The icon paths
in Cargo metadata are workspace-root-relative because cargo-bundle 0.11 expands
its resource globs from the process working directory.

## Native macOS compilation and appearances

Use full **Xcode 26 or later**, selected with `xcode-select` or `DEVELOPER_DIR`.
Command Line Tools alone do not include `actool`. Icon Composer is bundled in
Xcode under Xcode → Open Developer Tool → Icon Composer; use the bundled version
when editing this package. Compilation was checked with Xcode 26.6 (17F113).
Python 3.9 or later is required for the packaging script; no Python packages are
needed for compilation. CairoSVG/Pillow/Cairo are needed only for regeneration.

Apple's [Icon Composer guide](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer)
and the installed `man actool` describe the build pipeline. A `.icon` is source,
not a resource to copy into a finished application. The non-Xcode build invokes
Apple's asset compiler directly:

```sh
xcrun actool crates/desktop/assets/app-icon/macos/Probe.icon \
  --compile "$output_directory" --platform macosx \
  --minimum-deployment-target 11.0 --app-icon Probe \
  --output-partial-info-plist "$output_directory/icon-info.plist"
```

`scripts/compile-macos-icon.py` compiles into a temporary directory, validates the
catalog, copies `Assets.car` into `Probe.app/Contents/Resources`, and merges
Apple's partial plist into `Contents/Info.plist`. `CFBundleIconName=Probe` selects
the native icon; `CFBundleIconFile=Probe.icns` retains the original loose icon.
Run it **after cargo-bundle and before signing**. Release CI uses this same step
and verifies the resulting signed bundle before archiving it. For a target build,
use `target/<target-triple>/release/bundle/osx/Probe.app`. Rebuild the cargo bundle
before repeating compilation; the script refuses to replace an existing catalog.

Apple generates flattened Default renditions in the catalog for older macOS
versions. These may take precedence over the loose `.icns` on pre-Tahoe systems;
they retain the light composition, with Apple's enclosure. The original `.icns`
is also packaged unchanged for consumers that use `CFBundleIconFile`. This does
not raise Probe's deployment target. The script uses the bundle's minimum version
when specified, otherwise 11.0 (the current binary's deployment target).

An undocumented fallback-disabling flag discussed on Apple's developer forums
was evaluated, but Xcode 26.6 still generates fallback renditions with it. Probe
uses Apple's normal compiler output rather than depending on that flag.

### Depth and artwork

Back to front: **01 background/grid → 02 probe → 03 target**. Icon
Composer stores this as the reverse array order in `icon.json` (front to back).
The target is above the needle. Highlights, grip bands, collar and finger guard
remain with their object; floating highlight planes would change occlusion.
The source framing transform is preserved in both foreground layers, along with
the path coordinates, clips, and gradient stops.

Each group has Default and Dark image specializations, using the light and dark SVG palettes respectively. SVG foreground layers remain vector artwork.
The background/grid alone is a 1024px PNG rendered from its canonical SVG shapes,
because Apple's SVG importer does not support the grid's pattern fill. Unused
pattern definitions are omitted from foreground SVGs. Added glass, specular,
blur, translucency and shadow effects are disabled to preserve the existing
illustration; macOS still applies its native icon enclosure and presentation.

Mono uses the existing dark background/grid image and a graphite canvas fill,
annotated with Icon Composer's `tinted` specialization. This prevents the light
background from becoming a white plate in Mono/Clear. The foreground uses the
system's generated monochrome rendition; no separate Mono drawing is needed.
Clear Light still receives a lighter gray glass treatment from macOS, while
Clear Dark uses darker graphite. Clear and Tinted are system icon styles, not
additional Probe runtime themes. Review Clear in the Dock before release. Apple's
[Appearance settings guide](https://support.apple.com/guide/mac-help/mchlp1225/mac)
explains Default, Dark (Always/Auto), Clear (Light/Dark/Auto), and Tinted selection.
Changing only the app's light/dark theme does not select Dark icons when the
system's **Icon & widget style** is set to Default.

### Verification

The packaging verifier checks all three Default/Dark/Mono groups, their artwork
references, native stacks, generated legacy renditions, and exact preservation of
the loose `.icns`. It also rejects uncompiled `.icon` resources. Signature
verification runs separately after signing. These checks do not prove visual
switching in Dock or rendering on an older OS.

Local validation on macOS 26.7.1 with Xcode 26.6 checked Default, Dark and Mono
in Icon Composer, and verified the signed app both before and after ZIP extraction.
Apple's bundled `ictool` also rendered ClearLight and ClearDark with the graphite
Mono specialization; Default/Dark renders matched the prior version exactly.
Dock appearance switching and visual rendering on pre-Tahoe
macOS still require the manual checks below; catalog inspection alone is not
evidence of those results.

For native visual verification, launch the final packaged app (quit another
running Probe instance first so Launch Services does not activate that copy).
In System Settings → Appearance → Icon & widget style, check Default, Dark →
Always, Clear → Light/Dark, and Tinted. Inspect the Dock icon; Finder may continue
to display Default. Restore the initial system setting afterward. Check small
and large sizes, the target above the needle, grid, and grip rings. Copy
the same signed bundle to a pre-Tahoe macOS machine and check its light fallback.

## Documentation preview

`docs/assets/probe-app-icon.png` and `docs/assets/probe-app-icon-dark.png`
(relative to the workspace root) are 512px documentation previews with rounded
corners and transparent corner cutouts. Both are generated; README currently
uses only the light preview. Packaged icons remain fully opaque.

## Regeneration

Install [CairoSVG](https://cairosvg.org/documentation/) and Pillow. CairoSVG also
requires the system Cairo library (for example, `brew install cairo` on macOS or
`sudo apt-get install libcairo2` on Debian/Ubuntu). From the workspace root, run:

```sh
python3 -m pip install CairoSVG Pillow
python3 scripts/generate-app-icons.py
# Rebuild only Icon Composer artwork without changing legacy/Windows/Linux assets:
python3 scripts/generate-app-icons.py --composer-only
```

If CairoSVG cannot locate Homebrew's Cairo on Apple Silicon, run regeneration
with `DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib` in the environment.

Edit the canonical SVGs, then regenerate the derived depth artwork; edit effects
and appearance annotations in Icon Composer. The generator leaves `icon.json`
untouched. If the source structure changes, update its explicit depth split.

The script renders both SVGs to 1024px source previews, validates that rendered
icons are opaque, and rebuilds all platform assets from the light SVG at their
native sizes. It applies the same antialiased rounded-corner mask to the light
and dark documentation previews. Their corner radius is 112px at 512px resolution.

## Design source

The artwork is a vector redraw of the original golden-orange test probe and
contact rings on a subtle grid background. Clean color faces replace the original
raster texture while retaining the composition in light and dark variants.
