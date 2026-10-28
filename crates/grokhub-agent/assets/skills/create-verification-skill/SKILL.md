---
name: create-verification-skill
description: Project skill that drives the real app and proves behavior. /create-verification-skill.
disable-model-invocation: true
---

# Create Verification Skill

Scripted real-app proof: launch, user-path drive, evidence. Write a cold-agent skill at `.cursor/skills/verify-<app>/`.

## Interview (repo first)

Ask only what code cannot answer:

- **Surface** — primary UI/CLI/API/desktop/mobile/lib; note others
- **Run** — repo start command; ports, env, seed, auth
- **Drive** — existing harness first; else CDP/tmux/HTTP
- **Observe** — screenshots, transcripts, bodies, logs, exits, DB
- **Isolate** — side-by-side? If not, refuse double-drive

Fix a broken checkout before generating. Verification-only scaffolding OK if cleanup removes it.

## Generate

`SKILL.md` with frontmatter (`name: verify-<app>`, app + surface in description). No placeholders.

- **Launch** — exact start + usable readiness; migrations/seed; teardown. CLI: build once; each drive in its own PTY/tmux.
- **Doctor** — read-only process/build/port/auth check.
- **Drive** — real selectors/commands; stable handles.
- **Evidence** — what/where; real user path; action + result; side effects; mocks only at prod boundaries; observe dry-run skips.
- **Cleanup** — kill what you started; keep evidence at a named path.
- **Helpers** — executable; invocation in the skill body.

## Feature map

`features/README.md` + one file per top feature (start 3–5). H2s: `Sub-features`, `How to get to it (user POV)`, `Driving it with <harness>`, `Gotchas`. Cover reach, drive, observable proof. Missing seed → fail + Gotcha; never weaken. Map is source of truth.

## Prove then hand off

Once: launch → doctor → one mapped feature → evidence → cleanup; evidence must survive. Cleanup after failed tries. Stand-ins: list unverified setup. Point at `/maintain-verification-skill`.
