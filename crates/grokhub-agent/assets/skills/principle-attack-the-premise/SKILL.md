---
name: principle-attack-the-premise
description: "Apply when two or more fixes that share one premise have failed the same gate. Write the premise down and test it directly before you write another fix that assumes it."
---

# Attack the Premise

When two or more fixes that share one premise have failed the same gate, suspect the premise, not the fixes. Each failed fix is evidence against the belief it assumed.

- Write the premise down: the one sentence every failed fix assumed.
- Test it before the next fix. Find an observation that would come out differently if the premise were false, and make it. Look at how the failures are distributed, not only at how large they are. Make the test rerunnable.
- If the evidence contradicts the premise, the cause is elsewhere. Look for what the failures share that it did not predict. If one part always lands on the losing side, change what puts it there instead of adding a mechanism that cleans up after it. Remove the cause. Do not compensate for it.
- If the evidence confirms the premise, ask the next why (see **principle-fix-root-causes**).

Do not start the next fix before the premise is written down and tested.
