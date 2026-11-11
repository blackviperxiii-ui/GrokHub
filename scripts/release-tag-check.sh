#!/usr/bin/env bash
# A release tag must name the version the tagged commit builds. The update
# manifest takes its version from the tag and the binaries take theirs from
# VERSION, so a tag that disagrees is offered as an update forever, or never.
# Passes only for v<VERSION> and v<VERSION>-beta.N.
#
#   scripts/release-tag-check.sh <tag>
set -euo pipefail

tag="${1:?usage: release-tag-check.sh <tag>}"
ver="$(tr -d '[:space:]' < "$(cd "$(dirname "$0")/.." && pwd)/VERSION")"

if [[ ! "$ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "::error::VERSION is '$ver', not X.Y.Z; no release tag can match it" >&2
  exit 1
fi
if [[ "$tag" == "v$ver" ]]; then
  echo "release-tag-check: $tag is the stable tag for VERSION $ver"
  exit 0
fi
if [[ "$tag" == "v$ver-beta."* && "${tag#"v$ver-beta."}" =~ ^[0-9]+$ ]]; then
  echo "release-tag-check: $tag is a beta tag for VERSION $ver"
  exit 0
fi
echo "::error::release tag '$tag' does not match VERSION $ver (want v$ver or v$ver-beta.N)" >&2
exit 1
