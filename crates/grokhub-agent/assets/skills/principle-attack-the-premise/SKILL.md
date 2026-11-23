---
name: principle-attack-the-premise
description: Fixes sharing one premise keep failing the same gate. Test the premise first.
---

# Attack the Premise

Each failed fix is evidence against the belief it assumed.

- Write the premise: the one sentence every failed fix assumed.
- Test it: an observation that would differ if false; look at how failures are distributed, not just size. Make it rerunnable.
- Contradicted → cause is elsewhere; find what failures share. Confirmed → next why (**principle-fix-root-causes**).
- Remove the cause; don't compensate.

**Stop:** no next fix until the premise is written and tested.
