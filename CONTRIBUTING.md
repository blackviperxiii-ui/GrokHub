# Contributing to GrokHub

These rules apply to every contributor and every bot (Cursor, Claude, GrokHub). `CLAUDE.md` and `.cursor/rules/` hold the full repo gates; this is the short version.

## Beta first

- Every new feature and fix PR branches off `beta` and targets `beta`.
- `beta` moves to `main` only when Jeremy says so, as one squash-merged `beta` → `main` PR.
- Hotfixes go to `main` only when Jeremy says so. Afterward, `main` is merged back into `beta` with a merge commit, never a force-push.
- Never push straight to `main` or `beta`, never force-push, and never delete branches unless Jeremy asks.

## Green and ready, then stop

- Bots take a PR to green (Linux and Windows CI passing) and ready to merge, with proof of work, then stop and report it.
- Nobody merges, tags, or releases anything without Jeremy's explicit say. When he says merge, it's squash and merge.
- **Latest base branch:** the base is `beta` for feature and fix PRs, and `main` for hotfixes and the promotion. Fetch origin and merge the latest base into your branch (a merge commit, no rebase or force-push) before you start, before reporting the PR ready, and right before any merge or tag he asked for. Name the base branch and its starting SHA in the PR body.
- Every PR body has the proof of work (the real run path exercised and what was seen) and a **Skipped / not verified** section.

## Versions

- PRs into `beta` don't bump the version. They add their notes under `## Unreleased` in `CHANGELOG.md`.
- Beta builds show the base version plus `-beta`, with the branch and short SHA: `GrokHub 2.10.92-beta (beta @ abc1234)`. `crates/grokhub-app/build.rs` reads these from git; never edit the Cargo version for them.
- Each `beta` → `main` promotion carries exactly one patch bump and one release.
- A hotfix to `main` carries its own bump and is merged back into `beta`.
- A bump changes the files listed in `CLAUDE.md` (Commits, PRs, versions).

## Tests

- CI runs `cargo test --workspace --locked -- --test-threads=1` and `cargo clippy --workspace --all-targets -- -D warnings` on Linux and Windows.
- Locally, set `GROKHUB_CONFIG` to a temp dir and run only the tests you touched.
- Tests assert literal expected values. Never skip, ignore, delete, or loosen a test to get green.
- Format only the lines you touch; don't mass `cargo fmt`.

## Channels on your machine

- `./scripts/install.sh --user --channel beta` switches a Linux install to beta.
- `./scripts/install.sh --user --channel stable` switches it back to `main`.
- See the **Channels** section of [docs/REFERENCE.md](docs/REFERENCE.md) for details.

## Branch cleanup
After merge or close, delete your head branch. Scratch branches go as soon as the proof is done. Never delete `main` or `beta`.

## Merge-back (main → beta)
After a hotfix or a promotion lands on `main`, bring `beta` up with a **PR from main into beta**, merged with a **merge commit** (not squash), after linux + windows CI. Direct pushes to `beta` are blocked when the branch ruleset is active.

## Tooling
- Semgrep before Ready. dyl-review for draft asks only. Continual Learning updates `AGENTS.md`. No dyl-ready-pr merge/babysit.
- Grok Build bundled roles: explore, plan, implementer (use implementer for build/code).

