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
| baseline (all off) | 100.00% (22/22) | 7074.45 | — | 359663 | 204765 | 96.18% | 20088 | 2.41 | 0.32 | — | — |
| R2 prefix cache | 100.00% (22/22) | 6819.55 | -254.91 (-3.60%) | 358437 | 209147 | 98.56% | 20088 | 2.41 | 0.32 | yes | adopt |
| R3 tool-output summaries | 100.00% (22/22) | 7074.45 | 0 | 359663 | 204765 | 96.18% | 20088 | 2.41 | 0.32 | yes | spike |
| R4 deferred tools | 100.00% (22/22) | 4518.32 | -2556.14 (-36.13%) | 230078 | 131415 | 95.85% | 10308 | 2.41 | 0.32 | yes | adopt |
| R5 batched read-only | 100.00% (22/22) | 7074.45 | 0 | 359663 | 204765 | 96.18% | 20088 | 2.41 | 0.32 | yes | spike |
| all on | 100.00% (22/22) | 4374.55 | -2699.91 (-38.16%) | 228852 | 133352 | 97.76% | 10308 | 2.41 | 0.32 | yes | adopt |

## Per-technique notes

- **baseline (all off)**: 22/22 success, 53 model calls, 359663 input tokens of which 204765 cached (96.18% prefix-stable), 20088 schema bytes per request, cost/solved 7074.45.
- **R2 prefix cache**: **adopt** (Δ cost/solved -254.91 (-3.60%)). input tokens 359663 → 358437; prefix-stable 96.18% → 98.56%
- **R3 tool-output summaries**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. Fixture tool outputs are a few bytes, below the size that triggers a summary.
- **R4 deferred tools**: **adopt** (Δ cost/solved -2556.14 (-36.13%)). input tokens 359663 → 230078; prefix-stable 96.18% → 95.85%; schema bytes/request 20088 → 10308
- **R5 batched read-only**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. R5 runs consecutive read-only calls from one reply in parallel; the model still decides how many calls a reply holds, so requests and model calls stay the same and only wall time (not measured here) can move.
- **all on**: **adopt** (Δ cost/solved -2699.91 (-38.16%)). input tokens 359663 → 228852; prefix-stable 96.18% → 97.76%; schema bytes/request 20088 → 10308

## Verdict

Recommendations above follow the rule mechanically from the measured requests. They price prompt bytes only: the replay cannot see output quality, real tokenizer counts, provider cache pricing (cached input is billed at 0 here), or wall time. No default is flipped by this report; the user decides which settings to turn on.
