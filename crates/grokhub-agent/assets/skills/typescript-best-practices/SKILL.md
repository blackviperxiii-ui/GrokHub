---
name: typescript-best-practices
description: TypeScript rules for .ts/.tsx files.
paths: ["**/*.ts", "**/*.tsx"]
---

# TypeScript best practices

Apply **principle-type-system-discipline** first.

- Discriminated unions (`kind`), not optional-field bags.
- Brands minted by a verifying guard (`x is Id`), never `as`.
- Construct it legal: `[T, ...T[]]`; tighten only where loose types force `!` or casts.
- `unknown` for external data; schema library (`z.infer`) before hand guards.
- No `as` before validation; prefer `satisfies`. Guards must verify; name `isX`.
- Exhaustive: `const _exhaustive: never = x;`.
- Parse at boundaries (**principle-boundary-discipline**).
- Derive with `Pick`/`Omit`/`ReturnType` before new interfaces.
- Object args off hot paths. Don't mock what you can run. No shipped `console.log`.
