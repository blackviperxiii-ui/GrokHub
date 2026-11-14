# Optimize — detail

Use after the short playbook in `SKILL.md`. Goal: prove a real cost, fix the first wrong decision, remeasure.

## Not these skills

| Skill | Job |
|-------|-----|
| **optimize** | Find and remove runtime/build/IO/network/CPU/memory/bundle waste |
| **benchmark-checklist** | Vet whether a reported number is trustworthy before acting |
| **land-it** | Product feel / UX / QoL — not perf numbers |

## Measure (same artifact before and after)

Pick the artifact the user actually feels:

- **UI / web:** Lighthouse, Performance panel, React Profiler, Web Vitals
- **CPU / latency:** `time`, flamegraphs, sampling profilers, `perf`
- **IO / DB:** query plans, N+1 traces, slow-query logs
- **Network:** waterfall, payload size, cache hit rate
- **Bundle / build:** analyzer, compile timings, tree-shake gaps
- **Memory:** heap snapshots, RSS over a fixed workload

Write the claim you will retest (e.g. “median p95 of X under Y load”) before changing code. Ballpark only when the user asked for one; still name the tool and say it is a single run.

## Rank

Score each hot spot: **user impact × fix cost** → P0 / P1 / P2.

- P0: blocks a core loop or burns battery/CPU on idle
- P1: noticeable lag on a frequent path
- P2: nice-to-have / cold paths

Prefer one P0 root cause over a spray of P2 microtweaks (**Subtract Before You Add**).

## Root causes (examples)

- Wrong algorithm or data structure for the hot set
- N+1 queries / chatty APIs
- Sync work on the critical path (main thread, request hold)
- Over-render / unbounded lists without virtualization
- Eager work that should be lazy or incremental
- Missing indexes, cache, or batching where the profile proves it

Guards and “make it feel faster” spinners are not fixes unless the root cause is already gone.

## Remeasure and stop

- Same tool, same workload, same build mode (release vs debug).
- Report before/after with units and run count; if the gap is within noise, say so (**Explain the Number**).
- Stop when the stated criteria are met (**Laziness Protocol**). Do not keep tuning for sport.
- Contested comparative claims → **benchmark-checklist**. UI-facing ship confidence → **qa-walkthrough**.
