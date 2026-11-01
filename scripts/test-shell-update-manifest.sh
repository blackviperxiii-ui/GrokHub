#!/usr/bin/env bash
# Checks scripts/shell-update-manifest.sh against a throwaway minisign key:
# the manifest lists only the AppImage and the NSIS installer, points at the
# tag's release assets, carries signatures that verify, and refuses missing
# assets, missing signatures and odd tags. Needs minisign.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
script="$root/scripts/shell-update-manifest.sh"
ver="$(tr -d '[:space:]' < "$root/VERSION")"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
pass=0
failed=0
ok() { pass=$((pass + 1)); echo "ok: $1"; }
bad() { failed=$((failed + 1)); echo "FAIL: $1"; }

command -v minisign >/dev/null || { echo "test-shell-update-manifest: minisign is not installed" >&2; exit 1; }

minisign -G -W -p "$TMP/key.pub" -s "$TMP/key.sec" >/dev/null
d="$TMP/dist"
mkdir -p "$d"
app="grokhub-shell-v$ver-linux-amd64.AppImage"
exe="GrokHub-Shell-Setup-$ver.exe"
printf 'appimage bytes\n' > "$d/$app"
printf 'installer bytes\n' > "$d/$exe"
printf 'deb bytes\n' > "$d/grokhub-shell-v$ver-linux-amd64.deb"
for f in "$app" "$exe"; do
  minisign -S -s "$TMP/key.sec" -m "$d/$f" -t "loadout shell v$ver-beta.7" >/dev/null
done

if bash "$script" "$d" "v$ver-beta.7" "blackviperxiii-ui/GrokHub" >/dev/null; then
  ok "writes a manifest"
else
  bad "writes a manifest"
fi

check() {
  python3 - "$d/latest.json" "$1" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
want = sys.argv[2]
exec(compile(want, "check", "exec"), {"m": m})
PY
}

if check "assert m['version'] == '$ver-beta.7', m['version']"; then ok "version is the tag without v"; else bad "version is the tag without v"; fi
if check "assert sorted(m['platforms']) == ['linux-x86_64-appimage', 'windows-x86_64-nsis'], sorted(m['platforms'])"; then
  ok "only the AppImage and NSIS installer (no .deb)"
else
  bad "only the AppImage and NSIS installer (no .deb)"
fi
if check "assert m['platforms']['linux-x86_64-appimage']['url'] == 'https://github.com/blackviperxiii-ui/GrokHub/releases/download/v$ver-beta.7/$app'"; then
  ok "AppImage URL is the tag's release asset"
else
  bad "AppImage URL is the tag's release asset"
fi
if check "assert m['platforms']['windows-x86_64-nsis']['url'] == 'https://github.com/blackviperxiii-ui/GrokHub/releases/download/v$ver-beta.7/$exe'"; then
  ok "installer URL is the tag's release asset"
else
  bad "installer URL is the tag's release asset"
fi

# The signature field decodes back to a .minisig that verifies the asset.
for pair in "linux-x86_64-appimage:$app" "windows-x86_64-nsis:$exe"; do
  key="${pair%%:*}"
  f="${pair#*:}"
  python3 -c 'import json,sys,base64; m=json.load(open(sys.argv[1])); sys.stdout.buffer.write(base64.b64decode(m["platforms"][sys.argv[2]]["signature"]))' "$d/latest.json" "$key" > "$TMP/back.minisig"
  if minisign -V -q -p "$TMP/key.pub" -m "$d/$f" -x "$TMP/back.minisig"; then
    ok "$key signature verifies"
  else
    bad "$key signature verifies"
  fi
  printf 'tampered\n' >> "$d/$f"
  if minisign -V -q -p "$TMP/key.pub" -m "$d/$f" -x "$TMP/back.minisig" 2>/dev/null; then
    bad "$key signature rejects a changed asset"
  else
    ok "$key signature rejects a changed asset"
  fi
done

mv "$d/$exe.minisig" "$TMP/exe.minisig.bak"
if bash "$script" "$d" "v$ver-beta.7" "blackviperxiii-ui/GrokHub" >/dev/null 2>&1; then
  bad "missing .minisig fails"
else
  ok "missing .minisig fails"
fi
mv "$TMP/exe.minisig.bak" "$d/$exe.minisig"

mv "$d/$app" "$TMP/app.bak"
if bash "$script" "$d" "v$ver-beta.7" "blackviperxiii-ui/GrokHub" >/dev/null 2>&1; then
  bad "missing AppImage fails"
else
  ok "missing AppImage fails"
fi
mv "$TMP/app.bak" "$d/$app"

for t in "v$ver-rc.1" "$ver" "v$ver-beta."; do
  if bash "$script" "$d" "$t" "blackviperxiii-ui/GrokHub" >/dev/null 2>&1; then
    bad "tag '$t' is refused"
  else
    ok "tag '$t' is refused"
  fi
done

echo "passed=$pass failed=$failed"
(( failed == 0 ))
