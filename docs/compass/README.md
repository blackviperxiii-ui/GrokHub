# Compass files

Short maps of the places agents get lost in this repo. Read the one for the module you are about to edit before you open its code. Each file is 25–35 lines and answers the same questions: what it owns, quick commands, the 3–5 key files, the usual change recipe, what breaks it, what depends on it, and the non-obvious rules (many of them only written in code comments or source-scanning tests).

The format follows Meta's "compass, not encyclopedia" write-up (Engineering at Meta, 2026-04-06, "How Meta Used AI to Map Tribal Knowledge in Large-Scale Data Pipelines"). Repo-wide rules stay in `CLAUDE.md` and `AGENTS.md`; these files only add per-module knowledge.

| Compass | Covers |
| --- | --- |
| [grokhub-core](grokhub-core.md) | `crates/grokhub-core/`: the shared brain, re-exports, script-text tests |
| [amr](amr.md) | `crates/grokhub-core/src/amr/`: agent memory repo M0 and `/recall` |
| [slash](slash.md) | slash parsing (core), dispatch (app), native parity (agent), FFI kind |
| [grokhub-acp](grokhub-acp.md) | `crates/grokhub-acp/`: the `grok` CLI client, ACP, headless `grok -p`, cabin `GROK_HOME` |
| [grokhub-agent](grokhub-agent.md) | `crates/grokhub-agent/`: the native Lab engine |
| [harness](harness.md) | `crates/grokhub-agent/src/harness/`: Spike-0 approval gate, paths A/B/C/E |
| [grokhub-app](grokhub-app.md) | `crates/grokhub-app/`: the `grokhub` binary and its launch modes |
| [app-module](app-module.md) | `crates/grokhub-app/src/app/`: the `Cabin` UI and its tests |
| [app-config](app-config.md) | `crates/grokhub-app/src/config.rs`: `app.json` and safe disk stores |
| [desktop-mcp](desktop-mcp.md) | `crates/grokhub-app/src/desktop_mcp/` and `crates/grokhub-core/src/desktop_mcp.rs` |
| [grokhub-hub](grokhub-hub.md) | `crates/grokhub-hub/`: LAN `/v1` hub and pairing |
| [grokhub-ffi](grokhub-ffi.md) | `crates/grokhub-ffi/`: C ABI for Android and Windows |
| [install-scripts](install-scripts.md) | `scripts/install.sh` and friends, plus the tests that pin their text |
| [versions-and-channels](versions-and-channels.md) | version bumps, beta/stable channels, the build label |

## Keeping them true

`crates/grokhub-core/tests/compass_paths.rs` runs in the normal `cargo test --workspace` CI job. It fails when a backtick-quoted repo path (anything under `crates/`, `scripts/`, `docs/`, `packaging/`, `research/`, `screenshots/`, `.cursor/`, `.github/`, or a root file such as `CLAUDE.md`) no longer exists, when a backtick-quoted identifier (snake_case, CamelCase, SCREAMING_CASE, or a::b paths) no longer appears in those dirs or root files (the compass files and the test itself do not count), when a relative link breaks, when a file leaves 25–35 lines or drops a section, or when this table misses a file.

Run it with `cargo test -p grokhub-core --test compass_paths`. When it fails, fix the compass file in the same PR that moved the code. Write concrete paths (no `<placeholders>` or globs inside a repo path) so the check can see them.
