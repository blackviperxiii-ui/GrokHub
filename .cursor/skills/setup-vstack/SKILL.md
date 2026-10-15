---
name: setup-vstack
description: Configure Viper Stack for token efficiency. Writes an always-applied rule preferring Auto or Composer 2.5 Standard.
---

# Setup Vstack

Write `~/.cursor/rules/vstack-models.mdc` (or project `.cursor/rules/`) so every role uses `auto` (inherit parent).

## Steps

1. Detect available models (Task subagent slugs). `auto` and `inherit-parent` always valid.
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

4. Tell the user: parent chat Auto (Cost/Balance) or Composer 2.5 Fast OFF, then fresh session.

Keeps rigor while protecting token allowance.
