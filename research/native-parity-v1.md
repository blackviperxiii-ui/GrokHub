# Native parity v1

## Method

Phase 16 runs one fixed suite against the native engine and the CLI engine. Dry-run is the default (`--dry-run`, and the mode when no mode flag is given). The native engine uses scripted fake model clients. The CLI engine is the `grokhub-fake-acp` binary driven with no API key and without a cabin home. Dry-run uses no network and no credentials.

Wall time is measured for each item and printed on the example's stderr. This file records wall time as `dry-run`, and pool % and cost as `0`, so two dry-runs are byte-identical.

`--live` is refused before any client is built unless `GROKHUB_EVAL_API_KEY` is set to a non-empty value and `--budget-usd` is set with `0 < amount <= 0.20`. The key is read only from GROKHUB_EVAL_API_KEY. This harness does not read ~/.grok/auth.json, config.toml, secrets.json, or a keychain, and it does not read the cabin's stored credentials. Live mode is not implemented in this build and was not run for this report.

Suite items: an Xvfb desktop probe, a temp repo bugfix with a test, an Ask-mode refusal, background plus Halt, an MCP tool call through a fake stdio server, compaction of a long transcript, and an Imagine call that only builds the request.

This report does not switch the default engine.

## Results

| Item | Engine | Success | Turns | Wall time | Pool % | Cost | Status |
|---|---|---|---|---|---|---|---|
| xvfb-desktop | native | yes | 0 | dry-run | 0 | 0 | probe ok |
| xvfb-desktop | cli | yes | 0 | dry-run | 0 | 0 | probe ok |
| repo-bugfix | native | yes | 2 | dry-run | 0 | 0 | fixed |
| repo-bugfix | cli | yes | 1 | dry-run | 0 | 0 | scripted reply |
| ask-refusal | native | yes | 2 | dry-run | 0 | 0 | refused |
| ask-refusal | cli | yes | 1 | dry-run | 0 | 0 | refused |
| background-halt | native | yes | 2 | dry-run | 0 | 0 | halted |
| background-halt | cli | yes | 1 | dry-run | 0 | 0 | halted |
| mcp-tool | native | yes | 2 | dry-run | 0 | 0 | echoed |
| mcp-tool | cli | yes | 1 | dry-run | 0 | 0 | scripted tool |
| compaction | native | yes | 2 | dry-run | 0 | 0 | compacted |
| compaction | cli | yes | 1 | dry-run | 0 | 0 | compacted |
| imagine | native | yes | 1 | dry-run | 0 | 0 | request built |
| imagine | cli | yes | 1 | dry-run | 0 | 0 | scripted tool |

## GAPS

- live: `--live` is a stub. It refuses to start without GROKHUB_EVAL_API_KEY and a budget of $0.20 or less, and even with both it only prints "live mode is not implemented in this build" and exits non-zero. No live eval has been run, so nothing in this report measures a real model's success, turns, wall time, pool % or cost.
- cli engine: every CLI row ran against grokhub-fake-acp, which replays scripted events. Those rows show that the ACP client handles each scenario, not what the real Grok CLI does, so CLI-versus-native parity is not verified for any item.
- pool % and cost: written as 0 for every dry-run row. They were not measured.
- xvfb-desktop: the probe only checks that an X display answers (`xdpyinfo`), the same check for both engines. Neither engine's desktop tools were driven.

No default was switched.
