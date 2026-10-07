#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'HELP'
Usage: scripts/build-rpm.sh [OPTIONS]

Builds RPM packages for Probe (probe and probe-desktop).

Options:
  --builder BUILDER Choice of builder: cargo, rpmbuild, or auto (default: auto)
  --prebuilt        When using rpmbuild, package existing target/release binaries
  --output-dir DIR  Directory to place generated RPM packages (default: dist/rpm)
  -h, --help        Show this help message

Builders:
  cargo     Uses cargo-generate-rpm (via package.metadata.generate-rpm in Cargo.toml)
  rpmbuild  Uses rpmbuild and packaging/rpm/probe.spec
HELP
}

die() {
  echo "error: $*" >&2
  exit 1
}

BUILDER="auto"
PREBUILT=false
OUTPUT_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --builder)
      BUILDER="$2"
      shift 2
      ;;
    --cargo)
      BUILDER="cargo"
      shift
      ;;
    --rpmbuild)
      BUILDER="rpmbuild"
      shift
      ;;
    --prebuilt)
      PREBUILT=true
      shift
      ;;
    --output-dir)
      OUTPUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$REPO_ROOT"

if [[ -z "$OUTPUT_DIR" ]]; then
  OUTPUT_DIR="${REPO_ROOT}/dist/rpm"
fi
mkdir -p "$OUTPUT_DIR"

if [[ "$BUILDER" == "auto" ]]; then
  if command -v cargo-generate-rpm >/dev/null 2>&1; then
    BUILDER="cargo"
  elif command -v rpmbuild >/dev/null 2>&1; then
    BUILDER="rpmbuild"
  else
    die "Neither cargo-generate-rpm nor rpmbuild found. Please install either cargo-generate-rpm or rpmbuild."
  fi
fi

if [[ "$BUILDER" == "cargo" ]]; then
  command -v cargo-generate-rpm >/dev/null 2>&1 || die "cargo-generate-rpm is not installed"
  
  echo "==> Ensuring release binaries are built..."
  cargo build --release -p probe-cli --bin probe
  cargo build --release -p probe-desktop --bin probe-desktop

  echo "==> Generating RPM packages with cargo-generate-rpm..."
  (cd crates/cli && cargo generate-rpm --target-dir "${REPO_ROOT}/target")
  (cd crates/desktop && cargo generate-rpm --target-dir "${REPO_ROOT}/target")

  echo "==> Copying built RPM packages to ${OUTPUT_DIR}..."
  find "${REPO_ROOT}/target/generate-rpm" -type f -name '*.rpm' -exec cp -p {} "$OUTPUT_DIR" \;

elif [[ "$BUILDER" == "rpmbuild" ]]; then
  command -v rpmbuild >/dev/null 2>&1 || die "rpmbuild is required"
  command -v desktop-file-validate >/dev/null 2>&1 || die "desktop-file-validate is required"
  command -v tar >/dev/null 2>&1 || die "tar is required"

  VERSION="$(cargo pkgid -p probe-cli | sed 's/.*@//')"
  SPEC_FILE="${REPO_ROOT}/packaging/rpm/probe.spec"
  [[ -f "$SPEC_FILE" ]] || die "Spec file not found at ${SPEC_FILE}"

  RPM_TOPDIR="${REPO_ROOT}/target/rpmbuild"
  mkdir -p "${RPM_TOPDIR}/"{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}

  echo "==> Preparing source archive for probe-${VERSION}..."
  tar -czf "${RPM_TOPDIR}/SOURCES/probe-${VERSION}.tar.gz" \
    --transform "s,^\.,probe-${VERSION}," \
    --exclude='./target' \
    --exclude='./dist' \
    --exclude='./.git' \
    .

  cp "$SPEC_FILE" "${RPM_TOPDIR}/SPECS/probe.spec"

  RPMBUILD_ARGS=(
    -bb
    --define "_topdir ${RPM_TOPDIR}"
    --define "_sourcedir ${RPM_TOPDIR}/SOURCES"
  )

  if [[ "$PREBUILT" == "true" ]]; then
    CLI_BIN="${REPO_ROOT}/target/release/probe"
    DESKTOP_BIN="${REPO_ROOT}/target/release/probe-desktop"

    if [[ ! -f "$CLI_BIN" || ! -f "$DESKTOP_BIN" ]]; then
      echo "==> Building release binaries..."
      cargo build --release -p probe-cli --bin probe
      cargo build --release -p probe-desktop --bin probe-desktop
    fi

    echo "==> Building RPM packages using prebuilt binaries..."
    RPMBUILD_ARGS+=(
      --with prebuilt
      --define "prebuilt_cli ${CLI_BIN}"
      --define "prebuilt_desktop ${DESKTOP_BIN}"
    )
  else
    echo "==> Building RPM packages from source inside rpmbuild..."
  fi

  rpmbuild "${RPMBUILD_ARGS[@]}" "${RPM_TOPDIR}/SPECS/probe.spec"

  echo "==> Copying built RPM packages to ${OUTPUT_DIR}..."
  find "${RPM_TOPDIR}/RPMS" -type f -name '*.rpm' -exec cp -p {} "$OUTPUT_DIR" \;
else
  die "Unknown builder: ${BUILDER}. Choose 'cargo' or 'rpmbuild'."
fi

echo "==> Successfully built RPM packages in ${OUTPUT_DIR}:"
ls -lh "${OUTPUT_DIR}"/*.rpm
