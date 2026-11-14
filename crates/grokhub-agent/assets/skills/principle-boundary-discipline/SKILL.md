---
name: principle-boundary-discipline
description: Validation, errors, adapters. Guard at boundaries; trust types inside; pure logic.
---

# Boundary Discipline

- **Boundaries** (CLI, config, network, external APIs): validate, parse into domain types, return errors.
- **Inside:** typed data, propagate errors, no re-validation.
- **Across:** expose domain concepts, never wire or storage types.
- Logic in pure functions; thin framework shell.

**Test:** data crossing a boundary now? If not, the check is redundant.
