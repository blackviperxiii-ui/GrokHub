#!/usr/bin/env bash
# Repo friction guards. Each guard is its own file in scripts/ci-guards.d/,
# sourced in name order, so a new guard is a new file instead of an edit to a
# shared one. A guard sets fail=1 to fail the job. Today's guards fail CI when:
#   10) conflict markers are committed
#   20) god-file line budgets are exceeded (ratchet; raise only when intentional)
#   30) a PR edits Cargo.lock without a Cargo.toml dep edit or the dependencies
#       label, or a non-release PR touches version lockstep files
#   40) VERSION copies disagree (cargo xtask bump --check)
#   50) the linux or windows job in ci.yml could skip, and so pass as a
#       required check, when the changes job fails
#   60) a persist path spawns a thread or writes a file outside PersistService
#   70) a workflow or composite action uses an action by tag or branch instead
#       of a full commit SHA, or no workflow file can be read
#   75) a workflow step merges downloaded artifacts without a pattern, or with
#       one that takes the unsigned Windows staging files
#   76) release.yml's plan job could build a tag before
#       scripts/release-tag-check.sh has passed it
#   80) our code uses an API anti-cheats flag: memory reads, module lists,
#       injection or global keyboard/mouse hooks
#   90) Rust embeds Markdown or a docs/, screenshots/ or research/ file that
#       the embedded path filter in ci.yml does not cover, or that filter
#       drops the docs that tests read when they run
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
