# Compass: versions, channels, and the build label

## Owns
- The one lockstep version (`VERSION`, `[workspace.package] version` in `Cargo.toml`) and what `grokhub --version` prints: `GrokHub 2.10.93-beta (beta @ abc1234)` on beta, `GrokHub 2.10.93 (main @ abc1234)` on stable.
- Install channels: `stable` builds `main`, `beta` builds `beta`; the choice lives in the `channel` receipt in the config dir.
## Quick commands
- `cargo run -p grokhub-app -- --version`
- `cargo test -p grokhub-core channel` (version lines, receipt, `install.sh --dry-run`)
- `cargo test -p grokhub-app cabin_reports_version`
## Key files
- `crates/grokhub-app/build.rs`: sets `GROKHUB_BUILD_CHANNEL`, `GROKHUB_BUILD_BRANCH`, `GROKHUB_BUILD_SHA` from git and `GROKHUB_CHANNEL`.
- `crates/grokhub-core/src/channel.rs`: `Channel`, `CHANNEL_RECEIPT`, `version_line`, `parse_version_line`, `ChannelTips`, `beta_caught_up_to_main`, `channel_switch_shell`; `fetch_channel_tips` in `update.rs`.
- `crates/grokhub-app/src/update.rs`: `installed_channel`, `build_channel`, `build_version_line`, `channel_switch_cmds`, `try_auto_off_beta_channel`.
- `CHANGELOG.md`: `## Unreleased` collects beta notes.
## Change recipe
- PR into beta: no bump. Add notes under `## Unreleased` in `CHANGELOG.md` and stop.
- Promotion or hotfix only, on Jeremy's say: one patch bump across `VERSION`, `Cargo.toml`, the `grokhub-*` entries in `Cargo.lock`, `README.md` (headline and both Latest rows), `packaging/PKGBUILD`, `packaging/aur/PKGBUILD`, `packaging/windows/grokhub.iss`, and `cabin_reports_version` in `crates/grokhub-app/src/cli.rs`, plus a `CHANGELOG.md` section with the Linux and Windows artifact lines (`CLAUDE.md`).
## What breaks it
- Editing the Cargo version to get a beta label: build.rs derives `-beta` from the branch or `GROKHUB_CHANNEL`.
- Bumping things that must not move: Imagine `Quality (v2.0)`, specs marked `(do not bump)`, the diagnostics fixture, and `Version=` in `packaging/grokhub.desktop`.
- A partial bump: `cabin_reports_version` asserts the literal version, and `--locked` CI fails if `Cargo.lock` lags.
## What depends on it
- In-app Update (refuses a clone on the other channel), the Labs Beta toggle, `scripts/install.sh` (writes the receipt), and `grokhub --version`.
## Non-obvious
- build.rs reruns on `HEAD`, `packed-refs`, `logs/HEAD`, and the branch ref, so a pull or checkout relabels the next build; a detached HEAD reports branch `detached` unless an exact tag matches.
- A missing, empty, or unknown receipt reads as stable.
- Labs Beta auto-off compares trees, not tips: main moves by squash and the main → beta sync always adds a merge commit, so the SHAs never match. `fetch_channel_tips` fetches both branches; `beta_caught_up_to_main` is true when `origin/beta^{tree}` equals `origin/main^{tree}` (or the tips are the same commit). Then `switch_clone_to_main` checks the clone out on main (no rebuild; the next Update builds main) and the receipt becomes stable. A failed fetch never flips; a dirty clone stays on beta and says why.
- Windows has no beta: `installed_channel` reads a stray beta receipt as stable and Update says `BETA_LINUX_ONLY_NOTE`. Labs keeps the toggle disabled (`CHANNEL_WINDOWS_NOTE`); Windows updates come from the GitHub zip or `scripts/install-windows.ps1`.
- Every Update or channel switch appends a redacted tail to `update.log` in the config dir (about 256 KB, rotated once); a failure shows `update_fail_hint` / `channel_switch_fail_hint` plus the log path. A switch step gets 2400 s (`host_timeout_for`).
- `.github/`, `clippy.toml`, and release scripts change only with Jeremy's OK.
## See also
- [install-scripts](install-scripts.md), [grokhub-app](grokhub-app.md)
