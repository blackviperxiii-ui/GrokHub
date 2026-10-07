# Compass: Spike-0 harness (approval gate)

## Owns
- The cabin pre-check in front of Grok Build tool execution: hard floor (refuse, no bypass), hard class (park a card, even under Always), and the Access ladder (`AccessMode`: Readonly / Supervised / Full).
- One entry, `harness::decide`, for paths A (`grokhub-desktop` MCP), B (ACP ask), C (headless `grok -p` `--deny` rules), and E (native Lab engine). Every step writes a span.
- Spike-4a trust floor: `ConsentLedger` (`consent.jsonl`, user-click grants only), scopes all off (`Step::Scope`), and `guard_egress` (`Step::Egress`, log in `egress.jsonl`) on cabin-owned xAI calls and `/sync`.
## Quick commands
- `cargo test -p grokhub-agent harness::`
- `cargo test -p grokhub-app harness` (cabin cards; isolate `GROKHUB_CONFIG`)
## Key files
- `crates/grokhub-agent/src/harness/hard.rs`: `HardClass`, `hard_floor`, `classify`, `classify_ask`, `desk_classify`, `HEADLESS_DENY_RULES`.
- `crates/grokhub-agent/src/harness/approval.rs`: `decide`, `decide_harness`, `APPROVAL_TTL` (300 s), `hard_card_key`.
- `crates/grokhub-agent/src/harness/consent.rs` (`Grant`, `Scope`, `UserClick`) and `crates/grokhub-agent/src/harness/egress.rs` (`EgressReq`, `EgressLine`, `is_model_host`).
- `crates/grokhub-agent/src/harness/park.rs` and `crates/grokhub-agent/src/harness/span.rs`: park files and span JSONL.
- `crates/grokhub-app/src/app/harness_ui.rs` (cards, paths B/C/E) and `crates/grokhub-app/src/desktop_mcp/harness_gate.rs` (path A).
## Change recipe
- New hard pattern: classify it in `hard.rs`, add the matching `Bash(...)` / `MCPTool(...)` rule to `HEADLESS_DENY_RULES` if GB rules can express it, then bump the literal count in `headless_deny_rules_cover_the_floor_and_stubs` (47 today).
- New caller: build a `Step` and call `decide`; never add a side path around it. New outbound call: wrap it in `guard_egress` with honest `DataClass` values.
## What breaks it
- Anything that loosens Grok Build: an allow rule, a looser `--permission-mode`, or a `~/.grok` edit. The cabin may only tighten (`docs/superpowers/specs/2026-08-19-grok-build-gui.md`, Spike-0 addendum).
- Writing a `Grant` from anything but a Settings click (`only_a_settings_click_writes_a_grant`), or logging content or raw secrets to `egress.jsonl`.
- Letting Always-approve imply Full (`always_does_not_imply_full`) or Enter approve a hard card.
## What depends on it
- `crates/grokhub-app/src/app/acp.rs`, `crates/grokhub-app/src/app/chat_kick.rs`, `crates/grokhub-app/src/app/background.rs` (pass `HEADLESS_DENY_RULES` as `hard_deny`), `crates/grokhub-agent/src/gate.rs`, and `run_stdio` in `crates/grokhub-app/src/desktop_mcp/mod.rs`.
## Non-obvious
- The harness lives in grokhub-agent, not grokhub-core, even though the desktop MCP and ACP paths use it.
- The floor covers shell commands only (same scope as `host_safety`); read-only tools are never hard class.
- Park handoff is files: `{config_dir}/harness/park`, plus `harness/turn.json` so the `--mcp-desktop` process writes spans into the open chat's `spans/<chat>.jsonl`. No answer in `APPROVAL_TTL`, a halt, or a closed cabin means Deny.
- Spans only gain `#[serde(default)]` fields (`origin`, `consent_ref` are the newest); old lines must still parse. Typed text is stored as its length.
- Grok Build's own traffic is outside the cabin (owner decision D1); egress only guards calls GrokHub makes itself.
- Hard and ACP cards share `approval_card_width` (`APPROVAL_CARD_MAX_W`, 520) in `harness_ui.rs`; Esc denies a hard card. The Grant full card shows only with `GROKHUB_GRANT_FULL=1`: `grant_full_card_on` is read in `Cabin::new`, not `quiet_for_test`, so tests set `full_card_on`.
- GB `PreToolUse` hooks are not the lock: they fail open. `CLI_CREDENTIAL_DENY` from grokhub-acp rides along on path C.
## See also
- [desktop-mcp](desktop-mcp.md), [grokhub-agent](grokhub-agent.md), [grokhub-acp](grokhub-acp.md)
