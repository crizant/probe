#!/usr/bin/env python3
"""Compile Probe's Icon Composer assets into a cargo-bundle app, before signing."""

import argparse
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
ICONS = ROOT / "crates/desktop/assets/app-icon/macos"
ICON_NAME = "Probe"


def catalog_info(path):
    return json.loads(subprocess.check_output(["xcrun", "assetutil", "--info", str(path)]))


def verify_catalog(path):
    entries = catalog_info(path)
    # Apple's supported workflow generates flattened Default renditions for
    # pre-Tahoe macOS inside the catalog. Keep the original loose .icns too.
    if not any(e.get("AssetType") == "MultiSized Image" and e.get("Name") == ICON_NAME
               for e in entries):
        raise ValueError("Missing compiler-generated legacy icon fallback")
    for appearance in ("NSAppearanceNameAqua", "NSAppearanceNameDarkAqua", "ISAppearanceTintable"):
        groups = [e for e in entries if e.get("AssetType") == "IconGroup"
                  and e.get("Appearance") == appearance]
        if len(groups) != 3 or any(e.get("LayerCount") != 1 for e in groups):
            raise ValueError(f"Missing three independent depth groups for {appearance}")
        suffix = "dark" if appearance == "NSAppearanceNameDarkAqua" else "default"
        for group in groups:
            layer = group["Layers"][0]
            name = group['Name'].split('/')[-1]
            selected = "dark" if appearance == "ISAppearanceTintable" and name == "01-background-grid" else suffix
            if layer.get("Name") != f"Probe_Assets/{name}-{selected}":
                raise ValueError(f"Incorrect artwork selected for {appearance}")
        stack = next((e for e in entries if e.get("AssetType") == "IconImageStack"
                      and e.get("Name") == ICON_NAME and e.get("Appearance") == appearance), None)
        if stack is None:
            raise ValueError(f"Missing native icon stack for {appearance}")
        depth = list(dict.fromkeys(layer["Name"] for layer in stack["Layers"]
                                  if layer.get("AssetType") == "IconGroup"))
        if depth != ["Probe/01-background-grid", "Probe/02-probe", "Probe/03-target"]:
            raise ValueError(f"Incorrect native depth order for {appearance}")


def verify_bundle(app):
    contents = app / "Contents"
    with (contents / "Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    if info.get("CFBundleIconName") != ICON_NAME:
        raise ValueError("CFBundleIconName does not select the compiled icon")
    fallback = info.get("CFBundleIconFile", "")
    if fallback not in ("Probe", "Probe.icns"):
        raise ValueError("CFBundleIconFile does not select Probe's legacy icon")
    if (contents / "Resources/Probe.icns").read_bytes() != (ICONS / "Probe.icns").read_bytes():
        raise ValueError("The bundled legacy icon differs from the canonical .icns")
    verify_catalog(contents / "Resources/Assets.car")
    if list((contents / "Resources").glob("*.icon")):
        raise ValueError("Uncompiled Icon Composer source was bundled")


def compile_icon(app):
    version = subprocess.check_output(["xcrun", "xcodebuild", "-version"], text=True)
    if int(version.splitlines()[0].split()[1].split(".")[0]) < 26:
        raise ValueError("Native icons require full Xcode 26 or later (not Command Line Tools)")
    contents = app / "Contents"
    with (contents / "Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    if info.get("CFBundleIdentifier") != "dev.probe.desktop":
        raise ValueError("Expected a cargo-bundle Probe.app")
    resources = contents / "Resources"
    if (resources / "Assets.car").exists():
        raise ValueError("Assets.car already exists; rebuild the cargo bundle before compiling")
    if (resources / "Probe.icns").read_bytes() != (ICONS / "Probe.icns").read_bytes():
        raise ValueError("Expected the existing legacy Probe.icns from cargo-bundle")
    with tempfile.TemporaryDirectory(prefix="probe-icon-") as temporary:
        output = Path(temporary)
        partial = output / "icon-info.plist"
        subprocess.run([
            "xcrun", "actool", str(ICONS / "Probe.icon"),
            "--compile", str(output), "--platform", "macosx",
            "--minimum-deployment-target", info.get("LSMinimumSystemVersion", "11.0"),
            "--app-icon", ICON_NAME,
            "--output-partial-info-plist", str(partial),
            "--output-format", "human-readable-text", "--warnings", "--errors",
        ], check=True)
        verify_catalog(output / "Assets.car")
        with partial.open("rb") as stream:
            info.update(plistlib.load(stream))
        # Keep cargo-bundle's legacy artwork instead of actool's generated .icns.
        info["CFBundleIconFile"] = "Probe.icns"
        shutil.copyfile(output / "Assets.car", resources / "Assets.car")
        replacement = contents / "Info.plist.icon-tmp"
        with replacement.open("wb") as stream:
            plistlib.dump(info, stream, sort_keys=False)
        replacement.replace(contents / "Info.plist")
    verify_bundle(app)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path, help="path to cargo-bundle's Probe.app")
    parser.add_argument("--verify-only", action="store_true", help="inspect a final bundle without modifying it")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("Icon Composer compilation and verification require macOS")
    try:
        if args.verify_only:
            verify_bundle(args.app.resolve())
        else:
            compile_icon(args.app.resolve())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"error: {error}\n")
    print("Verified native Default/Dark/Mono stacks, generated legacy renditions, and original .icns")


if __name__ == "__main__":
    main()
