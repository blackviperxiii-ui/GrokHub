---
name: benchmark-checklist
description: Vet a perf number before you report or act on it. /benchmark-checklist.
disable-model-invocation: true
---

# Benchmark checklist

Evidence from runs, not guesses. A requested ballpark: one run, still check 4 and 7, say so. Choosing between options is never a ballpark.

**Before:** write the claim you'd ship; read what the script times; check `uptime`/`nproc` (busy → interleave sides).

1. **Why not double?** Limiter from a profile or counters in an unreported run, mapped to source. Watch the load tool.
2. **Tuned?** Each side as production runs it: release build, prod flags, pools, cache warmth, same data. Default-config side → tune before picking a winner.
3. **Limits?** Arithmetic vs bandwidth and cores. Cutting 10% gains ≤~11%. Past a limit = cache, no-op, or bug.
4. **Errors?** Count failures; check outputs are correct.
5. **Reproduces?** ≥5 runs per side, alternating. Median, range. Gap < spread = no difference.
6. **Matters?** Report as a share of the end-to-end path.
7. **Happened?** Confirm the work ran in the timed region; lazy code and timeouts fake numbers.

**Report:** verdict (faster, slower, no difference, inconclusive), number + unit, runs, range, limiter. Inconclusive if limiter unnamed, a side untuned, or 4/7 unchecked. PR body: one number, evidence linked.
