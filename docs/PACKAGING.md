# Release Packaging

Packaging uses the prebuilt release binaries. See [release.yml](../.github/workflows/release.yml)
for jobs and [Development](DEVELOPMENT.md#rust-toolchains) for compiler policy.

## macOS Packaging Tools

Native icons require full Xcode 26 or later (`actool`, `assetutil`), Python 3.9 or
later, and cargo-bundle 0.11.0. Compile icons after bundling and before signing;
follow the [app-icon workflow](../crates/desktop/assets/app-icon/README.md).
CairoSVG, Pillow, and system Cairo are needed only for regenerating artwork.
Windows and Linux use the existing generated icons.

## Linux RPM Packages

Tagged releases publish binary RPMs alongside Linux `.tar.gz` archives, reusing
their x86_64 binaries. `scripts/build-rpm.sh` reads `Cargo.toml` and converts SemVer
prereleases to RPM tilde versions (`0.11.0-beta.1` → `0.11.0~beta.1`). It runs
`scripts/verify-rpm.sh` before copying packages to the release output.
Test packaging with `bash scripts/test-rpm.sh`, which uses stand-in binaries.

## Linux DEB Packages

The release job packages existing Linux tarball binaries as
`probe-cli-<version>-linux-x64.deb` and `probe-desktop-<version>-linux-x64.deb`.
Package names are `probe` and `probe-desktop`, architecture `amd64`; executables
install in `/usr/bin`. Desktop also installs the desktop file and hicolor icons.

On Debian/Ubuntu amd64, install `dpkg-dev`, `binutils`, `file`,
`desktop-file-utils`, and the supplied binaries' runtime libraries. Packaging tests
also need `gcc` and `libc6-dev` for ELF stand-ins.

```bash
scripts/build-deb.sh --cli path/to/probe --desktop path/to/probe-desktop \
  --output-dir dist/deb
scripts/verify-deb.sh dist/deb
bash scripts/test-deb.sh
```

`dpkg-shlibdeps` derives runtime dependencies from the actual ELF binaries and
host symbols/shlibs metadata; missing libraries or metadata fail packaging.
Desktop explicitly depends on `hicolor-icon-theme` and `libvulkan1`, and recommends
the exact matching CLI plus `libwayland-client0`, `libwayland-egl1`, and `libegl1`.
Wayland/EGL remain optional on X11. Dependency metadata uses Ubuntu 22.04;
target systems still need compatible ABIs, graphics drivers, a graphical session,
and any required dynamically loaded libraries or portal services.

The version comes from `Cargo.toml`; `--expect-version` and the CLI verifier check
the original SemVer. Debian control versions use `~` for prereleases, revision `-1`,
and omit build metadata. Asset filenames retain full SemVer. URL-encode download
filenames, including literal `+build` suffixes as `%2B`.

Packaging preserves binaries and checks extracted bytes against the supplied files.
Both packages are staged and verified before replacing matching outputs; unrelated
files are preserved. Payloads include `md5sums`, DEP-5 copyright metadata referencing
`/usr/share/common-licenses/Apache-2.0`, and `changelog.Debian.gz`.

`SOURCE_DATE_EPOCH` controls changelog and package timestamps, defaulting to the
source commit time. Source trees without Git metadata must supply it; releases use
the tagged commit timestamp. Compression omits gzip filename/time headers. Identical
inputs and epoch on the same packaging host produce byte-identical DEBs despite
input mtimes. These are upstream binary packages, not Debian source packages.
