# Compass: decision journal

## Owns
- Local-first decision backlog / experiment journal: open questions with options, evidence, next experiment, and statuses Open / Investigating / Ready to decide / Decided / Deferred / Abandoned.
- Quiet dismissible due nudges, promote into a Workboard Todo, and promote into a mission: a chat linked by `mission_thread`.
## Quick commands
- `cargo test -p grokhub-core journal::`
- `GROKHUB_CONFIG=$(mktemp -d) cargo test -p grokhub-app journal_ -- --test-threads=1`
## Key files
- `crates/grokhub-core/src/journal.rs`: model plus pure create / edit / search / defer / decide / abandon / evidence / promote / nudge helpers. `promote_to_mission` links a chat and returns a `MissionStart` (title, drafted first message); `mission_entry` finds the decision for a chat; `open_questions` feeds the systems brief.
- `crates/grokhub-app/src/app/journal_ui.rs`: the Journal page opened by `/journal`. "To mission" calls `promote_journal_to_mission`.
- `crates/grokhub-app/src/config.rs`: `decisions_path`, `load_journal`, `save_journal` for `{config}/decisions.json`.
## Change recipe
- New `JournalEntry` field: `#[serde(default)]` and skip_serializing_if; add a JSON round-trip unit test for a legacy `{}` row.
- Keep the UI label **Journal**, never "Decisions" alone — the permission inbox already uses `decisions_waiting`.
## What breaks it
- Using AMR `NodeType::Decision`, `updates.json` Ideas, or `workboard.json` as the primary list store.
- Sending the mission's first message for the user: "To mission" only drafts it in the composer. The chat opens with a mission note (a slash result), so New chat and fresh-home never reuse it as an empty chat.
- Linking one chat to two decisions: a second "To mission" on a linked decision goes back to its chat.
## What depends on it
- `/journal` slash, `Nav::Journal`, Workboards via `promote_to_board` / `file_idea_todo`, the mission header (`MissionSeed::journal_question`), and the systems brief's decisions section (`open_questions`).
## Non-obvious
- Removals append audit lines on `history` so options and evidence can leave without erasing the record.
- Due nudges only paint on the Journal page; dismiss sets `nudge_dismissed` and never auto-decides.
## See also
- [app-config](app-config.md), [app-module](app-module.md), [amr](amr.md)
