#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/verify-rpm.sh OUTPUT_DIR

Check the CLI and desktop RPMs in OUTPUT_DIR.
The expected version is the workspace version in Cargo.toml.
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

[[ $# -eq 1 ]] || {
  usage >&2
  exit 2
}

output_dir="$1"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/workspace-version.sh
source "$repo_root/scripts/workspace-version.sh"

[[ -d "$output_dir" ]] || die "RPM output directory not found: ${output_dir}"
command -v rpm >/dev/null 2>&1 || die "rpm is required"
command -v rpm2cpio >/dev/null 2>&1 || die "rpm2cpio is required"
command -v cpio >/dev/null 2>&1 || die "cpio is required"

version="$(workspace_version "$repo_root/Cargo.toml")" \
  || die "failed to read the workspace version from Cargo.toml"

shopt -s nullglob
rpm_files=("$output_dir"/*.rpm)
shopt -u nullglob
[[ ${#rpm_files[@]} -eq 2 ]] \
  || die "expected 2 RPMs in ${output_dir}, found ${#rpm_files[@]}"

cli_rpm=""
desktop_rpm=""
for rpm_path in "${rpm_files[@]}"; do
  name="$(rpm -qp --queryformat '%{NAME}' "$rpm_path")"
  packaged_version="$(rpm -qp --queryformat '%{VERSION}' "$rpm_path")"
  arch="$(rpm -qp --queryformat '%{ARCH}' "$rpm_path")"
  rpm -qp --info "$rpm_path" >/dev/null
  [[ "$packaged_version" == "$version" ]] \
    || die "$(basename "$rpm_path") version ${packaged_version} does not match workspace version ${version}"
  [[ "$arch" == "x86_64" || "$arch" == "aarch64" ]] \
    || die "$(basename "$rpm_path") has unsupported architecture ${arch}"
  case "$name" in
    probe)
      [[ -z "$cli_rpm" ]] || die "found more than one probe RPM"
      cli_rpm="$rpm_path"
      ;;
    probe-desktop)
      [[ -z "$desktop_rpm" ]] || die "found more than one probe-desktop RPM"
      desktop_rpm="$rpm_path"
      ;;
    *)
      die "unexpected RPM package ${name} ($(basename "$rpm_path"))"
      ;;
  esac
done

[[ -n "$cli_rpm" && -n "$desktop_rpm" ]] \
  || die "expected probe and probe-desktop RPMs in ${output_dir}"

package_requires() {
  local rpm_path="$1"
  rpm -qp --requires "$rpm_path" || die "could not read requirements for $(basename "$rpm_path")"
}

reject_distro_package_requires() {
  local rpm_path="$1"
  local requires="$2"
  [[ -n "$requires" ]] || die "$(basename "$rpm_path") has no RPM requirements"
  if grep -E '^(libX11-xcb|vulkan-loader(-devel)?|libvulkan1)([[:space:]]|$)' <<<"$requires"; then
    die "$(basename "$rpm_path") requires a distro-specific package name"
  fi
}

payload_contains() {
  local rpm_path="$1"
  local expected="$2"
  local payload
  payload="$(rpm -qpl "$rpm_path")" || die "could not list files in $(basename "$rpm_path")"
  grep -Fx "$expected" <<<"$payload" >/dev/null \
    || die "$(basename "$rpm_path") is missing ${expected}"
}

cli_requires="$(package_requires "$cli_rpm")"
desktop_requires="$(package_requires "$desktop_rpm")"
reject_distro_package_requires "$cli_rpm" "$cli_requires"
reject_distro_package_requires "$desktop_rpm" "$desktop_requires"
grep -Fx 'hicolor-icon-theme' <<<"$desktop_requires" >/dev/null \
  || die "desktop RPM does not require hicolor-icon-theme"

payload_contains "$cli_rpm" "/usr/bin/probe"
payload_contains "$cli_rpm" "/usr/share/licenses/probe/LICENSE"
payload_contains "$desktop_rpm" "/usr/bin/probe-desktop"
payload_contains "$desktop_rpm" "/usr/share/licenses/probe-desktop/LICENSE"
payload_contains "$desktop_rpm" "/usr/share/applications/dev.probe.desktop.desktop"
for size in 16 24 32 48 64 128 256 512; do
  payload_contains "$desktop_rpm" \
    "/usr/share/icons/hicolor/${size}x${size}/apps/dev.probe.desktop.png"
done

extract="$(mktemp -d)"
trap 'rm -rf "$extract"' EXIT
mkdir -p "$extract/cli" "$extract/desktop"
rpm2cpio "$cli_rpm" | (cd "$extract/cli" && cpio -id --quiet)
rpm2cpio "$desktop_rpm" | (cd "$extract/desktop" && cpio -id --quiet)

[[ -x "$extract/cli/usr/bin/probe" ]] || die "packaged CLI binary is not executable"
[[ -x "$extract/desktop/usr/bin/probe-desktop" ]] || die "packaged desktop binary is not executable"
help_output="$("$extract/cli/usr/bin/probe" --help)"
[[ -n "$help_output" ]] || die "packaged CLI binary produced no --help output"

cmp -s \
  "$extract/desktop/usr/share/applications/dev.probe.desktop.desktop" \
  "$repo_root/packaging/linux/dev.probe.desktop.desktop" \
  || die "packaged desktop file does not match packaging/linux/dev.probe.desktop.desktop"
cmp -s "$extract/cli/usr/share/licenses/probe/LICENSE" "$repo_root/LICENSE" \
  || die "packaged CLI license does not match LICENSE"
cmp -s "$extract/desktop/usr/share/licenses/probe-desktop/LICENSE" "$repo_root/LICENSE" \
  || die "packaged desktop license does not match LICENSE"
for size in 16 24 32 48 64 128 256 512; do
  cmp -s \
    "$extract/desktop/usr/share/icons/hicolor/${size}x${size}/apps/dev.probe.desktop.png" \
    "$repo_root/crates/desktop/assets/app-icon/linux/hicolor/${size}x${size}/apps/dev.probe.desktop.png" \
    || die "packaged ${size}x${size} icon does not match the repository icon"
done

echo "Verified Probe ${version} CLI and desktop RPMs in ${output_dir}"
