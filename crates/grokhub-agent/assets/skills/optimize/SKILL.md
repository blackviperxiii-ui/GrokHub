---
name: optimize
description: App performance optimization. Measure the real artifact, find hot spots, fix root causes, remeasure. Not a number-vetting checklist or a product-feel dive. /optimize
---

# Optimize

Make the app faster and leaner with evidence. Not **benchmark-checklist** (vet a number) or **land-it** (product feel).

**Read `GUIDE.md` in this folder** for tools, ranking, and stop criteria.

Cite **viper-mode**, **principle-prove-it-works**, **principle-fix-root-causes**, **principle-explain-the-number**, **principle-subtract-before-you-add**.

## Playbook

1. Restate goal + user-visible lag or resource pain (**in-your-own-words**).
2. Measure first on the real artifact (profile, Lighthouse, `time`, flamegraph, bundle analyzer, query plan). No guessing.
3. Rank hot spots by user impact × fix cost (P0–P2).
4. Fix root causes (wrong algo, N+1, sync-on-critical-path, over-render, unbounded work) — not cosmetic microtweaks.
5. Remeasure the same artifact; report before/after with units.
6. Stop when criteria met (Laziness Protocol). Hand UI ship gate to **qa-walkthrough**. Vet a contested number with **benchmark-checklist**.
