# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- Direct keyring set_password must go through keyring_blob ------------------
# Windows Credential Manager caps one secret at 2560 UTF-16LE bytes (~1280
# characters). Every OS keyring write must use keyring_blob.rs so long OAuth
# JSON and other secrets are chunked. A bare set_password outside that file
# fails CI.
keyring_set_hits=$(git grep -n 'set_password' -- '*.rs' \
  ':(exclude)crates/grokhub-agent/src/keyring_blob.rs' || true)
if [[ -n "$keyring_set_hits" ]]; then
  echo "ci-guards: set_password must only appear in keyring_blob.rs (chunked writes):" >&2
  echo "$keyring_set_hits" >&2
  fail=1
else
  echo "ci-guards: no direct set_password outside keyring_blob"
fi
