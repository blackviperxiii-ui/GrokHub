---
name: recall
description: Rebuild recent working context from chat history, live repo state, and the shared record; tight current-state brief. "Where did I leave off". /recall.
disable-model-invocation: true
---

# Recall

Read only what in-scope threads need, then stop.

1. **Route.** One specific chat to resume → session pickup. Full state capsule given → use it.
2. **Lock scope**, say it: window (default 7 days), topic, workspace (active only unless asked). Never shrink "all" quietly.
3. **Mine chats** under the workspace's `agent-transcripts/`: order by mtime (`ls -t`), grep topic, read matching regions, skip this chat and subagent/eval noise. Many chats → fast subagents per slice; raw transcripts stay with them. Per chat: goal, decisions, open threads, corrections, artifacts, chat id.
4. **Sweep the shared record** whenever a feature, file, area, or bug is named: **why**'s investigators asking "current state, what didn't hold, what users still report". Skip only for pure activity recall.
5. **Verify live** every PR, branch, ticket with `git`/`gh`.

## Brief

- **Capsule** ≤5 bullets.
- **Threads**, one line each, one tag: `[merged #N]` `[open PR #N]` `[in flight <branch>]` `[verified, uncommitted]` `[reverted #N]` `[planned, not started]`.
- **Problems** ≤5, incl. reverted fixes and repeat reports.
- **Next move**: one concrete action.

Stay on topic. Through **unslop**; cite chat ids and sources; sanitize before anything public.
