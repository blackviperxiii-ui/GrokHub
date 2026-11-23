---
name: correct
description: Make repeated agent mistakes impossible; architecture, types, lint, test, docs last. /correct.
disable-model-invocation: true
---

# Correct

Assume agents see only open files and copy the nearest example. Find mistake classes (two hits) in commits, reverts, reviews, workaround comments.

Fix each at the highest level that works:
1. Architecture: one owner, one way, delete old paths.
2. Types; else a lint whose error names the fix.
3. Behavior tests.
4. Docs last, judgment only.

One commit per class; each check fails on a real past mistake. Keep a rule → enforcer table in the agent rules file.

**Reply:** class, evidence, level, why higher failed.
