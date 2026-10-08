#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/build-rpm.sh [OPTIONS]

Package prebuilt Probe binaries as CLI and desktop RPMs.
The package version is the workspace version in Cargo.toml.

Options:
  --cli PATH           Prebuilt probe binary (default: target/release/probe)
  --desktop PATH       Prebuilt probe-desktop binary
                       (default: target/release/probe-desktop)
  --output-dir DIR     Directory for the release RPM files (default: dist/rpm)
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
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
  || die "RPM packaging supports a numeric workspace version such as 0.10.6, got ${version}"

if [[ -n "$expect_version" && "$expect_version" != "$version" ]]; then
  die "workspace version ${version} does not match expected version ${expect_version}"
fi

cli_bin="${cli_bin:-$repo_root/target/release/probe}"
desktop_bin="${desktop_bin:-$repo_root/target/release/probe-desktop}"
output_dir="${output_dir:-$repo_root/dist/rpm}"

[[ -f "$cli_bin" ]] || die "CLI binary not found: ${cli_bin}"
[[ -f "$desktop_bin" ]] || die "desktop binary not found: ${desktop_bin}"
[[ -x "$cli_bin" && -x "$desktop_bin" ]] || die "prebuilt binaries must be executable"

command -v rpmbuild >/dev/null 2>&1 || die "rpmbuild is required"
command -v rpm >/dev/null 2>&1 || die "rpm is required"
command -v file >/dev/null 2>&1 || die "file is required"
command -v desktop-file-validate >/dev/null 2>&1 || die "desktop-file-validate is required"

absolute_path() {
  local path="$1"
  local dir base
  dir="$(cd "$(dirname "$path")" && pwd)"
  base="$(basename "$path")"
  printf '%s/%s\n' "$dir" "$base"
}

cli_bin="$(absolute_path "$cli_bin")"
desktop_bin="$(absolute_path "$desktop_bin")"

elf_arch() {
  local description
  description="$(file -b "$1")"
  case "$description" in
    *"ELF "*x86-64* | *"ELF "*x86_64*)
      printf 'x86_64\n'
      ;;
    *"ELF "*aarch64*)
      printf 'aarch64\n'
      ;;
    *)
      die "unsupported ELF binary $1 (${description})"
      ;;
  esac
}

rpm_arch="$(elf_arch "$cli_bin")"
desktop_arch="$(elf_arch "$desktop_bin")"
[[ "$rpm_arch" == "$desktop_arch" ]] \
  || die "CLI architecture ${rpm_arch} does not match desktop architecture ${desktop_arch}"

case "$rpm_arch" in
  x86_64)
    platform="linux-x64"
    ;;
  aarch64)
    platform="linux-arm64"
    ;;
esac

for tool_path in \
  "$repo_root/packaging/rpm/probe.spec" \
  "$repo_root/packaging/linux/dev.probe.desktop.desktop" \
  "$repo_root/LICENSE" \
  "$repo_root/README.md" \
  "$repo_root/crates/desktop/assets/app-icon/linux/hicolor"; do
  [[ -e "$tool_path" ]] || die "missing packaging input: ${tool_path}"
done

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
topdir="$stage/rpmbuild"
mkdir -p "$topdir"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS,TMP}

changelog_date="$(LC_ALL=C date +"%a %b %e %Y")"
echo "Packaging Probe ${version} RPMs from the supplied binaries"

if ! rpmbuild -bb \
  --target "$rpm_arch" \
  --define "_topdir ${topdir}" \
  --define "_tmppath ${topdir}/TMP" \
  --define "probe_version ${version}" \
  --define "probe_changelog_date ${changelog_date}" \
  --define "probe_cli ${cli_bin}" \
  --define "probe_desktop ${desktop_bin}" \
  --define "probe_desktop_file ${repo_root}/packaging/linux/dev.probe.desktop.desktop" \
  --define "probe_license ${repo_root}/LICENSE" \
  --define "probe_readme ${repo_root}/README.md" \
  --define "probe_icon_root ${repo_root}/crates/desktop/assets/app-icon/linux/hicolor" \
  "$repo_root/packaging/rpm/probe.spec" \
  >"$stage/rpmbuild.log" 2>&1; then
  cat "$stage/rpmbuild.log" >&2
  die "rpmbuild failed"
fi

produced="$stage/produced"
mkdir -p "$produced"
found=0
while IFS= read -r rpm_path; do
  [[ -n "$rpm_path" ]] || continue
  cp -p "$rpm_path" "$produced/"
  found=$((found + 1))
done < <(find "$topdir/RPMS" -type f -name '*.rpm')

[[ "$found" -eq 2 ]] \
  || die "this build produced ${found} RPM(s) under ${topdir}/RPMS; expected the CLI and desktop packages"

cli_rpm=""
desktop_rpm=""
while IFS= read -r rpm_path; do
  [[ -n "$rpm_path" ]] || continue
  name="$(rpm -qp --queryformat '%{NAME}' "$rpm_path")"
  packaged_version="$(rpm -qp --queryformat '%{VERSION}' "$rpm_path")"
  [[ "$packaged_version" == "$version" ]] \
    || die "${name} RPM version ${packaged_version} does not match workspace version ${version}"
  case "$name" in
    probe)
      [[ -z "$cli_rpm" ]] || die "this build produced more than one probe RPM"
      cli_rpm="$rpm_path"
      ;;
    probe-desktop)
      [[ -z "$desktop_rpm" ]] || die "this build produced more than one probe-desktop RPM"
      desktop_rpm="$rpm_path"
      ;;
    *)
      die "this build produced unexpected RPM package ${name}"
      ;;
  esac
done < <(find "$produced" -maxdepth 1 -type f -name '*.rpm')

[[ -n "$cli_rpm" && -n "$desktop_rpm" ]] \
  || die "this build did not produce both the probe and probe-desktop RPMs"

release_dir="$stage/release"
mkdir -p "$release_dir"
cp -p "$cli_rpm" "$release_dir/probe-cli-${version}-${platform}.rpm"
cp -p "$desktop_rpm" "$release_dir/probe-desktop-${version}-${platform}.rpm"

"$repo_root/scripts/verify-rpm.sh" "$release_dir"

mkdir -p "$output_dir"
find "$output_dir" -mindepth 1 -maxdepth 1 -type f -name '*.rpm' -delete
cp -p "$release_dir"/*.rpm "$output_dir/"
"$repo_root/scripts/verify-rpm.sh" "$output_dir"

echo "RPM packages:"
echo "  $output_dir/probe-cli-${version}-${platform}.rpm"
echo "  $output_dir/probe-desktop-${version}-${platform}.rpm"
