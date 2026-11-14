---
name: principle-separate-before-serializing-shared-state
description: Concurrent writers to one target. Remove the sharing; lock only for a real invariant.
---

1. Find shared mutable state.
2. Default: each actor owns its file, key, branch, or dir; merge on read. Two workers' fields in one `state.json` still share.
3. True single-target invariant only: serialize structurally (lockfile, phases, single writer, CAS). Conventions aren't concurrency control.
4. CHANGELOG / VERSION / plugin manifests: don't launch a second worker that edits them while another open PR owns those files.
