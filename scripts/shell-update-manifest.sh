#!/usr/bin/env bash
# Writes the shell's self-update manifest, latest.json, for one release
# (LU-06). The Tauri updater plugin reads it: one entry per platform that can
# replace itself in place, each with the asset's download URL and its
# minisign signature (base64 of the .minisig file, the plugin's format).
#   linux-x86_64-appimage  grokhub-shell-vX.Y.Z-linux-amd64.AppImage
#   windows-x86_64-nsis    GrokHub-Shell-Setup-X.Y.Z.exe
# The .deb is left out on purpose: the package manager owns it.
# Fails unless both assets and their .minisig files sit in the directory.
#
#   scripts/shell-update-manifest.sh <dist dir> <tag> <owner/repo>
set -euo pipefail

dir="${1:?usage: shell-update-manifest.sh <dist dir> <tag> <owner/repo>}"
tag="${2:?usage: shell-update-manifest.sh <dist dir> <tag> <owner/repo>}"
repo="${3:?usage: shell-update-manifest.sh <dist dir> <tag> <owner/repo>}"

if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-beta\.[0-9]+)?$ ]]; then
  echo "shell-update-manifest: unsupported tag '$tag' (want vX.Y.Z or vX.Y.Z-beta.N)" >&2
  exit 2
fi
if [[ ! "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  echo "shell-update-manifest: bad repo '$repo'" >&2
  exit 2
fi
ver="$(tr -d '[:space:]' < "$(cd "$(dirname "$0")/.." && pwd)/VERSION")"

# one <platform key> <asset name>: the asset and its .minisig must exist.
entries=()
one() {
  local asset="$dir/$2"
  if [ ! -s "$asset" ]; then
    echo "shell-update-manifest: $asset is missing" >&2
    exit 1
  fi
  if [ ! -s "$asset.minisig" ]; then
    echo "shell-update-manifest: $(basename "$asset") has no .minisig" >&2
    exit 1
  fi
  entries+=("$1" "$(basename "$asset")" "$(base64 -w0 < "$asset.minisig")")
}
one linux-x86_64-appimage "grokhub-shell-v$ver-linux-amd64.AppImage"
one windows-x86_64-nsis "GrokHub-Shell-Setup-$ver.exe"

python3 - "$dir/latest.json" "${tag#v}" "$tag" "$repo" "${entries[@]}" <<'PY'
import json, sys, datetime
out, version, tag, repo, *rest = sys.argv[1:]
platforms = {}
for key, name, sig in zip(rest[0::3], rest[1::3], rest[2::3]):
    platforms[key] = {
        "signature": sig,
        "url": f"https://github.com/{repo}/releases/download/{tag}/{name}",
    }
manifest = {
    "version": version,
    "notes": f"Loadout {tag}",
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": platforms,
}
with open(out, "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
PY
echo "shell-update-manifest: $dir/latest.json ($tag)"
