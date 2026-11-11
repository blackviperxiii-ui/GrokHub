#!/usr/bin/env bash
# Fail-closed regression tests for scripts/ci-guards.sh and scripts/ci-guards.d/.
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
mkdir -p "$W/scripts" "$W/crates/grokhub-app/src/app" "$W/crates/grokhub-core/src/slash"
cp "$SRC/scripts/ci-guards.sh" "$W/scripts/ci-guards.sh"
cp -r "$SRC/scripts/ci-guards.d" "$W/scripts/ci-guards.d"
for f in crates/grokhub-app/src/app/mod.rs crates/grokhub-app/src/cards.rs \
  crates/grokhub-core/src/slash/mod.rs crates/grokhub-app/src/app/tests.rs \
  crates/grokhub-app/src/app/persist.rs crates/grokhub-app/src/app/projects.rs \
  crates/grokhub-app/src/app/dock_ui.rs; do
  printf 'fn main() {}\n' >"$W/$f"
done
printf '[workspace.package]\nversion = "1.0.0"\n\n[workspace.dependencies]\n' >"$W/Cargo.toml"
printf 'name = "grokhub-app"\nversion = "1.0.0"\n' >"$W/Cargo.lock"
printf 'assert_eq!(env!("CARGO_PKG_VERSION"), "1.0.0");\n' >"$W/crates/grokhub-app/src/cli.rs"
printf '1.0.0\n' >"$W/VERSION"
mkdir -p "$W/packaging/windows"
printf '#define MyAppName "Old"\n#ifndef MyAppVersion\n  #define MyAppVersion "1.0.0"\n#endif\n' >"$W/packaging/windows/grokhub.iss"
mkdir -p "$W/.github/workflows"
SHA=0123456789abcdef0123456789abcdef01234567
# Job `a` stays last: the pinned-action cases append steps to it.
cat >"$W/.github/workflows/ci.yml" <<YAML
jobs:
  changes:
    outputs:
      code: \${{ github.event_name != 'pull_request' || steps.filter.outputs.code == 'true' || steps.paths.outputs.embedded == 'true' }}
      windows: \${{ github.event_name != 'pull_request' || steps.filter.outputs.windows == 'true' || steps.paths.outputs.embedded == 'true' }}
    steps:
      - id: paths
        with:
          filters: |
            embedded:
              - 'crates/**/*.md'
              - 'src-tauri/**/*.md'
              - 'PRIVACY.md'
  linux:
    needs: changes
    if: \${{ !cancelled() && (needs.changes.result != 'success' || needs.changes.outputs.code == 'true') }}
    steps:
      - name: changes job must succeed
        if: needs.changes.result != 'success'
        run: exit 1
  windows:
    needs: changes
    if: \${{ !cancelled() && (needs.changes.result != 'success' || (needs.changes.outputs.code == 'true' && needs.changes.outputs.windows == 'true')) }}
    steps:
      - name: changes job must succeed
        if: needs.changes.result != 'success'
        run: exit 1
  a:
    steps:
      - uses: actions/checkout@$SHA # v4
      - uses: ./.github/actions/local
YAML
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
printf '// harmless\n' >>"$W/crates/grokhub-core/src/slash/mod.rs"
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
sed -i 's/"Old"/"New"/' "$W/packaging/windows/grokhub.iss"
commit_pr
run_guard pass "grokhub.iss name edit without a version change passes"

fresh_pr
sed -i 's/"1.0.0"/"1.0.1"/' "$W/packaging/windows/grokhub.iss"
commit_pr
run_guard fail "grokhub.iss MyAppVersion bump without release fails"
LABELS=release run_guard pass "grokhub.iss MyAppVersion bump with release passes"

fresh_pr
sed -i '/MyAppVersion "/d' "$W/packaging/windows/grokhub.iss"
commit_pr
run_guard fail "grokhub.iss missing MyAppVersion define fails"

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
printf '      - uses: actions/cache@v4\n' >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "workflow action pinned to a tag fails"

fresh_pr
printf '      - uses: attacker/action@main # uses: ./local\n      - uses: attacker/action@v1 # uses: actions/checkout@%s\n' "$SHA" \
  >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "a trailing comment can't make an unpinned action look pinned"

fresh_pr
printf '      - {uses: actions/cache@v4}\n' >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "unpinned action in a flow-style step fails"

fresh_pr
printf '      - "uses": actions/cache@v4\n' >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "unpinned action under a double-quoted uses key fails"

fresh_pr
printf "      - 'uses': actions/cache@v4\n" >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "unpinned action under a single-quoted uses key fails"

fresh_pr
printf '      - uses : actions/cache@v4\n' >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "unpinned action with a space before the colon fails"

fresh_pr
printf '      - {"uses": actions/cache@v4}\n' >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "unpinned action under a quoted key in a flow-style step fails"

fresh_pr
printf '      - "uses": actions/cache@%s\n      - uses : actions/cache@%s\n' "$SHA" "$SHA" >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard pass "pinned action under a quoted or spaced uses key passes"

fresh_pr
printf '  b:\n    steps: [{name: x, uses: actions/cache@v4}]\n' >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "unpinned action in a flow-style list fails"

fresh_pr
printf '      - uses:\n          actions/cache@v4\n' >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "a uses: value on the next line fails"

fresh_pr
printf '      - {uses: actions/cache@%s}\n' "$SHA" >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard pass "pinned action in a flow-style step passes"

fresh_pr
printf '      - uses: docker://alpine@%s\n' "$SHA" >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "docker action with a 40-hex tag instead of a sha256 digest fails"

fresh_pr
printf 'jobs:\n  a:\n    steps:\n      - uses: actions/cache@main\n' >"$W/.github/workflows/other.yaml"
commit_pr
run_guard fail "unpinned action in a .yaml workflow fails"

fresh_pr
mkdir -p "$W/.github/actions/setup"
printf 'runs:\n  using: composite\n  steps:\n    - uses: foo/bar@v3\n' >"$W/.github/actions/setup/action.yml"
commit_pr
run_guard fail "unpinned action in a composite action fails"

fresh_pr
mkdir -p "$W/.github/actions/setup"
printf 'runs:\n  using: composite\n  steps:\n    - uses: "foo/bar@%s" # v3\n' "$SHA" >"$W/.github/actions/setup/action.yml"
commit_pr
run_guard pass "pinned action in a composite action passes"

fresh_pr
printf '      - uses: docker://alpine:3.20\n' >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "docker action without a digest fails"

fresh_pr
printf '      - uses: docker://alpine@sha256:%s%s\n' "$SHA" "0123456789abcdef01234567" >>"$W/.github/workflows/ci.yml"
commit_pr
run_guard pass "docker action with a digest passes"

CI_YML="$W/.github/workflows/ci.yml"

fresh_pr
sed -i "/^  linux:/,/^  windows:/s/^    if: .*/    if: needs.changes.outputs.code == 'true'/" "$CI_YML"
commit_pr
run_guard fail "linux skipping when the changes job fails is rejected"

fresh_pr
sed -i "/^  windows:/,/^  a:/s/^    if: .*/    if: needs.changes.outputs.code == 'true' \&\& needs.changes.outputs.windows == 'true'/" "$CI_YML"
commit_pr
run_guard fail "windows skipping when the changes job fails is rejected"

fresh_pr
sed -i '/^  windows:/,/^  a:/s/!cancelled() && //' "$CI_YML"
commit_pr
run_guard fail "windows without !cancelled() is rejected"

fresh_pr
sed -i '/^  linux:/,/^  windows:/{/run: exit 1/d}' "$CI_YML"
commit_pr
run_guard fail "linux without the step that fails on a failed changes job is rejected"

fresh_pr
sed -i 's/^  linux:/  linux-tests:/' "$CI_YML"
commit_pr
run_guard fail "a missing linux job is rejected"

fresh_pr
printf 'const A: &str = include_str!("../../../../README.md");\n' >>"$W/crates/grokhub-core/src/slash/mod.rs"
commit_pr
run_guard fail "embedding the root README, outside the embedded filter, fails"

fresh_pr
printf 'const A: &str = include_str!("../../../../docs/guide.txt");\n' >>"$W/crates/grokhub-core/src/slash/mod.rs"
commit_pr
run_guard fail "embedding a file under docs/ fails"

fresh_pr
printf 'const A: &[u8] = include_bytes!(\n    "../../../../screenshots/home.png"\n);\n' >>"$W/crates/grokhub-core/src/slash/mod.rs"
commit_pr
run_guard fail "embedding a screenshot with the path on the next line fails"

fresh_pr
printf 'const A: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"));\n' >>"$W/crates/grokhub-core/src/slash/mod.rs"
commit_pr
run_guard fail "embedding Markdown through a computed path fails"

fresh_pr
printf 'const A: &str = include_str!("prompt.md");\nconst B: &str = include_str!("../../../../PRIVACY.md");\n' >>"$W/crates/grokhub-core/src/slash/mod.rs"
commit_pr
run_guard pass "embedding Markdown under crates/ and the root PRIVACY.md passes"

fresh_pr
sed -i "/- 'PRIVACY.md'/d" "$CI_YML"
commit_pr
run_guard fail "an embedded filter that drops a path is rejected"

fresh_pr
sed -i "/^      code: /s/ || steps.paths.outputs.embedded == 'true'//" "$CI_YML"
commit_pr
run_guard fail "a code output that ignores the embedded filter is rejected"

fresh_pr
sed -i "/^      windows: /s/ || steps.paths.outputs.embedded == 'true'//" "$CI_YML"
commit_pr
run_guard fail "a windows output that ignores the embedded filter is rejected"

fresh_pr
mkdir -p "$W/.github/workflows/unreadable.yml"
run_guard fail "unreadable workflow file fails"
rmdir "$W/.github/workflows/unreadable.yml"

fresh_pr
"$REAL_GIT" -C "$W" rm -q "$W/.github/workflows/ci.yml"
commit_pr
run_guard fail "no workflows at all fails"

# A missing guard directory must fail, never pass with zero guards.
fresh_pr
mv "$W/scripts/ci-guards.d" "$TMP/guards.bak"
run_guard fail "empty guard directory fails"
mv "$TMP/guards.bak" "$W/scripts/ci-guards.d"

fresh_pr
"$REAL_GIT" -C "$W" remote remove origin
printf '// x\n' >>"$W/crates/grokhub-core/src/slash/mod.rs"
commit_pr
run_guard fail "unresolvable merge-base fails"
"$REAL_GIT" -C "$W" remote add origin "$TMP/origin.git"

echo "passed=$pass failed=$failed"
(( failed == 0 ))
