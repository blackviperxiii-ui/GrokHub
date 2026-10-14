# R6 technique A/B eval

Model: grok-4.7 (fixed; offline scripted replay)

## Method

A = all cost techniques off. Each B arm enables exactly one technique via a temp config dir (`prefix_cache.toml`, `tool_output_compact.toml`, `deferred_tools.toml`, `batched_tools.toml`), then an all-on arm. Same 20 fixture tasks and scripted model replies as R1. No network, no paid runs. Cost uses the offline tick model in `metrics::offline_cost_ticks` (1 tick / billable input + 4 / output + 4 / reasoning). Wall time is omitted so the report is byte-stable.

Recommendation rule: **adopt** only if cost per solved task falls and success delta is within ±1 task (noise); **reject** if success leaves noise or cost rises; otherwise **spike** (success within noise, cost flat — needs richer/live measurement before flipping any default).

## Arms

| Arm | Success | Cost/task | Cost/solved | Cache hit | In tokens | Out tokens | Δ success | Δ cost/solved | Noise | Rec |
|---|---|---|---|---|---|---|---|---|---|---|
| baseline (all off) | 100.00% (20/20) | 69.90 | 69.90 | 0.00% | 738 | 140 | — | — | — | — |
| R2 prefix cache | 100.00% (20/20) | 69.90 | 69.90 | 0.00% | 738 | 140 | +0.00% | 0 | yes | spike |
| R3 tool-output summaries | 100.00% (20/20) | 69.90 | 69.90 | 0.00% | 738 | 140 | +0.00% | 0 | yes | spike |
| R4 deferred tools | 100.00% (20/20) | 69.90 | 69.90 | 0.00% | 738 | 140 | +0.00% | 0 | yes | spike |
| R5 batched read-only | 100.00% (20/20) | 69.90 | 69.90 | 0.00% | 738 | 140 | +0.00% | 0 | yes | spike |
| all on | 100.00% (20/20) | 69.90 | 69.90 | 0.00% | 738 | 140 | +0.00% | 0 | yes | spike |

## Per-technique notes

- **baseline (all off)**: R1 baseline — 20/20 success, 738 input / 140 output tokens, cost/solved 69.90 (offline ticks).
- **R2 prefix cache**: recommendation **spike** (success within noise: yes; Δ cost/solved 0).
- **R3 tool-output summaries**: recommendation **spike** (success within noise: yes; Δ cost/solved 0).
- **R4 deferred tools**: recommendation **spike** (success within noise: yes; Δ cost/solved 0).
- **R5 batched read-only**: recommendation **spike** (success within noise: yes; Δ cost/solved 0).
- **all on**: recommendation **spike** (success within noise: yes; Δ cost/solved 0).

## Verdict

Scripted offline replay does not yet attribute cache hits or token savings to technique toggles (fixtures carry fixed usage). Every B arm therefore shows flat cost with success within noise → **spike** each technique pending instrumented measurement; do not flip defaults on without Viper's yes.
