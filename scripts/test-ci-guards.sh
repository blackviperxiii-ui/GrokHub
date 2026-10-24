#!/usr/bin/env bash
# Fail-closed regression tests for scripts/ci-guards.sh.
# Builds a throwaway origin + clone, runs the guard script in PR mode against
# crafted bad inputs, and asserts it exits non-zero (never skip-passes).
set -euo pipefail

SRC="$(cd "$(dirname "$0")/.." && pwd)"
REAL_GIT="$(command -v git)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
export REAL_GIT

# Stub cargo (xtask bump --check always ok) and a git shim that can fail one subcommand.
mkdir -p "$TMP/shim"
printf '#!/usr/bin/env bash\nexit 0\n' >"$TMP/shim/cargo"
printf '#!/usr/bin/env bash\nif [[ "${1:-}" == "${FAIL_GIT_SUBCMD:-}" ]]; then exit 2; fi\nexec "$REAL_GIT" "$@"\n' >"$TMP/shim/git"
chmod +x "$TMP/shim/cargo" "$TMP/shim/git"

"$REAL_GIT" init -q --bare -b beta "$TMP/origin.git"
"$REAL_GIT" clone -q "$TMP/origin.git" "$TMP/work" 2>/dev/null
W="$TMP/work"
mkdir -p "$W/scripts" "$W/crates/grokhub-app/src/app" "$W/crates/grokhub-core/src"
cp "$SRC/scripts/ci-guards.sh" "$W/scripts/ci-guards.sh"
for f in crates/grokhub-app/src/app/mod.rs crates/grokhub-app/src/cards.rs \
  crates/grokhub-core/src/slash.rs crates/grokhub-app/src/app/tests.rs \
  crates/grokhub-app/src/app/persist.rs crates/grokhub-app/src/app/projects.rs \
  crates/grokhub-app/src/app/dock_ui.rs; do
  printf 'fn main() {}\n' >"$W/$f"
done
printf '[workspace.package]\nversion = "1.0.0"\n\n[workspace.dependencies]\n' >"$W/Cargo.toml"
printf 'name = "grokhub-app"\nversion = "1.0.0"\n' >"$W/Cargo.lock"
printf 'assert_eq!(env!("CARGO_PKG_VERSION"), "1.0.0");\n' >"$W/crates/grokhub-app/src/cli.rs"
printf '1.0.0\n' >"$W/VERSION"
"$REAL_GIT" -C "$W" add -A
"$REAL_GIT" -C "$W" commit -q -m base
"$REAL_GIT" -C "$W" push -q origin HEAD:beta

pass=0
failed=0

# run_guard <expect: pass|fail> <name> [FAIL_GIT_SUBCMD]
run_guard() {
  local expect="$1" name="$2" shim="${3:-}" rc=0
  PATH="$TMP/shim:$PATH" FAIL_GIT_SUBCMD="$shim" GITHUB_EVENT_NAME=pull_request \
    GITHUB_BASE_REF=beta GITHUB_PR_LABELS="${LABELS:-none}" \
    bash "$W/scripts/ci-guards.sh" >"$TMP/out.log" 2>&1 || rc=$?
  if { [[ "$expect" == pass && $rc -eq 0 ]] || [[ "$expect" == fail && $rc -ne 0 ]]; }; then
    echo "ok   $name"
    pass=$((pass + 1))
  else
    echo "FAIL $name (expected $expect, rc=$rc)"
    sed 's/^/    | /' "$TMP/out.log"
    failed=$((failed + 1))
  fi
}

# fresh_pr: reset the PR branch to origin/beta; caller mutates, then commit_pr.
fresh_pr() {
  "$REAL_GIT" -C "$W" checkout -q -B pr origin/beta
  LABELS=none
}
commit_pr() {
  "$REAL_GIT" -C "$W" add -A
  "$REAL_GIT" -C "$W" commit -q --allow-empty -m change
}

fresh_pr
printf '// harmless\n' >>"$W/crates/grokhub-core/src/slash.rs"
commit_pr
run_guard pass "baseline harmless PR passes"

run_guard fail "git grep error fails" grep
run_guard fail "git diff error fails" diff

fresh_pr
printf '<<<<<<< HEAD\n' >>"$W/VERSION.txt"
commit_pr
run_guard fail "conflict marker fails"

fresh_pr
printf '[workspace]\nversion = "1.0.1"\n' >"$W/Cargo.toml"
commit_pr
run_guard fail "unparseable Cargo.toml version fails"

fresh_pr
printf '// no version assert here\n' >"$W/crates/grokhub-app/src/cli.rs"
commit_pr
run_guard fail "cli.rs missing cabin_reports_version line fails"

fresh_pr
printf 'name = "other"\nversion = "1.0.0"\n' >"$W/Cargo.lock"
commit_pr
LABELS=dependencies run_guard fail "Cargo.lock without grokhub versions fails"

fresh_pr
printf 'name = "grokhub-app"\nversion = "1.0.0"\n\nname = "grokhub-proto"\nversion = "1.0.0"\n' >"$W/Cargo.lock"
commit_pr
LABELS=dependencies run_guard pass "new grokhub crate at the current version passes"

fresh_pr
printf 'name = "grokhub-app"\nversion = "1.0.1"\n' >"$W/Cargo.lock"
commit_pr
LABELS=dependencies run_guard fail "grokhub-* lock version bump without release fails"

fresh_pr
printf 'fn f() {\n    std::thread::spawn(|| {});\n}\n' >>"$W/crates/grokhub-app/src/app/persist.rs"
commit_pr
run_guard fail "raw thread::spawn in a persist path fails"

fresh_pr
printf 'fn f() {\n    let _ = fs::write("x", "y");\n}\n' >>"$W/crates/grokhub-app/src/app/projects.rs"
commit_pr
run_guard fail "raw fs::write in a persist path fails"

fresh_pr
printf 'fn f() {\n    // ci-guard: allow-raw-io test reason\n    std::thread::spawn(|| {});\n}\n' >>"$W/crates/grokhub-app/src/app/dock_ui.rs"
commit_pr
run_guard pass "marked raw spawn in a persist path passes"

fresh_pr
rm "$W/crates/grokhub-app/src/app/persist.rs"
commit_pr
run_guard fail "missing raw-io guarded file fails"

fresh_pr
"$REAL_GIT" -C "$W" remote remove origin
printf '// x\n' >>"$W/crates/grokhub-core/src/slash.rs"
commit_pr
run_guard fail "unresolvable merge-base fails"
"$REAL_GIT" -C "$W" remote add origin "$TMP/origin.git"

echo "passed=$pass failed=$failed"
(( failed == 0 ))
