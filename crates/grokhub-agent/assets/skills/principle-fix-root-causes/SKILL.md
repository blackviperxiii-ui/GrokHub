---
name: principle-fix-root-causes
description: Debugging. Fix first wrong decision; guards only for legitimate state.
---

# Fix Root Causes

Reproduce, ask why to the root, fix there. Grep the pattern. Instrument when stuck. Guard only if the state is legitimate; else find what produced it. After a deep dig, propose a cheap always-on signal. Restart failures: stale config, cache, or locks first.
