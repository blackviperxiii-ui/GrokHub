---
name: principle-sequence-verifiable-units
description: Multi-step work and commit stacks. Small units, each checked before the next.
---

# Sequence Work into Verifiable Units

A break caught at its unit is cheap; after a batch, it's buried.

**Execution:** known-good → one change → check → proceed. Rebase onto clean trunk first. Check even when a script made the edit.

**Delivery:** stack commits so they prove the work: failing test then fix; subtraction before reshape; baseline before treatment; scaffold before feature. Each lands alone.
