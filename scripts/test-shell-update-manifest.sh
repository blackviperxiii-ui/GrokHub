#!/usr/bin/env bash
# Checks scripts/shell-update-manifest.sh against a throwaway minisign key:
# the manifest lists the AppImage, the .deb and the NSIS installer, plus the
# macOS .app archive only when the release built one, points at the
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
deb="grokhub-shell-v$ver-linux-amd64.deb"
printf 'deb bytes\n' > "$d/$deb"
for f in "$app" "$deb" "$exe"; do
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
if check "assert sorted(m['platforms']) == ['linux-x86_64', 'linux-x86_64-appimage', 'linux-x86_64-deb', 'windows-x86_64-nsis'], sorted(m['platforms'])"; then
  ok "the AppImage (plus plain linux-x86_64 alias), .deb and NSIS installer (no macOS build, no macOS entry)"
else
  bad "the AppImage (plus plain linux-x86_64 alias), .deb and NSIS installer (no macOS build, no macOS entry)"
fi
if check "assert m['platforms']['linux-x86_64'] == m['platforms']['linux-x86_64-appimage']"; then
  ok "linux-x86_64 aliases the AppImage entry"
else
  bad "linux-x86_64 aliases the AppImage entry"
fi
if check "assert m['platforms']['linux-x86_64-deb']['url'] == 'https://github.com/blackviperxiii-ui/GrokHub/releases/download/v$ver-beta.7/$deb'"; then
  ok ".deb URL is the tag's release asset"
else
  bad ".deb URL is the tag's release asset"
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
for pair in "linux-x86_64-appimage:$app" "linux-x86_64:$app" "linux-x86_64-deb:$deb" "windows-x86_64-nsis:$exe"; do
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

# A macOS build adds its .app archive, which must be signed too.
mac="grokhub-shell-v$ver-macos-arm64.app.tar.gz"
printf 'app archive bytes\n' > "$d/$mac"
if bash "$script" "$d" "v$ver" "blackviperxiii-ui/GrokHub" >/dev/null 2>&1; then
  bad "an unsigned macOS archive fails"
else
  ok "an unsigned macOS archive fails"
fi
minisign -S -s "$TMP/key.sec" -m "$d/$mac" -t "loadout shell v$ver" >/dev/null
if bash "$script" "$d" "v$ver" "blackviperxiii-ui/GrokHub" >/dev/null &&
  check "assert m['platforms']['darwin-aarch64-app']['url'] == 'https://github.com/blackviperxiii-ui/GrokHub/releases/download/v$ver/$mac'"; then
  ok "a macOS build adds darwin-aarch64-app"
else
  bad "a macOS build adds darwin-aarch64-app"
fi
rm -f "$d/$mac" "$d/$mac.minisig"

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

# A tag for another version than VERSION: the assets and binaries say $ver,
# so the manifest must not be written under a different number.
for t in "v9.9.9" "v9.9.9-beta.7" "v${ver}0" "v${ver}0-beta.7"; do
  rm -f "$d/latest.json"
  rc=0
  bash "$script" "$d" "$t" "blackviperxiii-ui/GrokHub" >/dev/null 2>"$TMP/err.log" || rc=$?
  if ((rc == 2)) && [ ! -e "$d/latest.json" ] && grep -qF "VERSION is $ver" "$TMP/err.log"; then
    ok "tag '$t' is refused: not VERSION $ver"
  else
    bad "tag '$t' is refused: not VERSION $ver (rc=$rc)"
  fi
done
if bash "$script" "$d" "v$ver" "blackviperxiii-ui/GrokHub" >/dev/null && check "assert m['version'] == '$ver', m['version']"; then
  ok "the stable tag v$ver still writes a manifest"
else
  bad "the stable tag v$ver still writes a manifest"
fi

echo "passed=$pass failed=$failed"
(( failed == 0 ))
