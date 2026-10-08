# Spike-1W: Windows confirm (Surface)

Run date 2026-10-07. Roadmap row Spike-1W, owner decision D3 (Windows uses the
same `harness::decide` gate as Linux, on the shipped `grokhub-desktop` MCP:
xcap + SendInput in `crates/grokhub-app/src/desktop_mcp/windows.rs`; no UIA
backend).

> **Not verified on the Surface.** This run happened in a Linux cloud box with
> no Windows machine attached. Every live checklist item and U1–U3 below is
> "not verified, needs the Surface". What this run did: a code review of the
> Windows gate paths, OS-neutral fixes with unit tests, and Windows CI as the
> only Windows evidence. Re-run the live checklist on the Surface before
> calling Spike-1W done.

## Live checklist

| # | Item | Result | Evidence |
|---|------|--------|----------|
| 1 | Build from source on the Surface (`cargo build --release -p grokhub-app`, scratch `GROKHUB_CONFIG`) | Not verified, needs the Surface | Windows CI builds and tests the workspace at the PR head |
| 2 | Path A soft click on a disposable window: span `path:"A"`, `ui_changed:true` | Not verified, needs the Surface | — |
| 3 | Path A hard park on a typed delete of a scratch file: Enter leaves it pending, Esc denies, file byte-identical | Not verified, needs the Surface | Unit: `desk_typing_is_checked_like_a_shell`, harness_gate delete tests (OS-neutral, run on Windows CI) |
| 4 | Path B hard card shape | Not verified, needs the Surface | — |
| 5 | Path C: real Windows `grok` headless, `--deny` beats `--always-approve` | Not verified, needs the Surface | See gap G1 (rule case on Windows) |
| 6 | Path D (1c): deny rules in argv, watchdog on an unasked CU frame | Not verified, needs the Surface | — |
| 7 | Spike-1b on Windows: ShellExecute + SetForegroundWindow, typing length-only in spans, PIN parks as credentials, Recycle Bin delete parks hard | Not verified, needs the Surface | Code read of `desktop_mcp/apps.rs` `win` module; see gaps G2, G3 |
| 8 | Halt from Ctrl+Alt+H, Ctrl+Shift+Esc and the tray denies every parked card | Not verified, needs the Surface | — |

## U1–U3

| Claim | Answer | Confidence | Why |
|-------|--------|------------|-----|
| U1: Grok Build ships or calls a .NET UI helper on Windows | Not verified | — | Needs `Get-Process grok*`, `.Modules` / `tasklist /m` on the Surface, idle and during one GB CU click |
| U2: that helper is mainly UI Automation | Not verified | — | Needs the module list (`UIAutomationCore.dll`, `clr.dll` / `coreclr.dll` / `hostfxr.dll`) and one ACP tool-name sequence |
| U3: accessibility/native actions are 2–4× faster and cheaper than screenshot loops | Not verified (hypothesis) | — | Needs the 10 + 10 run comparison below |

## GB built-in CU vs path A (Notepad, one line, save to `%TEMP%\spike1w\note.txt`)

| Metric | GB built-in CU (Ask) | GB → `grokhub-desktop` path A |
|--------|----------------------|-------------------------------|
| Success (of 10) | not measured | not measured |
| Median wall time per step | not measured | not measured |
| Screenshots per task | not measured | not measured |
| Tokens in / cached / out / reasoning per task | not measured | not measured |
| Gate path per step | D (expected) | A (expected) |

## Fixes in this PR (OS-neutral, unit tested)

1. **Key combos gate in any order and alias.** `desk_classify("key")` matched
   exact strings (`ctrl+alt+delete`), but the backend parses `Alt+Ctrl+Del`,
   `control+alt+delete` and `ctrl_l+alt_l+Delete` into the same press. Those
   ran soft. The gate now reads the combo with
   `grokhub_core::desktop_mcp::parse_key_combo`, the parser the backend uses.
   Test: `key_combos_gate_in_any_order_and_alias`.
2. **PowerShell launch flags with values.** `head_at` skipped only `-`
   flags after `powershell` / `pwsh`, so in
   `powershell -ExecutionPolicy Bypass -Command Remove-Item x` the head read
   as `bypass` and the delete ran soft (paths A typing, B and E).
   Value-taking flags (`-ExecutionPolicy`, `-ep`, `-WindowStyle`, …) now skip
   their value. Test: `windows_launch_flags_and_power_cmdlets_park`.
3. **Windows power, disk and Recycle Bin cmdlets.** `Stop-Computer`,
   `Restart-Computer`, `Format-Volume`, `Clear-Disk` and `format` (also
   `format.com`) are irreversible OS; `Clear-RecycleBin` is a delete. Each
   head has its five GB `--deny` forms (lowercase and PascalCase for the
   cmdlets), so `gb_deny_rules_cover_every_hard_name_and_command` holds;
   `HEADLESS_DENY_RULES` is 262 rules (`format.com` has its own five, since GB rules see the full name).

## Gaps left (written up, not fixed)

- **G1: path C rule case on Windows.** GB `--deny` matching is presumably
  case-sensitive. Rules list lowercase and PascalCase forms for cmdlets, but
  `REMOVE-ITEM x` or `Del x` would not match. The cabin classifier lowercases,
  so paths A, B and E still park; path C relies on
  `approval_gate_violation` after the fact. Confirm GB's matching on the
  Surface before adding more forms.
- **G2: Recycle Bin on a volume with no bin.** `SHFileOperationW` with
  `FOF_ALLOWUNDO | FOF_NOCONFIRMATION` deletes permanently on network shares
  and some removable drives, while the hard card says "move to the trash".
  The card is still hard and still names every path, so nothing goes without
  a click, but the wording overstates undo there. A fix needs a per-volume
  bin check (`SHQueryRecycleBinW`) or `IFileOperation`; out of a gate-only
  spike's scope.
- **G3: forward-slash paths to the Recycle Bin.** `C:/scratch/x.txt` passes
  the absolute-path check, but `SHFileOperationW` wants backslashes. Expected
  to fail closed (error, nothing deleted). Confirm on the Surface.
- **G4: `powershell -EncodedCommand`.** A base64 command body is opaque to
  the classifier and to GB rules. Not a hard class by the design's
  definition; listed so Spike-2 can decide.
- No UIA or other cabin-native Windows CU was added (D3).

## Router-ready

No model calls added or touched. No effort UI, no new `reasoning_effort` reads.
