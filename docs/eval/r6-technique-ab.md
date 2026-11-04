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
| baseline (all off) | 100.00% (22/22) | 7567 | — | 384912 | 219178 | 96.26% | 21589 | 2.41 | 0.32 | — | — |
| R2 prefix cache | 100.00% (22/22) | 7295 | -272 (-3.59%) | 383685 | 223935 | 98.65% | 21589 | 2.41 | 0.32 | yes | adopt |
| R3 tool-output summaries | 100.00% (22/22) | 7567 | 0 | 384912 | 219178 | 96.26% | 21589 | 2.41 | 0.32 | yes | spike |
| R4 deferred tools | 100.00% (22/22) | 4831.32 | -2735.68 (-36.15%) | 246223 | 140674 | 95.97% | 11122 | 2.41 | 0.32 | yes | adopt |
| R5 batched read-only | 100.00% (22/22) | 7567 | 0 | 384912 | 219178 | 96.26% | 21589 | 2.41 | 0.32 | yes | spike |
| all on | 100.00% (22/22) | 4678.41 | -2888.59 (-38.17%) | 244999 | 142814 | 97.89% | 11122 | 2.41 | 0.32 | yes | adopt |

## Per-technique notes

- **baseline (all off)**: 22/22 success, 53 model calls, 384912 input tokens of which 219178 cached (96.26% prefix-stable), 21589 schema bytes per request, cost/solved 7567.
- **R2 prefix cache**: **adopt** (Δ cost/solved -272 (-3.59%)). input tokens 384912 → 383685; prefix-stable 96.26% → 98.65%
- **R3 tool-output summaries**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. Fixture tool outputs are a few bytes, below the size that triggers a summary.
- **R4 deferred tools**: **adopt** (Δ cost/solved -2735.68 (-36.15%)). input tokens 384912 → 246223; prefix-stable 96.26% → 95.97%; schema bytes/request 21589 → 11122
- **R5 batched read-only**: **spike** (Δ cost/solved 0). no request byte, call or schema change on this fixture set. R5 runs consecutive read-only calls from one reply in parallel; the model still decides how many calls a reply holds, so requests and model calls stay the same and only wall time (not measured here) can move.
- **all on**: **adopt** (Δ cost/solved -2888.59 (-38.17%)). input tokens 384912 → 244999; prefix-stable 96.26% → 97.89%; schema bytes/request 21589 → 11122

## Verdict

Recommendations above follow the rule mechanically from the measured requests. They price prompt bytes only: the replay cannot see output quality, real tokenizer counts, provider cache pricing (cached input is billed at 0 here), or wall time. No default is flipped by this report; the user decides which settings to turn on.
