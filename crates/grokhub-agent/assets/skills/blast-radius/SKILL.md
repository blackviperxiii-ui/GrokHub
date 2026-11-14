---
name: blast-radius
description: What a change could break beyond the diff; prove the one fact it's safe because of by running real code. /blast-radius.
---

# Blast radius

Callers are a grep away. The job is the breakage grep won't show, and a writeup proves nothing until its facts are run.

**Proof ladder** (say where each fact stopped): 1 asserted · 2 `file:line` or library source · 3 walked the failure, it can't reach · 4 script/test on real code that fails loud · 5 reproduced in the app.

1. Read the change, incl. what it does that the diff doesn't spell out; pull PR and commits (**why** step 2).
2. Find the one fact it's safe because of; most risks fall with it.
3. Look where grep stops: pinned library source and patches, timing (microtasks, teardown), API JSON, DB columns, wire formats, other languages on the same bytes, flags, downstream hops.
4. Rate risks by real likelihood and cost; confirmed apart from cleared; real `file:line` only.
5. Prove the fact with a script or test; paste output.

**Hand back:** what it does · safety fact + rung + proof (or **unproven**) · risks (how, where, odds, cost, check) · cleared · cheapest pre-merge test, incl. your script. Through **unslop**.
