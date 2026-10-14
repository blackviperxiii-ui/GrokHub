---
name: setup-vstack
description: Configure Viper Stack models and budget for token efficiency. Detects available models and writes an always-applied rule preferring Auto or Composer 2.5 Standard.
---

# Setup Vstack

Write `~/.cursor/rules/vstack-models.mdc` (or project `.cursor/rules/`) that sets every role to `auto` so subagents inherit the parent chat model.

## Steps

1. Detect available models in this session (Task subagent slugs). `auto` and `inherit-parent` are always valid.
2. Prefer budget: medium (high effort) or small (medium effort).
3. Write the rule:

```
---
description: Viper Stack model choices — Auto / Composer Standard
alwaysApply: true
---
# budget: medium (high)
feature, refactoring: auto
bug-fix: auto
perf-issue: auto
hillclimb: auto
judgment and prose: auto
hardest tasks: auto
how explorer: auto
how explainer: auto
why investigators: auto
why synthesizer: auto
arena runners: auto
swarm workers: auto
architect runners: auto
interrogate reviewers: auto
```

4. Tell the user to set the parent chat to Auto (Cost/Balance) or Composer 2.5 with Fast OFF, then start a fresh session.

This keeps full rigor while protecting token allowance.
