---
name: principle-model-the-domain
description: Stateful logic or heavy branching. Encode the domain in a structure.
---

# Model the Domain

A structure matching the domain makes invalid states unrepresentable and deletes branches.

Use: a state machine over scattered booleans; typed models over loose params; maps or discriminated unions over branches across files; reducers over ad hoc mutation; modules owning one body of knowledge, not load/validate/save phases.

Don't force it. **Skipped if:** a feature adds another if/else branch, or a second boolean must track the first.
