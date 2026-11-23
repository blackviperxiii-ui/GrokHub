---
name: interrogate
description: Adversarial review; only findings proven by a run survive. "Tear this apart". /interrogate.
---

# Interrogate

Report findings; fix nothing. Scope: what the user names, else branch diff through working tree. Intent via **in-your-own-words**; ask if unclear.

**Adversaries:** fresh subagents, sequential by default. Brief: *Break this change. Prove each finding with a run against real code (failing test, repro, output). Report title, command, output, and the output intent requires. Omit anything not run. None is valid.*

**Verify:** rerun each command. Keep failures that break intent; merge shared causes.

**Output:** intent; findings (command, output, impact) or "no verified findings"; dropped counts with reasons.
