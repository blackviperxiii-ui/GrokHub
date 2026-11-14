---
name: create-formal-verification-skill
description: Project skill that formally verifies a high-cost, low-test property. /create-formal-verification-skill.
disable-model-invocation: true
---

# Create Formal Verification Skill

Tests miss cases. Formal checks cover a stated scope. Pick the strongest promise in the repo where a break hurts and tests rarely hit it.

## Tool

Choose cheapest fit: model checker (TLA+ default for ordering/concurrency), code-level verifier (memory/invariants), generated inputs if no verifier, or a proof when bounds are not enough. Prefer tools the repo can install.

## Quality

- Model/proof maps to real code steps and names.
- Show each property failing on a broken variant.
- Report counterexamples; do not weaken to pass; do not edit product code unless asked.
- State bounds/assumptions and what is out of scope.
- Plain-language property + harm; short code-term counterexample path.
- One cheap rerun command; record covered files/functions and sync commit.

## Generate

Write a project-local skill with frontmatter. Cold agents must know when to run it, how to tell bug vs model error, and when to advance the sync commit. Use real paths/commands. No placeholders. Point maintainers at `/maintain-formal-verification-skill`.

## Prove then hand off

Break the property on scratch product code; the skill must catch it. Drop scratch; restore prior result. Tell the user: property, tool/why, bounds, findings, how to run, gaps.
