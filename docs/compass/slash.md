# Compass: slash commands

## Owns
- Composer `/commands` (and `$ <cmd>`, which parses to `Slash::Sh`) that the cabin runs locally; they never reach the model. Parsing lives in grokhub-core, dispatch in the cabin, and native-thread parity with Grok CLI builtins in grokhub-agent.
## Quick commands
- `cargo test -p grokhub-core slash` (parser, picker, help)
- `cargo test -p grokhub-agent slash_parity`
- `cargo test -p grokhub-ffi` (C ABI `grokhub_slash_kind`)
## Key files
- `crates/grokhub-core/src/slash.rs`: `Slash`, `parse_slash`, `slash_kind`, `SLASH_COMMANDS` (picker rows), `slash_help`, `unknown_cabin_slash`, `is_cabin_slash_turn`.
- `crates/grokhub-app/src/app/slash.rs`: `run_slash` (one match arm per variant), `dispatch_native_slash`, `apply_unparsed_native_slash`.
- `crates/grokhub-app/src/app/chat_kick.rs`: the send path that tries `parse_slash`, then `unknown_cabin_slash`, then native parity, then the model.
- `crates/grokhub-agent/src/slash_parity.rs`: every Grok CLI builtin and what a native thread does with it.
## Change recipe
- Add a `Slash` variant, a `parse_slash` arm, a `slash_kind` string, a `SLASH_COMMANDS` row (`cmd`, `hint`, `insert`, `run_on_pick`), and a `slash_help` line in `crates/grokhub-core/src/slash.rs`.
- Handle it in `run_slash` in `crates/grokhub-app/src/app/slash.rs`, with a core test in `cabin_slash` or a new one.
- If it should count as a home habit, map its kind in `home_slash_cmd` (`crates/grokhub-core/src/chips.rs`).
## What breaks it
- Renaming a `slash_kind` string: it is the C ABI answer Android gets (`grokhub_slash_kind`) and the input to `home_slash_cmd`.
- Claiming a verb Grok Build owns: unknown slashes and CLI skills such as `/create-skill` must still reach `grok -p`. Only retired verbs (`/approve`, `/project binding`) are rejected locally.
- Changing `/help` or `/models` text: `is_cabin_slash_turn` keeps those dumps out of the next model kick by matching their first lines (`/help — this list`, model ids).
## What depends on it
- The composer picker (`filter_slash_hits` merges cabin rows with Grok extras from `grok_command_hits`), `/recall` and AMR, `/workflow` forwarding, and grokhub-ffi.
## Non-obvious
- `/learn` belongs to the cabin: a Grok extra with the same verb must not become an insert-only chip.
- `/workflow pause|resume|stop` with no target is `WorkflowUsage` and is not forwarded; any other first word is a launch name.
- `/btw` keeps persist id `ask`; `/bg` with no task moves the live reply to the background, except under Ask, where it shows `BG_ASK_OFF`.
- Slash results are stored with `SLASH_RESULT_PREFIX` so they stay on the pane.
- `/privacy` is a cabin view (`run_privacy`) and shadows the Grok CLI pager builtin. Slash text never writes a grant; only the Settings click in `ui_privacy_rows` does.
- `run_slash` calls `dispatch_native_slash` first on every thread. It returns false off native (Lab) threads; on them Remember, Dream, Inspect, Fork, Rewind, Usage, Models and Workflow return true and skip the cabin match.
## See also
- [amr](amr.md), [grokhub-ffi](grokhub-ffi.md), [app-module](app-module.md), [grokhub-agent](grokhub-agent.md)
