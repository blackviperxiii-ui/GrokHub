---
name: figure-it-out
description: Design an auditable playbook when none fits; big migrations, multi-part changes, work reviewed later. /figure-it-out.
---

# Figure it out

First deliverable: the workflow. Todo: read **viper-mode** Principles, then phases.

**A. Frame.** Done as a falsifiable predicate (**principle-prove-it-works**); scope, effort, blockers; rigor biased high for one-way doors. Rigor = gates and artifacts. Multi-hour run: one check-in.

**B. Design.** Landable units, riskiest first; scaffold and checks before features, with a pre-change baseline. One-way-door design → **architect**. Fan out only across seams, one branch per worker (**principle-separate-before-serializing-shared-state**). Write the phase list for review.

**C. Loop.** Per unit: hypothesis, smallest change, measure on the real artifact, keep or revert (**principle-sequence-verifiable-units**). Too-easy pass → suspect the observation. Gamed gate → harden; wrong gate → fix separately. Verdicts: VERIFIED, NOT VERIFIED, INCONCLUSIVE (not a pass).

**D. Trail.** **show-me-your-work**, a row per landed step.

**E. Hand back.** Check against the predicate on the real product; turn recurring corrections into checks (**correct**).

**Reply:** playbook, rigor, trail path, verified, open.
