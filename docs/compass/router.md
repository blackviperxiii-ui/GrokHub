# Compass: router (R0 model registry and shadow routes)

## Owns
- The model registry in `{config}/models/registry.json`: which Grok models exist, which this plan or key can use, and their state (`ModelState`: probing, live, degraded, quarantined, not_in_plan, redirected, ghost, retired, pruned). Sources share one `CatalogSource` trait; `XaiApiSource` and `GrokBuildSource` are the only router code that touches the network or runs `grok`.
- Passive health from real calls (`Observation` lines in `models/health.jsonl`, folded by `fold_health`). No health probes; the onboarding probe (`run_probe`) is the only one, and it only runs on `included` routes.
- Model profiles in `models/model_profiles/` (`ModelProfile`, versioned, last 3 kept) and `RuntimeSettings::from_profile`, which in R0 only fill the shadow record's `settings`.
- `Router::choose` (pure: inputs, registry and profiles passed in, `now_ms` as the clock) and the shadow `RouteRecord` on each `model-calls` span. R0 changes no request: `POLICY_LIVE` is false and `/why` is the only new surface.
## Quick commands
- `cargo test -p grokhub-core model_registry` (registry, health, probe, profiles; no network)
- `cargo test -p grokhub-agent route::` (choose, shadow log, golden bodies, refresh with fakes)
## Key files
- `crates/grokhub-core/src/model_registry/mod.rs`: `Registry::apply_refresh`, `Registry::fold`, `RegistryEvent`, `RefreshClock` (30 s after start when idle, then `REFRESH_EVERY_MS`, GB version, sign-in, signal, demand).
- `crates/grokhub-core/src/model_registry/health.rs` (`observe`, `classify_status`, `GHOST_STRIKES`), `crates/grokhub-core/src/model_registry/probe.rs` (`PROBE_RUNS_PER_DAY`, `PROBE_TOKEN_CAP_PER_RUN`, `Onboarding`), `crates/grokhub-core/src/model_registry/profile.rs` (`write_profile`, `clamp_effort`).
- `crates/grokhub-agent/src/route/mod.rs` (`Router`, `call_model`), `crates/grokhub-agent/src/route/shadow.rs` (`shadow_log`, `stream_shadowed`, `ClassScope`, `shadow_gb_turn`), `crates/grokhub-agent/src/route/policy.rs` (`CLASS_TABLE`), `crates/grokhub-agent/src/route/signals.rs`, `crates/grokhub-agent/src/route/refresh.rs`, `crates/grokhub-agent/src/route/sources.rs`.
- `crates/grokhub-app/src/app/router_ui.rs`: `tick_model_registry` on the heartbeat, `run_why`.
## Change recipe
- New model call site: send through `stream_shadowed` (native) or `call_model`, or call `shadow_log` after the call with its class. Never let the shadow change the request (`shadow_mode_leaves_request_bodies_and_agent_args_byte_identical`).
- New listing field: parse it in `parse_model_row`, add it to `ModelMeta` with `#[serde(default)]`, keep unknown as `None`, and extend `merge_from`.
## What breaks it
- Treating 403 content-safety or 429 `free-usage-exhausted` as health (`content_safety_and_free_usage_change_nothing`), or counting an unlisted 404 as a ghost strike.
- Putting prompt text in a route record (`route_records_hold_no_prompt_text_and_why_prints_at_most_ten_lines`), or a probe prompt built from user data (`probe_prompts_contain_no_user_data`).
- A new `ureq` builder in `sources.rs` without its `WRAPPED` count in `crates/grokhub-agent/src/harness/egress_coverage.rs`.
## What depends on it
- R1 (effort ladder) turns on `CLASS_TABLE`; R2a switches call sites to `RuntimeSettings`; R3 joins route records with `outcomes.jsonl` by `span_id`.
## Non-obvious
- `harness::decide` stays the only approval gate: the router picks how to think, never whether to act. A GB-only model gets a metadata-only profile (`not_run: gb_only`); a queued probe is not usable yet.
- Grok Build turns are logged at send (`provider` `grok_build`, no outcome, no health): GB owns those calls and its effort is set at spawn.
## See also
- [harness](harness.md), [grokhub-agent](grokhub-agent.md), [heartbeat](heartbeat.md), [self-improve](self-improve.md)
