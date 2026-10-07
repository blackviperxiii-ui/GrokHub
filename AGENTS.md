# AGENTS.md — GrokHub cabin notes

## Base branch
Feature/fix PRs → `beta`. Hotfixes → `main` only on Jeremy's say, then merge-back PR into beta (merge commit). No version bump on beta PRs.

## Bot
Prefer Grok Build on the box (roles: explore, plan, implementer). Cursor Cloud paused until Oct 11. One bot per job.

## Compass files
Read the compass file before editing a module: `docs/compass/README.md` lists one short map per crate and hot module (owns, change recipe, what breaks, gotchas). Fix it in the same PR when you move code; `compass_paths` fails on stale paths.

## Quality gates
Semgrep before Ready. dyl-review quick for draft asks — never dyl-ready-pr merge/babysit. Continual Learning keeps this file current.

## Branch cleanup
Delete own head after merge/close; scratch ASAP; never main/beta. GitHub auto-delete head branches is on.

## UI theme (Wave 1 + Critiquito motion 4–5)
- Composer glow: white `#e7e9ea` (`composer_glow_rgb` / FG). Breath via `icons::composer_breath`. Idle α≈0.25; streaming 0.25↔0.55; settle to idle in `GLOW_SETTLE_SECS` (200ms). Skip when `motion_ok` is false.
- Always permission: white ring on dark; settle fill α 0→1 and scale 0.98→1.0 over `ALWAYS_SETTLE_SECS` (140ms), then stop — no perpetual pulse.
