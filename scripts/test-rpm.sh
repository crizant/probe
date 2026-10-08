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
  *x86-64* | *x86_64*)
    platform="linux-x64"
    ;;
  *aarch64*)
    platform="linux-arm64"
    ;;
  *)
    die "unsupported ELF architecture: ${description}"
    ;;
esac

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
printf 'previous\n' >"$output/probe-9.9.9-1.x86_64.rpm"
bash "$repo_root/scripts/build-rpm.sh" \
  --cli "$tmp/probe" \
  --desktop "$tmp/probe-desktop" \
  --expect-version "$version" \
  --output-dir "$output"
[[ ! -e "$output/probe-9.9.9-1.x86_64.rpm" ]] \
  || die "successful build left an RPM from a previous run"
[[ -f "$output/probe-cli-${version}-${platform}.rpm" ]] || die "missing CLI release RPM"
[[ -f "$output/probe-desktop-${version}-${platform}.rpm" ]] || die "missing desktop release RPM"
bash "$repo_root/scripts/verify-rpm.sh" "$output"

echo "RPM packaging checks passed for Probe ${version}"
