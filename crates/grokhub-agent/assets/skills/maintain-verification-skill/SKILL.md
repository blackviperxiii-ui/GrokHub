---
name: maintain-verification-skill
description: Audit verify skill + feature map; one PR max. /maintain-verification-skill.
disable-model-invocation: true
---

# Maintain Verification Skill

Upkeep for `/create-verification-skill` (verify skill + feature map). Unit: feature. Tasks → **follow-through**.

## Outcomes

- **clean** — source + live done; nothing to ship
- **changed** — one PR of proven doc/harness/map fixes
- **blocked** — unfinished or unsafe; name why

## Scope

Edit only the verify skill dir. Never product code. Dead map → fix map or report product bug.

## Pass

0. **Locate** — launch/drive + map (`verify-*/`). Many → ask; none → `/create-verification-skill`.
1. **Index** — README vs files; fix missing/extra/dup/dead.
2. **Source wave** — parallel read-only subagent per feature → summary / entry points / drift-or-none / live recipe. No drive/edit.
3. **Reconcile** — all summarized; merge recipes; spot-check drift; missing surface needs source path.
4. **Live** — always; drive per Launch; every feature. Doctor before first drive, each session, after fails (wedged → reset). Evidence survives cleanup; clean residue. Drift doctor → fix once, retry. Unreachable needs prereq + route. Re-drive harness fixes. Teardown; keep evidence.
5. **Triage** — bad POV → doc; undrivable working app → harness; broken app → out of PR.
6. **Ship** — changed: one PR after re-read. Else report. Notes uncommitted.
