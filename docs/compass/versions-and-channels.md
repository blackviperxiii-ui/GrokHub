# Compass: versions, channels, and the build label

## Owns
- The one lockstep version (`VERSION`, `[workspace.package] version` in `Cargo.toml`) and the build label Settings → About shows: `2.13.1-beta.3` on beta, `2.13.1` on stable (`grokhub-shell --version` prints the plain version).
- Install channels: `stable` builds `main`, `beta` builds `beta`; the choice lives in the `channel` receipt in the config dir.
## Quick commands
- `grokhub-shell --version`
- `cargo xtask bump --check` / `cargo xtask bump <X.Y.Z>`
- `cargo test -p grokhub-core channel` · `cargo test -p xtask`
## Key files
- `src-tauri/build.rs`: sets the shell's base version, channel and beta N (GROKHUB_SHELL_BASE, GROKHUB_SHELL_CHANNEL, GROKHUB_SHELL_BETA_N) from `VERSION`, git and `GROKHUB_CHANNEL`; the version function in `src-tauri/src/shell_update.rs` builds the label from them.
- `crates/grokhub-core/src/channel.rs`: `Channel`, `CHANNEL_RECEIPT`, `version_line`, `parse_version_line`.
- `src-tauri/src/shell_update.rs`: the app's Stable/Beta switch (`set_channel` writes the receipt) and its self-update.
- `xtask/src/main.rs` + `changelog.d/README.md`: release ownership; feature PRs add fragments, bump folds them.
- `CHANGELOG.md`: dated sections; leftover `## Unreleased` is folded by `xtask bump`.
## Change recipe
- PR into beta: no bump. Add `changelog.d/<slug>.md` and stop (do not edit `CHANGELOG.md` or version files). CI fails version-file edits without the `release` label (`scripts/ci-guards.sh`).
- Promotion or hotfix only, with the maintainer's approval: label `release`, then `cargo xtask bump <X.Y.Z>` updates `VERSION`, `Cargo.toml`, `Cargo.lock` `grokhub-*`, `docs/REFERENCE.md`, both PKGBUILDs (their `pkgver=` line), `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.lock`, then folds fragments (`CLAUDE.md`).
## What breaks it
- Editing the Cargo version to get a beta label: build.rs derives `-beta.N` from the branch or `GROKHUB_CHANNEL` plus commit count since the newest `vX.Y.Z` tag.
- Bumping things that must not move: Imagine `Quality (v2.0)`, specs marked `(do not bump)`, the diagnostics fixture, and `Version=` in `packaging/grokhub.desktop`.
- A partial bump: `ci-guards` runs `cargo xtask bump --check` and blocks non-`release` version edits; `--locked` CI fails if `Cargo.lock` lags.
## What depends on it
- Settings → About → Updates (reads and writes the receipt) and `scripts/install.sh` (writes the receipt).
## Non-obvious
- Beta N is `git rev-list --count <newest-vX.Y.Z>..HEAD`; missing tags (shallow clone) → `beta.0` until `git fetch --tags`. build.rs reruns on `HEAD`, `packed-refs`, `logs/HEAD`, and the branch ref.
- A missing, empty, or unknown receipt reads as stable. Beta never turns itself off; switching back is the Updates page's Stable choice.
- The app updates both channels from signed GitHub releases: stable from `releases/latest`, beta from the newest `vX.Y.Z-beta.N` or legacy `beta-v*` pre-release by `created_at` (`pick_release` in `crates/grokhub-core/src/release_verify.rs`). `.github/workflows/release.yml` publishes those tags as prerelease, never Latest.
- `.github/`, `clippy.toml`, and release scripts ship in PRs like other changes (squash-merge into `beta` on green).
## See also
- [install-scripts](install-scripts.md), grokhub-app
