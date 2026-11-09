# R6 technique A/B eval

Model: grok-4.7 (fixed; offline scripted replay)

## Method

A = every optional cost technique off. Each B arm enables exactly one technique via a temp config dir (`tool_output_compact.toml`, `deferred_tools.toml`, `batched_tools.toml`, `prompt_diet.toml`), then an all-on arm of the first three. The stable prompt prefix is always on, in every arm. Same 22 fixture tasks and scripted model replies in every arm. No network, no paid runs.

Input usage is measured, not scripted: the replay captures every outbound request (system messages, then the tools array, then the other input items, as the wire body sends them) and estimates tokens as bytes / 4. The part of a request that repeats the start of the previous request in the same task counts as cached, the way a provider prefix cache would hit; the rest is billable. Output and reasoning tokens still come from the fixtures. Cost uses the offline tick model in `metrics::offline_cost_ticks` (1 tick / billable input + 4 / output + 4 / reasoning). The scratch workspace path is replaced, terminal output (sh or PowerShell, so it differs by platform) counts as a fixed placeholder, and wall time is omitted, so the report is byte-stable.

Columns: **Prefix-stable** = bytes shared with the previous request / bytes of every request after a task's first. **Schema B/req** = tools-array bytes per model call. **Calls/task** = model calls per task. **Tools/call** = tool calls per model call.

Recommendation rule: **adopt** only if cost per solved task falls and success delta is within ±1 task (noise); **reject** if success leaves noise or cost rises; otherwise **spike** (success within noise, cost flat — needs richer/live measurement before flipping any default).

## Arms

| Arm | Success | Cost/solved | Δ cost/solved | In tokens | Cached | Prefix-stable | Schema B/req | Calls/task | Tools/call | Noise | Rec |
|---|---|---|---|---|---|---|---|---|---|---|---|
| baseline (switches off) | 100.00% (22/22) | 7959.41 | — | 418926 | 244559 | 98.71% | 22310 | 2.41 | 0.32 | — | — |
| R3 tool-output summaries | 100.00% (22/22) | 7959.41 | 0 | 418926 | 244559 | 98.71% | 22310 | 2.41 | 0.32 | yes | spike |
| R4 deferred tools | 100.00% (22/22) | 5211.23 | -2748.18 (-34.53%) | 273258 | 159351 | 98.03% | 11316 | 2.41 | 0.32 | yes | adopt |
| R5 batched read-only | 100.00% (22/22) | 7959.41 | 0 | 418926 | 244559 | 98.71% | 22310 | 2.41 | 0.32 | yes | spike |
| prompt diet (lean prompt, sparse line numbers) | 100.00% (22/22) | 7004.91 | -954.50 (-11.99%) | 368328 | 214960 | 98.53% | 22310 | 2.41 | 0.32 | yes | adopt |
| all on | 100.00% (22/22) | 5211.23 | -2748.18 (-34.53%) | 273258 | 159351 | 98.03% | 11316 | 2.41 | 0.32 | yes | adopt |

## Per-technique notes

- **baseline (switches off)**: 22/22 success, 53 model calls, 418926 input tokens of which 244559 cached (98.71% prefix-stable), 22310 schema bytes per request, cost/solved 7959.41.
- **R3 tool-output summaries**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. Fixture tool outputs are a few bytes, below the size that triggers a summary.
- **R4 deferred tools**: **adopt** (Δ cost/solved -2748.18 (-34.53%)). input tokens 418926 → 273258; prefix-stable 98.71% → 98.03%; schema bytes/request 22310 → 11316
- **R5 batched read-only**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. R5 runs consecutive read-only calls from one reply in parallel; the model still decides how many calls a reply holds, so requests and model calls stay the same and only wall time (not measured here) can move.
- **prompt diet (lean prompt, sparse line numbers)**: **adopt** (Δ cost/solved -954.50 (-11.99%)). input tokens 418926 → 368328; prefix-stable 98.71% → 98.53%
- **all on**: **adopt** (Δ cost/solved -2748.18 (-34.53%)). input tokens 418926 → 273258; prefix-stable 98.71% → 98.03%; schema bytes/request 22310 → 11316

## Verdict

Recommendations above follow the rule mechanically from the measured requests. They price prompt bytes only: the replay cannot see output quality, real tokenizer counts, provider cache pricing (cached input is billed at 0 here), or wall time. No default is flipped by this report; the user decides which settings to turn on.
