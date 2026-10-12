# Compass: decision journal

## Owns
- Local-first decision backlog / experiment journal: open questions with options, evidence, next experiment, and statuses Open / Investigating / Ready to decide / Decided / Deferred / Abandoned.
- Quiet dismissible due nudges and promote into a Workboard Todo (Mission promotion waits on Mission-based OS).
## Quick commands
- `cargo test -p grokhub-core journal::`
- `GROKHUB_CONFIG=$(mktemp -d) cargo test -p grokhub-app journal_ -- --test-threads=1`
## Key files
- `crates/grokhub-core/src/journal.rs`: model plus pure create / edit / search / defer / decide / abandon / evidence / promote / nudge helpers.
- `crates/grokhub-app/src/app/journal_ui.rs`: the Journal page opened by `/journal`.
- `crates/grokhub-app/src/config.rs`: `decisions_path`, `load_journal`, `save_journal` for `{config}/decisions.json`.
## Change recipe
- New `JournalEntry` field: `#[serde(default)]` and skip_serializing_if; add a JSON round-trip unit test for a legacy `{}` row.
- Keep the UI label **Journal**, never "Decisions" alone — the permission inbox already uses `decisions_waiting`.
## What breaks it
- Using AMR `NodeType::Decision`, `updates.json` Ideas, or `workboard.json` as the primary list store.
- Inventing a Mission type here instead of coordinating with Mission-based OS.
## What depends on it
- `/journal` slash, `Nav::Journal`, Workboards via `promote_to_board` / `file_idea_todo`.
## Non-obvious
- Removals append audit lines on `history` so options and evidence can leave without erasing the record.
- Due nudges only paint on the Journal page; dismiss sets `nudge_dismissed` and never auto-decides.
## See also
- [app-config](app-config.md), [app-module](app-module.md), [amr](amr.md)
