# Compass: grokhub-app config.rs (app.json and disk stores)

## Owns
- `AppConfig` (persisted as `app.json`), `config_dir()` resolution, and the safe-disk helpers every store uses: `atomic_write`, `create_private`, `load_json_or` / `load_json`, `quarantine`, `fit_history_json`, and the size caps (`MEMORY_FILE_CAP`, `JSON_STORE_CAP`, `HISTORY_STORE_CAP`).
- Paths for `chat.json`, `workboard.json`, `hub-state.json`, `memory/` (SOUL/USER/MEMORY), and the Imagine dirs.
## Quick commands
- `cargo test -p grokhub-app config::` (isolates itself via `test_config_root`)
- `GROKHUB_CONFIG=$(mktemp -d) cargo run -p grokhub-app` (a throwaway cabin config)
## Key files
- `crates/grokhub-app/src/config.rs`: everything above, plus its own tests at the bottom.
- `crates/grokhub-app/src/secrets.rs`: `secrets.json` (console key, OAuth). Never `app.json`.
- `crates/grokhub-app/src/app/persist.rs`: background writes that call `save_in` and friends.
- `crates/grokhub-app/src/app/settings.rs`: the Settings page that edits most fields.
## Change recipe
- New setting: add a field to `AppConfig` with `#[serde(default)]` (or `default = "fn"`), set it in `impl Default for AppConfig`, and add `skip_serializing_if` when an old file should not grow a key.
- Add a test that parses `{}` and a pre-change file, like `memory_backend_defaults_legacy_and_parses_amr`.
## What breaks it
- A field without a serde default: every existing `app.json` fails to parse and is quarantined (the user loses settings).
- Reading a store with `read_to_string` or past its cap: tests such as `cabin_config_loads_do_not_slurp_huge_files` assert on the source.
- Reordering fns: those tests slice the file between `pub fn load(` and `pub fn save(`, `pub fn load_chat(` and `pub fn workboard_path(`, and so on.
## What depends on it
- Every page in `crates/grokhub-app/src/app/`, the desktop MCP (reads `desktop_control` on each call), threads, secrets, update, and the AMR `/recall` switch.
## Non-obvious
- `AppConfig` is camelCase on disk; `memory_backend` is the one snake_case key.
- `load()` forces `host_on = true` and `yolo = false`, and `persistable_permission_mode` never lets Always survive a relaunch.
- `save_in` clears `api_key` before writing: the console key lives in `secrets.json` only.
- `config_dir()`: a test-thread pin (`TestConfigDir`), then `GROKHUB_CONFIG`, then `%APPDATA%\GrokHub` on Windows, then `~/.config/GrokHub`, then `.grokhub` in the cwd. Workers capture the dir at schedule time so a later env change cannot move the file.
- `GROKHUB_CONFIG` is process-global: tests take `hold_test_config` and a `test_config_root`; CI runs `--test-threads=1`.
- `atomic_write` creates the temp file 0600 (user-only DACL on Windows via `crates/grokhub-app/src/win_acl.rs`) before any bytes land.
## See also
- [app-module](app-module.md), [amr](amr.md), [grokhub-app](grokhub-app.md)
