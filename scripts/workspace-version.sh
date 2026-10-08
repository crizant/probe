# shellcheck shell=bash
# Shared by the RPM packaging scripts. Source this file; do not execute it.

workspace_version() {
  local cargo_toml="$1"
  local version

  [[ -f "$cargo_toml" ]] || return 1
  version="$(
    awk '
      $0 == "[workspace.package]" { in_pkg = 1; next }
      in_pkg && /^\[/ { in_pkg = 0 }
      in_pkg && /^version[[:space:]]*=/ {
        if (match($0, /"[^"]+"/)) {
          print substr($0, RSTART + 1, RLENGTH - 2)
          found = 1
          exit
        }
      }
      END { if (!found) exit 1 }
    ' "$cargo_toml"
  )" || return 1
  [[ -n "$version" ]] || return 1
  printf '%s\n' "$version"
}

# RPM versions cannot contain a hyphen. SemVer prerelease hyphens become tildes:
# 0.11.0-beta.1 -> 0.11.0~beta.1. The workspace SemVer itself is unchanged.
rpm_version_from_semver() {
  local semver="$1"
  local tilde="~"

  [[ "$semver" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || return 1
  # Store the tilde in a variable. A bare ~ in this replacement is expanded
  # to a home directory by Bash 4 and newer.
  printf '%s\n' "${semver//-/$tilde}"
}
