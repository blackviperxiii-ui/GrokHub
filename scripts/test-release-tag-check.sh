#!/usr/bin/env bash
# Checks scripts/release-tag-check.sh in a throwaway tree whose VERSION is
# 1.2.3: only v1.2.3 and v1.2.3-beta.N pass, and a VERSION that is not X.Y.Z
# passes nothing.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/scripts"
cp "$root/scripts/release-tag-check.sh" "$TMP/scripts/release-tag-check.sh"
pass=0
failed=0

# expect <want exit code> <tag>
expect() {
  local want="$1" tag="$2" rc=0
  bash "$TMP/scripts/release-tag-check.sh" "$tag" >"$TMP/out.log" 2>&1 || rc=$?
  if ((rc == want)); then
    pass=$((pass + 1))
    echo "ok: VERSION $(tr -d '[:space:]' < "$TMP/VERSION"), tag '$tag' exits $want"
  else
    failed=$((failed + 1))
    echo "FAIL: VERSION $(tr -d '[:space:]' < "$TMP/VERSION"), tag '$tag' exits $rc, want $want"
    sed 's/^/    | /' "$TMP/out.log"
  fi
}

printf '1.2.3\n' > "$TMP/VERSION"
expect 0 v1.2.3
expect 0 v1.2.3-beta.3
expect 0 v1.2.3-beta.870
expect 1 v9.9.9
expect 1 v9.9.9-beta.3
expect 1 v1.2.4
expect 1 v1.2.30
expect 1 v1.2
expect 1 v1x2x3
expect 1 vfoo
expect 1 1.2.3
expect 1 v1.2.3-rc.1
expect 1 v1.2.3-beta.
expect 1 v1.2.3-beta.x
expect 1 v1.2.3-beta.3-extra
expect 1 "v1.2.3 "

# The message names both sides, so the log says what to fix.
bash "$TMP/scripts/release-tag-check.sh" v9.9.9 >"$TMP/out.log" 2>&1 || true
if grep -qF "release tag 'v9.9.9' does not match VERSION 1.2.3" "$TMP/out.log"; then
  pass=$((pass + 1)); echo "ok: the refusal names the tag and VERSION"
else
  failed=$((failed + 1)); echo "FAIL: the refusal names the tag and VERSION"; sed 's/^/    | /' "$TMP/out.log"
fi

printf '\n' > "$TMP/VERSION"
expect 1 v
expect 1 v-beta.1
printf '1.2\n' > "$TMP/VERSION"
expect 1 v1.2

echo "passed=$pass failed=$failed"
(( failed == 0 ))
