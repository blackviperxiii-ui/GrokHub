---
name: maintain-formal-verification-skill
description: Resync formal verification after code drifts. /maintain-formal-verification-skill.
disable-model-invocation: true
---

# Maintain Formal Verification Skill

A green check that describes dead code is worse than a red one. Upkeep for skills from `/create-formal-verification-skill` (or any project model/proof/verifier skill) after merges or edits that skipped recheck.

## Outcomes (per check)

- **in sync** — covered code unchanged since sync; check passes
- **updated** — verification now matches current code; check passes
- **bug** — verification matches code; property fails (finding, not drift)
- **blocked** — cannot sync or run; say why

A check can be both **updated** and **bug**.

## Scope

Edit only verification artifacts and the verify skill. Never edit product code unless asked. Never weaken a property or model to force green.

## Pass

1. Find verify skills by behavior (run a checker/prover against named code). Collect property, coverage, bounds, command, sync commit (else last verification-file commit). None → point to `/create-formal-verification-skill`.
2. Run each check once before edits; record the result.
3. Diff covered code (and renames/moves/new writers of guarded state) since sync. Classify: no effect / needs model update / needs property update / possible bug.
4. Update verification to describe current code. Keep properties that still apply.
5. Rerun. Failures after an honest update are **bug**. Failures you cannot separate from drift are **blocked**.
6. Advance sync commit only after the verification describes the new tip.

## Hand off

Per check: outcome, what changed in verification, findings in code terms, how to rerun.
