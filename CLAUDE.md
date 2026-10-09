# GrokHub repo gates

Native Rust cabin (`crates/`). These rules apply to every Cursor, Claude, and Grok chat working here. `.cursor/rules/repo-gates.mdc` holds the same rules.

## Branches, merges, releases
- **Beta first.** Every new feature and fix PR from every bot (Cursor, Claude, GrokHub) branches off `beta` and targets `beta`. `beta` moves to `main` only on Jeremy's say, as one squash-merged `beta` → `main` PR. Hotfixes go to `main` only on his say, and then `main` is merged back into `beta` (a merge commit, no force). Never push straight to `main` or `beta`, never force-push, never delete branches unless Jeremy asks.
- **Green and ready, then stop.** Bots get a PR green (Linux and Windows CI passing) and ready to merge, with proof of work, then report it. Never merge, tag, or release anything, Claude's and Cursor's PRs included, without Jeremy's explicit say. When he says merge, it's squash and merge. Steps: `.cursor/rules/merge-prs.mdc`.
- No changes to `.github/`, `clippy.toml`, or release scripts (`scripts/make-*release*`, `packaging/windows/`, `packaging/aur/`) without his OK. Version-bump lines (below) are the exception.

## Secrets
- Never commit `.env*` (only `.env.example`), tokens, keys, certs, `auth.json`, `secrets.json`, or credential files. Tests use obvious fakes (`sk-abcdefghijklmnopqrstuv`).
- Never read, copy, link, or reuse `~/.grok/auth.json` or `~/.grok/config.toml`, or the Grok CLI's credentials. Imagine uses its own sign-in in the OS keychain.
- If you find a committed secret, don't rewrite history. Report the file, commit, type, and first 4 characters, never the value.

## Tests (CI is the gate)
CI runs these on Linux and Windows with the toolchain from `rust-toolchain.toml` (stable):

```sh
cargo nextest run --workspace --locked --profile ci
cargo test --doc --workspace --locked
cargo clippy --workspace --all-targets -- -D warnings
```

- nextest runs each test in its own process, in parallel (`.config/nextest.toml`). Without nextest, `cargo test --workspace --locked -- --test-threads=1` runs the same tests. A test that only fails side by side goes in the `serial` group with a comment, never skipped.

- A full local `cargo test` can touch the real `~/.grok` and `~/.config/GrokHub`. Set `GROKHUB_CONFIG` to a temp dir, run only the crate or test you changed, or rely on CI.
- **Test shape:** assert literal expected values, no tautologies (would it still pass if the code returned a default or stub?). Never skip, `#[ignore]`, delete, or loosen a test to get green; fix the code and report suspect tests. Don't add `#[allow(dead_code)]`; delete dead code.
- Don't mass `cargo fmt`. Format only the lines you touch.
- `chat_composer_pins_stop_on_the_right` (`crates/grokhub-app/src/app/tests.rs`) reads a 12,000-byte window after `ComposerStackSlot::Pill =>` in `chat_ui.rs`. The last string it checks for sits about 310 bytes from the window's edge, and less with Windows CRLF line endings. Don't grow the Pill arm before the Stop handler. Move code into helpers instead of widening the window.

## How bots work
- **Proof of work:** every PR body names the real run path exercised and what was seen (CI green is the floor, not the proof), plus a **Skipped / not verified** line.
- **Findings:** log non-blocking findings to the box findings log and fix them in batches; no per-finding or one-test PRs.
- **Stacks:** order them so every PR ends green; one agent owns rebase and topology, others push only their own branch.
- **Agent briefs** follow the template: intent, data shape, scope and non-goals, file boundaries, required evidence, file pointers, exact error plus at most 20 log lines.
- **CI failures:** find and classify the cause before at most one rerun; a repeat failure is real.
- **Latest base branch:** the base branch is `beta` for feature and fix PRs, and `main` for hotfixes and the `beta` → `main` promotion. Before touching a branch, fetch origin, confirm it is based on the latest base branch, and merge the base into it (a merge commit; no rebase or force-push). Fetch and re-check before reporting a PR ready, right before a merge he asked for, and again before a tag he asked for. Never render or screenshot from a stale checkout. Every PR body and report names the base branch and the SHA the work started from.

## Commits, PRs, versions
- Titles: a plain sentence for PRs into `beta`. A PR that bumps (a promotion or a hotfix) uses `Cabin X.Y.Z: <what changed>`, or a plain sentence ending in `(X.Y.Z)`. Add a short bullet body and the PR number when merged.
- **Versions:** PRs into `beta` don't bump the version; they add their notes under `## Unreleased` in `CHANGELOG.md`. Semver (Jeremy, 2026-10-09): **PATCH** (`x.y.Z`) is a fix or polish rollout; **MINOR** (`x.Y.0`, patch back to 0) is a promotion with at least one new user-facing feature; **MAJOR** (`X.0.0`) is a big milestone, only on Jeremy's say. No cap on any part (`2.10.99` was followed by `2.11.0` because it added features, not because of a cap). Each `beta` → `main` promotion carries exactly one bump (PATCH or MINOR by that rule) and one release. A hotfix to `main` is a PATCH bump and is merged back into `beta`. Beta builds will carry `-beta.N` (e.g. `2.11.0-beta.3`) so beta never burns release numbers; until that follow-up lands, beta's `VERSION` stays plain and builds show the base version plus `-beta` with the branch and short SHA (`GrokHub 2.11.0-beta (beta @ abc1234)`), which `crates/grokhub-app/build.rs` reads from git; never edit the Cargo version for it.
- A bump changes `VERSION` once, in the same files as #453: `VERSION`, `Cargo.toml` `[workspace.package] version`, the `grokhub-*` entries in `Cargo.lock`, `docs/REFERENCE.md` (headline, both Latest rows, and the `--version` examples; `README.md` has no version strings), `packaging/PKGBUILD`, `packaging/aur/PKGBUILD`, `packaging/windows/grokhub.iss`, and the `cabin_reports_version` assert in `crates/grokhub-app/src/cli.rs`. Add a `CHANGELOG.md` section with the Linux and Windows artifact lines.
- Don't bump the Imagine `Quality (v2.0)`, specs marked `(do not bump)`, the diagnostics fixture, or `packaging/grokhub.desktop` `Version=`.

## Branch cleanup
- After a PR merges or closes, delete **your own** head branch (`gh pr view --json headRefName` then `git push origin --delete <branch>`). Prefer GitHub auto-delete when enabled.
- Delete scratch / proof / throwaway branches as soon as the proof is done.
- Never delete `main` or `beta`.

## Which bot runs the job
- **Grok Build** (this box): default for coding, PRs, CI watches, Semgrep, and GrokHub ship work while Cursor Cloud is out.
- **Cursor**: UI Critiquito handoff coordination stays with the parent; Cursor Cloud agents resume after Oct 11. Prefer Grok Build for implement/explore/plan roles when both are available.
- Pick one bot per job; do not double-run the same PR.

## Grok Build roles (bundled)
Grok Build 1.0.49+ ships explore / plan / implementer (and reviewer, test-writer, …) under `~/.grok/bundled/roles/`. There is no separate "build" role name — use **implementer** for build/code. Invoke with `grok --agent <role>` or project agent config when spawning subagents. Do not invent custom role TOML unless Jeremy asks.

## Semgrep + dyl-review + Continual Learning
- Run **Semgrep** on touched paths before marking a PR Ready (`semgrep --config=auto` or the Semgrep plugin). Fix or justify findings in the PR body. Do **not** use dyl-ready-pr merge/babysit.
- **dyl-review** (quick): draft review asks for the human; never auto-post merge.
- **Continual Learning**: when mining chats, update `AGENTS.md` via the continual-learning skill / agents-memory-updater. Keep AGENTS.md short and factual.

