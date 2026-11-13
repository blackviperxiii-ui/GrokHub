# Sourced by scripts/ci-guards.sh from the repo root. Set fail=1 to fail CI.
# --- Scratch test files ---------------------------------------------------------
# Throwaway harnesses (a real-media walk, a one-off probe) go in `tests/zz_*`
# so they are easy to spot; they must never be committed.
scratch=$(git ls-files -- 'crates/*/tests/zz_*' 'src-tauri/tests/zz_*' 'ui/src/zz_*')
if [[ -n "$scratch" ]]; then
  echo "ci-guards: scratch test files are committed (delete them):" >&2
  echo "$scratch" >&2
  fail=1
else
  echo "ci-guards: no scratch test files"
fi
