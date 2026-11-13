---
name: reflect
description: Mine this conversation for durable learnings and route each to a concrete skill edit, after your approval. Use when the user says reflect. /reflect.
disable-model-invocation: true
---

# Reflect

Skip when the chat is trivial or an existing skill was followed correctly. One-offs aren't learnings.

1. **Transcript.** Find this conversation's file in the `agent-transcripts/` dir the system prompt names (match the opening prompt). Never glob other projects. None? Pass a tight digest.
2. **Review.** Three lenses, fresh subagents in agent mode (MCP needed to check cited tickets and threads), sequential by default:
   - **Judgment** — where the agent misread intent, skipped a principle, or was corrected.
   - **Tooling** — repeated commands, missing scripts, failed tool use, slow loops.
   - **Divergent** — what a different approach would have done better.
   Each finding: evidence (quote/turn), proposed target skill, proposed edit.
3. **Synthesize** into Accepted / Rejected / Backlog; spot-check citations.
4. **Structure check.** Anything a lint, script, flag, or runtime check would enforce better → Backlog (**correct**).
5. **Apply only after explicit approval.** Show the full list; the user picks. Skill edits affect every future agent. Edit a local copy (`.cursor/skills/` or `~/.cursor/skills/`), never `plugins/cache/`; never push or open a PR from here. Trivial edits directly; substantive ones or new skills via the skill-creation flow; keep vstack token budgets.

**Reply:** edits applied (path + one line), new skills, backlog filed, dropped with reason.
