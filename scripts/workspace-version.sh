# shellcheck shell=bash
# Shared by the Linux packaging scripts. Source this file; do not execute it.

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

# Debian prereleases sort before the final release via ~. Keep hyphens inside
# prerelease identifiers; the explicit -1 is the Debian revision. Build metadata
# has no SemVer precedence, so omit it from control metadata, not asset names.
deb_version_from_semver() {
  local semver="$1" core prerelease identifier
  local number='(0|[1-9][0-9]*)'
  local identifiers='[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*'
  [[ "$semver" =~ ^$number\.$number\.$number(-$identifiers)?(\+$identifiers)?$ ]] || return 1
  core="${semver%%+*}"
  if [[ "$core" == *-* ]]; then
    prerelease="${core#*-}"
    local -a parts
    IFS=. read -r -a parts <<<"$prerelease"
    for identifier in "${parts[@]}"; do
      [[ ! "$identifier" =~ ^0[0-9]+$ ]] || return 1
    done
    printf '%s~%s-1\n' "${core%%-*}" "$prerelease"
  else
    printf '%s-1\n' "$core"
  fi
}
