# Compass: install scripts

## Owns
- `scripts/install.sh`: Linux clone install. Builds `grokhub-hub` (and the `grokhub` shim) with cargo and `grokhub-shell` with `tauri build --no-bundle`, installs into `$PREFIX` (default `~/.local`), the desktop entry and icons, user systemd units, and ffmpeg/alsa via the distro package manager. It does not install the Grok Build CLI. Writes `source` and the `channel` receipt into `$GROKHUB_CONFIG` (default `~/.config/GrokHub`).
- `scripts/sync-user-integration.sh`, `scripts/build-hands.sh` (legacy sidecars, not called by install).
## Quick commands
- `./scripts/install.sh --dry-run` / `./scripts/install.sh --channel beta --dry-run` (prints channel, branch, prefix, receipt; no build)
- `cargo test -p grokhub-core hands` and `cargo test -p grokhub-core desktop_entry` (the text checks on these scripts)
## Key files
- `scripts/install.sh`, `scripts/sync-user-integration.sh`.
- `crates/grokhub-core/src/hands.rs`: `missing_uinput_daemon_receipts_are_distinct` asserts on the text of the install scripts, `packaging/PKGBUILD` and the AUR files; `pkgbuilds_build_and_install_the_shell_and_hub` pins both PKGBUILDs' `build()` to the same steps as `install.sh`.
- `crates/grokhub-core/src/desktop_entry.rs` and `crates/grokhub-core/src/channel.rs`: more checks on `install.sh` and `sync-user-integration.sh` (`include_str!` text checks, and runs of its channel switch and package block).
## Change recipe
- Edit the script, then run the core tests above: they pin strings like `update-desktop-database`, `enable --now grokhub-hub.service`, `alsa-utils`, and the absence of `build-hands.sh`, `ydotoold`, and `pgrep grokhub`.
- Keep `install.sh` and both PKGBUILDs in step; the tests check all three. Edit them in a normal PR; CI green, then squash-merge into `beta` (`CLAUDE.md`). A PKGBUILD's `pkgver=` line moves only with `cargo xtask bump`.
- `CARGO_TARGET_DIR`: `install.sh` and both PKGBUILDs run `realpath -m` before the build so a relative value stays one folder for the hub and the shell (tauri cds into `src-tauri`).
- After an AUR `PKGBUILD` recipe edit, refresh `packaging/aur/.SRCINFO` with `python3 scripts/aur-srcinfo.py --write` (ci-guards compares them; `xtask bump` rewrites the version lines).
## What breaks it
- A new step that can fail the overlay under `set -euo pipefail`: package installs and the systemd steps deliberately continue on failure.
- Leaving `CARGO_TARGET_DIR` relative: the shell build and the install paths disagree after tauri's cwd change.
- Adding a Grok Build CLI install or update step: `crates/grokhub-core/tests/no_grok_cli.rs` fails when packaging or an install script installs or bundles the CLI, or a deleted CLI installer comes back.
- A remembered channel that does not match the checkout: `install.sh` refuses so a build is not labeled for the wrong channel.
## What depends on it
- The README install docs and `grokhub-hub --update`, which tells a source install to run `scripts/install.sh --channel <channel>`, and a prebuilt user AppImage (Arch/CachyOS) to use Settings → Updates.
- The desktop app's Updates page: it shares the `channel` receipt; prebuilt `…/bin` installs without a live `source` receipt self-update via the AppImage; a clone receipt still points at `scripts/install.sh --from-source` (keep both files' names and one-line format).
## Non-obvious
- The `--dry-run` line (`channel=… branch=… switch=… prefix=… receipt=…`) is asserted literally by `install_sh_parses_channel_flags_and_follows_the_receipt`, which runs the real script on unix.
- No `--channel`: the receipt wins, missing means stable (`main`). `--channel` refuses a dirty tree (except a lone Cargo.lock), fetches `origin/$BRANCH` by refspec, `checkout -B --track` when the local branch is missing, and otherwise fast-forwards only (refuses when local is ahead).
- `GROKHUB_CHANNEL` is exported into `cargo build` and `tauri build` so the shell's build.rs labels the binary for the saved channel.
- The desktop entry's `Exec=grokhub-shell %u` is rewritten to the absolute prefix path (keeps `%u` for `loadout://` links), so launch works without `~/.local/bin` on PATH.
- `.cursor/install.sh` is the Cloud Agent bootstrap, not an installer: apt build deps plus `cargo build --workspace --locked`.
## See also
- [versions-and-channels](versions-and-channels.md), [grokhub-hub](grokhub-hub.md), grokhub-shell
