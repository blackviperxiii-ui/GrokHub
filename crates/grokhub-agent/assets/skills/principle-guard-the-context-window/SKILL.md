---
name: principle-guard-the-context-window
description: "Apply when context is filling up: large outputs, long files, repeated reads, fan-out planning. Route bulk to subagents; keep summaries in the main thread, not raw payloads."
---

# Guard the Context Window

**Why:** Context overflow degrades reasoning quality, creates compression artifacts, and halts progress.

**Pattern:**
- **Route bulk to subagents.** Large outputs, long files, and repeated reads go to a subagent started with the host's subagent tool (`Task` in Cursor and Claude Code). Keep summaries in the main thread, not raw payloads. Subagents inherit the parent model (omit `model`, or `auto`).
- **Size phases and cap scope.** Limit files per phase, set turn budgets, account for mechanism costs.
