# Compass: grokhub-app (the `grokhub` binary)

## Owns
- The only GUI: the eframe/egui cabin (`crates/grokhub-app/src/app/`), tray, titlebar, theme, icons, markdown, cards, Imagine, voice, threads, secrets, update, and OAuth.
- Every launch mode, parsed in `crates/grokhub-app/src/cli.rs`: window, `--agent`/`--tray`, `--hub`, `--mcp-desktop`, `--oauth`, `--update`, `--doctor`, `--version`, `--help`.
## Quick commands
- `cargo run -p grokhub-app` (cabin) / `cargo run -p grokhub-app -- --version`
- `GROKHUB_CONFIG=$(mktemp -d) cargo test -p grokhub-app -- --test-threads=1`
- `cargo build -p grokhub-app -p grokhub-hub -p grokhub-ffi` (Linux CI; the Windows release build skips `grokhub-ffi`)
## Key files
- `crates/grokhub-app/src/main.rs`: `main`, the `Launch` dispatch, `run_cabin`, `run_hub`, Windows console handling.
- `crates/grokhub-app/build.rs`: channel, branch, and short SHA for `--version`, and the Windows icon.
- `crates/grokhub-app/src/theme.rs` (dark tokens, `composer_glow_rgb`, `motion_ok`) and `crates/grokhub-app/src/cards.rs` (catalog chrome).
- `crates/grokhub-app/src/threads.rs` (`threads.json`) and `crates/grokhub-app/src/secrets.rs` (`secrets.json`).
## Change recipe
- UI change: find the page in `crates/grokhub-app/src/app/` (read [app-module](app-module.md)), keep logic pure in grokhub-core, and add a cabin test.
- New CLI flag: add a `Launch` variant and arm in `parse_args`, dispatch it in `main`, update both help strings (Windows and non-Windows), and add a `cli.rs` test.
## What breaks it
- Dead code: CI runs clippy with `-D warnings` workspace-wide, and `CLAUDE.md` forbids `#[allow(dead_code)]`. Delete it.
- Dropping `windows_subsystem = "windows"` or `AttachConsole` from `main.rs` (`windows_cabin_is_a_gui_subsystem`).
- Editing the `cabin_reports_version` literal outside a release bump.
## What depends on it
- Users and packaging: `scripts/install.sh`, `scripts/install-windows.ps1`, `packaging/grokhub.desktop`, `packaging/systemd/grokhub.service`, and CI's binary-exists checks.
- Grok Build runs `grokhub --mcp-desktop` as its desktop MCP server.
## Non-obvious
- It is a binary crate (no `lib.rs`): every test is an in-crate unit test, so `pub(crate)` is enough and integration tests cannot reach it.
- Default feature `fx` pulls `eframe/wgpu` for the composer glow shader (`crates/grokhub-app/src/fx/composer_glow.wgsl`).
- Many tests `include_str!` their own source (`cards.rs`, `desktop.rs`, `theme.rs`, `config.rs`, `main.rs`) and assert on literal strings or fn order. Search for the string before renaming.
- `--mcp-desktop` must keep stdout JSON-RPC only; logs go to stderr.
- No Electron: CI fails if root `package.json`, `src/`, or `desktop/` come back.
## See also
- [app-module](app-module.md), [app-config](app-config.md), [desktop-mcp](desktop-mcp.md), [versions-and-channels](versions-and-channels.md)
