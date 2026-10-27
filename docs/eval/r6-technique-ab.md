# R6 technique A/B eval

Model: grok-4.7 (fixed; offline scripted replay)

## Method

A = all cost techniques off. Each B arm enables exactly one technique via a temp config dir (`prefix_cache.toml`, `tool_output_compact.toml`, `deferred_tools.toml`, `batched_tools.toml`), then an all-on arm. Same 22 fixture tasks and scripted model replies in every arm. No network, no paid runs.

Input usage is measured, not scripted: the replay captures every outbound request (system messages, then the tools array, then the other input items, as the wire body sends them) and estimates tokens as bytes / 4. The part of a request that repeats the start of the previous request in the same task counts as cached, the way a provider prefix cache would hit; the rest is billable. Output and reasoning tokens still come from the fixtures. Cost uses the offline tick model in `metrics::offline_cost_ticks` (1 tick / billable input + 4 / output + 4 / reasoning). The scratch workspace path is replaced, terminal output (sh or PowerShell, so it differs by platform) counts as a fixed placeholder, and wall time is omitted, so the report is byte-stable.

Columns: **Prefix-stable** = bytes shared with the previous request / bytes of every request after a task's first. **Schema B/req** = tools-array bytes per model call. **Calls/task** = model calls per task. **Tools/call** = tool calls per model call.

Recommendation rule: **adopt** only if cost per solved task falls and success delta is within ±1 task (noise); **reject** if success leaves noise or cost rises; otherwise **spike** (success within noise, cost flat — needs richer/live measurement before flipping any default).

## Arms

| Arm | Success | Cost/solved | Δ cost/solved | In tokens | Cached | Prefix-stable | Schema B/req | Calls/task | Tools/call | Noise | Rec |
|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline (all off) | 100.00% (22/22) | 7267.86 | — | 369468 | 210315 | 96.20% | 20828 | 2.41 | 0.32 | — | — |
| R2 prefix cache | 100.00% (22/22) | 7004.55 | -263.32 (-3.62%) | 368242 | 214882 | 98.60% | 20828 | 2.41 | 0.32 | yes | adopt |
| R3 tool-output summaries | 100.00% (22/22) | 7267.86 | 0 | 369468 | 210315 | 96.20% | 20828 | 2.41 | 0.32 | yes | spike |
| R4 deferred tools | 100.00% (22/22) | 4532.23 | -2735.64 (-37.64%) | 230781 | 131812 | 95.86% | 10361 | 2.41 | 0.32 | yes | adopt |
| R5 batched read-only | 100.00% (22/22) | 7267.86 | 0 | 369468 | 210315 | 96.20% | 20828 | 2.41 | 0.32 | yes | spike |
| all on | 100.00% (22/22) | 4387.91 | -2879.95 (-39.63%) | 229555 | 133761 | 97.77% | 10361 | 2.41 | 0.32 | yes | adopt |

## Per-technique notes

- **baseline (all off)**: 22/22 success, 53 model calls, 369468 input tokens of which 210315 cached (96.20% prefix-stable), 20828 schema bytes per request, cost/solved 7267.86.
- **R2 prefix cache**: **adopt** (Δ cost/solved -263.32 (-3.62%)). input tokens 369468 → 368242; prefix-stable 96.20% → 98.60%
- **R3 tool-output summaries**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. Fixture tool outputs are a few bytes, below the size that triggers a summary.
- **R4 deferred tools**: **adopt** (Δ cost/solved -2735.64 (-37.64%)). input tokens 369468 → 230781; prefix-stable 96.20% → 95.86%; schema bytes/request 20828 → 10361
- **R5 batched read-only**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. R5 runs consecutive read-only calls from one reply in parallel; the model still decides how many calls a reply holds, so requests and model calls stay the same and only wall time (not measured here) can move.
- **all on**: **adopt** (Δ cost/solved -2879.95 (-39.63%)). input tokens 369468 → 229555; prefix-stable 96.20% → 97.77%; schema bytes/request 20828 → 10361

## Verdict

Recommendations above follow the rule mechanically from the measured requests. They price prompt bytes only: the replay cannot see output quality, real tokenizer counts, provider cache pricing (cached input is billed at 0 here), or wall time. No default is flipped by this report; the user decides which settings to turn on.
