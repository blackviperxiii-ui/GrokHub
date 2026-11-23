---
name: principle-test-behavior-not-implementation
description: Writing or keeping a test. Assert what users observe, not internals.
---

# Test Behavior, Not Implementation

Behavior: return values, errors, readable state, external effects (request sent, row written). Implementation: private helpers, call order, intermediate data.

Two imagined edits:

- **Break the behavior.** Still passes? Proves nothing. Usual causes: loose assertion, expected value computed by the code, restated constants, asserting data the test built.
- **Change internals**, same results. Fails? Pinned to implementation.

Keep tests that catch the first and survive the second. Use the real interface; derive expectations from the requirement. Payload to a mocked external service is behavior; an internal helper call isn't. Error type is contract; message wording isn't.
