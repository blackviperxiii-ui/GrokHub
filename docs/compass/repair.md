# Compass: repair (diagnose now, apply later)

## Owns
- Spike-8b diagnose (pillar P6 step 1): "something's wrong with my computer", `/diagnose`, or the native `diagnose` tool runs fixed read-only probes and answers in plain words. Spike-9 adds fixing (apply); nothing here writes to the system.
- Probes are ids (`ProbeId`), never commands. `probe_spec` maps each id to one program and argv per `Os`, run by `run_spec` with no shell, a 10 s `PROBE_TIMEOUT`, and `OUTPUT_CAP` per stream.
## Quick commands
- `cargo test -p grokhub-agent repair::` (classifier, fixtures, gate, redaction, timeout)
- `cargo test -p grokhub-app diagnose` and `cargo test -p grokhub-app wifi_intent` (cabin answers; isolate `GROKHUB_CONFIG`)
## Key files
- `crates/grokhub-agent/src/repair/mod.rs`: `diagnose`, `DiagnoseCtx`, `report_text`, `probes_for_intent`, `SCOPE_ASK`, `NEEDS_ADMIN`.
- `crates/grokhub-agent/src/repair/probes.rs`: `ProbeId`, `ProbeSpec`, `probe_spec_with`, `PackageManager`, `read_only_violation`.
- `crates/grokhub-agent/src/repair/interpret.rs`: `interpret`, `Finding`, `Severity`, `DISK_WARN_PCT`, `DISK_CRIT_PCT`, `MEM_WARN_PCT`.
- `crates/grokhub-agent/src/repair/run.rs` (`run_spec`, `redact_output`) and `crates/grokhub-app/src/app/repair_ui.rs` (`run_diagnose`, `try_diagnose_intent`).
## Change recipe
- New probe: add a `ProbeId` variant with `key` and `label`, its argv in `probe_spec_with`, a parser in `interpret.rs` with a fixture test, and bump the counts in `every_probe_argv_on_both_oses_is_read_only`.
- New write verb or wrapper to refuse: add it to `WRITE_WORDS`, `WRAPPERS`, or `PS_WRITES`, with a case in `the_classifier_catches_writes_and_shell_wrappers`.
## What breaks it
- A probe that takes user text, a shell (`sh -c`, `cmd /c`, `Invoke-Expression`), a pipe or `;` in a PowerShell script, or a refresh that writes package caches.
- Running anything before `Step::Scope` for `system_state` allows it, or skipping the per-probe `harness::decide` tool step.
- Output reaching a span or the model before `redact_output`, or a span without `Origin::Repair`.
## What depends on it
- The native registry (`tool_schemas`, `dispatch` in `crates/grokhub-agent/src/tools/mod.rs`), `Slash::Diagnose` in `crates/grokhub-core/src/slash.rs`, and the send path in `crates/grokhub-app/src/app/chat_kick.rs`.
## Non-obvious
- No elevation ever: a probe marked `needs_admin` (zypper verify) is skipped with "needs admin, skipped". Apt uses `dpkg --audit` because `apt-get check` takes the dpkg lock.
- Findings are worded by the interpreter, so diagnose makes no model call. The first sentence never names a command.
- `probes_for_intent` is narrow on purpose: "the build is broken on my machine" still goes to the model. No `system_state` grant means zero probes and the `SCOPE_ASK` line, never a new card or chrome.
## See also
- [harness](harness.md), [slash](slash.md), [grokhub-agent](grokhub-agent.md)
