# Changelog

## Unreleased

## 2.9.15 — 2026-09-22

One cabin, two ship artifacts on the same tag. Cursor **#49** (hover stays inside the slot), **#50** (user bubble wraps and keeps its gap), **#51** (thought expand, minimize, and hide), and **#52** (Account display name and local profile picture) are on `main`.

- Linux: `grokhub-linux-v2.9.15.tar.gz` and AUR `pkgver=2.9.15`.
- Windows: `GrokHub-Setup-2.9.15.exe` and `grokhub-windows-v2.9.15.zip`.
- Skills cards, Automations cards, and the Imagine living wall keep hover inside the slot. The grown plate is clamped to the card. Card tint is the frame fill. The wall keeps the still and only strokes inside the tile.
- A long user bubble wraps inside the row on a narrow pane and a wide one. The leading gap stays reserved.
- A thought starts expanded. Minimize leaves one short row that opens again. Hide stops drawing that thought. The reply stays. Hide and Minimize survive the live-to-stored handoff.
- Account sets a display name and a local profile picture copied into cabin config. A saved name wins over the OAuth name on the avatar menu and the rail. The avatar menu, the rail, and the connected hint do not show the email.
- Default model remains `grok-4.7`. Effort remains None / Minimal / Low / Medium / High / Extra High. Max is not offered.
- Voice stays Ara, OAuth push-to-talk, reply body only. The line stays open until Stop, the live mic, or Ctrl+G / Super+G.
- Grok Build CLI **1.0.38** alpha (unchanged). Headless spawn stays `grok -p --output-format streaming-json` + `--alpha`.

## 2.9.14 — 2026-09-21

One cabin, two ship artifacts on the same tag. Cursor **#44** (Ask card names the action), **#45** (teach a routine), **#46** (no running line above the composer), **#47** (one update for CLI and cabin), and **#48** (History lists the chat cwd) are on `main`.

- Linux: `grokhub-linux-v2.9.14.tar.gz` and AUR `pkgver=2.9.14`. One Update runs only what is newer: `grok update --alpha` first when a newer alpha exists, then the cabin overlay. Linux PATH prepends `$HOME/.grok/bin:$HOME/.local/bin`.
- Windows: `GrokHub-Setup-2.9.14.exe` and `grokhub-windows-v2.9.14.zip` from `packaging/windows/` + Inno. The same pending rules. When the cabin is newer, Settings → Update downloads the GitHub zip when there is no source clone **or** the leftover clone is not a usable `main` checkout. A `main` clone overlays with `install-windows.ps1`.
- Titlebar chip says **Update CLI**, **Update cabin**, or **Update CLI and cabin**. Checked at launch and every 2 hours. `/update` and `grokhub --update` use that same plan. A current alpha is left alone.
- The Thinking / Running / Waiting line no longer sits above the composer, including when the chat pane is scrolled. The green live dot stays on the turn; hover is still the current action. The context usage bar and the Voice · Listening strip stay.
- The Ask card names the command, path, or site. Live secrets stay redacted.
- Naming a schedule teaches the watched steps on Automations. The rewind snapshot is not stored.
- History lists `grok sessions` from the chat cwd. On Windows the session home is USERPROFILE, so a dialogue saved under the work root still shows after restart. Same rule on Linux.
- Default model remains `grok-4.7`. Effort remains None / Minimal / Low / Medium / High / Extra High. Max is not offered.
- Voice stays Ara, OAuth push-to-talk, reply body only. The line stays open until Stop, the live mic, or Ctrl+G / Super+G.
- Grok Build CLI **1.0.38** alpha (unchanged). Headless spawn stays `grok -p --output-format streaming-json` + `--alpha`.

## 2.9.13 — 2026-09-21

One cabin, two ship artifacts on the same tag. Cursor **#43** (composer Stop disc, mic ease, no transcript Running Stop, no status line above the composer) is on `main`.

- Linux: `grokhub-linux-v2.9.13.tar.gz` and AUR `pkgver=2.9.13`. `/update` overlays the GUI then `grok update --alpha` with `$HOME/.grok/bin:$HOME/.local/bin` prepended.
- Windows: `GrokHub-Setup-2.9.13.exe` and `grokhub-windows-v2.9.13.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone **or** the leftover clone is not a usable `main` checkout, then `grok update --alpha`.
- Composer Stop is a disc with a small rounded mark. Idle Stop and the idle mic sit still. They ease while hovered, pressed, listening, speaking, or a reply is running. Same paint on Linux and Windows.
- The transcript Running row has no Stop. A green live dot plus Thinking / Running / Waiting stays on the turn (and above the composer when the pane is scrolled); hover is the current action.
- The changing status text above the composer is gone. The context usage bar stays.
- Default model remains `grok-4.7`. Effort remains None / Minimal / Low / Medium / High / Extra High. Max is not offered. A saved `reasoningEffort` of `max` loads as Extra High (`xhigh`).
- Voice stays Ara, OAuth push-to-talk, reply body only. The line stays open until Stop, the live mic, or Ctrl+G / Super+G.
- Grok Build CLI **1.0.38** alpha (unchanged). Headless spawn stays `grok -p --output-format streaming-json` + `--alpha`.

## 2.9.12 — 2026-09-21

One cabin, two ship artifacts on the same tag. Cursor **#42** (default chat model `grok-4.7`, Max dropped from effort) is on `main`.

- Linux: `grokhub-linux-v2.9.12.tar.gz` and AUR `pkgver=2.9.12`. `/update` overlays the GUI then `grok update --alpha` with `$HOME/.grok/bin:$HOME/.local/bin` prepended.
- Windows: `GrokHub-Setup-2.9.12.exe` and `grokhub-windows-v2.9.12.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone **or** the leftover clone is not a usable `main` checkout, then `grok update --alpha`.
- Empty Settings pin, greeting, and chips use `grok-4.7`. An explicit `/model` pin is left as saved. Legacy Think / Max modes send `grok-4.7` at `high` / `xhigh`.
- Effort is None / Minimal / Low / Medium / High / Extra High. Max is not offered. A saved `reasoningEffort` of `max` loads as Extra High (`xhigh`) and is not sent.
- Voice stays Ara, OAuth push-to-talk, reply body only. The line stays open until Stop, the live mic, or Ctrl+G / Super+G.
- Grok Build CLI **1.0.38** alpha (unchanged). Headless spawn stays `grok -p --output-format streaming-json` + `--alpha`.

## 2.9.11 — 2026-09-21

One cabin, two ship artifacts on the same tag. Cursor **#41** (stronger shared button feel, skip off-screen chat rows, width-keyed heights, stable row ids) is on `main`.

- Linux: `grokhub-linux-v2.9.11.tar.gz` and AUR `pkgver=2.9.11`. `/update` overlays the GUI then `grok update --alpha` with `$HOME/.grok/bin:$HOME/.local/bin` prepended.
- Windows: `GrokHub-Setup-2.9.11.exe` and `grokhub-windows-v2.9.11.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone **or** the leftover clone is not a usable `main` checkout, then `grok update --alpha`.
- Shared buttons hover-scale to 1.035 over 120ms, shrink on press, and scale plus fill on keyboard focus. Same paint on Linux and Windows.
- Off-screen chat rows skip paint. Heights are cached by thread and pane width. Each painted row uses a stable id so a skipped neighbor does not move selection or hover.
- Voice stays Ara, OAuth push-to-talk, reply body only. The line stays open until Stop, the live mic, or Ctrl+G / Super+G.
- Grok Build CLI **1.0.38** alpha (unchanged). Headless spawn stays `grok -p --output-format streaming-json` + `--alpha`.

## 2.9.10 — 2026-09-19

One cabin, two ship artifacts on the same tag. Cursor **#40** (PTT line stays open until Stop, plus Bugbot Autofix) is on `main`.

- Linux: `grokhub-linux-v2.9.10.tar.gz` and AUR `pkgver=2.9.10`. `/update` overlays the GUI then `grok update --alpha` with `$HOME/.grok/bin:$HOME/.local/bin` prepended.
- Windows: `GrokHub-Setup-2.9.10.exe` and `grokhub-windows-v2.9.10.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone **or** the leftover clone is not a usable `main` checkout, then `grok update --alpha`.
- PTT stays live after one listen/speak turn. Indicator + Stop remain until Stop, the live-green mic, or Ctrl+G / Super+G. Same on Linux and Windows.
- Halt / grok -p Err / Consult call `maybe_continue_ptt` so the line is not dead with the indicator still on. Failed STT returns `Hold` instead of respawning listen every frame.
- TTS still uses `voice_tts_script` (reply body, not thinking). Voice is Ara.
- Grok Build CLI **1.0.38** alpha (unchanged). Headless spawn stays `grok -p --output-format streaming-json` + `--alpha`.

## 2.9.9 — 2026-09-19

One cabin, two ship artifacts on the same tag. Cursor **#39** (PTT voice on Linux and Windows; TTS speaks the reply body, not thinking) is on `main`.

- Linux: `grokhub-linux-v2.9.9.tar.gz` and AUR `pkgver=2.9.9`. `/update` overlays the GUI then `grok update --alpha` with `$HOME/.grok/bin:$HOME/.local/bin` prepended.
- Windows: `GrokHub-Setup-2.9.9.exe` and `grokhub-windows-v2.9.9.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone **or** the leftover clone is not a usable `main` checkout, then `grok update --alpha`.
- Hey Grok is push-to-talk on both platforms: `listen_turn` → chat → `speak_reply`. A console key no longer opens duplex PCM (`Realtime` / `PcmSink`).
- TTS runs `voice_tts_script` / `assistant_prose` first, so `THINKING:` / `<think>` / host protocol never hit `grok_tts`. Chat still shows the thought process.
- Voice is Ara. Live Voice · Listening sits above the composer with Stop. PTT returns to Idle after STT.
- Grok Build CLI **1.0.38** alpha (unchanged). Headless spawn stays `grok -p --output-format streaming-json` + `--alpha`.

## 2.9.8 — 2026-09-19

One cabin, two ship artifacts on the same tag. Cursor **#37** (voice Ara, live strip, Stop, PTT Idle reset), **#38** (glanceable Thinking / Running / Waiting), and **#36** (CLI alpha 1.0.38 pin) are on `main`. Superseded alpha ports **#35**–**#31** closed with `ours`.

- Linux: `grokhub-linux-v2.9.8.tar.gz` and AUR `pkgver=2.9.8`. `/update` overlays the GUI then `grok update --alpha` with `$HOME/.grok/bin:$HOME/.local/bin` prepended.
- Windows: `GrokHub-Setup-2.9.8.exe` and `grokhub-windows-v2.9.8.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone **or** the leftover clone is not a usable `main` checkout, then `grok update --alpha`.
- Voice is Ara (realtime `session.voice` and TTS `voice_id`). Live Voice · Listening sits above the composer with Stop. PTT `listen_turn` returns `voice_state` to Idle after STT so the row does not stay Listening and Stop / mic / Ctrl+G do not halt the reply.
- Chat turns show a green live dot plus Thinking / Running / Waiting; hover is the current action (permission title, elicit server, or last tool). Composer attach pulse stays when scrolled.
- Grok Build CLI **1.0.38** alpha. Headless spawn/parse (`grok -p --output-format streaming-json`, `agent stdio`, `sessions`, `export`, `update --alpha`) is unchanged from 1.0.36. `clone --cone` dropped in 1.0.37 (cabin never calls `clone`). Unpackaged `grok update --check --json` still reports `channel=stable` / `latestVersion=1.0.34`; the cabin stays on alpha via `grok update --alpha`.

## 2.9.7 — 2026-09-12

One cabin, two ship artifacts on the same tag. Cursor **#28** (deslop first-run / Latest-notify), **#29** (Windows leftover clone uses zip), and **#30** (Get Started OAuth errors, Linux CLI PATH) are on `main`.

- Linux: `grokhub-linux-v2.9.7.tar.gz` and AUR `pkgver=2.9.7`. `/update` overlays the GUI then `grok update --alpha` with `$HOME/.grok/bin:$HOME/.local/bin` prepended.
- Windows: `GrokHub-Setup-2.9.7.exe` and `grokhub-windows-v2.9.7.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone **or** the leftover clone is not a usable `main` checkout, then `grok update --alpha`.
- Get Started shows live device-code / OAuth failures (`access_denied`, expired token, start/poll errors). Leftover wall or install status is not painted as an OAuth error. Latest → Settings still overlays Get Started.

## 2.9.6 — 2026-09-12

One cabin, two ship artifacts on the same tag. Cursor **#26** (first-run CLI alpha) and **#27** (cabin Latest notify + CLI alpha update) are on `main`.

- Linux: `grokhub-linux-v2.9.6.tar.gz` and AUR `pkgver=2.9.6`. `/update` overlays the GUI then `grok update --alpha`.
- Windows: `GrokHub-Setup-2.9.6.exe` and `grokhub-windows-v2.9.6.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone, then `grok update --alpha`.
- First cabin launch and reinstall automatically install Grok Build CLI **alpha** when `grok` is missing or unusable. Windows Setup runs the official installer (`install-grok-alpha.ps1`) and does not assume grok is already on PATH. Wait / Get Started paint as `CentralPanel`. The Install control is hidden when grok is present or an alpha install is already running.
- When GitHub Latest is a newer cabin, the titlebar shows **Update available** (in-app — Settings → Update, not a web page). Settings → **Update Grok Build CLI** runs `grok update --alpha` when grok is already installed.

## 2.9.5 — 2026-09-12

One cabin, two ship artifacts on the same tag. Cursor **#19** (Imagine video playback), **#20** (profile menu), **#21** (smart chips), **#22** (Windows icon/tray), **#23** (Settings trim), **#24** (chat reuse), and **#25** (CLI stays on alpha) are on `main`.

- Linux: `grokhub-linux-v2.9.5.tar.gz` and AUR `pkgver=2.9.5`. `/update` overlays the GUI then `grok update --alpha`.
- Windows: `GrokHub-Setup-2.9.5.exe` and `grokhub-windows-v2.9.5.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone, then `grok update --alpha`.
- Imagine videos play in-pane instead of the image zoom. Avatar menu is Settings / Help / Connect. Empty-home greeting and chips rank from the situation. Windows uses the PNG cabin icon and tray Quit.
- Settings Account is OAuth connect/sign-out. Behavior quiet hours is one dropdown. GitHub Settings is gone. Update is overlay + Update + Restart, plus **Install Grok Build CLI** when grok is missing or broken. About drops the today-stats line and the model catalog.
- Left-rail **Chat** reuses one empty draft (no New chat button). After a reply starts, Chat opens a new chat. Old convos stay in sidebar History (`grok sessions`).
- Cabin stays on Grok Build CLI **alpha**. A leftover stable `grok` is detected (`~/.grok/config.toml` `[cli] channel` or `grok update --check --json`) and switched with `grok update --alpha`. A working alpha install is not yanked.

## 2.9.4 — 2026-09-11

One cabin, two ship artifacts on the same tag. Cursor **#18** (full-pane chat bubbles, faded thought process, collapsed Work tree) is on `main`.

- Linux: `grokhub-linux-v2.9.4.tar.gz` and AUR `pkgver=2.9.4`. `/update` overlays the GUI then `grok update` on the current channel — it does not pass `--alpha`.
- Windows: `GrokHub-Setup-2.9.4.exe` and `grokhub-windows-v2.9.4.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone, then `grok update --alpha`.
- User and assistant chats are Grok-dark bubbles that wrap with the pane. Thinking stays flush, faded, and labeled Thought process. Tool calls, diffs, and computer-use frames sit in a collapsed Work tree.

## 2.9.3 — 2026-09-11

One cabin, two ship artifacts on the same tag. Cursor **#17** (Windows Grok Build CLI install when missing or broken; no looping loader dialog) is on `main`.

- Linux: `grokhub-linux-v2.9.3.tar.gz` and AUR `pkgver=2.9.3`. `/update` overlays the GUI then `grok update` on the current channel — it does not pass `--alpha`.
- Windows: `GrokHub-Setup-2.9.3.exe` and `grokhub-windows-v2.9.3.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone, then `grok update --alpha`.
- Settings → **Install Grok Build CLI** (Account or Update) and first-run install Grok Build CLI **alpha** when `grok` is missing or broken (stub MZ / `STATUS_DLL_NOT_FOUND`). A hard-failed `grok.exe` shows one cabin error, not a looping Windows loader dialog.
- Install control hides when a working CLI is present. After install, `~/.grok/bin` is preferred over a leftover PATH `grok`.

## 2.9.2 — 2026-09-11

One cabin, two ship artifacts on the same tag. Cursor **#14** (composer hover tips), **#15** (first-run Get Started OAuth + CLI alpha), and **#16** (Windows Update without a clone) are on `main`.

- Linux: `grokhub-linux-v2.9.2.tar.gz` and AUR `pkgver=2.9.2`. `/update` overlays the GUI then `grok update` on the current channel — it does not pass `--alpha`.
- Windows: `GrokHub-Setup-2.9.2.exe` and `grokhub-windows-v2.9.2.zip` from `packaging/windows/` + Inno. Settings → Update downloads the GitHub zip when there is no source clone, then `grok update --alpha`.
- First-time installers ship Grok Build CLI alpha (`GROK_CHANNEL=alpha`). First run is Get Started — Super Grok OAuth signs in `grok` too.
- Hover Chat / Plan / Ask, Ask / Auto / Always, and Effort for a short Grok-dark tip.

## 2.9.1 — 2026-09-11

One cabin, two ship artifacts on the same tag. Cursor **#11** (Windows fold), **#10** (official restyle + Imagine), **#12** (Grok dark hover), and **#13** (`/learn`) are on `main`.

- Linux: `grokhub-linux-v2.9.1.tar.gz` and AUR `pkgver=2.9.1`. `/update` overlays the GUI then `grok update` on the current channel — it does not pass `--alpha`.
- Windows: `GrokHub-Setup-2.9.1.exe` and `grokhub-windows-v2.9.1.zip` from `packaging/windows/` + Inno. Same cabin version as Linux.
- Official Grok dark tokens, 768px column, Imagine parser/timeouts/proxy/on-stage errors, square titlebar glyphs.
- Dark chrome hover is `#1C1F23`, not a cream wash.
- `/learn` (and `/learn reflect`) click and typed send run cabin reflect.
- Quiet-hour clocks type into buffers; Save keeps the last-good window. An hour alone (`7`) is not a clock.
- History search drops the previous needle's hits and ignores a walk that no longer matches the box.
- Re-opening the memory file already in the editor keeps unsaved typing.

## 2.9.0 — 2026-09-04

GrokHub cabin for Grok Build **1.0.21** (covers 1.0.18–1.0.21). Cursor **#9** is on `main`.

- Host receipts keep non-UTF-8 lines (Latin-1 / binary dumps) instead of dropping them, and stop pumping on a pipe read error.
- Headless `grok -p` spend fields (`usage`, `modelUsage`, cost) ride through the existing `/usage` parser.
- Clippy is gated in CI (`-D warnings`, `dead_code` stays advisory).

TUI-only 1.0.18–1.0.21 (ghost prompt, `/btw`, dock, `--plugin-dir` SDK inject) stay in `grok`.

## 2.8.2 — 2026-09-01

- New chat and sidebar History clicks put the cursor in the composer.

## 2.8.1 — 2026-09-01

- `/update`, Settings → Update, and `grokhub --update` also run `grok update` on the current channel after the overlay install.

## 2.8.0 — 2026-09-01

GrokHub cabin for Grok Build **1.0.17** alpha (covers 1.0.15–1.0.17).

- MCP tools that need a form or URL (`x.ai/mcp/elicit`) paint an Accept / Decline card instead of failing the turn.
- URL elicitation opens the link; form elicitation can take the first string field.
- Failed or waiting `input_required` tool calls stay visible on the Queue.
- 1.0.15–1.0.16 CLI speed and token-refresh fixes ride through `grok -p` / ACP with no extra cabin work.

TUI-only 1.0.15–1.0.17 (ghost prompt, `/btw` table copy, dock focus, drag-copy tips) stay in `grok`.

## 2.7.1 — 2026-08-31

GrokHub cabin for Grok Build **1.0.14** alpha.

- `/usage` also runs `grok usage <session>` for persisted per-turn tokens and cost.
- Retry status in the composer shows a short reason (1.0.14 retry line).
- Failed task/todo tool calls stay on the Queue as **failed**.
- `/inspect` notes Grok Build version and that Claude bypass locks are advisory.
- Subagent coordinator “unreachable” retries instead of killing the turn.
- `/models` lists per-effort model ids when the CLI prints them.

## 2.7.0 — 2026-08-29

GrokHub cabin for Grok Build **1.0.13** stable.

### Grok Build 1.0.13

- Truncated replies and transient 5xx / stalls keep the turn instead of dumping an error.
- Compaction failures show the real CLI message.
- Credit-limit errors offer **Try Again** (`/retry`).
- Hook `ask` reasons paint on the permission card.
- Loops page reminds you to stop a loop when the work is done.
- Overlay / docs use `grok update` on the stable channel (`--alpha` is optional).

### History = `grok sessions`

- The sidebar and History page are `grok sessions list` only (no subagent disk walk).
- Delete runs `grok sessions delete` against `~/.grok`, then relists after that command finishes so rows cannot flicker back.
- New chats use the user Grok home so they appear in the same list as the TUI.

### This desktop

- Chat is `grok -p` with `--sandbox off` and a desktop rule: Grok has filesystem and shell here.
- Grok.com-style “I don’t have access to your computer” thoughts are stripped from the pane.
- Halt / Stop still SIGTERMs the `grok -p` child.

## 2.6.42 — 2026-08-26

ACP Ask, compact/rewind, context bar, Grok Build 1.0.11–1.0.12 wiring, thought clustering, History See all / Delete all.
