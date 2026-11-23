---
name: principle-minimize-reader-load
description: Hard-to-trace code. Fewer layers to trace, less state to hold.
---

# Minimize Reader Load

Reader work has two axes: **layers to trace** and **state to hold**.

- Inline one-caller wrappers, one-impl adapters, speculative indirection.
- Each layer must change the abstraction; collapse pass-throughs.
- Small interfaces hiding real decisions.
- Returns over mutation; locals over fields over globals; derive, don't sync.
- Name invariants once, at the boundary.

**Test:** "where does X come from, what changes X?" in 30s?
