---
name: follow-through
description: Ledger, check-ins, stall detection, status updates; proven done and handoff. /follow-through
---

# Follow Through

Accountability for assigned work. It makes sure every task an agent took on is advanced, proven done, or handed back with an owner. It is not a verification skill or a ship gate. Those are **create-verification-skill**, **maintain-verification-skill**, **qa-walkthrough** and **av-qa-walkthrough**. Use it with **viper-mode** on multi-step work.

The failure this prevents is quiet: a task that was accepted and then dropped, a PR opened and never merged, a "done" that nobody checked. The ledger makes each of those visible.

## 1. Ledger

Keep one entry per task with these fields:

- id or title
- owner
- status: todo, doing, blocked or done
- started
- last progress (what changed, and when)
- next check

Use the host's todo list or board when it has one. Otherwise write `follow-through-ledger.md` at the repo root or under `.cursor/`, and name the path once so the user can find it.

One **doing** task per agent. If the project prefers one card or thread at a time, honor it. Do not start a second worker on files that are shared (CHANGELOG, README, tokens) while another PR that touches them is open. Queue the work or wait.

Write the entry when you accept the task, not when you start it. A task you accepted and did not record is the one that gets lost.

## 2. Check-ins

Re-read the ledger at the start of a turn, at each milestone and at the end of a turn. At each check-in:

- Every **doing** task must have advanced since the last check. If it has not, treat it as a stall.
- Every assigned task must still be on the ledger. A task cannot disappear. It ends as done, blocked or handed off.
- Update "last progress" with what actually changed, not what you intend.

## 3. Stall detection

A task is stalled when any of these holds:

- No progress for 30 minutes of active work, or across 2 check-ins.
- The same action repeated with the same result.
- Waiting on something outside your control.
- An error loop.

Name the stall in the ledger. Make one real attempt to unblock it by a different route. If that fails, escalate to the owner with the block, what you tried, and the exact thing you need from them. Then move to work that does not depend on it.

Never fake progress. "Still working on it" with nothing new is a stall report, so write it as one.

## 4. Proactive status

Report without being asked at each milestone, at each stall or blocker, and at completion. Use four parts: **done**, **doing**, **blocked**, **next**. Attach the evidence for each claim: the PR, the CI result, the files.

Keep it short. A status the user has to decode is not a status.

## 5. Done means proven

Mark a task **done** only with evidence that the outcome exists: the merged commit, the passing run, the artifact a user can open. "I pushed it" is not done. "It merged and the checks on the merge commit passed" is.

When someone else merged your PR, read the merge commit body and the head's checks before you call it done. A stray trailer, personal info in the message, or a check that never finished is a finding. Where the repo runs no CI, run its checks locally and say that is what the evidence is.

Announce a merge or a green CI result in a new message. Never as a silent edit to an old one.

Use **create-verification-skill**, **qa-walkthrough** or **av-qa-walkthrough** for the proof itself when they fit. This skill tracks who owns what. It does not replace their checks.

## 6. Handoff and closeout

Before you yield or the session ends, every open task is in one of three states:

- **done**, with its evidence.
- **handed off**, with a named owner and enough context for them to continue without asking.
- **blocked**, with the next step and who can take it.

Nothing is left unowned.

Run `python3 <this skill>/scripts/closeout.py` on every PR the run opened. It refuses **done** unless each PR is merged or is named with `--pending` and its blocker.
