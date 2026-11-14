---
name: principle-make-operations-idempotent
description: Steps that run amid crashes and retries. Same end state however prior runs ended.
---

# Make Operations Idempotent

Derive each step from existing state, not the last run's intent: read state, compare to goal, apply the difference. A repeat finds nothing to do.

Leftovers (locks, temp files, half-written records, claimed jobs) must look stale wherever a run stopped. Write temp then rename; compare by content.

**Test:** twice? Crash at every point? Same end state? If "it depends", add reconciliation.
