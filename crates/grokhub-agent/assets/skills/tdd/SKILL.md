---
name: tdd
description: Failing test first for a bug, only when asked or the test path is cheap and obvious. /tdd.
---

# TDD Bug Fix

1. Pin intended vs current behavior and the smallest repro.
2. Use the narrowest existing test layer. None obvious? Don't build a harness for this.
3. Smallest test that would have caught it; encode intent, not internals.
4. Run it: fails for the intended reason, else fix the test.
5. Smallest fix keeping nearby contracts.
6. Rerun: passes.

**Impractical?** Closest executable check (script, repro, browser run, log assertion). No test beats a bad one (tests mocks, pins internals, timing-dependent, heavy infra).

Never bend tests to a wrong implementation or weaken assertions silently. Flaky: make it deterministic.

**Report:** failing-before check and failure, passing-after run, or why failing-before wasn't shown.
