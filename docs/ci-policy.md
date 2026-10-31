# CI policy: what runs when

This page describes which GitHub Actions jobs run on which event, as the workflows under `.github/workflows/` define them. If you change a workflow, update this page in the same PR.

Columns, left to right:

- **PR**: a pull request into `beta` or `main`.
- **Queue**: `merge_group`.
- **Beta**: a push to `beta`, which every merged PR makes.
- **Main**: a push to `main`, made by a promotion or a hotfix.
- **Tag**: a `v*` tag.
- **Nightly**: the 08:00 UTC schedule. It fires from `main` but builds `beta`.
- **Manual**: `workflow_dispatch`.

## ci.yml

| Job | PR | Queue | Beta | Main | Tag | Nightly | Manual |
|---|---|---|---|---|---|---|---|
| `changes`, `fmt`, `guards` | yes | yes | yes | yes | yes | yes | yes |
| `linux` (required) | code ¹ | yes | yes | yes | yes | yes ² | yes |
| `windows` (required) | Rust ³ | yes | yes | yes | yes | yes ² | yes |
| `deny` | lock ⁴ | yes | no | yes | yes | yes ² | yes |
| `shell` | ubuntu ⁵ | ubuntu, windows | no | ubuntu, windows | 3 OS | 3 OS ² | 3 OS |
| `phone (android)` | phone ⁶ | yes | no | phone ⁶ | yes | yes ² | yes |
| `phone (ios)` | no | no | no | no | yes | yes ² | yes |
| `cross-check` (macOS, iOS, Android, wasm) | no | no | no | no | yes | yes ² | yes |
| `windows-release` | no | no | no | no | no | yes ² | yes |
| `nightly-green` | no | no | no | no | no | yes | no |

1. Skipped when a PR touches only `docs/`, `*.md`, `screenshots/` or `research/`.
2. Skipped when `beta` has not moved since the last green nightly (see below).
3. Same as 1, and also skipped when a PR touches only `ui/` or workflows other than `ci.yml`.
4. Only when `Cargo.lock` or `deny.toml` changes.
5. Only when shell paths change (`ui/`, `src-tauri/`, the shared crates, `ci.yml`). The Windows WebView2 smoke test runs on Queue, Main, Tag, Nightly and Manual.
6. Only when `ui/`, `src-tauri/` or `ci.yml` changes.

Every gate is a job-level `if:`, never a workflow-level path filter. A skipped job still reports, so a required check (`fmt`, `guards`, `linux`, `windows`, `pii-guard`) never sits waiting.

A newer push to the same PR, or a newer push to `beta`, cancels the run it replaces. Runs on `main`, tags, the nightly and manual runs always finish.

### Nightly skip when green

The `changes` job looks up the cache key `ci-nightly-green-<beta sha>`. If the key is there, `code` is false and every build job skips. Only `changes`, `fmt` and `guards` run, all on Linux. `nightly-green` saves the key after a nightly in which no job failed or was cancelled. Any new commit on `beta` changes the SHA, so the next nightly runs in full. If the cache entry is evicted, the nightly just runs in full again.

## release.yml

| Job | Beta | Tag | Nightly | Manual on `beta` |
|---|---|---|---|---|
| `plan` | yes | yes | yes | yes |
| `linux-bundle` | yes, cached | yes | yes ⁷ | yes, cached |
| `windows-installer` (pack-script tests after Package) | yes, cached | yes, signed ⁸ | yes ⁷ | yes, cached |
| `shell bundles` | ubuntu, windows; cached | 3 OS | 3 OS ⁷ | 3 OS, cached |
| `publish` | `vX.Y.Z-beta.N` pre-release | release (Latest only for `vX.Y.Z`) | pre-release ⁷ | pre-release |

7. The nightly skips everything after `plan` when `vX.Y.Z-beta.N` already exists, which means nothing new landed on `beta`.
8. SignPath signs tag builds when its secrets are configured. Beta builds stay unsigned.

- Rust caches (`linux-release`, `windows-release`, `shell-<os>`) are used only on the `beta` ref. Tags and the nightly build cold.
- A newer beta build cancels one still running. Tag runs never cancel.
- A manual run on any branch other than `beta` builds nothing.
- Pull requests and pushes to `main` never run release.yml.

## Other workflows

| Workflow | Runs on |
|---|---|
| `pii-guard.yml` (required `pii-guard`) | PR, Queue, push to `beta` and `main`. Superseded PR and beta runs are cancelled. |
| `deck-shots.yml` | Manual only. Windows screenshots (deck shots and web search shots). 20-minute job; deck shots stop after 6 minutes. Pick shots with the `only` input. |
| `auto-merge.yml` | PR events into `beta`. Turns on squash auto-merge. Builds nothing. |
| `retarget.yml` | PRs opened against `main`. Moves them to `beta`. Builds nothing. |
| `cards-sync.yml`, `claude.yml` | Bot workflows: schedule, issue, review and manual triggers. They build nothing. |

## Rules for workflow changes

- Heavy jobs stay off pull requests: release builds, screenshots, packaging, macOS and full matrices. PRs run the light gate.
- Every job sets `timeout-minutes`.
- `actions/upload-artifact` steps for CI evidence (smoke and deck screenshots) keep files for 7 days or less. release.yml's build artifacts only carry files to `publish`, and the files ship on the GitHub release.
- Required check names (`fmt`, `guards`, `linux`, `windows`, `pii-guard`) don't change without the matching ruleset change.
- Actions are pinned to a commit SHA (`scripts/ci-guards.sh`).
