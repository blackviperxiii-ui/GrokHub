# Compass: mission (foreground mission read model)

## Owns
- `crates/grokhub-core/src/mission.rs`: `MissionView`, `MissionSeed`, status/check enums. Pure projection for one foreground mission.
- `crates/grokhub-app/src/app/mission_ui.rs`: cabin seed gather + Work-tree paint (`paint_mission_header`).
## Quick commands
- `cargo test -p grokhub-core mission --locked -- --test-threads=1`
- `GROKHUB_CONFIG=$(mktemp -d) cargo test -p grokhub-app mission_ui -- --test-threads=1`
## Key files
- `crates/grokhub-core/src/mission.rs`: `MissionView::project`, `mission_worthy_intent`, checklist status map.
- `crates/grokhub-app/src/app/mission_ui.rs`: `mission_seed_here`, `paint_mission_header`.
- Entry signals: Brief (`brain/brief.rs`), `todos_for`, `EpisodeUi`, hard/soft parks, `TaskOutcome`.
## Change recipe
- New status rule: add it in `decide_status` with a literal unit test; keep the UI a thin paint of the DTO.
- New signal: extend `MissionSeed` and fill it in `mission_seed_here` only.
## What breaks it
- Teaching the mission module to run tools or decide approvals (that stays in harness / episode).
- Growing the Composer Pill arm for mission chrome (`chat_composer_pins_stop_on_the_right`).
## What depends on it
- Work tree in `chat_ui.rs` (`paint_tool_cards` calls `paint_mission_header` before the episode header).
## Non-obvious
- No persisted mission file: reopen re-projects from Brief, todos, episode, parks, outcomes.
- Entry points: task/repair/research Brief, desktop episode, Continue/resume + hard-park approval.
- Casual chat with no work returns `None` (no empty chrome).
## See also
- [harness](harness.md), [self-improve](self-improve.md), [app-module](app-module.md), [grokhub-core](grokhub-core.md)
