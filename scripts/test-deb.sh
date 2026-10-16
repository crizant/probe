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
[[ "$(deb_version_from_semver '0.11.0')" == '0.11.0-1' ]] || die 'incorrect stable version'
[[ "$(deb_version_from_semver '0.11.0-beta.1')" == '0.11.0~beta.1-1' ]] || die 'incorrect prerelease'
[[ "$(deb_version_from_semver '0.11.0-rc.1-test+build.1')" == '0.11.0~rc.1-test-1' ]] || die 'incorrect hyphen or build metadata'
[[ "$(deb_version_from_semver '0.11.0+build.1')" == '0.11.0-1' ]] || die 'build metadata affected precedence'
for invalid in 0.11 01.2.3 1.2.3-01 1.2.3-a..b 1.2.3+; do
  if deb_version_from_semver "$invalid"; then die "invalid SemVer accepted: $invalid"; fi
done
versions=(0.11.0-alpha 0.11.0-alpha.1 0.11.0-alpha.beta 0.11.0-beta 0.11.0-beta.2 0.11.0-beta.11 0.11.0-rc.1 0.11.0)
for ((i=1; i<${#versions[@]}; i++)); do
  dpkg --compare-versions "$(deb_version_from_semver "${versions[i-1]}")" lt "$(deb_version_from_semver "${versions[i]}")" || die 'incorrect prerelease ordering'
done

command -v gcc >/dev/null 2>&1 || die "gcc is required"
command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb is required"

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
    die "DEB packaging tests require x86_64 ELF binaries, got ${description}"
    ;;
esac

stale="$tmp/stale"
mkdir -p "$stale"
printf 'stale-cli\n' >"$stale/probe-cli-${version}-linux-x64.deb"
printf 'stale-desktop\n' >"$stale/probe-desktop-${version}-linux-x64.deb"
set +e
bash "$repo_root/scripts/build-deb.sh" \
  --cli "$tmp/probe" \
  --desktop "$tmp/missing-probe-desktop" \
  --output-dir "$stale" \
  >"$tmp/failed.out" 2>"$tmp/failed.err"
status=$?
set -e
[[ "$status" -ne 0 ]] || die "build with a missing desktop binary reported success"
[[ "$(cat "$stale/probe-cli-${version}-linux-x64.deb")" == "stale-cli" ]] \
  || die "failed build replaced a previous CLI DEB"
[[ "$(cat "$stale/probe-desktop-${version}-linux-x64.deb")" == "stale-desktop" ]] \
  || die "failed build replaced a previous desktop DEB"
if grep -q 'DEB packages:' "$tmp/failed.out"; then
  die "failed build reported packaged DEBs"
fi

# A valid ELF with an unavailable runtime library must fail dependency scanning
# without replacing either previous package.
printf 'int probe_dependency(void) { return 0; }\n' >"$tmp/dependency.c"
printf 'extern int probe_dependency(void); int main(void) { return probe_dependency(); }\n' >"$tmp/needs-dependency.c"
gcc -shared -fPIC -Wl,-soname,libprobe-test-missing.so -o "$tmp/libprobe-test-missing.so" "$tmp/dependency.c"
gcc -o "$tmp/needs-dependency" "$tmp/needs-dependency.c" -L"$tmp" -l:libprobe-test-missing.so
rm "$tmp/libprobe-test-missing.so"
if bash "$repo_root/scripts/build-deb.sh" --cli "$tmp/probe" \
  --desktop "$tmp/needs-dependency" --output-dir "$stale" >"$tmp/dependency.out" 2>&1; then
  die 'build accepted an ELF with a missing runtime dependency'
fi
grep -q 'could not determine runtime dependencies' "$tmp/dependency.out" || die 'dependency scanning did not fail as expected'
[[ "$(cat "$stale/probe-cli-${version}-linux-x64.deb")" == stale-cli ]] || die 'dependency failure replaced CLI output'
[[ "$(cat "$stale/probe-desktop-${version}-linux-x64.deb")" == stale-desktop ]] || die 'dependency failure replaced desktop output'

output="$tmp/out"
mkdir -p "$output"
printf 'unrelated\n' >"$output/other-tool-1.2.3-1.x86_64.deb"
printf 'keep\n' >"$output/notes.txt"
printf 'replace-me\n' >"$tmp/stale-probe-deb"
cp "$tmp/stale-probe-deb" "$output/probe-cli-${version}-linux-x64.deb"
bash "$repo_root/scripts/build-deb.sh" \
  --cli "$tmp/probe" \
  --desktop "$tmp/probe-desktop" \
  --expect-version "$version" \
  --output-dir "$output"
[[ "$(cat "$output/other-tool-1.2.3-1.x86_64.deb")" == "unrelated" ]] \
  || die "successful build removed an unrelated DEB"
[[ "$(cat "$output/notes.txt")" == "keep" ]] \
  || die "successful build removed an unrelated file"
if cmp -s "$output/probe-cli-${version}-linux-x64.deb" "$tmp/stale-probe-deb"; then
  die "successful build kept a stale Probe CLI DEB"
fi

script_bin="$tmp/not-elf-probe"
script_desktop="$tmp/not-elf-desktop"
printf '#!/bin/sh\nprintf "probe\\n"\n' >"$script_bin"
cp "$script_bin" "$script_desktop"
chmod +x "$script_bin" "$script_desktop"
set +e
bash "$repo_root/scripts/build-deb.sh" \
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
    "$dest/crates/desktop/assets/app-icon/linux"
  cp -p "$repo_root/scripts/build-deb.sh" "$dest/scripts/build-deb.sh"
  cp -p "$repo_root/scripts/verify-deb.sh" "$dest/scripts/verify-deb.sh"
  cp -p "$repo_root/scripts/workspace-version.sh" "$dest/scripts/workspace-version.sh"
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

spaced="$tmp/checkout with spaces"
spaced_bins="$tmp/bin dir"
stage_tree "$spaced" "$version"
mkdir -p "$spaced_bins"
cp "$tmp/probe" "$spaced_bins/probe"
cp "$tmp/probe-desktop" "$spaced_bins/probe-desktop"
chmod +x "$spaced_bins/probe" "$spaced_bins/probe-desktop"
bash "$spaced/scripts/build-deb.sh" \
  --cli "$spaced_bins/probe" \
  --desktop "$spaced_bins/probe-desktop" \
  --output-dir "$tmp/output with spaces"

prerelease="0.11.0-beta.1+build.7"
deb_prerelease="0.11.0~beta.1-1"
stage_tree "$tmp/prerelease" "$prerelease"
set +e
bash "$tmp/prerelease/scripts/build-deb.sh" \
  --cli "$tmp/probe" \
  --desktop "$tmp/probe-desktop" \
  --expect-version "$deb_prerelease" \
  --output-dir "$tmp/prerelease-out" \
  >"$tmp/prerelease-expect.out" 2>"$tmp/prerelease-expect.err"
status=$?
set -e
[[ "$status" -ne 0 ]] \
  || die "--expect-version accepted the converted DEB version"
grep -q 'does not match expected version' "$tmp/prerelease-expect.err" \
  || die "--expect-version did not reject the converted DEB version"
bash "$tmp/prerelease/scripts/build-deb.sh" \
  --cli "$tmp/probe" \
  --desktop "$tmp/probe-desktop" \
  --expect-version "$prerelease" \
  --output-dir "$tmp/prerelease-out"
packaged_deb_version="$(
  dpkg-deb --field \
    "$tmp/prerelease-out/probe-cli-${prerelease}-linux-x64.deb" Version
)"
[[ "$packaged_deb_version" == "$deb_prerelease" ]] \
  || die "prerelease DEB version is ${packaged_deb_version}, expected ${deb_prerelease}"
[[ -f "$tmp/prerelease-out/probe-desktop-${prerelease}-linux-x64.deb" ]] \
  || die "prerelease desktop asset did not keep the SemVer filename"

# Inspect dependency metadata derived from real dynamically linked stand-ins.
for asset in probe-cli probe-desktop; do
  dependencies="$(dpkg-deb --field "$output/$asset-$version-linux-x64.deb" Depends)"
  [[ "$dependencies" == *"libc6 (>= "* ]] || die 'missing detected libc dependency'
  [[ "$dependencies" != *'-dev'* ]] || die 'build dependency leaked into runtime metadata'
done
mkdir -p "$tmp/extracted-cli" "$tmp/extracted-desktop"
dpkg-deb --extract "$output/probe-cli-$version-linux-x64.deb" "$tmp/extracted-cli"
dpkg-deb --extract "$output/probe-desktop-$version-linux-x64.deb" "$tmp/extracted-desktop"
cmp "$tmp/probe" "$tmp/extracted-cli/usr/bin/probe"
cmp "$tmp/probe-desktop" "$tmp/extracted-desktop/usr/bin/probe-desktop"
# Verifier rejects a package whose control metadata no longer matches its name.
mkdir -p "$tmp/tampered"
cp "$output/probe-desktop-$version-linux-x64.deb" "$tmp/tampered/"
dpkg-deb --control "$output/probe-cli-$version-linux-x64.deb" "$tmp/extracted-cli/DEBIAN"
sed -i 's/^Architecture: amd64$/Architecture: arm64/' "$tmp/extracted-cli/DEBIAN/control"
chmod 0755 "$tmp/extracted-cli/DEBIAN"
dpkg-deb --root-owner-group --build "$tmp/extracted-cli" "$tmp/tampered/probe-cli-$version-linux-x64.deb" >/dev/null
if bash "$repo_root/scripts/verify-deb.sh" "$tmp/tampered" >"$tmp/tampered.out" 2>&1; then
  die 'verification accepted incorrect package architecture'
fi
grep -q 'incorrect architecture' "$tmp/tampered.out" || die 'verifier failed for an unexpected reason'
if bash "$repo_root/scripts/verify-deb.sh" "$stale" >"$tmp/verify.out" 2>&1; then
  die 'verification accepted stale non-packages'
fi

echo "DEB packaging checks passed for Probe ${version}"
