#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/release.sh <version>

Bumps the workspace version, stamps CHANGELOG.md with the version and date,
runs release checks, commits, tags, and pushes.

The changelog date is the current date in Australia/Brisbane.

Examples:
  scripts/release.sh 0.2.0
  scripts/release.sh v0.2.0
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

package_version() {
  cargo pkgid -p probe-cli | sed 's/.*@//'
}

stamp_changelog() {
  VERSION="$1" RELEASE_DATE="$2" perl - <<'PERL'
use strict;
use warnings;

my $version = $ENV{VERSION} || "";
my $date = $ENV{RELEASE_DATE} || "";
my $file = "CHANGELOG.md";
my $tmp;

sub fail {
    my ($message) = @_;
    print STDERR "error: $message\n";
    exit 1;
}

END {
    unlink $tmp if defined $tmp && -e $tmp;
}

fail("version is required")
    unless $version =~ /^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$/;
fail("release date must be YYYY-MM-DD")
    unless $date =~ /^[0-9]{4}-[0-9]{2}-[0-9]{2}$/;
fail("CHANGELOG.md is missing") unless -f $file;

open my $in, "<", $file or fail("could not read $file: $!");
my $text = do { local $/; <$in> };
close $in;

my $unreleased = () = $text =~ /^## \[Unreleased\]$/mg;
fail("CHANGELOG.md must contain exactly one '## [Unreleased]' heading")
    unless $unreleased == 1;

fail("CHANGELOG.md already has a [$version] release section")
    if $text =~ /^## \[\Q$version\E\] - /m;

my ($body) = $text =~ /^## \[Unreleased\]\n(.*?)(?=^## |\z)/ms;
fail("could not read the Unreleased section") unless defined $body;
fail("CHANGELOG.md has no unreleased notes to release")
    unless $body =~ /^[-*] /m;

$text =~ s/^## \[Unreleased\]\n/## [Unreleased]\n\n## [$version] - $date\n/m
    or fail("failed to stamp the Unreleased heading");

$text =~ s{^(\[([^]]+)\]: (\S+/releases/tag/)v\2)\n}{[$version]: ${3}v$version\n$1\n}m
    or fail("CHANGELOG.md is missing a release link to update");

$tmp = "$file.tmp.$$";
open my $out, ">", $tmp or fail("could not write $tmp: $!");
print {$out} $text or fail("could not write $tmp: $!");
close $out or fail("could not write $tmp: $!");
rename $tmp, $file or fail("could not replace $file: $!");
PERL
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

[[ $# -eq 1 ]] || {
  usage >&2
  exit 2
}

version="${1#v}"
tag="v${version}"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] \
  || die "version must look like 0.2.0, 0.2.0-beta.1, or v0.2.0"

git rev-parse --show-toplevel >/dev/null 2>&1 || die "must be run from a git worktree"
repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

[[ -z "$(git status --porcelain)" ]] \
  || die "worktree must be clean before starting a release"

branch="$(git symbolic-ref --quiet --short HEAD)" \
  || die "must be on a branch, not a detached HEAD"

git remote get-url origin >/dev/null 2>&1 || die "git remote 'origin' is required"

if git rev-parse --verify --quiet "refs/tags/${tag}" >/dev/null; then
  die "local tag ${tag} already exists"
fi

if git ls-remote --exit-code --tags origin "refs/tags/${tag}" >/dev/null 2>&1; then
  die "remote tag ${tag} already exists on origin"
fi

current_version="$(package_version)"

[[ "$current_version" != "$version" ]] \
  || die "workspace version is already ${version}"

release_date="$(TZ=Australia/Brisbane date +%Y-%m-%d)"
[[ "$release_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] \
  || die "could not determine the Australia/Brisbane release date"

stamp_changelog "$version" "$release_date"

VERSION="$version" perl -0pi -e '
  BEGIN { $version = $ENV{"VERSION"}; }
  $count = s/(\[workspace\.package\]\s*.*?^version = ")[^"]+(")/$1$version$2/ms;
  END { exit($count == 1 ? 0 : 1); }
' Cargo.toml || die "failed to update [workspace.package] version in Cargo.toml"

cargo check --workspace
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features

updated_version="$(package_version)"

[[ "$updated_version" == "$version" ]] \
  || die "Cargo metadata still reports ${updated_version}, expected ${version}"

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: ${tag}"
git tag -a "$tag" -m "Release ${tag}"

git push origin "$branch"
git push origin "$tag"

echo "Released ${tag}. GitHub Actions will build and publish the release artifacts."
