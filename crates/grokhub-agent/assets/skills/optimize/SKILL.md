---
name: optimize
description: App performance optimization. Measure the real artifact, find hot spots, fix root causes, remeasure. Not a number-vetting checklist or a product-feel dive. /optimize
---

# Optimize

Make the app faster and leaner with evidence. Not **benchmark-checklist** (vet a number) or **land-it** (product feel).

**Read `GUIDE.md` in this folder** for tools, ranking, and stop criteria.

Cite **viper-mode**, **principle-prove-it-works**, **principle-fix-root-causes**, **principle-explain-the-number**, **principle-subtract-before-you-add**.

## Playbook

1. **Restate.** Say the goal and the lag or resource pain the user actually feels (**in-your-own-words**). Write the claim you will retest, with its unit and workload, before you change code.
2. **Measure first, on the real artifact.** Use the release build and a realistic workload, with a profiler, Lighthouse, `time`, a flamegraph, a bundle analyzer or a query plan. Run it more than once and note the spread. No guessing, and no optimizing code the profile did not point at.
3. **Rank.** Score each hot spot by user impact times fix cost, as P0 to P2. One P0 root cause beats a spray of P2 tweaks.
4. **Fix root causes.** Wrong algorithm or data structure, N+1 queries, synchronous work on the critical path, over-rendering, unbounded work. A spinner, a guard or a cosmetic microtweak is not a fix. Change one thing at a time so each result has one cause.
5. **Remeasure the same artifact.** Same tool, same workload, same build mode. Report before and after with units and run count. If the gap is inside the noise, say so and do not claim a win.
6. **Check nothing else got worse.** Run the tests, and remeasure the neighbors a fix could have moved (memory after a cache, startup after a preload).
7. **Stop when the criteria are met** (Laziness Protocol). Hand the UI ship gate to **qa-walkthrough**. Vet a contested number with **benchmark-checklist**.
