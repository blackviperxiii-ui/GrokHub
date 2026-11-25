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
| baseline (switches off) | 100.00% (22/22) | 8685.73 | — | 457412 | 267066 | 98.81% | 25143 | 2.41 | 0.32 | — | — |
| R3 tool-output summaries | 100.00% (22/22) | 8685.73 | 0 | 457412 | 267066 | 98.81% | 25143 | 2.41 | 0.32 | yes | spike |
| R4 deferred tools | 100.00% (22/22) | 5482.45 | -3203.27 (-36.88%) | 287626 | 167752 | 98.13% | 12329 | 2.41 | 0.32 | yes | adopt |
| R5 batched read-only | 100.00% (22/22) | 8685.73 | 0 | 457412 | 267066 | 98.81% | 25143 | 2.41 | 0.32 | yes | spike |
| prompt diet (lean prompt, sparse line numbers) | 100.00% (22/22) | 7711.36 | -974.36 (-11.22%) | 405774 | 236864 | 98.67% | 25143 | 2.41 | 0.32 | yes | adopt |
| all on | 100.00% (22/22) | 5482.45 | -3203.27 (-36.88%) | 287626 | 167752 | 98.13% | 12329 | 2.41 | 0.32 | yes | adopt |

## Per-technique notes

- **baseline (switches off)**: 22/22 success, 53 model calls, 457412 input tokens of which 267066 cached (98.81% prefix-stable), 25143 schema bytes per request, cost/solved 8685.73.
- **R3 tool-output summaries**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. Fixture tool outputs are a few bytes, below the size that triggers a summary.
- **R4 deferred tools**: **adopt** (Δ cost/solved -3203.27 (-36.88%)). input tokens 457412 → 287626; prefix-stable 98.81% → 98.13%; schema bytes/request 25143 → 12329
- **R5 batched read-only**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. R5 runs consecutive read-only calls from one reply in parallel; the model still decides how many calls a reply holds, so requests and model calls stay the same and only wall time (not measured here) can move.
- **prompt diet (lean prompt, sparse line numbers)**: **adopt** (Δ cost/solved -974.36 (-11.22%)). input tokens 457412 → 405774; prefix-stable 98.81% → 98.67%
- **all on**: **adopt** (Δ cost/solved -3203.27 (-36.88%)). input tokens 457412 → 287626; prefix-stable 98.81% → 98.13%; schema bytes/request 25143 → 12329

## Verdict

Recommendations above follow the rule mechanically from the measured requests. They price prompt bytes only: the replay cannot see output quality, real tokenizer counts, provider cache pricing (cached input is billed at 0 here), or wall time. No default is flipped by this report; the user decides which settings to turn on.
