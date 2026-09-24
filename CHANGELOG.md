# Changelog

## Unreleased

## 2.10.24 — 2026-09-24

Cabin History is the cabin's own chats, kept on the thread through headless `grok -p`. It is not `grok sessions list`. Creating a project does not wipe those chats. Background jobs such as “summarize the workboard” stay off History. Leave a project chat and click the project again: the same chat opens. Pins stay. Transcripts stay. A project click still does not open the Workboard.

- Linux: `grokhub-linux-v2.10.24.tar.gz` and AUR `pkgver=2.10.24`.
- Windows: `GrokHub-Setup-2.10.24.exe` and `grokhub-windows-v2.10.24.zip`.
- Cursor cabin 2.10.24. VERSION 2.10.24. Draft only. Not tagged. Rebased onto main 2.10.23 (`6861d05`).

## 2.10.23 — 2026-09-24

A second prompt on the open chat stays on the same History row. The cabin resumes that Grok session. A different id from the follow-up is not a new channel. A new History row is only a new chat. Transcripts stay. Project folder filter and pins stay.

- Linux: `grokhub-linux-v2.10.23.tar.gz` and AUR `pkgver=2.10.23`.
- Windows: `GrokHub-Setup-2.10.23.exe` and `grokhub-windows-v2.10.23.zip`.
- Cursor cabin 2.10.23. VERSION 2.10.23. Draft only. Not tagged. Rebased onto main 2.10.22 (keep Reply inside the window).

## 2.10.22 — 2026-09-24

Copy and Reply under a user bubble stay inside the window. A short message such as “hey” still sits on the right and still wraps when it is long. The Reply label no longer draws past the right edge.

- Linux: `grokhub-linux-v2.10.22.tar.gz` and AUR `pkgver=2.10.22`.
- Windows: `GrokHub-Setup-2.10.22.exe` and `grokhub-windows-v2.10.22.zip`.
- Cursor cabin 2.10.22. VERSION 2.10.22. Draft only. Not tagged. Rebased onto main 2.10.21 (Plan does not rename the chat).

## 2.10.21 — 2026-09-24

Clicking Plan switches the session to Plan. The thread title and the History row stay. Chat and btw pills are unchanged. The session id still clears so the next turn is a new Grok session.

- Linux: `grokhub-linux-v2.10.21.tar.gz` and AUR `pkgver=2.10.21`.
- Windows: `GrokHub-Setup-2.10.21.exe` and `grokhub-windows-v2.10.21.zip`.
- Cursor cabin 2.10.21. VERSION 2.10.21. Draft only. Not tagged. Rebased onto main 2.10.20 (quick-chip dismiss).

## 2.10.20 — 2026-09-24

Quick-chip × stays in its reserved slot. Hovering it no longer flashes the pointer, and a click dismisses that chip on the composer and on empty home. The chip row is otherwise unchanged.

- Linux: `grokhub-linux-v2.10.20.tar.gz` and AUR `pkgver=2.10.20`.
- Windows: `GrokHub-Setup-2.10.20.exe` and `grokhub-windows-v2.10.20.zip`.
- Cursor cabin 2.10.20. VERSION 2.10.20. Draft only. Not tagged. Rebased onto main 2.10.19 (session actions beside minimize).

## 2.10.19 — 2026-09-24

Compact, Copy session, and Export leave the composer. They sit in a three-bar menu immediately beside minimize. A titlebar press opens the menu the same way as the other chrome buttons (`titlebar_chrome_hit`). Each action is unchanged. Settings, Chat / Plan / btw, Ask / Auto / Always, and the quick chips stay put. The context usage bar stays. View plan and Fork stay on the thread.

- Linux: `grokhub-linux-v2.10.19.tar.gz` and AUR `pkgver=2.10.19`.
- Windows: `GrokHub-Setup-2.10.19.exe` and `grokhub-windows-v2.10.19.zip`.
- Cursor cabin 2.10.19. VERSION 2.10.19. Draft only. Not tagged. Rebased onto main 2.10.18 (home update feed).

## 2.10.18 — 2026-09-24

Signed-in empty home drops the Coding / Life chip and the under-greeting workboard summary card. That slot is an update feed in `updates.json`: newest first, hidden when nothing is undismissed (no “No updates” placeholder). A finished `/loop` posts `automation_done` from `poll_grok_loop`. A night recipe replay that finishes posts the same kind. Saving a clock job or interval loop posts `schedule_created` from `commit_schedule`. `suggestion` and `automate_offer` are typed cards for a later producer. Open marks a card opened and leaves it; Dismiss removes it. No interest learning and no `interest_update`.

- Linux: `grokhub-linux-v2.10.18.tar.gz` and AUR `pkgver=2.10.18`.
- Windows: `GrokHub-Setup-2.10.18.exe` and `grokhub-windows-v2.10.18.zip`.
- Cursor cabin 2.10.18. VERSION 2.10.18. Draft only. Not tagged. Rebased onto main 2.10.17 (session pin and rename).

## 2.10.17 — 2026-09-23

Right-click a History chat to Pin, Unpin, or Rename. Double-click the row to rename it in place. Pinned chats sit above the rest, last pinned first (`pinned_ms` in `threads.json`). Unpin puts the chat back with the other chats by last use. A blank or whitespace name is rejected and the previous title stays. Rename does not clear the pin. A project filter only hides other chats; it does not drop a pin or a title. `click_project_opens_board` stays false.

- Linux: `grokhub-linux-v2.10.17.tar.gz` and AUR `pkgver=2.10.17`.
- Windows: `GrokHub-Setup-2.10.17.exe` and `grokhub-windows-v2.10.17.zip`.
- Cursor cabin 2.10.17. VERSION 2.10.17. Draft only. Not tagged.

## 2.10.16 — 2026-09-23

Workboards is its own rail row, directly under Skills and Connectors. The page is a kanban (Todo, Doing, Blocked, Done) stored in `workboard.json`. Create, edit, move, and archive cards there. A card can link a chat; Open chat switches to that thread and leaves Workboards on the rail. When a run actually starts, `kick_model` calls `note_inflight_card` and upserts one Doing card for that thread (title from the user ask, or the thread label). `finish_acp_turn` calls `settle_turn_card`, which moves that card to Done and applies any `WORK_PIN:` / `WORK_UPDATE:` lines in the assistant text. A project click still only filters chats.

- Linux: `grokhub-linux-v2.10.16.tar.gz` and AUR `pkgver=2.10.16`.
- Windows: `GrokHub-Setup-2.10.16.exe` and `grokhub-windows-v2.10.16.zip`.
- Cursor cabin 2.10.16. VERSION 2.10.16. Draft only. Not tagged.

## 2.10.15 — 2026-09-23

A project is a folder of persistent chats. Selecting it filters sidebar History and files new chats there. Global chats stay on disk; click the project again to see every chat. Delete unassigns those chats back to History and does not wipe transcripts. Clicking or creating a project does not open the Workboard.

- Linux: `grokhub-linux-v2.10.15.tar.gz` and AUR `pkgver=2.10.15`.
- Windows: `GrokHub-Setup-2.10.15.exe` and `grokhub-windows-v2.10.15.zip`.
- Cursor cabin 2.10.15. VERSION 2.10.15. Draft only. Not tagged.

## 2.10.14 — 2026-09-23

btw replaces the Questions label on the session pill (saved id stays `ask`). A live run is not cancelled; the side ask waits, then sends look-safe. Compact sits on the existing context bar. Copy session and Export are on the thread chrome; per-bubble Copy stays. View plan reopens a Plan-mode plan. Fork shows only on a long thread (12+ turns) or when context is at least half the budget, with a one-time how-it-works note. `/rewind` is unchanged.

- Linux: `grokhub-linux-v2.10.14.tar.gz` and AUR `pkgver=2.10.14`.
- Windows: `GrokHub-Setup-2.10.14.exe` and `grokhub-windows-v2.10.14.zip`.
- Cursor cabin 2.10.14. VERSION 2.10.14. Not tagged until MERGE GREEN.

## 2.10.13 — 2026-09-23

Quick chips are one fixed-height line. Long labels ellipsize instead of wrapping or clipping. A chip that does not fully fit is dropped, not cut off at the window edge. The same ranked pool stays up mid-conversation (habit / static chips when the LLM row is not ready). Click, dismiss, and session pills are unchanged.

- Linux: `grokhub-linux-v2.10.13.tar.gz` and AUR `pkgver=2.10.13`.
- Windows: `GrokHub-Setup-2.10.13.exe` and `grokhub-windows-v2.10.13.zip`.
- Cursor cabin 2.10.13. VERSION 2.10.13. Not tagged until MERGE GREEN.


## 2.10.12 — 2026-09-23

Settings → Cabin defaults pins the chat model (Auto saves empty), reasoning effort, Ask or Auto, and Chat / Plan / Questions into `app.json` for headless `grok -p`. Always stays on the composer. Always collapse starts thoughts folded in every session. Collapsing any thought folds that session; expand opens one thought at a time. Quiet Collapse / Expand stays; Hide is not painted. The first close-to-tray toast is saved as `closeToTrayTipSeen`. Later closes, including after a relaunch, stay quiet. Quiet hours skip that toast and leave the flag clear, so the first close outside quiet hours can still tip once. Close-to-tray and tray Show / Quit are unchanged.

- Linux: `grokhub-linux-v2.10.12.tar.gz` and AUR `pkgver=2.10.12`.
- Windows: `GrokHub-Setup-2.10.12.exe` and `grokhub-windows-v2.10.12.zip`.
- Cursor cabin 2.10.12. VERSION 2.10.12. Not tagged until MERGE GREEN.

## 2.10.11 — 2026-09-23

Skills and Connectors cards in a row share one height. Descriptions clamp to three lines and ellipsize on a word boundary. Use in chat and the other tile actions sit on the bottom of the card, so a short description does not leave the button high.

- Linux: `grokhub-linux-v2.10.11.tar.gz` and AUR `pkgver=2.10.11`.
- Windows: `GrokHub-Setup-2.10.11.exe` and `grokhub-windows-v2.10.11.zip`.
- Cursor cabin 2.10.11. VERSION 2.10.11. Not tagged until MERGE GREEN.

## 2.10.10 — 2026-09-23

Thought process header is one quiet collapse control (chevron plus Collapse / Expand). Hide is gone. Collapse still toggles the thought body the way Minimize did. Tool-call rows are unchanged.

- Linux: `grokhub-linux-v2.10.10.tar.gz` and AUR `pkgver=2.10.10`.
- Windows: `GrokHub-Setup-2.10.10.exe` and `grokhub-windows-v2.10.10.zip`.
- Cursor cabin 2.10.10. VERSION 2.10.10. Not tagged until MERGE GREEN.

## 2.10.9 — 2026-09-23

Skills and Connectors `grok_tile` wraps the full description; Use in chat sits under the body (no title-row overlap, no 80-char mid-word clip). Accepting a Suggested automation dismisses it from `suggestions.json` and the UI every time. Quiet daily session-derived Suggested Automations and Skills land on the night/review path and persist. Automations no longer paints Follow along / Teach this once. Loop and scheduled titles wrap instead of `take(40)`. History indexes a live session as soon as it is created and Windows re-lists on a 3s watch so new chats do not lag forever.

- Linux: `grokhub-linux-v2.10.9.tar.gz` and AUR `pkgver=2.10.9`.
- Windows: `GrokHub-Setup-2.10.9.exe` and `grokhub-windows-v2.10.9.zip`.
- Cursor cabin 2.10.9. VERSION 2.10.9. Not tagged until MERGE GREEN.

## 2.10.8 — 2026-09-23

Selecting a cabin skill follows into grok -p / ACP: `Follow skill {name}` matches, and the kick prepends `active_skill_follow`. Skills Suggested tiles from the nightly review Add via `save_skill`. Connectors owns the GitHub PAT plus read-only Who am I / List repos tiles (`run_connector` only — no writes, no other websites). `/workflow` `/compact` `/rewind` honor the PermissionMode pill: Ask is fail-closed if ACP is down; Auto/Always keep composer flags and session mode. Questions / Look session mode maps to `--permission-mode default` (never the invalid CLI value `ask`). Scrolled-up chat is one down-arrow jump (click = latest; Last you is right-click or hold). Home chips pad inside the fill and wrap without ellipsis. The Questions session pill keeps id `ask` with 8px inset.

- Linux: `grokhub-linux-v2.10.8.tar.gz` and AUR `pkgver=2.10.8`.
- Windows: `GrokHub-Setup-2.10.8.exe` and `grokhub-windows-v2.10.8.zip`.
- Cursor cabin 2.10.8. VERSION 2.10.8. Not tagged until MERGE GREEN.

## 2.10.7 — 2026-09-23

Owner UI: assistant bubbles keep real inner padding (no left/top clip of the first glyphs). The chat pane sits flush beside the sidebar. Empty-home pulse, usage/meta, and suggestion chips wrap to two lines instead of a one-line ellipsis. History titles still ellipsize only when the rail width forces it; hover shows the full title. Session pill label is **Questions** (same look-only mode). Same paint on Linux and Windows.

- Linux: `grokhub-linux-v2.10.7.tar.gz` and AUR `pkgver=2.10.7`.
- Windows: `GrokHub-Setup-2.10.7.exe` and `grokhub-windows-v2.10.7.zip`.
- Cursor cabin 2.10.7. VERSION 2.10.7.

## 2.10.6 — 2026-09-23

Cabin C chrome: shared explicit-yes confirm sheet (Ask Always, session Always, destructive host). Empty-home Coding / Life chip (default Coding). Titlebar Quiet until X when Behavior quiet hours are active. History Last you / fork branch map. Device glance only when hub share or a last frame is bound.

- Linux: `grokhub-linux-v2.10.6.tar.gz` and AUR `pkgver=2.10.6`.
- Windows: `GrokHub-Setup-2.10.6.exe` and `grokhub-windows-v2.10.6.zip`.
- Cursor cabin 2.10.6. VERSION 2.10.6.

## 2.10.5 — 2026-09-23

Empty-home cabin pulse (signed-in, not Scratch) sits under the greeting: next job or Morning brief seed, pinned goal, up to two open workboard titles, muted `usage_line`. No weather, mail, or calendar stubs. Ask-card Always is a second beat that names session-wide skip and that night / loop / phone inherit `--always-approve` until quit. Enter is still Allow, Esc is still Deny. Composer Always stays session-only.

- Linux: `grokhub-linux-v2.10.5.tar.gz` and AUR `pkgver=2.10.5`.
- Windows: `GrokHub-Setup-2.10.5.exe` and `grokhub-windows-v2.10.5.zip`.
- Cursor cabin 2.10.5. VERSION 2.10.5.

## 2.10.4 — 2026-09-23

Look is look-only on Auto/Always as well as Permission Ask. Headless Look uses `grok -p --permission-mode ask` and does not inject `CABIN_DESKTOP_RULES` / desktop-do-the-work. Plan stays plan. Chat stays chat. Night, inbox, and anticipate inherit `scheduled_args` like loops: scheduled Ask skips ACP and does not silent `--always-approve`. Night marks the slot ran after a live kick, not before; a send that never starts a kick is skipped so the slot does not retry every 5s. `scheduled_perm` clears when the chat turn ends so the next typed Ask uses ACP. Composer Ask leftover flags match scheduled Ask (no yolo). Ask ACP deny and fatal `AcpEvent::Err` resume PTT. Ask fail-closed copy names Install Grok Build CLI / Start agent in Settings → Update. `AcpHandle` drop backtraces stay behind `GROKHUB_ACP_DROP_TRACE`.

- Linux: `grokhub-linux-v2.10.4.tar.gz` and AUR `pkgver=2.10.4`.
- Windows: `GrokHub-Setup-2.10.4.exe` and `grokhub-windows-v2.10.4.zip`.
- Cursor cabin 2.10.4. VERSION 2.10.4.

## 2.10.3 — 2026-09-23

Always idle matches Ask/Auto (no amber). Selected Always is a 2px amber stroke only — dark `#E8A838`, light `#B86E00` — same elevated fill as Auto, no yellow wash or text. Weight is the 2px ring plus selected type. Risk tip stays. Hi-fi pass: canvas / elevated / hover, 1px borders, soft card elevation, Fluent 16/20 icons, Inter 16 / 13 / 12, ~120ms motion, session row quieter than permission. ACP Ask, Look, Enter/Esc, and redaction are unchanged.

- Linux: `grokhub-linux-v2.10.3.tar.gz` and AUR `pkgver=2.10.3`.
- Windows: `GrokHub-Setup-2.10.3.exe` and `grokhub-windows-v2.10.3.zip`.
- Cursor cabin 2.10.3. VERSION 2.10.3. Not tagged until MERGE GREEN.

## 2.10.2 — 2026-09-22

Settings → **Update** stays visible when the 2-hour probe found nothing. A click still overlays CLI then cabin so a missed GitHub Latest or alpha check can land. The titlebar chip still hides until something is newer. Same on Linux and Windows.

- Linux: `grokhub-linux-v2.10.2.tar.gz` and AUR `pkgver=2.10.2`.
- Windows: `GrokHub-Setup-2.10.2.exe` and `grokhub-windows-v2.10.2.zip`.
- Cursor cabin 2.10.2. VERSION 2.10.2. Not tagged until MERGE GREEN.

## 2.10.1 — 2026-09-22

Cabin UI pass on Linux and Windows. Session pill Ask is now Look. Permission Ask / Auto / Always keep their names. Always uses an amber danger stroke and does not persist. Voice strip is Listening / Speaking / Ready and hides about a second after Ready. Empty-home chips keep one primary (fill + stroke + weight). When ranking yields none, one muted **Nothing queued** placeholder stays — no filler action chips.

- Linux: `grokhub-linux-v2.10.1.tar.gz` and AUR `pkgver=2.10.1`.
- Windows: `GrokHub-Setup-2.10.1.exe` and `grokhub-windows-v2.10.1.zip`.
- Cursor **#60**: chips, Look, Always tone, voice Ready. VERSION 2.10.1. Not tagged until MERGE GREEN.

## 2.10.0 — 2026-09-22

Ask is a fail-closed ACP gate. Permission Ask starts `grok agent stdio` so Allow / Deny can show. If ACP cannot start or dies, the turn is denied. It does not fall through to headless `grok -p --sandbox off`. Auto/Always stay on `grok -p`.

- Linux: `grokhub-linux-v2.10.0.tar.gz` and AUR `pkgver=2.10.0`.
- Windows: `GrokHub-Setup-2.10.0.exe` and `grokhub-windows-v2.10.0.zip`.
- Cursor **#54**: Ask calls `ensure_acp`; ACP down denies the turn. Night, loops, tray, and secrets are unchanged.

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
