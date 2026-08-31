#!/usr/bin/env bash
# Prepare a release: bump the version, verify, commit, tag.
#
# It stops short of pushing: the tag is what starts the publish. The command is printed at the end.
#
# Usage: scripts/release.sh 0.2.0
set -euo pipefail

version=${1:?a version, e.g. 0.2.0}
command -v git-cliff >/dev/null || {
  echo "git-cliff is required to regenerate CHANGELOG.md: brew install git-cliff" >&2
  exit 1
}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || {
  echo "not a semver version: $version" >&2
  exit 1
}

cd "$(dirname "${BASH_SOURCE[0]}")/.."

[ -z "$(git status --porcelain)" ] || {
  echo "the working tree has changes — commit or stash them first" >&2
  exit 1
}
git rev-parse -q --verify "refs/tags/v$version" >/dev/null && {
  echo "v$version already exists" >&2
  exit 1
}

# The bundles take their version from the crate, and the release workflow refuses a tag
# that disagrees with it.
sed -i '' "s/^version = \".*\"/version = \"$version\"/" src-tauri/Cargo.toml
# Rewrites Cargo.lock's own entry for the crate, so the build never has to.
(cd src-tauri && cargo update --workspace --offline >/dev/null)
# `npm version` rather than an edit in place: it carries the number into package-lock.json too.
npm version "$version" --no-git-tag-version >/dev/null

git-cliff --tag "v$version" -o CHANGELOG.md

npm test

git commit -aqm "chore: release v$version"
git tag -a "v$version" -m "OverLingo v$version"

echo
echo "v$version is committed and tagged. Nothing has left this machine yet."
echo "Publish it with:"
echo
echo "    git push origin main v$version"
