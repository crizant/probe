#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }
[[ $# -eq 1 ]] || { echo 'Usage: scripts/verify-deb.sh OUTPUT_DIR' >&2; exit 2; }
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/workspace-version.sh
source "$repo_root/scripts/workspace-version.sh"
version="$(workspace_version "$repo_root/Cargo.toml")" || die 'cannot read workspace version'
deb_version="$(deb_version_from_semver "$version")" || die "unsupported SemVer: $version"
output_dir="$1"
[[ -d "$output_dir" ]] || die "DEB output directory not found: $output_dir"
for tool in dpkg-deb file desktop-file-validate; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
done
extract="$(mktemp -d)"
trap 'rm -rf "$extract"' EXIT
for name in probe probe-desktop; do
  asset="$name"
  [[ "$name" != probe ]] || asset=probe-cli
  package="$output_dir/$asset-$version-linux-x64.deb"
  [[ -f "$package" ]] || die "missing DEB $package"
  [[ "$(dpkg-deb --field "$package" Package)" == "$name" ]] || die "incorrect package name: $package"
  [[ "$(dpkg-deb --field "$package" Version)" == "$deb_version" ]] || die "incorrect version: $package"
  [[ "$(dpkg-deb --field "$package" Architecture)" == amd64 ]] || die "incorrect architecture: $package"
  dpkg-deb --info "$package" >/dev/null
  root="$extract/$name"
  dpkg-deb --extract "$package" "$root"
  [[ -x "$root/usr/bin/$name" ]] || die "missing executable: $name"
  description="$(file -b "$root/usr/bin/$name")"
  [[ "$description" == *"ELF 64-bit"*"executable"*x86-64* ]] || die "not an x86_64 ELF executable: $name"
  cmp -s "$root/usr/share/doc/$name/copyright" "$repo_root/LICENSE" || die "incorrect license: $name"
  cmp -s "$root/usr/share/doc/$name/README.md" "$repo_root/README.md" || die "incorrect README: $name"
  if [[ "$name" == probe ]]; then
    help_output="$("$root/usr/bin/probe" --help)"
    [[ -n "$help_output" ]] || die 'packaged CLI produced no --help output'
  else
    depends="$(dpkg-deb --field "$package" Depends)"
    [[ ", $depends," == *", hicolor-icon-theme,"* ]] || die 'missing hicolor-icon-theme dependency'
    [[ "$(dpkg-deb --field "$package" Recommends)" == "probe (= $deb_version)" ]] || die 'incorrect CLI recommendation'
    desktop_file=usr/share/applications/dev.probe.desktop.desktop
    cmp -s "$root/$desktop_file" "$repo_root/packaging/linux/dev.probe.desktop.desktop" || die 'incorrect desktop file'
    desktop-file-validate "$root/$desktop_file"
    for size in 16 24 32 48 64 128 256 512; do
      icon="hicolor/${size}x${size}/apps/dev.probe.desktop.png"
      cmp -s "$root/usr/share/icons/$icon" "$repo_root/crates/desktop/assets/app-icon/linux/$icon" || die "incorrect icon: $size"
    done
  fi
done
echo "Verified Probe $version CLI and desktop DEBs in $output_dir"
