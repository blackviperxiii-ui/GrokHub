---
name: follow-through
description: Ledger, check-ins, stall detection, status updates; proven done and handoff. /follow-through
---

# Follow Through

Agent accountability for assigned work—not verify skills or ship gates (**create-verification-skill**, **maintain-verification-skill**, **qa-walkthrough**, **av-qa-walkthrough**). Use with **viper-mode** on multi-step work.

## 1. Ledger

One entry per task: id/title, owner, status (todo, doing, blocked, done), started, last progress, next check. Host todo/board if present; else `follow-through-ledger.md` at repo root or `.cursor/` (name path once). One **doing** task per agent.

## 2. Check-ins

Turn start, each milestone, turn end: re-read ledger. **doing** must advance since last check; assigned tasks cannot disappear.

## 3. Stall detection

Stall: no progress past 30m active or 2 check-ins, repeated action, external wait, or error loop. Name it; one unblock try; escalate to owner with block, tries, exact ask. Never fake progress.

## 4. Proactive status

Unprompted at milestones, stalls/blockers, completion: **done / doing / blocked / next** plus evidence (PR, CI, files).

## 5. Done means proven

**done** only with evidence. Use **create-verification-skill**, **qa-walkthrough**, or **av-qa-walkthrough** for proof when they fit; this skill tracks ownership, not their checks.

## 6. Handoff and closeout

Before yield or session end: each open task is **done**, handed off with owner, or **blocked** with next step. Nothing unowned.
