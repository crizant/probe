#!/usr/bin/env bash
# Print one release body from the existing Keep a Changelog file.
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: bash scripts/release-notes.sh <tag> [changelog]" >&2
  exit 2
fi

version="${1#v}"
changelog="${2:-CHANGELOG.md}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || {
  echo "error: invalid release tag: $1" >&2
  exit 1
}
[[ -r "$changelog" ]] || {
  echo "error: cannot read changelog: $changelog" >&2
  exit 1
}

awk -v version="$version" '
  { sub(/\r$/, "") }
  /^## / {
    active = ($0 == "## [" version "]" || index($0, "## [" version "] - ") == 1)
    if (active) found++
    next
  }
  # The oldest release ends at the changelog-wide reference links.
  /^\[[^]]+\]: / { active = 0 }
  active {
    body = body $0 "\n"
    if ($0 ~ /[^[:space:]]/ && $0 !~ /^### /) content = 1
  }
  END {
    if (found != 1) {
      print "error: expected exactly one changelog section for [" version "], found " (found + 0) > "/dev/stderr"
      exit 1
    }
    if (!content) {
      print "error: changelog section for [" version "] has no release notes" > "/dev/stderr"
      exit 1
    }
    sub(/^\n+/, "", body)
    sub(/\n+$/, "", body)
    print body
  }
' "$changelog"
