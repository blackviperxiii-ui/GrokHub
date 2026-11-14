---
name: statechart
description: Mermaid statechart of how a system moves; bugs as numbered paths. Use for /statechart.
---

# Statechart

Show how a system moves between states in the reader's words (PM, engineer, reviewer)—not code ids. State labels = what they observe ("Paying"). Step labels = the moving event ("tap Pay", "lease expires").

Chart workflow only—no TLC/Java scripts here. Model checkers → `/create-formal-verification-skill` and `/maintain-formal-verification-skill`. Never load poteto-mode or treat upstream pstack as runtime.

## Direction

- **From model** — TLA+/formal model (or concrete code) exists: derive the chart.
- **From chart** — owners agree first: draw Mermaid, then skeleton a model.

If code has a spec, scratch-copy it for the chart; keep invariants. Tie vars/actions to reviewable code; one indivisible step per action. Optional: audit against the authors' passing config.

## Model → chart

1. Few states; split only to separate bugs or show a same-label step.
2. Each agreed rule = one invariant (reader sentence as comment). Add needed vars; never drop a rule to pass. Settled-only: hold where nothing else is enabled. Goals: assert never-reaches, take the counterexample as the path, name it in hand-off.
3. Map names → reader labels. Separate reader-driven actions when needed. Large overviews: shortest paths to settled + every bug path. Same end, different causes → name the cause. Optional color groups; broken-rule states stay distinct.
4. Proposed fix → second model; re-render.
5. Read every diagram: numbered paths in order; fix code-like labels.
6. Vs code: replay bugs via real entry points/observables only. Model wrong until proven—fix model, not rule. Design-only: checker trace is evidence; say so.

## Chart → model

1. Mermaid `stateDiagram-v2`. One region per independent part. One `%% never` line per rule. Restarts/stops = edges onward.
2. Agree PNG with owners. Broken rule on agreed chart = design bug—escalate.
3. Add real guards/vars; re-compare; ask if code or chart is wrong.

## Hand-off

Mermaid (+ PNG). Per bug: rule, numbered steps, reproduced?, fix holds?. No path → checker counterexample. Quote run-summary counts. List gaps. Point maintainers at the formal-verification skills.
