---
name: show-me-your-work
description: Reviewable decision trail for long or unattended work; append-only TSV, one row per decision. Local by default, commit when a reviewer needs it. /show-me-your-work.
disable-model-invocation: true
---

# Show me your work

One canonical log. Other skills route their trail here and don't restate the format.

## Format

TSV, single-line cells, header:

```
ts	phase	decision	why	evidence	result
```

- **ts** ISO8601. **phase** phase or workstream. **decision** what was chosen or done, one line. **why** plain reason. **evidence** a pointer, never prose: SHA, PR, `file:line`, artifact path. **result** `tests green`, `reverted`, `INCONCLUSIVE`, `open`.

Write rows like telling a teammate (**unslop** applies). Log forks, finished units with their check, pivots and reverts with the trigger, blockers, fixed gates. Loops: one row per iteration. Skip the trivial. Strip tabs/newlines from cells; prefix cells starting `=`, `+`, `-`, `@` with `'`.

A new run (pickup, new chat, replacement agent) adding to an existing log first writes a `start` row naming the prior `ts` range it didn't write and its own id. `start` means nothing else.

## Where

`decisions.tsv` in the work dir, or `.audit/<task>.tsv` for parallel efforts, gitignored. Commit only when a reviewer needs the trail to trust the result.

## Rules

Append-only: a wrong call gets a superseding row; never edit history. Prefer evidence from committed scripts.

## Before hand-back

1. **Audit** this run's rows against this run's transcript only (never glob other projects' chats): each row is real, each pointer resolves and shows the claim, unlogged forks are gaps → add rows. Correct with new rows, never edits.
2. **Second look.** A fresh subagent (different model family if available) reads trail + transcript and flags weak evidence, skipped verification, risky calls, skim-missable gaps.

Reply ends with **Attention**: `reviewed by <model>`, then flags pointing at rows, or "No flags". Render: `column -s$'\t' -t decisions.tsv`.
