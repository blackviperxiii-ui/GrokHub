# Compass: install scripts

## Owns
- `scripts/install.sh`: Linux clone install. Builds `grokhub` + `grokhub-hub` in release, installs into `$PREFIX` (default `~/.local`), the desktop entry and icons, user systemd units, and ffmpeg/alsa via the distro package manager. It does not install the Grok Build CLI. Writes `source` and the `channel` receipt into `$GROKHUB_CONFIG` (default `~/.config/GrokHub`).
- `scripts/install-windows.ps1` (Windows clone overlay), `scripts/sync-user-integration.sh`, `scripts/build-hands.sh` (legacy sidecars, not called by install).
## Quick commands
- `./scripts/install.sh --dry-run` / `./scripts/install.sh --channel beta --dry-run` (prints channel, branch, prefix, receipt; no build)
- `cargo test -p grokhub-core hands` and `cargo test -p grokhub-core desktop_entry` (the text checks on these scripts)
## Key files
- `scripts/install.sh`, `scripts/install-windows.ps1`, `scripts/sync-user-integration.sh`.
- `crates/grokhub-core/src/hands.rs`: `missing_uinput_daemon_receipts_are_distinct` asserts on the text of the install scripts, `packaging/PKGBUILD`, the AUR files, and `scripts/make-release-bundle.sh`.
- `crates/grokhub-core/src/desktop_entry.rs` and `crates/grokhub-core/src/update.rs`: more `include_str!` checks on `install.sh`, `install-windows.ps1`, and `sync-user-integration.sh`.
## Change recipe
- Edit the script, then run the core tests above: they pin strings like `update-desktop-database`, `enable --now grokhub-hub.service`, `alsa-utils`, `Overlay-Locked`, and the absence of `build-hands.sh`, `ydotoold`, and `pgrep grokhub`.
- Keep `install.sh` and `scripts/make-release-bundle.sh` in step; the tests check both. The release bundle script needs Jeremy's OK to edit (`CLAUDE.md`).
## What breaks it
- A new step that can fail the overlay under `set -euo pipefail`: package installs and the systemd steps deliberately continue on failure.
- Adding a Grok Build CLI install or update step: `crates/grokhub-core/tests/no_grok_cli.rs` fails when packaging or an install script installs or bundles the CLI, or a deleted CLI installer comes back.
- A remembered channel that does not match the checkout: `install.sh` refuses so a build is not labeled for the wrong channel.
## What depends on it
- In-app Update and `grokhub --update` (`install.sh --user`), the Labs beta toggle (`channel_switch_cmds` in `crates/grokhub-app/src/update.rs` adds `--channel`), and the README install docs.
## Non-obvious
- The `--dry-run` line (`channel=… branch=… switch=… prefix=… receipt=…`) is asserted literally by `install_sh_parses_channel_flags_and_follows_the_receipt`, which runs the real script on unix.
- No `--channel`: the receipt wins, missing means stable (`main`). `--channel` refuses a dirty tree and only fast-forwards the local branch.
- `GROKHUB_CHANNEL` is exported into `cargo build` so `crates/grokhub-app/build.rs` labels the binary.
- The desktop entry's `Exec=grokhub` is rewritten to the absolute prefix path, so launch works without `~/.local/bin` on PATH.
- Windows Setup ships only `grokhub.exe` and `grokhub-hub.exe` and leaves PATH alone; `hands.rs` asserts both `Source:` lines in `packaging/windows/grokhub.iss`.
- Windows overlay renames a locked running `grokhub.exe` to `.old`, then copies.
- `.cursor/install.sh` is the Cloud Agent bootstrap, not an installer: apt build deps plus `cargo build --workspace --locked`.
## See also
- [versions-and-channels](versions-and-channels.md), [grokhub-hub](grokhub-hub.md), [grokhub-app](grokhub-app.md)
