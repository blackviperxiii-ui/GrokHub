---
name: principle-fix-root-causes
description: Debugging. Reproduce, trace to root cause, fix there; no guards that only hide crashes.
disable-model-invocation: true
---

# Fix Root Causes

Reproduce, ask why to the root, fix there. Grep the pattern. Instrument when stuck.

After a deep dig, propose a cheap always-on signal that would have shown the fact at first look.

Restart failures: check stale config, cache, or locks before rewriting code.
