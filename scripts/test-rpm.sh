#!/usr/bin/env bash
set -euo pipefail

die() {
  echo "error: $*" >&2
  exit 1
}

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/workspace-version.sh
source "$repo_root/scripts/workspace-version.sh"
cd "$repo_root"

version="$(workspace_version "$repo_root/Cargo.toml")" \
  || die "failed to read the workspace version from Cargo.toml"
[[ "$(rpm_version_from_semver "0.11.0")" == "0.11.0" ]] \
  || die "stable SemVer was changed for the RPM version"
[[ "$(rpm_version_from_semver "0.11.0-beta.1")" == "0.11.0~beta.1" ]] \
  || die "prerelease SemVer was not mapped to an RPM version"
[[ "$(rpm_version_from_semver "0.11.0-rc.1-test")" == "0.11.0~rc.1~test" ]] \
  || die "a prerelease hyphen remained in the RPM version"
[[ "$(rpm_version_from_semver "0.11.0+build.1")" == "0.11.0+build.1" ]] \
  || die "build metadata was changed for the RPM version"
if rpm_version_from_semver "0.11"; then
  die "an incomplete version was accepted"
fi

if grep -E '^Version:[[:space:]]*[0-9]' packaging/rpm/probe.spec; then
  die "packaging/rpm/probe.spec hard-codes a version; inject probe_version at package time"
fi
grep -q '^Version:[[:space:]]*%{probe_version}[[:space:]]*$' packaging/rpm/probe.spec \
  || die "packaging/rpm/probe.spec must set Version from %{probe_version}"

if grep -E '^[[:space:]]*(BuildRequires|Requires):' packaging/rpm/probe.spec \
  | grep -E 'libX11-xcb|vulkan-loader|libvulkan1|cargo|rust'; then
  die "packaging/rpm/probe.spec uses a distro-specific or compile-time dependency"
fi

if grep -n '\[Desktop Entry\]' .github/workflows/release.yml; then
  die "release workflow embeds a desktop file; copy packaging/linux/dev.probe.desktop.desktop"
fi
grep -q 'packaging/linux/dev.probe.desktop.desktop' .github/workflows/release.yml \
  || die "release workflow does not install packaging/linux/dev.probe.desktop.desktop"

command -v gcc >/dev/null 2>&1 || die "gcc is required"
command -v rpmbuild >/dev/null 2>&1 || die "rpmbuild is required"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

cat >"$tmp/probe.c" <<'EOF'
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--help") == 0) {
#ifdef DESKTOP
        puts("probe-desktop");
#else
        puts("probe");
#endif
        return 0;
    }
    return 0;
}
EOF
gcc -o "$tmp/probe" "$tmp/probe.c"
gcc -DDESKTOP -o "$tmp/probe-desktop" "$tmp/probe.c"
description="$(file -b "$tmp/probe")"
case "$description" in
  *x86-64* | *x86_64*) ;;
  *)
    die "RPM packaging tests require x86_64 ELF binaries, got ${description}"
    ;;
esac
platform="linux-x64"

stale="$tmp/stale"
mkdir -p "$stale"
printf 'stale-cli\n' >"$stale/probe-cli-${version}-linux-x64.rpm"
printf 'stale-desktop\n' >"$stale/probe-desktop-${version}-linux-x64.rpm"

set +e
bash "$repo_root/scripts/build-rpm.sh" \
  --cli "$tmp/probe" \
  --desktop "$tmp/missing-probe-desktop" \
  --output-dir "$stale" \
  >"$tmp/failed.out" 2>"$tmp/failed.err"
status=$?
set -e
[[ "$status" -ne 0 ]] || die "build with a missing desktop binary reported success"
[[ "$(cat "$stale/probe-cli-${version}-linux-x64.rpm")" == "stale-cli" ]] \
  || die "failed build replaced or removed a stale CLI RPM"
[[ "$(cat "$stale/probe-desktop-${version}-linux-x64.rpm")" == "stale-desktop" ]] \
  || die "failed build replaced or removed a stale desktop RPM"
if grep -q 'RPM packages:' "$tmp/failed.out"; then
  die "failed build reported packaged RPMs"
fi

output="$tmp/out"
mkdir -p "$output"
printf 'unrelated\n' >"$output/other-tool-1.2.3-1.x86_64.rpm"
printf 'keep\n' >"$output/notes.txt"
printf 'replace-me\n' >"$tmp/stale-probe-rpm"
cp "$tmp/stale-probe-rpm" "$output/probe-cli-${version}-${platform}.rpm"
bash "$repo_root/scripts/build-rpm.sh" \
  --cli "$tmp/probe" \
  --desktop "$tmp/probe-desktop" \
  --expect-version "$version" \
  --output-dir "$output"
[[ "$(cat "$output/other-tool-1.2.3-1.x86_64.rpm")" == "unrelated" ]] \
  || die "successful build removed an unrelated RPM"
[[ "$(cat "$output/notes.txt")" == "keep" ]] \
  || die "successful build removed an unrelated file"
[[ -f "$output/probe-cli-${version}-${platform}.rpm" ]] || die "missing CLI release RPM"
[[ -f "$output/probe-desktop-${version}-${platform}.rpm" ]] || die "missing desktop release RPM"
if cmp -s "$output/probe-cli-${version}-${platform}.rpm" "$tmp/stale-probe-rpm"; then
  die "successful build kept a stale Probe CLI RPM"
fi
bash "$repo_root/scripts/verify-rpm.sh" "$output"

script_bin="$tmp/not-elf-probe"
script_desktop="$tmp/not-elf-desktop"
printf '#!/bin/sh\nprintf "probe\\n"\n' >"$script_bin"
cp "$script_bin" "$script_desktop"
chmod +x "$script_bin" "$script_desktop"
set +e
bash "$repo_root/scripts/build-rpm.sh" \
  --cli "$script_bin" \
  --desktop "$script_desktop" \
  --output-dir "$tmp/arch-out" \
  >"$tmp/arch.out" 2>"$tmp/arch.err"
status=$?
set -e
[[ "$status" -ne 0 ]] || die "a non-x86_64 binary was packaged"
grep -q 'x86_64' "$tmp/arch.err" || die "unsupported architecture error did not mention x86_64"

stage_tree() {
  local dest="$1"
  local cargo_version="$2"
  mkdir -p \
    "$dest/scripts" \
    "$dest/packaging/linux" \
    "$dest/packaging/rpm" \
    "$dest/crates/desktop/assets/app-icon/linux"
  cp -p "$repo_root/scripts/build-rpm.sh" "$dest/scripts/build-rpm.sh"
  cp -p "$repo_root/scripts/verify-rpm.sh" "$dest/scripts/verify-rpm.sh"
  cp -p "$repo_root/scripts/workspace-version.sh" "$dest/scripts/workspace-version.sh"
  cp -p "$repo_root/packaging/rpm/probe.spec" "$dest/packaging/rpm/probe.spec"
  cp -p "$repo_root/packaging/linux/dev.probe.desktop.desktop" \
    "$dest/packaging/linux/dev.probe.desktop.desktop"
  cp -p "$repo_root/LICENSE" "$dest/LICENSE"
  cp -p "$repo_root/README.md" "$dest/README.md"
  cp -R "$repo_root/crates/desktop/assets/app-icon/linux/hicolor" \
    "$dest/crates/desktop/assets/app-icon/linux/hicolor"
  cat >"$dest/Cargo.toml" <<EOF
[workspace.package]
version = "${cargo_version}"
EOF
}

prerelease="0.11.0-beta.1"
rpm_prerelease="0.11.0~beta.1"
spaced="$tmp/checkout with spaces"
spaced_bins="$tmp/bin dir"
stage_tree "$spaced" "$prerelease"
mkdir -p "$spaced_bins" "$tmp/prerelease-out"
cp "$tmp/probe" "$spaced_bins/probe"
cp "$tmp/probe-desktop" "$spaced_bins/probe-desktop"
chmod +x "$spaced_bins/probe" "$spaced_bins/probe-desktop"
printf 'unrelated\n' >"$tmp/prerelease-out/other-tool-1.2.3-1.x86_64.rpm"

set +e
bash "$spaced/scripts/build-rpm.sh" \
  --cli "$spaced_bins/probe" \
  --desktop "$spaced_bins/probe-desktop" \
  --expect-version "$rpm_prerelease" \
  --output-dir "$tmp/prerelease-out" \
  >"$tmp/prerelease-expect.out" 2>"$tmp/prerelease-expect.err"
status=$?
set -e
[[ "$status" -ne 0 ]] \
  || die "--expect-version accepted the converted RPM version"
[[ "$(cat "$tmp/prerelease-out/other-tool-1.2.3-1.x86_64.rpm")" == "unrelated" ]] \
  || die "rejected prerelease build removed an unrelated RPM"

bash "$spaced/scripts/build-rpm.sh" \
  --cli "$spaced_bins/probe" \
  --desktop "$spaced_bins/probe-desktop" \
  --expect-version "$prerelease" \
  --output-dir "$tmp/prerelease-out"
[[ -f "$tmp/prerelease-out/probe-cli-${prerelease}-${platform}.rpm" ]] \
  || die "prerelease CLI asset did not keep the SemVer filename"
[[ -f "$tmp/prerelease-out/probe-desktop-${prerelease}-${platform}.rpm" ]] \
  || die "prerelease desktop asset did not keep the SemVer filename"
[[ "$(cat "$tmp/prerelease-out/other-tool-1.2.3-1.x86_64.rpm")" == "unrelated" ]] \
  || die "prerelease build removed an unrelated RPM"
packaged_rpm_version="$(
  rpm -qp --queryformat '%{VERSION}' \
    "$tmp/prerelease-out/probe-cli-${prerelease}-${platform}.rpm"
)"
[[ "$packaged_rpm_version" == "$rpm_prerelease" ]] \
  || die "prerelease RPM version is ${packaged_rpm_version}, expected ${rpm_prerelease}"
bash "$spaced/scripts/verify-rpm.sh" "$tmp/prerelease-out"

echo "RPM packaging checks passed for Probe ${version}"
