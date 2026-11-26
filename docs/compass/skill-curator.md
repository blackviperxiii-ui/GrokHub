# Compass: skill curator

## Owns
- Tracking views, uses and patches for self-made cabin skills; Active → Stale → Archived; pin/lock; weekly Pulse. Never deletes skill files.
## Quick commands
- `cargo test -p grokhub-core skill_curator::`
- `cargo test -p grokhub-agent skill_curator::`
## Key files
- `crates/grokhub-core/src/skill_curator.rs`: settings, lifecycle, store, weekly report text.
- `crates/grokhub-agent/src/skill_curator.rs`: archive/restore with ChangeLedger, weekly pass.
- `crates/grokhub-core/src/skill.rs`: `SkillMeta.archived` / `last_used_at` on SKILL.md.
- `crates/grokhub-agent/src/route/skill_metrics.rs`: runtime uses the curator folds in.
- `crates/grokhub-agent/src/skill_write.rs`: saves keep SkillMeta so archive flags survive edits.
## Change recipe
- New counter: extend `CuratorSkillEntry` or `CuratorSkillInput`, assert literal counts in tests.
- New setting: `skillCurator` in app.json via `SkillCuratorSettings` (re-read each tick).
## What breaks it
- Deleting skill folders on retire; archive must move only under `.archived`.
- Archiving when the clear off switch is false.
- Ledger lines that omit the real writer (`self_manage` vs `user`).
## What depends on it
- Skills page UI, Pulse digests Sunday slot, ChangeLedger skill history.
## Non-obvious
- Library-pack origin skills are not self-made and are never auto-archived.
- Config applies without restart: digests re-read `skillCurator` each tick.
## See also
- [self-improve](self-improve.md), [self-manage](self-manage.md), [harness](harness.md)
