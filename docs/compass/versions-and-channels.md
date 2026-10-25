# Compass: versions, channels, and the build label

## Owns
- The one lockstep version (`VERSION`, `[workspace.package] version` in `Cargo.toml`) and what `grokhub --version` prints: `GrokHub 2.13.1-beta.3 (beta @ abc1234)` on beta, `GrokHub 2.13.1 (main @ abc1234)` on stable.
- Install channels: `stable` builds `main`, `beta` builds `beta`; the choice lives in the `channel` receipt in the config dir.
## Quick commands
- `cargo run -p grokhub-app -- --version`
- `cargo xtask bump --check` / `cargo xtask bump <X.Y.Z>`
- `cargo test -p grokhub-core channel` · `cargo test -p grokhub-app cabin_reports_version` · `cargo test -p xtask`
## Key files
- `crates/grokhub-app/build.rs`: sets `GROKHUB_BUILD_CHANNEL`, `GROKHUB_BUILD_BRANCH`, `GROKHUB_BUILD_SHA`, `GROKHUB_BUILD_BETA_N` from git and `GROKHUB_CHANNEL`.
- `crates/grokhub-core/src/channel.rs`: `Channel`, `CHANNEL_RECEIPT`, `version_line`, `parse_version_line`, `ChannelTips`, `beta_caught_up_to_main`, `channel_switch_shell`; `fetch_channel_tips` in `update.rs`.
- `crates/grokhub-core/src/update.rs`: `CabinSemver`, `parse_cabin_semver`, `cabin_version_newer`, `beta_auto_off_for_versions`.
- `xtask/src/main.rs` + `changelog.d/README.md`: release ownership; feature PRs add fragments, bump folds them.
- `CHANGELOG.md`: dated sections; leftover `## Unreleased` is folded by `xtask bump`.
## Change recipe
- PR into beta: no bump. Add `changelog.d/<slug>.md` and stop (do not edit `CHANGELOG.md` or version files). CI fails version-file edits without the `release` label (`scripts/ci-guards.sh`).
- Promotion or hotfix only, on Viper's say: label `release`, then `cargo xtask bump <X.Y.Z>` updates `VERSION`, `Cargo.toml`, `Cargo.lock` `grokhub-*`, `docs/REFERENCE.md`, both PKGBUILDs, `packaging/windows/grokhub.iss`, and `cabin_reports_version` in `crates/grokhub-app/src/cli.rs`, then folds fragments (`CLAUDE.md`).
## What breaks it
- Editing the Cargo version to get a beta label: build.rs derives `-beta.N` from the branch or `GROKHUB_CHANNEL` plus commit count since the newest `vX.Y.Z` tag.
- Bumping things that must not move: Imagine `Quality (v2.0)`, specs marked `(do not bump)`, the diagnostics fixture, and `Version=` in `packaging/grokhub.desktop`.
- A partial bump: `cabin_reports_version` asserts the literal; `ci-guards` runs `cargo xtask bump --check` and blocks non-`release` version edits; `--locked` CI fails if `Cargo.lock` lags.
## What depends on it
- In-app Update (refuses a clone on the other channel), the Labs Beta toggle, `scripts/install.sh` (writes the receipt), and `grokhub --version`.
## Non-obvious
- Beta N is `git rev-list --count <newest-vX.Y.Z>..HEAD`; missing tags (shallow clone) → `beta.0` until `git fetch --tags`. build.rs reruns on `HEAD`, `packed-refs`, `logs/HEAD`, and the branch ref.
- A missing, empty, or unknown receipt reads as stable. Updater compares with prerelease order; stable never gets a prerelease offer. Labs auto-off still uses trees (`beta_caught_up_to_main`); `beta_auto_off_for_versions` is the pure version rule.
- Without a source clone both channels update from signed GitHub releases: stable from `releases/latest`, beta from the newest `vX.Y.Z-beta.N` or legacy `beta-v*` pre-release by `created_at` (`pick_release` in `crates/grokhub-core/src/release_verify.rs`). `.github/workflows/release.yml` publishes those tags as prerelease, never Latest.
- Every Update or channel switch appends a redacted tail to `update.log`. `.github/`, `clippy.toml`, and release scripts ship in PRs like other changes (squash-merge into `beta` on green).
## See also
- [install-scripts](install-scripts.md), [grokhub-app](grokhub-app.md)
