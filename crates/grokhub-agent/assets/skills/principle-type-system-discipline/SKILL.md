---
name: principle-type-system-discipline
description: Designing types or signatures. Make illegal states unrepresentable.
---

# Type System Discipline

- Brand primitives that differ in meaning (`UserId` vs `OrderId`); validate once.
- External data is untyped until parsed at the boundary.
- Derive types from the owning schema (proto, OpenAPI, migrations).
- Exhaust variants; define errors out of existence. TS: **typescript-best-practices**.
