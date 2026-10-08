#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/verify-rpm.sh OUTPUT_DIR

Check the Probe CLI and desktop RPMs for the workspace version in OUTPUT_DIR.
Other files in that directory are ignored.
Release filenames use the workspace SemVer. The RPM Version field uses the
tilde form of a prerelease, so 0.11.0-beta.1 is 0.11.0~beta.1.
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
rpm_version="$(rpm_version_from_semver "$version")" \
  || die "workspace version ${version} cannot be packaged as an RPM"

cli_rpm="${output_dir}/probe-cli-${version}-linux-x64.rpm"
desktop_rpm="${output_dir}/probe-desktop-${version}-linux-x64.rpm"
[[ -f "$cli_rpm" ]] || die "missing CLI RPM ${cli_rpm}"
[[ -f "$desktop_rpm" ]] || die "missing desktop RPM ${desktop_rpm}"

check_rpm_identity() {
  local rpm_path="$1"
  local expected_name="$2"
  local name packaged_version arch
  name="$(rpm -qp --queryformat '%{NAME}' "$rpm_path")"
  packaged_version="$(rpm -qp --queryformat '%{VERSION}' "$rpm_path")"
  arch="$(rpm -qp --queryformat '%{ARCH}' "$rpm_path")"
  rpm -qp --info "$rpm_path" >/dev/null
  [[ "$name" == "$expected_name" ]] \
    || die "$(basename "$rpm_path") package name is ${name}, expected ${expected_name}"
  [[ "$packaged_version" == "$rpm_version" ]] \
    || die "$(basename "$rpm_path") RPM version ${packaged_version} does not match ${rpm_version} for workspace version ${version}"
  [[ "$arch" == "x86_64" ]] \
    || die "$(basename "$rpm_path") architecture is ${arch}; RPM packages support only x86_64"
}

check_rpm_identity "$cli_rpm" probe
check_rpm_identity "$desktop_rpm" probe-desktop

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
