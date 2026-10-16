#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/build-deb.sh [OPTIONS]

Package prebuilt Linux x86_64 Probe binaries as CLI and desktop DEBs.
The package version is the workspace version in Cargo.toml.
Release filenames keep that SemVer. The DEB Version field uses tilde
prerelease syntax, so 0.11.0-beta.1 is packaged as 0.11.0~beta.1-1.

Options:
  --cli PATH           Prebuilt probe binary (default: target/release/probe)
  --desktop PATH       Prebuilt probe-desktop binary
                       (default: target/release/probe-desktop)
  --output-dir DIR     Directory for the release DEB files (default: dist/deb)
  --expect-version VER Fail when Cargo.toml is not VER
  -h, --help           Show this help message
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/workspace-version.sh
source "$repo_root/scripts/workspace-version.sh"

expect_version=""
cli_bin=""
desktop_bin=""
output_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cli)
      [[ $# -ge 2 ]] || die "--cli requires a path"
      cli_bin="$2"
      shift 2
      ;;
    --desktop)
      [[ $# -ge 2 ]] || die "--desktop requires a path"
      desktop_bin="$2"
      shift 2
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || die "--output-dir requires a path"
      output_dir="$2"
      shift 2
      ;;
    --expect-version)
      [[ $# -ge 2 ]] || die "--expect-version requires a version"
      expect_version="$2"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

version="$(workspace_version "$repo_root/Cargo.toml")" \
  || die "failed to read the workspace version from Cargo.toml"
deb_version="$(deb_version_from_semver "$version")" \
  || die "DEB packaging supports workspace versions such as 0.10.6 or 0.11.0-beta.1, got ${version}"

if [[ -n "$expect_version" && "$expect_version" != "$version" ]]; then
  die "workspace version ${version} does not match expected version ${expect_version}"
fi

cli_bin="${cli_bin:-$repo_root/target/release/probe}"
desktop_bin="${desktop_bin:-$repo_root/target/release/probe-desktop}"
output_dir="${output_dir:-$repo_root/dist/deb}"

[[ -f "$cli_bin" ]] || die "CLI binary not found: ${cli_bin}"
[[ -f "$desktop_bin" ]] || die "desktop binary not found: ${desktop_bin}"
[[ -x "$cli_bin" && -x "$desktop_bin" ]] || die "prebuilt binaries must be executable"

for tool in dpkg dpkg-deb dpkg-shlibdeps file desktop-file-validate; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
done

for binary in "$cli_bin" "$desktop_bin"; do
  description="$(file -b "$binary")"
  case "$description" in
    *"ELF 64-bit"*"executable"*x86-64* | *"ELF 64-bit"*"executable"*x86_64*) ;;
    *) die "DEB packages support only Linux x86_64 executables; $binary is $description" ;;
  esac
done

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/release" "$stage/debian"
# dpkg-shlibdeps expects a Debian source control file, even with -O.
cat >"$stage/debian/control" <<'EOF'
Source: probe
Section: devel
Priority: optional
Maintainer: Probe Contributors <probe@users.noreply.github.com>

Package: probe
Architecture: amd64
Description: Fast, native, local-first API client
EOF

package_binary() {
  local name="$1" binary="$2" asset="$3"
  local root="$stage/debian/$name" dependencies
  mkdir -p "$root/DEBIAN"
  install -D -p -m 0755 "$binary" "$root/usr/bin/$name"
  install -D -p -m 0644 "$repo_root/LICENSE" "$root/usr/share/doc/$name/copyright"
  install -D -p -m 0644 "$repo_root/README.md" "$root/usr/share/doc/$name/README.md"
  # Analyze the staged ELF, not a build dependency list. Missing libraries or
  # missing Debian shlibs/symbols metadata are fatal; never guess dependencies.
  if ! dependencies="$(cd "$stage" && LC_ALL=C dpkg-shlibdeps -O \
    "-edebian/$name/usr/bin/$name" 2>"$stage/$name-shlibdeps.log")"; then
    cat "$stage/$name-shlibdeps.log" >&2
    die "could not determine runtime dependencies for $name"
  fi
  cat "$stage/$name-shlibdeps.log" >&2
  # Some dpkg versions only warn for missing unversioned SONAMEs. Those
  # libraries are still needed by this ELF; refuse incomplete metadata.
  if grep -q 'cannot find library' "$stage/$name-shlibdeps.log"; then
    die "could not determine runtime dependencies for $name"
  fi
  [[ "$dependencies" == shlibs:Depends=* ]] \
    || die "dpkg-shlibdeps did not return Depends for $name"
  dependencies="${dependencies#shlibs:Depends=}"
  if [[ "$name" == probe-desktop ]]; then
    dependencies="${dependencies:+$dependencies, }hicolor-icon-theme"
    install -D -p -m 0644 "$repo_root/packaging/linux/dev.probe.desktop.desktop" \
      "$root/usr/share/applications/dev.probe.desktop.desktop"
    desktop-file-validate "$root/usr/share/applications/dev.probe.desktop.desktop"
    for size in 16 24 32 48 64 128 256 512; do
      install -D -p -m 0644 \
        "$repo_root/crates/desktop/assets/app-icon/linux/hicolor/${size}x${size}/apps/dev.probe.desktop.png" \
        "$root/usr/share/icons/hicolor/${size}x${size}/apps/dev.probe.desktop.png"
    done
  fi
  cat >"$root/DEBIAN/control" <<EOF
Package: $name
Version: $deb_version
Architecture: amd64
Section: devel
Priority: optional
Maintainer: Probe Contributors <probe@users.noreply.github.com>
Homepage: https://github.com/crizant/probe
Installed-Size: $(du -sk "$root/usr" | awk '{print $1}')
Description: Fast, native, local-first API client ($name)
 Probe provides native tools for local API work.
EOF
  if [[ -n "$dependencies" ]]; then
    printf 'Depends: %s\n' "$dependencies" >>"$root/DEBIAN/control"
  fi
  if [[ "$name" == probe-desktop ]]; then
    printf 'Recommends: probe (= %s)\n' "$deb_version" >>"$root/DEBIAN/control"
  fi
  # dpkg-deb only archives the payload: no stripping or other ELF rewriting.
  find "$root" -type d -exec chmod 0755 {} +
  dpkg-deb --root-owner-group -Zxz --build "$root" \
    "$stage/release/$asset-$version-linux-x64.deb"
  dpkg-deb --extract "$stage/release/$asset-$version-linux-x64.deb" "$stage/check-$name"
  cmp -s "$binary" "$stage/check-$name/usr/bin/$name" \
    || die "packaged $name differs from the supplied binary"
}

package_binary probe "$cli_bin" probe-cli
package_binary probe-desktop "$desktop_bin" probe-desktop
"$repo_root/scripts/verify-deb.sh" "$stage/release"

mkdir -p "$output_dir"
for asset in probe-cli probe-desktop; do
  cp -p "$stage/release/$asset-$version-linux-x64.deb" "$output_dir/$asset-$version-linux-x64.deb"
done
"$repo_root/scripts/verify-deb.sh" "$output_dir"
echo "DEB packages:"
echo "  $output_dir/probe-cli-$version-linux-x64.deb"
echo "  $output_dir/probe-desktop-$version-linux-x64.deb"
