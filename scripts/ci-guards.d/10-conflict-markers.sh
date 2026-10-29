# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- 1. Conflict markers (study: ^<<<<<<< / ^>>>>>>>) ---------------------------
# git grep exits 1 for "no match"; anything else is an error and must fail.
grep_rc=0
markers=$(git grep -nE '^(<<<<<<<|>>>>>>>)' -- . ':!.git/**' 2>/dev/null) || grep_rc=$?
if (( grep_rc > 1 )); then
  echo "ci-guards: git grep for conflict markers errored (rc=$grep_rc)" >&2
  fail=1
elif [[ -n "$markers" ]]; then
  echo "ci-guards: conflict markers found:" >&2
  echo "$markers" >&2
  fail=1
else
  echo "ci-guards: no conflict markers"
fi
