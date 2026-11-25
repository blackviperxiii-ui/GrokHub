---
name: arena
description: "Spawn N parallel candidates at the same task, pick a base, graft the strongest parts of the losers into it. Use for /arena, 'arena this', 'throw it in the arena', or when one attempt at a large or contested design or other non-trivial artifact would lock in the wrong shape."
disable-model-invocation: true
---

# Arena

Run an arena only for a large or contested design, or when the user asks for one. A settled shape does not need one.

Open a todolist with one entry per phase (Frame, Fan out, Cross-judge, Pick, Graft, Verify) before launching anything.

## Phase A: Frame

Every candidate gets the same prompt, so the prompt is the contract.

1. State the artifact each candidate is producing.
2. Derive the rubric, 3-6 concrete gradeable criteria for what success looks like on *this* task. It is the picker's tool in Phase D. Candidates only see the task.
3. Pick the runners. By default the panel is two or three fresh subagents on the parent model, so omit `model` and spawn general-purpose subagents with the host's subagent tool (`Task` in Cursor and Claude Code). Same-model seats differ by brief, so give each seat one design direction to argue from when the task has more than one. Use runners from different vendors only when the user asks for a high-stakes multi-model pass, and then take one seat per vendor from the models the host offers. In Cursor, the line that names `arena` in `~/.cursor/rules/vstack-models.mdc` overrides the default when `/setup-vstack` wrote one. An `auto` or `inherit-parent` entry is the parent model. If the subagent tool rejects a model slug, use the nearest valid slug of the same family or drop the seat, keep at least two runners, and say so. A slug's family is its text before the first hyphen, after dropping a leading `cursor-`. Spawn more for multiple design directions, and repeat one model when the work is generation-bound, not judgment-sensitive. Pick each subagent's reasoning level per the Subagents section of the **viper-mode** skill, medium or small unless the task needs more.
4. Give each candidate its own output location (a git worktree where possible, otherwise `/tmp/arena-<slug>/candidate-<n>/`), per the **principle-separate-before-serializing-shared-state** skill.

## Phase B: Fan out

Spawn all N subagents in one message, in the background where the host supports it (`run_in_background: true` in Cursor and Claude Code). The first line of each kickoff is `Read <path>/worker.md before anything else.`, with `<path>` the absolute path of the `viper-mode` skill folder. Then give each one the task, the path to the shared grounding, its own output path, and instructions to produce the artifact and a short rationale naming the alternatives it considered and what it rejected.

If a candidate produces no output, proceed with N-1 and note the dropout.

## Phase C: Cross-judge

After all Phase B candidates complete, spawn one fresh readonly judge. By default it runs on the parent model, and what makes it independent is that it starts fresh, sees only the candidates and the rubric, and never saw your framing notes. In a multi-model pass the judge is never from the parent's vendor, so use the first available model from another vendor. In Cursor, a line that names the arena judge in `~/.cursor/rules/vstack-models.mdc` overrides this, and an `auto` or `inherit-parent` entry is the parent model. If you do not know the parent's model in a multi-model pass, use the first real slug and say that the vendor check did not run. If the subagent tool rejects a slug, use the nearest valid slug of the same family, then the next such entry, or skip the judge, and say so. While you read in Phase D, the judge scores each candidate, by path label, on each rubric criterion and recommends a base.

## Phase D: Pick a base

Read every candidate end to end before picking. Score each against the rubric criterion by criterion, not on holistic feel, and compare with the cross-judge when Phase C ran one. Agreement on the base confirms the pick. Disagreement means one of you is biased or the rubric was ambiguous. Read both rationales before deciding.

Pick the base a future maintainer can extend most easily without breaking invariants. When two feel tied, prefer the cleaner boundary or smaller API, per the Laziness Protocol (the **principle-laziness-protocol** skill).

## Phase E: Graft

Walk each losing candidate once more for what is worth porting into the base, usually one or two things per candidate, not most of it. Fold each graft in by hand, per the **principle-redesign-from-first-principles** skill, so the result stays coherent under one mental model. Don't paste mechanically.

If the candidates converge on one shape, that is strong agreement. Note it and ship the consensus shape. No graft is needed. If they wildly diverge, Phase A was under-specified. Reframe and re-run rather than averaging the divergence.

## Phase F: Verify

Verify the synthesized artifact like any other output, per the **principle-prove-it-works** skill. If verification finds a problem the arena missed, either Phase A was wrong (re-frame and re-run) or a candidate caught it and you missed the graft (go back to Phase E). Don't paper over it.

## Outputs

One synthesized artifact, with a short synthesis note alongside naming the base and why, the cross-judge's verdict or that Phase C skipped the judge, each graft with its source candidate, what was rejected and why, any convergence or dropouts, and the verification result.
