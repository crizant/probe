#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
extractor="$repo_root/scripts/release-notes.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/changelog.md" <<'CHANGELOG'
# Changelog

## [Unreleased]

- Future changes.

## [1.20.0] - 2026-10-05

- A different version.

## [1.2.0] - 2026-10-04

### Added

- First note with `code` and [a link](https://example.com).

### Fixed

- Second note.

## [1.2.0-beta.1] - 2026-10-03

- Preview note.

## [1.1.0] - 2026-10-02

- Oldest note.

[1.2.0]: https://example.com/releases/tag/v1.2.0
[1.1.0]: https://example.com/releases/tag/v1.1.0
CHANGELOG

cat > "$tmp/expected.md" <<'NOTES'
### Added

- First note with `code` and [a link](https://example.com).

### Fixed

- Second note.
NOTES

assert_notes() {
  bash "$extractor" "$1" "$2" > "$tmp/actual.md"
  diff -u "$tmp/expected.md" "$tmp/actual.md"
}

assert_failure() {
  if bash "$extractor" "$1" "$2" > "$tmp/actual.md" 2> "$tmp/error"; then
    echo "error: expected extraction to fail for $1" >&2
    exit 1
  fi
  [[ ! -s "$tmp/actual.md" ]]
  grep -F "$3" "$tmp/error" > /dev/null
}

# Exact version matching, section boundaries, Markdown preservation, optional v.
assert_notes v1.2.0 "$tmp/changelog.md"
assert_notes 1.2.0 "$tmp/changelog.md"
# Windows checkouts use CRLF.
sed 's/$/\r/' "$tmp/changelog.md" > "$tmp/crlf.md"
assert_notes v1.2.0 "$tmp/crlf.md"

printf '%s\n' '- Preview note.' > "$tmp/expected.md"
assert_notes v1.2.0-beta.1 "$tmp/changelog.md"
printf '%s\n' '- Oldest note.' > "$tmp/expected.md"
assert_notes v1.1.0 "$tmp/changelog.md"
# A final section without reference links ends at EOF.
printf '## [2.0.0]\n\n- Final note.\n' > "$tmp/final.md"
printf '%s\n' '- Final note.' > "$tmp/expected.md"
assert_notes v2.0.0 "$tmp/final.md"

# In-section H2 headings and reference definitions must not truncate notes.
cat > "$tmp/expected.md" <<'NOTES'
[before]: https://example.com/before

- Read [before].

## Upgrade instructions

- Follow [guide].

[guide]: https://example.com/guide

- A note after the link definition.

## [Compatibility]

- Keep this non-version heading too.
NOTES
{
  printf '## [3.0.0] - 2026-10-05\n\n'
  cat "$tmp/expected.md"
  printf '\n## [2.0.0] - 2026-10-04\n\n- Older note.\n'
} > "$tmp/references.md"
assert_notes v3.0.0 "$tmp/references.md"
# The oldest release keeps its local definitions but excludes global release links.
{
  printf '## [3.0.0] - 2026-10-05\n\n'
  cat "$tmp/expected.md"
  printf '\n[3.0.0]: https://example.com/releases/tag/v3.0.0\n'
  printf '[2.0.0]: https://example.com/releases/tag/v2.0.0\n'
} > "$tmp/references-final.md"
assert_notes v3.0.0 "$tmp/references-final.md"

assert_failure v1.2.1 "$tmp/changelog.md" 'section for [1.2.1], found 0'
assert_failure vUnreleased "$tmp/changelog.md" 'invalid release tag'
assert_failure v1.2.0 "$tmp/missing.md" 'cannot read changelog'
printf '## [1.2.0] - 2026-10-04\n\n### Added\n\n## [1.1.0]\n\n- Older note.\n' > "$tmp/empty.md"
assert_failure v1.2.0 "$tmp/empty.md" 'has no release notes'
cat "$tmp/changelog.md" "$tmp/changelog.md" > "$tmp/duplicate.md"
assert_failure v1.2.0 "$tmp/duplicate.md" 'section for [1.2.0], found 2'

echo 'Release notes extraction tests passed.'
