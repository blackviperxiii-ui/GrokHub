# Compass: repair (diagnose, then fix)

## Owns
- Spike-8b diagnose (pillar P6 step 1): "something's wrong with my computer", `/diagnose`, or the native `diagnose` tool runs fixed read-only probes and answers in plain words. Diagnose itself writes nothing to the system.
- Spike-9 fix (pillar P6 steps 2–4): `plans_for` turns warning findings into `FixPlan`s (what, why, undo, risk; empty ones are rejected). Rules classify each step through `harness::decide(Step::Repair)`: the shell floor and hard class, then `REPAIR_FLOOR` and the repair table (package removal, cache and file deletes, `reg delete`, drivers, partitions and boot, boot-critical service disable, piped passwords). `ApplyRun` refuses unattended, takes the restore point (file backups into `{config_dir}/rewind/repair-<id>`, plus a snapper / Timeshift / btrfs / System Restore step), writes the `restore_point` span first, then hands soft steps to Grok Build one at a time (path B, under the pill), parks hard ones on the hard card (`ParkSource::Repair`), never runs hard-floor ones, re-runs the finding's probe, and Undo fix restores files byte for byte.
- Probes are ids (`ProbeId`), never commands. `probe_spec` maps each id to one program and argv per `Os`, run by `run_spec` with no shell, a 10 s `PROBE_TIMEOUT`, and `OUTPUT_CAP` per stream.
## Quick commands
- `cargo test -p grokhub-agent repair::` (classifier, fixtures, gate, redaction, timeout)
- `cargo test -p grokhub-app diagnose`, `cargo test -p grokhub-app wifi_intent` and `cargo test -p grokhub-app repair_ui` (cabin answers and fix cards; isolate `GROKHUB_CONFIG`)
## Key files
- `crates/grokhub-agent/src/repair/mod.rs`: `diagnose`, `DiagnoseCtx`, `report_text`, `probes_for_intent`, `SCOPE_ASK`, `NEEDS_ADMIN`.
- `crates/grokhub-agent/src/repair/probes.rs`: `ProbeId`, `ProbeSpec`, `probe_spec_with`, `PackageManager`, `read_only_violation`.
- `crates/grokhub-agent/src/repair/interpret.rs`: `interpret`, `Finding`, `Severity`, `DISK_WARN_PCT`, `DISK_CRIT_PCT`, `MEM_WARN_PCT`.
- `crates/grokhub-agent/src/repair/run.rs` (`run_spec`, `redact_output`) and `crates/grokhub-app/src/app/repair_ui.rs` (`run_diagnose`, `try_diagnose_intent`, Spike-9 `FixUi`, `fix_card`, `apply_fix`, `poll_fix`, `paint_fix_cards`).
- `crates/grokhub-agent/src/repair/fix.rs` (`FixPlan`, `DraftStep`, `StepClass`, `classify_step`, `gate_text`, `plans_for`, `elevate`), `crates/grokhub-agent/src/repair/restore.rs` (`RestorePoint`, `Snapshot`, `FileBackup`, `backup_files`, `restore_files`), `crates/grokhub-agent/src/repair/apply.rs` (`ApplyRun`, `Action`, `gb_prompt`, `STEP_OK`), tests in `crates/grokhub-agent/src/repair/fix_tests.rs`.
## Change recipe
- New probe: add a `ProbeId` variant with `key` and `label`, its argv in `probe_spec_with`, a parser in `interpret.rs` with a fixture test, and bump the counts in `every_probe_argv_on_both_oses_is_read_only`.
- New fix: add a `plan_for` arm with all four texts, steps as `DraftStep`s (never a class), names from probe output only through `safe_name`, and its row in `every_fix_card_has_what_why_undo_and_risk`. New hard repair pattern: a row in the `fix.rs` tables plus a case in `steps_are_classified_by_rules_soft_hard_and_floor`.
- New write verb or wrapper to refuse: add it to `WRITE_WORDS`, `WRAPPERS`, or `PS_WRITES`, with a case in `the_classifier_catches_writes_and_shell_wrappers`.
## What breaks it
- A probe that takes user text, a shell (`sh -c`, `cmd /c`, `Invoke-Expression`), a pipe or `;` in a PowerShell script, or a refresh that writes package caches.
- Running anything before `Step::Scope` for `system_state` allows it, or skipping the per-probe `harness::decide` tool step.
- Output reaching a span or the model before `redact_output`, or a span without `Origin::Repair`.
- A fix step the cabin runs itself, a step sent before the `restore_point` span, a hard-floor step with a command or a Fix it button, Enter answering a fix card, an `UndoAsk` built outside a click (`only_typing_or_a_click_builds_an_undo_ask`), or a "fixed" line without a `pass` verify span.
## What depends on it
- The native registry (`tool_schemas`, `dispatch` in `crates/grokhub-agent/src/tools/mod.rs`), `Slash::Diagnose` in `crates/grokhub-core/src/slash.rs`, and the send path in `crates/grokhub-app/src/app/chat_kick.rs`.
## Non-obvious
- Diagnose never elevates: a probe marked `needs_admin` (zypper verify) is skipped with "needs admin, skipped". Apt uses `dpkg --audit` because `apt-get check` takes the dpkg lock.
- Findings are worded by the interpreter, so diagnose makes no model call. The first sentence never names a command.
- Fix steps never type a password: Linux steps that need root use `pkexec` (polkit's own prompt), Windows ones a UAC `RunAs`; `sudo -S` or `--stdin` is hard credentials. Grok Build replies `STEP_OK` or `STEP_FAILED`; no reply or a send it couldn't take stops the fix. A failed snapshot step keeps going on the file backup. Undo can't roll a snapshot back; it prints the plain steps with the snapshot's name. `probes_for_intent` is narrow on purpose: "the build is broken on my machine" still goes to the model. No `system_state` grant means zero probes, the `SCOPE_ASK` line, and Spike-8a's `ScopeAsks` card (`SCOPE_ASK_WHY`); no new chrome.
## See also
- [harness](harness.md), [slash](slash.md), [grokhub-agent](grokhub-agent.md)
