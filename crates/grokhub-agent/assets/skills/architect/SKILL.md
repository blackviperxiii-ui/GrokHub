---
name: architect
description: Sketch types, signatures, and module boundaries before code; implement against the sketch. "Design this". /architect.
---

# Architect

Todo: Ground, Sketch, Agree, Implement, Scrap.

**A. Ground.** Run **how** on every touched subsystem; naming a file isn't grounding. Ownership or layering changes → **why** too. Skip only for greenfield.

**B. Sketch.** Caller usage first, then types, signatures, module map, `not implemented` bodies, pseudocode. Design it twice: two whole-shape alternatives, sequential by default; parallel per `architect` line only on high-stakes ask. Would a change that looks right from one file be right repo-wide? Prefer the deeper interface: more hidden, smaller surface. Synthesize one; record why.

**C. Agree.** Default proceed; pause only if asked. Sketch can land as a scaffold commit (**principle-foundational-thinking**). Pressure-test with **interrogate**. Shape pushback → redo B.

**D. Implement.** Sketch is the contract. A deviation is signal: sketch wrong, requirement missed, or code overreaching?

**E. Scrap** on a pattern: repeated workarounds, unrelated special cases, type escape hatches, a lock the sketch said was unneeded, callers needing internals. Rerun **how**, redesign as if constraints were day one (**principle-redesign-from-first-principles**), shrink first (**principle-subtract-before-you-add**), back to B.

**Output:** types and signatures (small) or module map plus types; rationale with usage, candidates, decision.
