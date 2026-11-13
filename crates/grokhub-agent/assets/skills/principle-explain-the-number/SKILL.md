---
name: principle-explain-the-number
description: Before measuring or trusting a measured number. Find its limiter.
---

# Explain the Number

- Before running, name the result that would change a decision and the noise floor.
- **Why not double?** Name the bounding resource (core, lock, disk, network, load tool) from a profile, mapped to source. A guess isn't one.
- Rule out: errors, cached or skipped work, code that never ran, an untuned side, noise, a piece too small to matter.
- Keep run count, spread, limiter with the number.

Perf: **benchmark-checklist**. Evals: same questions per trial.
