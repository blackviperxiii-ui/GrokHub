---
name: architect
description: "Sketch types, signatures, and module structure before code, then stay in the loop while implementation fills in. Use for /architect, 'architect this', 'design this', or non-trivial work where jumping to code would lock in the wrong shape."
---

# Architect

Before writing code, trace every system the change touches, then sketch two different shapes for it (types, signatures, module boundaries) and pick the one where each piece of state has one owner, each task has one way to do it, and callers need to know least. Redesign instead of patching if workarounds repeat.

For a large or contested design, run the **arena** skill instead of sketching alone.

Open a todolist with one entry per phase (Ground, Sketch, Agree, Implement, Scrap) before sketching.

## A. Ground

Run the **how** skill on every subsystem the change touches. Naming a file isn't grounding. When the change moves ownership or layering, run **why** too, so the sketch does not undo a decision the record explains. Skip only for greenfield.

## B. Sketch

Write caller usage first, the code a caller would write against the new shape. Then the types, the signatures, the module map, function bodies that throw `not implemented`, pseudocode for anything subtle, and schemas (**database-architect**).

Design it twice. Sketch two whole-shape alternatives, one after the other, each a different shape and not a variation of the first. A large or contested design goes to **arena** instead.

Pick the one where each piece of state has one owner, each task has one way to do it, and callers need to know least. Ask whether the shape is right repo-wide, not only locally. Prefer the deeper interface, the one that hides more behind a smaller surface.

Synthesize one sketch from the two. Record why it won and what the other got right.

## C. Agree

Default to proceeding. Pause for the user only if they asked to see the sketch first. The sketch can land as a scaffold commit (**principle-foundational-thinking**). Pressure-test it with **interrogate**. If the pushback is about the shape, redo B.

## D. Implement

The sketch is the contract. Implement against it. A deviation is a signal, so stop and name which it is. Either the sketch was wrong, a requirement was missed, or the code is overreaching.

## E. Scrap

Scrap the design when a pattern shows up, not on a single wart.

- Workarounds repeat.
- Special cases pile up that have nothing to do with each other.
- Type escape hatches appear.
- A lock appears that the sketch ruled out.
- Callers need to know internals.

Then rerun **how**, redesign as if the new constraints had been there from day one (**principle-redesign-from-first-principles**), shrink first (**principle-subtract-before-you-add**), and go back to B. Do not patch.

## Output

For a small design, the types and signatures. For a larger one, the module map plus the types. Either way, a rationale with the caller usage, the candidates, and the decision.
