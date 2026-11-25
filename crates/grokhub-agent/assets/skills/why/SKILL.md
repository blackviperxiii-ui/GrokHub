---
name: why
description: "Use for 'why does X work this way', 'why we picked Y', design rationale, regressions, postmortems, data-backed thresholds, or where a number came from. Returns a cited read on decisions and tradeoffs across every evidence source. Use how for runtime behavior. /why."
---

# Why

Answer from the record, not the code: read the commits and PR discussions that introduced the target and each later change, and other sources if the trail is thin. Cite what the authors wrote, mark your own inferences as inferences, and say what the record does not answer.

## 1. Target and question

The target is a piece of code, a pattern, a feature, or a decision. If the ask is vague, guess the target from context (open files, recent edits), state your guess, and proceed. The user can redirect.

## 2. Code anchor (inline, before spawning)

Build the anchor yourself before any investigator starts.

- Paths and line ranges, and the key symbols.
- `git blame -L <start>,<end> <path>` on the lines in question.
- `git log --follow -p <path>` for the file's history across renames.
- `git log --oneline -20` for recent context.
- PR numbers from commit subjects, such as `(#123)`, then the PR bodies, reviews, and linked tickets (`gh pr view <n> --comments`).

Every investigator gets this anchor.

## 3. Investigators (parallel, one source each)

List the MCP servers and tools the host has connected. Spawn one investigator per category that has a source, using the host's subagent tool (`Task` in Cursor and Claude Code).

- Source control (always).
- Issue tracker.
- Long-form docs.
- Team chat.
- Observability.
- Error tracking.
- Analytics warehouse.

Run investigators in agent mode, not readonly, because readonly strips MCP access on hosts that have that flag. They write nothing. The first line of every kickoff is `Read <path>/worker.md before anything else.`, with `<path>` the absolute path of the `viper-mode` skill folder. Subagents run on the parent model, so omit `model`. In Cursor, the line that names `why` in `~/.cursor/rules/vstack-models.mdc` overrides that when `/setup-vstack` wrote one.

When the target is defensive code (a guard, a retry, a timeout, a special case), also hunt for the incidents and postmortems that explain it.

Skip a category only in writing. Either there is no MCP for it, which is a gap, or it is irrelevant to the question, and you say why. Answer inline, without investigators, only when the PR already holds the full answer, and say that it does. A source that returned nothing is a finding. Report nulls.

## 4. Synthesize

One synthesizer gets the findings, the nulls, the skips, the anchor, and the question. It sorts every statement into one of four kinds.

- **Found.** A cited fact (commit, PR, ticket, message). Quote what the authors wrote.
- **Inferred.** Reasoned from facts. Say from which ones.
- **Competing hypotheses.** When the evidence splits, state each reading and what would tell them apart.
- **Unknown.** What no source answered.

## Output

Use these sections, in this order.

- **The Question.**
- **The Code.** The anchor, with `file:line`.
- **What We Found.**
- **What We Can Infer.**
- **Competing Hypotheses.**
- **What We Don't Know.**
- **Sources Consulted.** One line per category. Mark each empty or skipped one with the reason.
- **Confidence Summary.**

Keep the confidence language the evidence supports. Do not round "probably" up to "is". When the read comes before a change, end with Preserve / Change / Avoid / Risk.

Avoid recency bias. The latest commit is rarely the whole story. Trace back to the change that introduced the target.
