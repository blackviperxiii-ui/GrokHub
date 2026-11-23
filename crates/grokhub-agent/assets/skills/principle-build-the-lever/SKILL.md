---
name: principle-build-the-lever
description: Non-trivial work. Build the script, codemod, or check that does or proves it.
---

# Build the Lever

A tool works the same way every time, reruns free, and turns "trust me" into "run this". Skip only for a couple of obvious edits.

- Do one unit by hand, then build the tool; rerun on that unit and diff. Make it rerun-safe.
- Codemod for edits, generator for files, query for analysis, rerunnable check for verification.
- A script beats fan-out; don't have delegates hand-apply it.
- Fanning out? Recipe, checks, and fences in one skill delegates read but can't edit.
- No tool in the diff? Not applied.
- Smallest script, never a framework. Commit it if it outlives the session.
