#!/usr/bin/env bash
# Repo friction guards. Each guard is its own file in scripts/ci-guards.d/,
# sourced in name order, so a new guard is a new file instead of an edit to a
# shared one. A guard sets fail=1 to fail the job. Today's guards fail CI when:
#   10) conflict markers are committed
#   20) god-file line budgets are exceeded (ratchet; raise only when intentional)
#   30) a PR edits Cargo.lock without a Cargo.toml dep edit or the dependencies
#       label, or a non-release PR touches version lockstep files
#   40) VERSION copies disagree (cargo xtask bump --check)
#   60) a persist path spawns a thread or writes a file outside PersistService
#   70) a workflow uses an action by tag or branch instead of a full commit SHA
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0

# Fail closed: a missing or empty guard directory must not pass.
shopt -s nullglob
guards=(scripts/ci-guards.d/*.sh)
shopt -u nullglob
if ((${#guards[@]} == 0)); then
  echo "ci-guards: no guards found in scripts/ci-guards.d/" >&2
  exit 1
fi
for guard in "${guards[@]}"; do
  # shellcheck source=/dev/null
  source "$guard"
done

if (( fail != 0 )); then
  exit 1
fi
echo "ci-guards: ok (${#guards[@]} guards)"
