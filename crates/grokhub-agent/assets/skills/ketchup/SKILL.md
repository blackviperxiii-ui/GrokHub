---
name: ketchup
description: Catch user up since last message; list waiting decisions decidable from the reply alone. Use for /ketchup.
disable-model-invocation: true
---

# Ketchup

Catch up since the user's last message before `/ketchup`. Explain changes; list waiting calls decidable from this reply alone. Notifications/subagent results ≠ their messages. They remember that message only.

Plain short words. User impact, not method ("tests pass", not "CI green"). Drop hashes/paths/commands/counts unless needed to act. Name PRs/agents as the user would.

## Gather

Read-only; visuals from existing artifacts only. Collect work since that message, subagent reports, other chat. Refresh PR/CI/agent state. Re-check numbers; note corrections.

Action item = only user can take (choice, approval, irreversible, missing access). Include prior still-waiting. Leftover work ≠ item. Never invent calls. Unasked extras ≠ items unless irreversible+waiting: one line, no visual.

One call may cover many items. Per item: link, impact, evidence, risk, pick. "unknown" if missing. Omit absent links.

## Reply

Words on calls. Happened ≤100 words excl. pictures. Skip no-impact work. ~30 words/item; never drop items to fit. Reply alone must decide.

Open: what waits, or nothing. Idle+empty → say so and stop.

Pictures first. Then one spoken bullet per asked outcome (done/running/failed/unchecked); name fixed bugs; each line true alone. Never mark done what still waits; omit action-item overlap. ≤3 knowledge/next-step lines. One line for leftover work you can do.

Then items: call + pick (first steps, irreversible) + link. One item → one bullet (call, pick, impact, evidence, risk). Shared call → once, then per-item bullets riskiest first. Always state risk. Wait cost only beyond delay. Options: each gain+cost (+state); costs = risk.

## Visuals

Visual = screenshot, video, Mermaid, chart, image — not a table. Draw when picture > prose. Never status/action lists or disconnected boxes. No fact twice. ≤6 boxes, ≤8 words, spoken labels. Embed media for visible changes; else "No screenshot yet" + delta. Known facts only. Never invent numbers or undrawn screens. Link only real artifacts.
