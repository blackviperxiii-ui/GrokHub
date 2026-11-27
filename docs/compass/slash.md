# Compass: slash commands

## Owns
- Composer `/commands` (and `$ <cmd>`, which parses to `Slash::Sh`) that the desktop app runs locally; they never reach the model. Parsing lives in grokhub-core and dispatch in the desktop app (src-tauri/src/slash.rs).
## Quick commands
- `cargo test -p grokhub-core slash` (parser, picker)
- `cargo test -p grokhub-shell slash`, run in src-tauri/ (desktop dispatch)
## Key files
- `crates/grokhub-core/src/slash/mod.rs`: `Slash`, `parse_slash`, `slash_kind`, `SLASH_COMMANDS` (picker rows), `unknown_cabin_slash`, `is_cabin_slash_turn`.
- `crates/grokhub-core/src/slash/registry.rs` and `crates/grokhub-core/src/slash/cmds/`: every command, declared with `register_slash!` and listed in `SLASH_REGISTRY`; `parse_slash` (`crates/grokhub-core/src/slash/parser.rs`) splits the line and looks the command word up there. `crates/grokhub-core/src/slash/effect.rs`: `slash_effect` (Local, AgentTurn, Action) and one sample line per variant.
- the retired cabin's slash.rs: `run_slash` (one match arm per variant), dispatch_native_slash, apply_unparsed_native_slash.
- the retired cabin's chat_kick.rs: the send path that tries `parse_slash`, then `unknown_cabin_slash`, then native parity, then the model.
- `src-tauri/src/slash.rs`: the desktop composer's local slash commands (see See also).
## Change recipe
- Add a `Slash` variant, `slash_kind`, and `SLASH_COMMANDS` row in `crates/grokhub-core/src/slash/mod.rs`, and a `register_slash!` entry in a `crates/grokhub-core/src/slash/cmds/` file listed in `SLASH_REGISTRY` (e.g. `/brief` → `Slash::Brief` in `cmds/views.rs`).
- Pick its `slash_effect` and add a `SLASH_SAMPLES` line. An `Action` needs a gate row in the retired cabin's slash_gate_tests.rs naming its confirm, consent or permission path.
- Handle it in the desktop app's src-tauri/src/slash.rs; map habits in `home_slash_cmd` (`chips.rs`) only when needed.
## What breaks it
- Renaming a `slash_kind` string: it is the input to `home_slash_cmd`, so home habits stop counting.
- Claiming a skill verb: unknown slashes and skill slashes such as `/create-skill` must still go to the native engine as a turn (`unknown_cabin_slash`). Only retired verbs are rejected locally: `/approve`, `/project binding`, and the routing verbs `/model`, `/m`, `/mode`, `/models`, `/effort` and `/why`, because the router alone picks the model and effort.
- Changing `is_cabin_slash_turn`: it keeps the `/help` dumps and model catalogs that older builds saved out of the next model kick by matching their first lines (`/help — this list`, model ids).
## What depends on it
- The composer picker (`filter_slash_hits` merges cabin rows with Grok extras from `grok_command_hits`), `/recall` and AMR, and `/workflow` forwarding.
## Non-obvious
- `/learn` belongs to the cabin: a Grok extra with the same verb must not become an insert-only chip.
- `/workflow pause|resume|stop` with no target is `WorkflowUsage` and is not forwarded; any other first word is a launch name.
- `/btw` keeps persist id `ask`; `/bg` with no task lists the background runs and what they found.
- Slash results are stored with `SLASH_RESULT_PREFIX` so they stay on the pane. `message_origin` reads that tag (never the text) and makes them `ChatKind::Result`, which keeps one bubble as it ages and never folds into a thought (GL-05).
- `/skills changes|undo|restore` parse in `parse_skills_arg` (`crates/grokhub-core/src/slash/cmds/changes.rs`); any other `/skills` word stays `GrokSkills`. Undo and restore run only from the user's typing (typed_send) or a row click in the retired cabin's skill_undo.rs; from a night job, automation, Pulse run, or idea they only show the rows.
- `/privacy` is a cabin view (run_privacy) and shadows the Grok CLI pager builtin. Slash text never writes a grant; only a Settings click in ui_privacy_rows or ui_scope_rows does. Its ghost Revoke rows (paint_privacy_revokes) answer a pointer click only.
- `run_slash` calls dispatch_native_slash first on every thread. It returns false off native (Lab) threads; on them Remember, Dream, Inspect, Rewind, Usage and Workflow return true and skip the cabin match.
- `/rewind step` / `/rewind step <n>` (`Slash::RewindStep`) lists or restores `file_checkpoint` steps for the thread's grok_session; bare `/rewind` stays N/A on native.
## See also
- [amr](amr.md), [grokhub-agent](grokhub-agent.md). Desktop `src-tauri/src/slash.rs` runs /imagine, /journal, /learn, /mcps, /plugins, /resume. `/learn` reflects; `/learn chat|url|folder|.pdf` drafts a skill via `learn_skill` (scanner + Confirm). /download starts a catalog model or opens Local AI.
