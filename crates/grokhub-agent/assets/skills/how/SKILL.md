---
name: how
description: "Use for 'how does X work', code walkthroughs before changing something, and placement / ownership / layering questions ('where should this live', 'which package owns this', 'is this the right layer'). Explains subsystem architecture, runtime flow, onboarding mental models. Use why for motivation. /how."
---

# How

Explore the codebase to answer "how does X work?" questions.

If the scope is ambiguous, state your interpretation and explore. The user can redirect.

## 1. Size it

Decide whether the question is simple or complex before spawning anything.

- **Simple.** One module or one function. One explainer explores and explains in one pass.
- **Complex.** Several subsystems, or a flow that crosses process or package boundaries. Pick 2 to 4 distinct angles, such as the data model, the runtime flow, the entry points, and the error paths. Spawn one explorer per angle together in one message. Then one explainer synthesizes their findings into a single explanation.

When in doubt, treat it as simple.

## 2. Explain

Spawn one read-only general-purpose subagent with the host's subagent tool (`Task` in Cursor and Claude Code, with `readonly: true` where the host has that flag) that explores and explains in one pass. For a complex question the explorers run first with the same read-only setup, and the explainer gets their findings as file pointers.

The first line of every kickoff is `Read <path>/worker.md before anything else.`, with `<path>` the absolute path of the `viper-mode` skill folder. Subagents run on the parent model, so omit `model`. In Cursor, the line that names `how` in `~/.cursor/rules/vstack-models.mdc` overrides that when `/setup-vstack` wrote one, and `auto` or `inherit-parent` on that line still means the parent model.

Give the subagent the user's question and this brief.

> You are writing an architectural explanation for a senior engineer. Explore the codebase and write an explanation that a senior engineer unfamiliar with this area could read and walk away with a solid mental model. They should understand the architecture well enough to start working in it confidently. You have read-only access to the codebase.

The explanation is a working mental model, the way a senior engineer onboards a peer. It is not annotated source.

Ask for this output shape, and drop any section that doesn't apply.

- **Overview.** What the thing is and what it is for, in a few sentences.
- **Key Concepts.** The terms and types a reader needs before the flow makes sense.
- **How It Works.** The runtime flow, step by step.
- **Where Things Live.** Each piece with its `file:line`.
- **Gotchas.** What surprises a newcomer, and what breaks if they guess.

Real code only. Every snippet and every `file:line` comes from the tree as it stands, never from memory of how such code usually looks.

## 3. Present

Present the subagent's output. Light edits for clarity or context from the conversation are fine. Do not substantially rewrite it.
