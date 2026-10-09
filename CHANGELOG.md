# Changelog

## Unreleased

- Long tasks no longer stop at 60 steps, 30 minutes or 50 turns: a desktop session, a chat, `/bg` and night jobs run until they're done, you Stop or Halt. A run that's stuck or repeating itself is told to re-plan and keeps going instead of pausing, and approving a card after the reply ended now picks the task back up instead of waiting for you to type.
- Bug hunt (`docs/audits/bug-hunt-2026-10-08.md`). The hard-action check no longer misses a delete or shutdown after a newline, inside `$(...)` or backticks, or behind `env`, `time`, `nice`, `timeout`, `pkexec` or `busybox`, and it now catches MCP tools named like `reply`, `forward`, `sendMessage`, `send_sms`, `post_tweet`, `create_charge`, `buy`, `transfer` or `drop_table`. A one-time approval no longer covers a longer command that contains it. Step logs no longer keep `Authorization` headers, `x-api-key`, access or refresh tokens or client secrets. Long host output, the nightly review and the step log no longer crash on accented or CJK text at the size cap. A permissions file saved with a BOM keeps its Deny rules, and a file that can't be read is never overwritten. Screenshots work again on GNOME with a non-ASCII Pictures folder. On Windows, the maximize button restores again. Quit from the tray no longer hangs on a busy MCP server, and a clean exit no longer leaves a pid file that can stop the next launch. A failed settings or key save now says so instead of "Saved".
- Dream time is a setting now (Settings → Behavior → Dream time). Dream, the nightly review and the Sunday self-review run at the hour you pick instead of always at 9 PM, and moving it after tonight's run doesn't run them twice.
- Settings → Cabin defaults has an "On-device model for background tasks" switch (off by default). Turned on, background work like dream, digests and summaries can run on your own machine while chat stays on your cloud model, and it takes effect without a restart. Until a local model is installed the row says "No local model installed" and background work keeps using the cloud.
- A short setup wizard greets a fresh install: welcome, two app settings (close to tray, dream time), then an on-device model step that checks your GPU, memory and free disk and says which model fits, how big the download is and where it will go. Skip any step; it won't open on its own again, and Settings → Behavior → Setup (or Set up next to the on-device model switch) brings it back. Downloading the model comes in a later update.
- The setup wizard's on-device model step can now download the suggested model (Qwen 2.5 from Hugging Face) into `~/GrokHub/models`, with a progress bar, Pause, Cancel and Resume (it picks up where it stopped, even after a restart), and a checksum check before it counts as installed. GrokHub can't run the model yet, so background work stays on your cloud model and Settings says so.
- Remote MCP servers that need a sign-in work now (Settings → Labs → Native MCP). Each remote server with no credentials of its own gets a Sign in row: GrokHub finds the server's sign-in page, registers itself, and opens your browser; when you approve, the row says "Signed in to Linear as ada@example.com" and the server connects. The token is sealed on this device with your keyring key, is refreshed before it expires, and Sign out removes it. A server whose entry already has its own `Authorization` header or saved token keeps using that.
- Adding your own AI provider (Settings → Cabin defaults) now asks before it saves the key: a credentials card says "Save your key for openrouter.ai to your keyring", and Esc drops the key. A new "Provider type" row lets you mark a proxy as OpenAI- or Anthropic-compatible when its address doesn't say. "Get a key" opens the key page for OpenAI, Anthropic, OpenRouter, Groq, Mistral or DeepSeek, and "Sign in with OpenRouter" gets a key through your browser instead of copy and paste. After the models refresh, a quiet line names what came in ("Added 2 models from openrouter.ai: …") or says the provider turned the key down.
- The suggestion chips under the chat box sit centered on it at every window width and display scale, instead of hugging its left edge. "Nothing queued" is centered too.
- `/memory dream` and the weekly self-review cards open with a short "What changed" block: how many notes or steps were added, merged and retired or removed, then the top three by name ("Merged: \"uses pnpm\", \"likes dark mode\""). The full report follows below it.
- When something outside GrokHub kills a run (exit 143) and the automatic retry doesn't finish it, a card now says so by name: "Crashed: Index ~/Projects (exit 143, killed)", with Open (its chat) and Retry. Before, it only flickered in the status line. Stop from GrokHub and a retry that finishes post nothing.
- `/health` answers in the chat with one block: pending GrokHub and Grok Build CLI updates, each failed check, automation or MCP server by name, and when the last dream ran and what it merged and retired. Each part says so when there's nothing to report ("Updates: none pending.", "Services: all 6 ok."). It used to jump to Settings and put the doctor checks in the status line.
- Record my screen: `/record what's wrong` (or "Record my screen" in the palette) takes a still every 2 seconds for up to 2 minutes, with a red "● Recording 0:12 of 2:00" indicator and Stop on top of every page. The stills stay in `~/GrokHub/recordings`. On Stop, six of them and your note go to Grok once, and the report lands in the chat and on a feed card that names the recording ("Screen recording 0:42: video stutters on 4K YouTube"): what it saw, the likely cause and steps you can take. Nothing is changed on the computer. Delete recording on the card asks first. It's off until you turn on Settings → Cabin defaults → Screen recording.
- Check my audio: `/audiocheck` (or "Check my audio" in the palette) lists your outputs and inputs, listens to the default input for 5 seconds (`/audiocheck yeti` picks another), and says what's wrong by name: muted, volume too low, silent, clipping, too quiet, dropouts or crackle, plus the default output ("Audio check: Yeti Stereo Microphone input clipping at -0.2 dBFS; default output is HDMI"). The report lands in the chat and on a feed card with fixes to try. Nothing is changed, and the clip stays in memory, never saved or sent.
- Quiet self-review: a new switch in Settings → Behavior makes the Sunday self-review log its suggestions (skill drafts, fixes, reverts, router tuning and the weekly router line) instead of posting Pulse cards. Settings shows the five newest under the switch ("2026-10-11 · Make a skill: deploy: You did this 4 times in the last two weeks"). It's off by default, and nothing changes when it's off.
- When Auto's self-tuning promotes a model or a start level, Home shows one info card that names it: "Why grok-4-fast for everyday chat: 92% pass vs 85%, promoted Oct 6", with how much cheaper and faster it ran in the canary. It asks nothing and replaces the plain "Router changed router tuning" note for promotions. Router messages no longer point at an Undo that the change list doesn't show; they point at `/why table`.
- Memory retention is its own setting now (Settings → Behavior → Memory retention: 30, 60, 90, 180 or 365 days, or Keep everything; 90 by default, as before). Once a day, at any hour and whether or not dream runs, unsure memory notes nothing links to that weren't updated in that long are retired (marked forgotten, still on disk). `/memory prune` runs it now and says how many it retired and kept. Memory repo mode only.
- Desktop control has five new tools, matching the Desktop app. `watch_path` starts watching a file or folder (four levels deep, up to 16 watches), `watch_events` lists what was created, modified or deleted since the last look, and `unwatch_path` stops it. `get_window_geometry` reads a window's size and position and `set_window_geometry` moves or resizes it (X11 and Windows; KDE Wayland can set but not read). Moving a window is held while the desktop is locked or halted, like other input, and needs no new approval.
- A desktop session no longer stops and waits when the final check says the task isn't done. It re-plans with the checker's reason and keeps going. A checker that can't be reached is tried again, then once on a stronger route, and only then the session re-plans with "checker unavailable". If the checker gives the same reason three times in a row and the screen hasn't changed, the session ends with a note naming the goal and the reason ("Couldn't confirm: …"), with no card and no pause. Hard approvals still park as before.
- A desktop session catches a loop sooner. Clicking the same button that fails twice, or the same button that changes nothing three times, or hitting the same error three times, makes it re-plan right away instead of after eight idle steps (that backstop is now five). After any re-plan the next three steps quietly run one model and one effort step up, then drop back, with no card or prompt. Goal follow-ups no longer stop after four steps: they go on until the goal is done or blocked, or you Stop or Halt.
- A desktop session that had to recover now writes one short lesson when it ends: what failed, what got it past that, and which app it was in ("Gedit: click 'Save' changed nothing 3× → then key 'ctrl+s'"). Lessons are written with no extra model call and stay on your machine under `lessons/`. A run that went smoothly writes none. Each night at dream time GrokHub merges duplicate lessons and drops ones older than 90 days. Loading lessons into the next session comes in a later update.
- Unattended Grok Build runs now block a delete or shutdown written in capitals (`REMOVE-ITEM`, `DEL`), called by full path (`/bin/rm`, `C:\Windows\System32\shutdown.exe`) or behind `cmd /c`, `powershell -Command` or `bash -c`, and they refuse encoded PowerShell (`-EncodedCommand`, `-enc`) outright; elsewhere encoded PowerShell asks first. Moving files to the Recycle Bin on a network or removable drive now says on the card that they will be deleted for good.
- Cleanup, no behavior change: removed 29 functions and constants nothing called, folded duplicate text-clip, `now_ms` and time-ago helpers into one copy each, dropped the unused `base64` dependency, and fixed two broken links in `docs/REFERENCE.md`.

## 2.10.98 — 2026-10-08

- Security: a phone, or a computer paired under another computer's name, can no longer take over a session through Inhabit. Phones can't pair, a bundle goes only to the computer it names by id, and a bundle with no destination goes to nobody. Risky typed text on a hard card and in the inbox no longer shows API keys, GitHub tokens or bearer tokens; other typing shows as "type N chars into <window>".
- The Home suggestion for a paused job now names it ("Paused: Fix the tray icon") and says how long it has been paused. Clicking it opens that job's chat, or the Workboard when the job has no chat, instead of a new Discuss chat about "that job".
- Speed spans on the auto router, the harness guards and the send path, a speed bench (`speed_bench`) and `docs/audits/speed-2026-10-08.md`. Each route record now carries how long the router's decision took (`timing_us`). Nothing else changes.
- Sending a message is faster: the router no longer re-reads up to 4 MB of the model-call log on every new message, every Grok Build turn or every desktop step. It follows the log and each chat's span file in memory and reads only what was added, and the week's spend is read without holding its lock. On the bench, Enter to request went from 28 ms to under 1 ms (p50), and a Grok Build send no longer stalls the window for about 30 ms. Approvals, guards and cost rules are unchanged.

- Linux: `grokhub-linux-v2.10.98.tar.gz` and AUR `pkgver=2.10.98`.
- Windows: `GrokHub-Setup-2.10.98.exe` and `grokhub-windows-v2.10.98.zip`.

## 2.10.97 — 2026-10-08

- Chat no longer fails with HTTP 400 "does not support parameter reasoningEffort": GrokHub sends a thinking level only to models that take one (Grok 4.7, 4.6, 4.5 and 4.20 multi-agent), never to a non-reasoning model. A model you picked that xAI lists under another name, or that isn't listed but answers, stays in use, and the false "Paused: no model in your plan is answering" card is gone.
- Labs → Beta moves a main install to beta again: the switch no longer stops on GrokHub's own `Cargo.lock` change, and Beta stays on after a sync.
- KDE screenshots ask KWin once: after you refuse, GrokHub stops asking and uses the fallback, and the Arch packages' menu entry starts GrokHub by its full path.
- A model change the router makes on its own is now a note that tells you, not an Undo / Keep row that asks.

- Linux: `grokhub-linux-v2.10.97.tar.gz` and AUR `pkgver=2.10.97`.
- Windows: `GrokHub-Setup-2.10.97.exe` and `grokhub-windows-v2.10.97.zip`.

## 2.10.96 — 2026-10-08

Chat on the native engine works again: GrokHub sent `connection_add` and `connection_disable` to the model twice (once from Spike-5b's connection tools, once from Spike-5c's self-manage tools), and the API refused every turn with "Duplicate function definition provided". The self-manage table is now the only source, and a native engine error no longer reads as "ACP session/new failed". When Update stops because new files in your GrokHub folder are in the way, it now says so instead of asking you to commit or stash.

Fork is removed. The "Long thread" and "Context is half full" offer under the chat, its Fork button and the "How fork works" card are gone, along with `/fork`, the Fork button on native History rows and the Branch markers on the History page. The context usage bar stays, and `/bg` still runs on its own copy of the chat's session.

Router R3b (other providers). You can add your own key for another AI provider, OpenAI-compatible like OpenRouter or Anthropic, under Settings → Cabin defaults → Add a provider. It is off until you add a key and allow the destination: the key goes into your OS keyring (never a file, a log or the model), and allowing it is a hard send card (click only, Esc denies) with Revoke under Settings → Permissions → Other providers. GrokHub then lists that provider's models and uses one only when you pick it under "Use for your chats", never on its own, never as a fallback and never to save money; sensitive data never goes there without a grant that names it. Every call to it is in the egress log.

Router R3a (learning). The router now tunes itself from how real tasks went, and asks only before changes that cost more money. A task you've done well three times at a lower thinking level in the last two weeks starts there next time (never for hard actions, never right after a rejected check). Cheaper or faster settings for a kind of step are tried quietly first, then on a small share of low-risk steps, kept only if quality holds, and rolled back at once if quality drops; every change is on the change list with Undo. A change that would cost more is a weekly suggestion card that waits for your Accept. The weekly review gets one router line, and `/why table` shows what's tuned and what's being tried. Plumbing for an on-device model is in but switched off, with no runtime and no way to turn it on yet.

Router R2b (cost classes). Auto now knows what each model costs you. Models in your plan, and API-key models under your price limit (Settings → Cabin defaults, $15 per million by default), are used freely. Grok 4.7 Fast is used on its own only while you wait on a reply, a long chain or a task about to hit its time limit, never for background, automation or proactive work, and never with "Use Grok 4.7 Fast when you're waiting" turned off; when the week is tight it uses Grok 4.7 instead. A model over your price limit asks once on a hard money card (click only, Esc denies); the approval is listed under Settings → Permissions → Paid upgrades with Revoke. GrokHub never buys credits or opens a top-up page. With a weekly spend cap set (API key) or your plan's usage limit hit, background work moves to cheaper settings at 80% (one card a week), pauses at 100%, and chats ask before going on; background work stays under 15% of the cap. If your plan's models keep failing big tasks, one card at most every 30 days says a bigger plan includes more capacity; "Not now" mutes it.

Router R2a (model healing). Auto now picks the model too, from a routing table GrokHub builds from your plan's models and a small fixed check per kind of step (`/why table`, Settings "How Auto picks"; every new table can be undone). A model that keeps failing rests and comes back after a short check; a model you picked is kept while it answers, and while it rests GrokHub uses the closest working model and says so once. Retired, missing or redirected models and plan changes get one Home update each, a new model in your plan gets one, and when nothing in your plan answers the step pauses with a card instead of failing. Settings tags models that aren't in your plan or aren't answering and hides retired ones, with a Refresh models button.

Router R1 (automatic effort). GrokHub now picks how hard to think on every step by itself: low for routine work, higher for hard work, one step up after a failed tool, a rejected check or a correction, and back down after clean steps. The effort dropdown, the Settings effort row and `/effort` are gone (`/effort` now says effort is automatic; a saved effort is dropped once). A read-only "Auto · Medium" chip shows the current level; hover for the reason, click for `/why`. Say "think hard" or "keep it quick" to steer one task. Background work keeps the same effort as before, preparing a hard action never goes below High, and the model picker is unchanged with Auto first.

Router R0 (watch only). GrokHub now keeps its own list of which Grok models exist, which your plan or key can use, and which are healthy (`models/registry.json`, refreshed 30 seconds after start, every 6 hours, and when your sign-in or Grok Build version changes), builds a profile for each new model with a small capped check (only when it's included in your plan), and logs on every model call which model and effort it would have picked and why. Nothing changes yet: calls still use your model and effort. `/why` shows the last 10 reasons and `/why models` lists each model's state.

- Linux: `grokhub-linux-v2.10.96.tar.gz` and AUR `pkgver=2.10.96`.
- Windows: `GrokHub-Setup-2.10.96.exe` and `grokhub-windows-v2.10.96.zip`.

## 2.10.95 — 2026-10-08

Fix it (Spike-9). After a diagnose finds something, GrokHub shows one card per fix with what's wrong, what it will do, why, how to undo it and the risk. Fix it (a click, never Enter) backs up every settings file the fix touches, takes a system snapshot when snapper, Timeshift, btrfs or System Restore is there, then hands each step to Grok Build one at a time; deleting files, removing apps or drivers, boot and partition changes park a hard card, and wiping a disk is never done in the app. Your computer's own password prompt is yours to answer. GrokHub checks again and only says fixed when the problem is gone; Undo fix puts the files back exactly. Fixing never runs unattended.

Self-improvement (Spike-7). Each finished task leaves one outcome record (`outcomes.jsonl`: success on VERIFY_OK or no undo in 24 hours; failure on a detector finding, a deny, an undo, a correction, or the retry ladder running out). On Sunday night after the review hour, a quiet pass posts at most five Pulse Suggestion cards with numbers ("3 of 5 weekly-report runs needed a fix") and a diff: skill changes, drafts for tasks done the same way three times in two weeks, and "Revert <skill> to v<n>?" when a change made a skill worse. Apply and Revert are click-only and go through the change ledger, and every skill patch, nightly or weekly, applies only when a dry-run replay of the skill's last recorded run passes. Proposals aimed at policy, consent, egress, the hard-class list, Access or the gate are refused with a finding.

Done for you (Spike-6b). GrokHub may now do a small, safe, undoable thing on its own (today: turn off a connection) when every term of the autonomy ceiling holds: soft and undoable through the change ledger, inside a scope you granted, Access Supervised or Full, the pill on Auto or Always, MindCheck sure you won't mind (`p_mind` under 0.2 with history), confidence at least 0.8, not quiet hours, busy or halted, and at most 5 a day. It still goes through `harness::decide` and the normal native step, then posts a Done for you card on Home and Pulse with Undo and "Don't do this again" (click only, white accent). Undo puts it back exactly; "Don't do this again" means it always asks for that kind of thing. A send, payment, delete or secret is only prepared and waits on a hard card, even under Always and Full, and an unattended one times out to Deny.

Beta fixes where today's spikes meet: a file manager Delete key spelled `Shift_L+Delete` or `shift+shift+del` now parks the hard Delete card like `Delete` does; text typed through the Cua driver (`type_text`, `set_value`) is logged as its length only; long desktop sessions get the same network ports, origin tag and workspace permission rules as a normal turn, and a read the rules deny no longer slips through a fan-out; Stop or Halt during a session's model call ends the session and denies its parked steps instead of showing a "cancelled" error; a diagnose answer goes to the chat that asked even after you switch chats; and a failed update, package or Windows Update check says it couldn't check instead of "up to date".

More beta fixes: an old phone row in `hub-state.json` is dropped on load, so its token no longer opens the snapshot, frame or inhabit routes; a drag that lets go on Send, Pay or Delete parks like a click; a desktop session that ends with a step still parked denies it instead of leaving an Approve that does nothing, and approving one with no turn running says to send a message; the session goal and Steers reach the worker and the checker in full (spans still keep 200 characters); the Work-tree step count reads only new span lines; diagnose leaves messages about code (file names, paths, "module") to the model; memory repo mode brings back a merged line you remember again, keeps one of two notes after an edit back, lets the dream tidy notes a trail mentions, does not treat a dream-retired copy as forgotten, keeps a MEMORY.md line the store refused, links learned facts to their own chat's turn, and retries a failed import; step search keeps one row per chat when titles repeat; the Cua proxy hands a late-started child the MCP handshake; Stop and Halt end an HTTP MCP send waiting on its card; indexer excludes ignore case; the egress log reads `\` as the end of a host; and stopping a monitor always reports it cancelled, never "ended (killed)".

Proactive cards (Spike-6a). GrokHub now notices what you might need (a blocked or fresh board card, a failing automation, a routine, and calendar or mail only with your grant) and offers it as an "I can …" card on Pulse and Home, at most 3 per 4 hours and 8 a day, none in quiet hours (queued until they end) or while you are busy. Not this mutes a topic for 7 days, and 2 dismissals in a row halve the budget for a day. Nothing runs without your click; a hard step such as sending is only prepared as a draft, and Send… parks the hard card even under Always and Full. No new pages or chrome.

Grok manages itself (Spike-5c). A new `grokhub-self` tool server, registered in the cabin's own Grok home, lets Grok list, create, change, turn off and remove its skills, connections and automations; the native Lab engine gets the same tools. Creates and changes are logged with Undo and follow your permission pill, deletes and token-needing connections always wait for your click, a token is typed on a card and never reaches Grok, logs or the change ledger, and nothing can touch consent, egress, Access or harness policy. Changes the server makes show as Work-tree rows while the cabin is open. At most 5 new skills a day and 2 new automations a week. No new UI.

Diagnose (Spike-8b). Tell GrokHub "something's wrong with my computer" (or "my wifi doesn't work"), type `/diagnose`, or let a native thread call the `diagnose` tool: it checks disk, memory, services, logs, network and updates with a fixed list of read-only probes (no shell, 10 s timeout, output redacted) and answers in plain words. Needs System state in Settings → Permissions; without it nothing runs. Nothing is changed on your computer. No new UI.

Trust floor, finished (Spike-4c). Every call GrokHub itself makes now goes through the egress guard and shows up in `/privacy`: Labs web fetch (each redirect too), HTTP MCP servers, Imagine, the plugin index, Pulse previews, the GitHub tool, update checks, cited-link checks, sign-in, and local browser control (loopback, not logged). Memory sent to a place you haven't granted waits on a hard Send card and nothing goes until you click Approve. Spans and send-log lines say who started the step (you, a heartbeat act, or a scheduled job), and cabin model calls log their tokens and cost. A coverage test fails on any new fetch that skips the guard. Grok Build's own traffic stays outside, as before. No new UI beyond new `/privacy` rows. No version bump.

User model core (Spike-5a, only with `"memory_backend": "amr"`): memory nodes gain `routine`, `need` and `mind_prior` types plus optional `consent_ref` and `sensitivity` lines (old nodes read as before); rule-based writers turn chat notes, corrections ("no, I meant …", "9:30, not 9", which supersede the old note), edits, Pulse ledger lines, card signals, skill runs and automation outcomes into nodes, skipping any line with a secret, password, PIN or one-time code; `amr/index.sqlite` is a rebuildable FTS5 index of plain notes; forget also leaves the index and the hub snapshot; reflect returns a diff instead of writing; and the harness keeps a MindCheck prior per action (a deny or Undo sets 0.6 and asks first for 30 days, each approve lowers it 0.05, a dismiss raises it 0.1, at or over 0.2 asks) that never touches hard cards. An agent-started forget is a hard delete card. Nothing acts on the prior yet, and the `/memory` rows with Edit and Forget buttons come in a follow-up. No UI changes.

Spike-3a: History and palette search now find tool steps too ("click OK" finds the click), shown as chat · turn · tool · decision, and Enter opens that chat at that turn's Work card. It reads the existing span files (newest 200 files, 20,000 lines, 40 rows) and works in either memory mode. With `"memory_backend": "amr"`, each turn with desktop or tool steps also leaves one trail note in `amr/` (tool counts, decisions, findings, verify result; typed text only as its length), sealed when the turn touched a hard-class step; `/recall` finds it. Scratch chats write no trail. No new pages or chrome.

Local indexers (Spike-8a). Once you allow a scope in Settings → Permissions or on an inline ask card, GrokHub learns from it on this computer: one folder's files (names, dates, titles and headings), installed apps, one browser's visited sites (by host, from a read-only temp copy of the history table only), and disk, service, log and update counts. It reads one scope per heartbeat on a low-priority thread, never on battery, in quiet hours, while halted, or while private data is locked; keys, password stores, cookies and logins are never read. What it learns is sealed in the memory repo and listed under each row with "Forget these"; a revoke stops it on the next tick. Calendar and mail stay grant rows only. Nothing is sent anywhere.

Memory repo M3 (only with `"memory_backend": "amr"`): once a night after the review, GrokHub tidies `amr/` with no model call. It merges near-duplicate notes (a `supersedes` edge and a tombstone), retires old low-confidence notes nothing links to, never deletes a file or edits USER.md or SOUL.md, skips private notes it can't unlock, and writes a plain report to `amr/dreams/<date>.md`. `/memory dream` shows the latest one. Halt skips the night. `/dream` (Imagine) is unchanged. No UI changes.

Memory repo M1–M2 (only with `"memory_backend": "amr"` in `app.json`): reflect, chat insights, `/memory note` and `/remember`, Memory Save and native `/remember` now write one node each in `amr/` instead of a MEMORY.md line; `/recall`, History and the native first-turn pack read `amr/` and the old files together; `/forget <topic>` leaves a tombstone; and the first AMR use imports learned insights and chip preferences once, with a report in `amr/dreams/`. Legacy is unchanged. No UI changes.

Self-management ledger (Spike-5b). Connections and automations GrokHub adds, changes, or removes on its own now keep the version they replace, show a Work-tree row ("Grok added connection notes") with Undo and Keep, and appear in the next Home update; `/connections changes` and `/automations changes` list them. Agent-made connections land in the cabin's own MCP config (never `~/.grok`), deletes and token-needing connections are hard cards, a token is typed by you and sealed with your keychain key, and GrokHub adds at most 2 automations a week on its own until you keep one.

Removed the scrapped phone/Android pairing: the phone-only hub routes (`/v1/task`, `/v1/task/:id`, `/v1/inbox`, `/v1/results`, `/v1/frame.jpg`, `/v1/voice/client-secret`), the `grokhub-ffi` C ABI crate, and phone wording on Devices, slash help, and the Always sheet. Computer-to-computer pairing, `/sync`, `/inhabit`, `/send`, and desktop voice are unchanged, and an old `hub-state.json` with phone rows still loads. No version bump.

Safety loop (Spike-1a). The harness now catches a click that changed nothing, a step repeated with no effect, "done" with no check, and a claim no step backs; it retries once, backtracks, then pauses for you (hard actions never retry), and typing into a password, PIN, OTP, 2FA or verification-code field is a hard credentials action whose value never reaches spans or logs. No UI changes.

Decision inbox and real desktop actions (Spike-1b). "N things need a decision. Everything else is on track." now opens into one row per waiting decision on Home and the Workboard, each with what Grok wants, the chat it is from, and Approve/Deny (hard rows need a mouse click, Esc denies, Halt denies them all); Grok can open an app, focus a window and type with `ui_changed` checked, and any delete (rm, del, Remove-Item, trash, the Recycle Bin, or Delete in a file manager) parks a hard card naming the exact paths.

Grok Build's own computer use is checked too (Spike-1c, path D). On Auto or Always, and whenever desktop control is off, every Grok Build run denies its own screen, mouse and keyboard tools so desktop work goes through the cabin's gated desktop tools; a computer-use step that still runs without an ask is logged, a hard one (send, pay, delete, credentials) stops the turn and shows "Grok tried to … without asking" with Approve / Deny, and with desktop control off it just stops the turn.

Windows gate fixes (Spike-1W, code review only; the live Surface run is still to do). Ctrl+Alt+Delete and the other session-ending keys park in any modifier order or spelling (`Alt+Ctrl+Del`), a PowerShell delete behind `-ExecutionPolicy Bypass` parks, and `Stop-Computer`, `Restart-Computer`, `Format-Volume`, `Clear-Disk`, `format` and `Clear-RecycleBin` park as hard actions on every path, unattended runs included. No UI changes.

Cua Driver as an optional second pair of hands on Linux (Spike-2a). Behind a `cuaDriver` flag in `app.json` that is off by default (no Settings control), `grokhub --mcp-cua` starts the pinned MIT cua-driver-rs 0.34.0 in bounded mode with a cabin manifest and puts every Cua call through the same approval gate as the desktop tools: hard actions park the same card, Halt kills the driver, and spans say `driver:"cua"`. The agent cursor marker on the Work-tree frame now travels by one of Cua's six motion styles (signature arc, the default, which keeps today's look), and reduced motion snaps on the first frame.

Clicks are checked by what they hit (Spike-2b). Before a desktop or Cua click runs, the cabin reads the control under it (AT-SPI on Linux capped at 300 ms, Cua's accessibility tree; unknown on Windows) and a Send, Post, Reply, Upload, Pay, Buy, Place order, Checkout, Delete, Remove, Empty trash, Reset, Wipe or Format button parks the same hard card, now worded "Grok wants to click Send in <window>", even under Always or Full; look-alikes such as "Sender", "Payload", "Removed items" or "Reset zoom" stay soft. Spans add `target` and `target_rule` (the rule, never the label), a denied hard step in a native-engine batch marks the rest "Not executed: earlier action failed", and once desktop and MCP tools pass 40 together the native engine switches to `search_tool` / `use_tool`, where a hard tool still parks.

Long supervised desktop sessions (Spike-3b). A message you type while desktop control is on starts one desktop session per chat: every step, Steer and pause shares one episode id in the trace, the Work tree heads it "Desktop session · 12 steps · 4 min" above the last frame and cursor, and it pauses after 60 steps or 30 minutes with a Continue / Stop card that counts in the needs-attention line (nothing continues on its own). On the native engine each step starts the model fresh from a 48,000-byte episode view whose older steps fold into short summaries (`zoom` opens them), hard steps still park, and the session only finishes when an independent check that sees just the goal and the final screen says VERIFY_OK. Halt, Stop or 10 idle minutes end it.

Release bump list: the version strings that were in the old README (headline, both Latest rows, `--version` examples) now live in `docs/REFERENCE.md`, so `CLAUDE.md`, `.cursor/rules/repo-gates.mdc` and the versions compass list that file instead. `README.md` has no version strings. Docs only, no version bump.

- Linux: `grokhub-linux-v2.10.95.tar.gz` and AUR `pkgver=2.10.95`.
- Windows: `GrokHub-Setup-2.10.95.exe` and `grokhub-windows-v2.10.95.zip`.

## 2.10.94 — 2026-10-07

Channel fixes: Labs Beta auto-off works after a promote and really switches back, Update and channel-switch errors say what went wrong, and Windows no longer trips over a beta receipt.

- **Auto-off compares code, not commits (Linux).** Main moves by squash and the main → beta sync always adds a merge commit, so the two tips never matched and Beta stayed on. The cabin now fetches `beta` and `main` from your clone's `origin` and treats beta as caught up when `origin/beta` and `origin/main` have the same tree (the same tip commit still counts). Checked at the same times as before: opening Settings → Labs (at most once a minute) and after a successful Update.
- **Auto-off really switches back.** It checks your clone out on `main` at `origin/main` and then writes `stable`, so the next Update pulls and builds main instead of failing with "source clone is on beta". No rebuild happens at that moment: the code is already the same. If the clone has uncommitted changes, is on another branch, or has a local `main` commit that isn't on `origin/main`, nothing is touched, Beta stays on, and the status line says why. If the fetch fails (offline, timeout, missing branch), Beta stays on and nothing is shown. Re-enable Beta anytime.
- **Update errors name the cause.** A failed Update or channel switch now reads the real output and says which it was in plain words, with the command in parentheses: unsaved code changes in the source folder, Rust too old (`rustup update`), source folder not found, the folder on the other branch, or a build failure. The status ends with "Details:" and the path of the new `update.log` in the config folder. Each attempt (time, channel, the commands, and the last 60 lines of output with secrets redacted) is appended there; the log rotates once at about 256 KB. `grokhub --update` logs too.
- **Windows: beta is Linux-only.** A leftover `beta` receipt on Windows reads as stable: Update runs as stable instead of failing and says "Beta is Linux-only for now, so GrokHub updated to stable." The Labs toggle stays disabled with its note.
- **Cold builds get time.** A channel switch may run up to 40 minutes (was 15) with the progress bar still showing; other Update steps keep 15.

No new crates, no new network destinations. No version bump.

Heartbeat throttle. The pulse still wakes every 15 seconds, but the things it starts on its own (anticipating a need, the automatic ideas ask, the nightly review) now share a budget, so it can't burn tokens or keep nudging you. Approval rules and hard cards are unchanged.

- **A budget for proactive acts.** At most one every 15 minutes, 3 an hour and 8 a day by default. To change them, edit `"heartbeat"` in `app.json` (`minIntervalMin`, `maxPerHour`, `maxPerDay`, `backoffAfter`, `backoffMaxMin`, `haltHoldMin`); `maxPerDay: 0` turns proactive acts off. There is no Settings control. A file without that key keeps the defaults and doesn't grow one.
- **Backs off when it isn't helping.** After 3 acts in a row that came back empty (no new ideas, an empty review, an anticipate turn nobody answered within 15 minutes) or that you dismissed (Stop on its turn, Dismiss or Not this on a Pulse card), the gap doubles each time, up to 4 hours. One useful act brings the normal pace back.
- **Waits for you.** Nothing proactive starts during a reply, with text in the composer, or while a card is waiting on you. Idle reflect waits too.
- **Halt stops it at once.** Tray Halt and the halt hotkeys hold the pulse for 15 minutes or until you send a message. Only local upkeep runs in that time: no anticipate, ideas, review, digest lookup, living wall, phone inbox or scheduled job starts, and an ideas or review reply already on its way is dropped.
- **Scheduled jobs are unchanged.** Automations and loops keep their own clock and daily cap, outside this budget, and still run as background runs that never take the composer.
- **Traced without content.** Each decision goes to `spans/heartbeat.jsonl` as allow or hold with a reason (`busy`, `min_interval`, `hour_cap`, `day_cap`, `backoff`, `halted`, `off`). It never includes prompts, replies or chat ids. A repeated hold is written once.

No version bump.

Scopes and locked-state polish (Critiquito SB-01 to SB-11). The consent rules are unchanged: grants still come only from a mouse click in Settings, `harness::decide` and the fail-closed rules are untouched, and a hard card still has no Always.

- **Locked means locked (SB-01).** While private data is locked (no keyring, missing or wrong key), every Allow in Settings → Permissions, the folder field, the browser picker and Choose folder… are shown disabled, and hovering one says "Locked: keyring unavailable" (or which lock it is). Revoke is disabled too, because a revoke is a ledger write and a locked ledger takes none.
- **A way out (SB-02).** The lock message now has one next step for your OS and a **Try again** button that asks the keyring again right away and refreshes the page. Linux: "Unlock or start your keyring (GNOME Keyring or KWallet), then Try again." Windows names Windows Credential Manager and macOS the macOS Keychain; neither mentions Secret Service. `/sync` and `/privacy` give the same step in short form.
- **Folder rows read by name (SB-03, SB-06).** A folder grant is titled "Files in notes". The hint shows the whole path cut in the middle, and hovering shows all of it. `/privacy` uses the same short name, with the path on hover over its Revoke row. You can allow several folders, one at a time: the row reads "Files in a folder", then "Add a folder".
- **Choose folder… (SB-04).** A button next to the folder field opens your system's folder dialog (on Linux through the desktop portal, with no GTK). Picking a folder only fills the field; Allow still grants. If no dialog can open (no portal or zenity), the row says so and asks you to type the path. Typing a path still works, and the muted placeholder matches your OS (`C:\Users\you\Notes`, `/home/you/Notes`, `/Users/you/Notes`).
- **`/privacy` lists each grant once (SB-05).** One Grants list: a bullet with its since-time for each grant that is on, then one "Off: …" line, then the screen setting. The duplicate summary line and the second heading are gone.
- **Calmer rows (SB-07, SB-08, SB-09, SB-11).** Each hint starts with On or Off in the brighter text colour. Allow is an outline button on the scope rows and Sync, so nothing on the page nudges you to grant. The intro says 'Screen access is "Let Grok control the desktop" in Settings → Cabin defaults.' The `/privacy` Revoke rows and the `/skills changes` Undo / Restore rows (SU-06) line up with the report text, their pills in one column.
- SB-10 (naming the calendar source and mail account) waits for the readers; there is a TODO in the code.

One small new crate: `rfd` 0.17 (and `pollster` 0.4), with only its `xdg-portal` backend on Linux, so no new system libraries and no CI change. No version bump.

Skill changes GrokHub makes on its own can be undone (harness design §12 P3, first slice of the Spike-5 ChangeLedger). `harness::decide` and the hard-card rules are unchanged.

- **Every version is kept.** Before the nightly review patches a skill, before a skill learned from a host run is written, and before cleanup moves a never-used skill aside, the current `SKILL.md` is copied to `changes/skills/<name>/` in the cabin config (the last 20 versions per skill). Each write adds one line to `changes/skills.jsonl`: skill, time, who (`self_manage` or `user`), a short reason, and the file hash before and after. The ledger holds no skill text, and secrets in the reason are redacted. Adding a skill from Suggested is logged too.
- **`/skills undo <name>`** puts back the version before the newest change, byte for byte, and logs the undo. Run it again to step back further. Undoing a skill GrokHub created removes its folder; its text stays in history. **`/skills restore <name>`** brings back a removed skill. **`/skills changes`** lists recent changes, with an Undo or Restore button per skill under the newest list. The buttons answer a mouse click only.
- **Only you undo.** Undo and restore run only from a line you type in the composer or a click. The same text from a night job, an automation, a phone task, a Pulse run, an idea, or a model reply only shows the list. A patch you undid is not applied again by the next nightly review.

No new crates, no network calls. No version bump.

Spike-4b trust floor (privacy and consent, second slice). The consent rules are unchanged: grants still come only from a click in Settings, and a hard card still has no Always, Enter does not approve it, and Esc or the timeout denies it.

- **Private data is encrypted on disk.** `consent.jsonl`, `egress.jsonl` (and its rolled `egress.1.jsonl`) and private AMR notes (`amr/nodes/<id>.sealed`) are sealed with ChaCha20-Poly1305. The key is made on the first private write and lives in your OS keyring (Secret Service on Linux, Credential Manager on Windows, Keychain on macOS); only a short hash of it is stored next to the data. Your existing plain-text ledger and send log keep working and are sealed in place, line by line, on the next write. Nothing is dropped.
- **Fails closed.** If the keyring can't be reached, or the key is missing or doesn't match, GrokHub says so in Settings → Permissions, `/privacy` and `/recall`, no grant applies, nothing new is saved, and nothing is ever written as plain text. `/sync` doesn't send, because it couldn't be logged. Chats with Grok still work; their send-log lines are skipped until the keyring is back.
- **Recall packs are masked.** Memory recalled into a native-engine prompt has emails, phone numbers, card numbers, SSN-shaped numbers and street addresses replaced with `[email]`, `[phone]`, `[card]`, `[ssn]` and `[address]`, on top of the existing secret redaction. Code, versions, hashes and `git@` remotes are left alone. Your own `/recall` view is not masked.
- **Settings → Permissions → What GrokHub can read.** One row per scope (files in one folder, installed apps, browser history, calendar, mail, system state), all off. Allow (filled) takes a mouse click only, so Enter or Space on a focused button never grants; Revoke is a ghost. `/privacy` lists them by name, and its Revoke works for them too. Nothing reads a scope yet. Allow on Sync to paired computers is now mouse-click only as well. A files grant also refuses any folder that holds your home folder, like `/home`.

No new crates (ring, zeroize and keyring were already in `Cargo.lock`). No version bump.

Slash results get their own style, plus two small fixes (Critiquito GL-05, SY-09, SY-10). The consent rules are unchanged.

- **Slash and system results keep their look (GL-05).** Older results, like the `/sync` line and the `/privacy` report, used to collapse under an italic "Thought process" header once something newer arrived, so app output looked like model thinking. They now stay in the normal reply bubble at every age and never get the thought label or its Collapse control. The pane tells them apart by the tag the cabin adds when it writes them, not by their text. Model reasoning still collapses exactly as before, and collapsing it no longer folds a result away with it. The ghost Revoke still sits under the newest `/privacy` report.
- **"chats and memory" in sentences (SY-09).** The `/sync` card body, the Settings note and the `/privacy` intro now read "chats and memory". Compact spots keep "chats, memory": the card command, the `/privacy` grant and egress lines, and the row hints. Logs and files still keep the `chat` / `personal` ids.
- **Settings → Permissions spacing (SY-10).** "Leaving this computer" and "Command rules" get 12px more space above them, at the same size, so each heading starts a new group instead of reading as another row.

No version bump.

Privacy and consent follow-ups (Critiquito SY-01 to SY-08). The consent rules are unchanged: grants still come only from a click in Settings, and a hard card still has no Always, Enter does not approve it, and Esc or the timeout denies it.

- **`/sync` with nothing paired** sends nothing, logs nothing and parks no card. It posts "Nothing paired yet. Start share to pair a computer." With at least one paired computer it works as before (the hard Send card when there is no grant), then posts "Synced chats and memory to N computers" in the chat instead of jumping to Devices.
- **The `/sync` card** on an empty chat now uses the same left-aligned card as the chat column (up to 520px wide, ragged-right text, buttons on the left), not a centered and justified one.
- **One name for the data:** the card, `/privacy` and Settings all say "chats, memory". Logs and files keep the `chat` / `personal` ids. `/privacy` calls the hub destination "paired computers".
- **`/privacy`:** under the newest report, each active grant gets a ghost Revoke button. Only a click can use it, and granting is still only in Settings. The grant id and the `egress.jsonl` name are gone (the heading is now "Sent in the last 7 days (no content stored)"), and a fresh report no longer adds "No grants yet." under the "off" line.
- **Settings → Permissions:** the trust note and the Sync row sit under "Leaving this computer", and the rules note sits under "Command rules" above Rule. Revoke is a ghost button; Allow stays filled.

No version bump.

Spike-4a trust floor (privacy and consent, first slice). Nothing new leaves this computer without your OK:

- **Consent ledger:** `consent.jsonl` in the config directory holds grants you make with a click. They are revocable, and slash text or the agent cannot write them (the hard floor refuses agent writes to the file). Learning scopes (files in one folder, apps, browser history, calendar, mail, system state) all start off, and nothing reads them yet.
- **Egress guard:** cabin xAI calls stay allowed by default. Any other destination that would get personal data parks a hard Send card: Approve or Deny only, Enter does not approve, and Esc or the 5 min timeout denies. `egress.jsonl` records host, data classes, node ids and the grant, never content or raw secrets.
- **`/sync` asks first:** syncing chats and memory to paired computers now parks that card unless Settings → Permissions → *Sync to paired computers* is allowed. Approve sends once. Revoking it drops the shared snapshot.
- **`/privacy`:** lists your grants, the scopes, and what left this computer in the last 7 days. It replaces the Grok CLI pager builtin of the same name.

Spans gain `origin` and `consent_ref`, and old span files still read. Grok Build's own traffic is outside GrokHub and is not guarded. AEAD at rest, PII redaction of recall packs, and per-scope Settings rows follow in 4b. No version bump.

Compass files: `docs/compass/` adds 14 short maps (25–35 lines each) for the crates and the modules agents get lost in (AMR, slash, the Spike-0 harness, the `app/` UI, `config.rs`, the desktop MCP, install scripts, versions and channels), indexed in `docs/compass/README.md` and linked from AGENTS.md. A new `compass_paths` test in grokhub-core fails when a compass file names a repo path or identifier that no longer exists, breaks a link, leaves 25–35 lines, or drops out of the index. It runs in the existing `cargo test --workspace` CI step. Docs and a test only; no UI change. No version bump.

Approval cards share one width (up to 520px) and one "N things need a decision" line. A hard action uses a danger Approve on a 2px frame, Deny stays a ghost, and the note says Esc denies (Enter still does not approve). Commands and tool ids are monospace; notes stay proportional. The Grant full card stays hidden unless `GROKHUB_GRANT_FULL=1`, and it says click and type skip asking while deletes, sends, money and credentials still ask. A finished tool no longer repeats a stale "running" next to its completed chip. The agent cursor has a dark outline, and the desktop toggle names that hard floor, with its hint wrapping clear of the switch. No version bump.

AMR M0: a local agent-memory schema under `amr/` in the config directory, plus a `/recall` read path. The default stays legacy SOUL/USER/MEMORY. Opt in with `"memory_backend": "amr"` in `app.json`. No Settings control, no migration, and `amr/` is not hub-synced. No version bump.

Spike-0 harness: the cabin adds a stricter pre-check on top of Grok Build. GB still owns Ask / Auto / Always and computer use; the cabin never loosens it. What the pre-check does:

- **Hard floor deny, no bypass:** credential paths, `rm -rf /`, fork bomb, `mkfs`, `dd` to a disk, and `curl|sh` as root.
- **Hard-class park, even under Always:** money, send, delete, credentials, and irreversible OS actions show a white card. It offers Approve / Deny only (no Always), Enter does not approve, Esc denies, and it times out to Deny after 5 min. Halt denies every parked card.
- **Access:** Settings → *Let Grok control the desktop* is Readonly / Supervised, and it gates desktop tools only. Full is one inline Grant full card in the Work tree. Always never grants it.

Where the checks run:

- The `grokhub-desktop` MCP dispatch, on Linux and Windows.
- ACP permission asks.
- Headless `grok -p` on Auto / Always, via appended `--deny` rules. A denied hard step parks a card, and Approve re-runs it once on ACP Ask.
- The native Lab engine.

Every step writes a cabin-local span to `spans/<chat>.jsonl` (`path`, `chat_id`, `turn`, `ui_changed`, redacted args). The `approval_gate_violation` check flags a hard action with no approve span. The last approved click shows as the agent cursor marker on the Work-tree frame, and the parked count joins the needs-attention line. The grok-build-gui spec now says selected Always is white, not amber. No version bump.

The Projects Name field lines up with Cancel: same height, centered on the row, filled like Filter chats, with a muted hint and a white focus ring while it is staged. Cancel's right edge meets the same rail inset as the “+”. If Grok Build never answers and the list is empty, Skills and Connectors say "Grok Build didn't answer. Refresh to try again." If a timeout keeps the previous list, a muted line above those tiles says "Showing the last list — Grok Build timed out." No version bump.

Projects “+” asks for a folder name and shows Cancel beside the Name field. Cancel drops the staged folder the same way Esc does. Skills and Connectors no longer stay on Loading… when the Grok Build catalog is slow: the three catalog commands run together, and if nothing has arrived after 18 seconds the page settles (last list kept, or empty) with a timeout instead of spinning. Refresh tries again. No version bump.

Settings → Account picks up a Grok sign-in written by `grokhub --oauth` (or another process) while the cabin is already open, so About/doctor saying xAI auth present no longer leaves Account on Sign in with Grok with a blank identity. When OAuth is present, Account shows Connected with the Grok name and/or email and Sign out; the device-code path stays for a true sign-out. No version bump.

Card deck polish: after × the new front card eases in briefly instead of jump-cutting; each open peek strip shows that card's own title; fly-in snaps under reduced motion and the mid-flight offset reads more clearly; the New here chip tip says "Suggested because you're new here". No version bump.

On Automations, Run is a ghost pill and a failed job's Retry stays filled, each job has an Enabled switch, next runs read as a local time such as today 7:30 AM or Tue 9:00 AM, the intro is plain, Suggested says the ideas come from your recent work, and Discuss says you opened the post from your feed.

Failed Automations jobs show Retry and a View last run link, and Remove sits behind a ··· menu that asks Remove '…'? before deleting the job.

Pulse leftovers (Critiquito re-check): opened idea headings follow the row type (Do → "What Apply will do", Automate → "What Apply will schedule", Learn → "What I'll learn"); Suggest ideas while signed out stays enabled and shows an amber "Sign in to Grok to get ideas." with Open Settings, cleared once signed in; Ideas loading replaces empty copy with "Looking for ideas in your recent work…" and three #16181c skeleton rows; empty Ideas / Feed use the new copy, with inline Suggest ideas and a Feed instructions link; Feed image slots are a plain #16181c skeleton while loading and drop on fail; Search palette closes on outside click or navigation, uses the "Search pages and commands" placeholder, and names rows as the sidebar does (old names still find them). No version bump.

Feed cards show a short takeaway under the title (about two sentences, cut on a word or sentence, with URLs left on the Read link) instead of a long paragraph chopped mid-sentence. A digest skips the model's opening line ("I'll look up…") and leads with the news, followed by why it matters to you. Discuss on a digest or suggestion opens the main chat with that post's title, takeaway, source link, and why it matters, and puts the cursor in the composer. On the home deck, hovering a digest, suggestion, or image card expands it into the same card: source and age, the short takeaway, the image, Liked, and Discuss. × still only removes it from the deck. No version bump.

The command palette hides Devices, Agents, and Connectors until a query names them. They are not on the sidebar rail. Typing devices, device, agents, agent, connectors, or connector still opens that page (connectors also matches Skills and Connectors, which contains the word). Night still opens Automations; there is no Night row. On Pulse, an empty Ideas list shows Suggest ideas once, in the body. The header button stays when ideas are listed, and it reads Suggesting… while a suggestion is loading.

- Linux: `grokhub-linux-v2.10.94.tar.gz` and AUR `pkgver=2.10.94`.
- Windows: `GrokHub-Setup-2.10.94.exe` and `grokhub-windows-v2.10.94.zip`.

## 2.10.93 — 2026-10-06

Imagine reuses the Settings → Account Grok sign-in. Opening Imagine no longer asks for a second separate sign-in when Account is already signed in, and it stops re-prompting after a successful auth: credentials try Imagine's own keychain first, then Account (`secrets.oauth`, with the same live/refresh path Lab mode uses), then the console API key. The composer Sign in control and the need-signin message point at Settings → Account instead of starting another OAuth wall. Tests cover Account-only reuse, Imagine-keychain preference, no re-prompt on a second `imagine_cred` call, and the exact need-signin sentence.

Composer streaming glow is a white breath (`#e7e9ea`, icons.rs breath math): idle α≈0.25, streaming 0.25↔0.55, settles to idle in 200ms. Always permission settles in 140ms (fill α + scale 0.98→1.0) to a white ring, then stops. Reduced-motion skips the animation. Branch-cleanup and bot-choice notes; Semgrep / dyl-review / Continual Learning wired in the rules. Grok Build explore/plan/implementer roles documented. No version bump.

When Labs Beta is on and `origin/beta` tip matches `origin/main` (beta promoted / caught up), the cabin **auto-turns the Beta toggle off** and writes `stable` to the channel receipt so Updates pull main. Checked after a successful Update and when opening Settings → Labs (60s cooldown). Linux implements this now; Windows documents the same behavior for when channels land. Re-enable Beta anytime.

Settings → Labs **Beta channel** toggle: on fetches, builds, and installs from `beta` (same as `install.sh --user --channel beta`); off returns to stable/`main`. Shows the current channel, branch, and commit next to the toggle, progress while it builds, and a restart prompt when done. Failures keep the previous binary (backup + restore) and spell out dirty clone, build failed, or no clone. Windows shows the toggle disabled with a short note until the installer supports channels.

Fixes from an overnight debug run of `beta`. `grokhub --update` on a beta install now pulls `origin beta`; it compared the build against the stable release tag, and since beta never bumps the version it always said "look current" and pulled nothing. The update steps say "Pulling origin/beta…" on beta instead of naming `origin/main`. `grokhub --version` no longer keeps an old SHA after a pull when `git gc` has packed the branch ref, because the build now also watches the reflog. A plain `./scripts/install.sh --user` stops with a clear error when the checkout is on the other channel's branch (for example a beta receipt on `main`), instead of labeling a `main` build as beta and leaving Update refusing the clone. Lab mode no longer hangs every turn when the Settings → Account sign-in has expired and its refresh fails (offline, or a dead refresh token): a failed refresh waits 30 seconds before the next try, sharing the backoff the background refresh already had, and the saved sign-in is kept. On Pulse, dismissing a Feed post hides that post only; before, it pushed every later post with the same title (tomorrow's run report for that job, or the next quiet-hours digest) below the line for good. Ideas keep the Dismiss penalty. Dismissing the quiet-hours digest no longer swallows the next night's digest, and releasing quiet hours marks only the cards it held, not older posts that share a title.


Stable and beta channels. `./scripts/install.sh --user --channel beta` fetches origin, checks out the `beta` branch, builds, and installs; `--channel stable` goes back to `main`. The choice is saved in a `channel` receipt in the config folder, so a plain `./scripts/install.sh --user` stays on it, and Settings → Update, `/update`, and `grokhub --update` pull `origin beta` on a beta install, never `origin main`. `grokhub --version` now reads `GrokHub 2.10.92-beta (beta @ abc1234)` on beta and `GrokHub 2.10.92 (main @ f7dcf9a)` on stable; `build.rs` reads the branch and SHA from git, and the Cargo version is unchanged. CI also runs on `beta`. Channels are Linux-only for now.

The bot rules go beta first: every new feature and fix PR targets `beta`, `beta` moves to `main` only on Jeremy's say as a squash-merged PR, and hotfixes go to `main` on his say and are merged back into `beta`. PRs into `beta` don't bump the version; each promotion to `main` carries one bump and one release. The rules are in `CLAUDE.md`, `.cursor/rules`, and a new `CONTRIBUTING.md`.

Cleanup with no visible change: the cabin no longer switches off Rust's dead-code and unused-import warnings for the whole app, and about 2,000 lines of code nothing called are gone. That includes the old empty-home pulse rows, the duplex realtime Voice socket and its PCM helpers (Hey Grok stays push-to-talk), the Eyes Scan windshield and its browser-status cache, the Grok-session sidebar actions, card reactions, the teach-a-routine path, and a few unused helpers. Small helpers only tests use are now compiled for tests only, and Windows-only helpers only for Windows. A chat-bubble wrap test that was missing its `#[test]` line runs again.

Keyboard and palette polish. Shift+Enter now starts a new line in the chat composer, as it does in most chat apps; before, it sent the message. Ctrl+Enter still starts a new line, Enter still sends, and Alt+Enter still queues while a reply runs. Ctrl+, opens Settings. The command palette (Ctrl+K) shows each row's shortcut at the right (New chat Ctrl+N, Settings Ctrl+,, Hey Grok Ctrl+G), its rows line up on the left like the sidebar, and a new Keyboard shortcuts row opens the shortcut sheet. Copy diagnostics in the palette now copies the bundle to the clipboard, as the Settings button does, instead of pasting it into the status line. The bundle names the full build (`2.10.92-beta (beta @ abc1234)`) and the OS, and a key that shows up in the status line is redacted before it is copied. New tests drive Shift+Enter, Ctrl+Enter, Enter, and Alt+Enter through a composer field wired like the chat's, press Ctrl+, through the app's input handler, and read the clipboard command the palette's Copy diagnostics sends.

The card deck on the main chat window stays where it is. Hovering it used to lift the whole deck above the chat box, and moving the pointer up to the cards dropped it back down. Now a single card doesn't move at all. With several, the cards behind the front one slide up just far enough to show their titles, and the card under the pointer rises only enough to read in full. × on a card there takes it off that deck only: Pulse keeps it, and nothing is marked dismissed. The deck holds at most three cards, and when one leaves, the next waiting card slides in. The count on the deck includes the cards still waiting.

- Linux: `grokhub-linux-v2.10.93.tar.gz` and AUR `pkgver=2.10.93`.
- Windows: `GrokHub-Setup-2.10.93.exe` and `grokhub-windows-v2.10.93.zip`.

## 2.10.92 — 2026-10-04

Hotfix: Lab mode (Settings → Labs, the native engine) now uses the Grok sign-in you already have. Since 2.10.70 it only looked at the Imagine page's own sign-in and the console API key, so if you signed in with Grok from Settings → Account (the sign-in the Grok CLI path and the rest of GrokHub use), every Lab mode message failed with "Sign in with Grok or add an API key." Lab mode now tries, in order: the Imagine sign-in, the Settings → Account sign-in, then your API key. A token inside its refresh window is renewed in the background; an expired one with a refresh token is renewed once and saved back to GrokHub's own `secrets.json` through the same atomic, owner-only write the rest of the app uses. A UI refresh and a Lab mode turn holding the same refresh token now share one refresh instead of both spending it. Lab mode still never reads or writes the Grok CLI's own login files. If only the Grok CLI is signed in, the message now says so: "Lab mode uses GrokHub's sign-in, not the Grok CLI's. Sign in with Grok in Settings → Account, or add an API key." With no sign-in and no key it still says "Sign in with Grok or add an API key." New tests run a Lab mode turn through the native engine against a local fake server, using fake tokens in a temp home: a signed-in, no-key turn sends `Authorization: Bearer test-access-token` and gets the fake reply, an expired token calls refresh once and the turn uses the new token, and a missing sign-in returns the exact message.

- Linux: `grokhub-linux-v2.10.92.tar.gz` and AUR `pkgver=2.10.92`.
- Windows: `GrokHub-Setup-2.10.92.exe` and `grokhub-windows-v2.10.92.zip`.

## 2.10.91 — 2026-10-04

Ideas is now **Pulse**, one page with two tabs, and **Feed** comes first: Pulse opens on Feed, and the switch at the top reads Feed | Ideas. **Feed** is a column of posts with the source, the headline, two or three sentences, the source's own image, and Like / Discuss. **Ideas**, the second tab, lists what the cabin could do for you next, grouped by category: up to four of the strongest first, then Financial Management, Productivity, Relationships, Health & Fitness, Shopping, and More ideas. Each row has a category icon, a bold "I can …" line, a short reason, and a ··· menu. Images come only from the post's thumbnail or the linked page's `og:image`, are cached in `pulse-images/` in the config folder, and show a placeholder until they arrive; tests use local files and never the network. The Home card deck and the Ideas board move into Pulse once on first launch, with every saved idea, card, pin, and feed setting kept. The deck no longer sits on the empty chat by default; Settings → Behavior → Cards on the empty chat turns it back on.

Every card is now Do, Watch, Learn, Automate, or Quiet, and plain rules rank them instead of the model: a skill ask scores +1000, something due within a day +500, unfinished work on the same topic +250, a repeat +120, an idea you accepted +80, Not this −400, and Dismiss −200, on top of a base of 150 (Do), 140 (Automate), 130 (Learn), or 110 (Watch). Under 100 a card is Quiet and stays off the page. The model only writes the sentence. The ··· menu has Run in the background (it sends the idea as `/bg`, so it runs out of sight like other background work; Ask mode refuses it, as it refuses every `/bg`), Snooze until 9 AM (or tomorrow 9 AM after 9), Always do this (That's right / That's wrong on a Learn card), Open, Not this, and Dismiss. Likes, Not this, Dismiss, and right/wrong each add one line to `MEMORY.md`, such as `- [2026-10-04] pulse: dismissed "Sort the photos" reason=not-this`, and the next ranking reads them. Cards that arrive during quiet hours wait and come back as one "While you were in quiet hours" post.

**Feed instructions** on the Feed tab is a plain-text prompt that shapes every future post. You can edit it any time, and after three likes or skips the cabin rewrites it itself, keeping your words and adding what it learned under "Show more of" and "Show less of". New tests cover the rename and old links, category order, the card types, each button (Run sends `/bg` with the idea's text on the fake Grok CLI, Snooze hides until 09:00, Dismiss removes, Not this writes the ledger line and drops the topic's score), the ranking scores, an instructions rewrite on the fake model, the quiet-hours post, the one-time move of old Home and Ideas data, and cached feed images with the placeholder.

A design pass tightened the page. Hover or arrow onto an idea and it lights up with a blue ring and shows Run · Snooze · Dismiss inline; with a row focused, R runs it, S snoozes, Enter opens, N is Not this, and D dismisses, and the ··· menu lists the same keys. Idea titles are the model's own "I can …" line, up to 70 characters; when it doesn't write one, a plain fallback such as "I can learn your release notes from merged PRs" replaces the old "I can help with …" wording. A reminder idea is now labeled Automate everywhere, so the row and the opened card agree. Each tab has its own empty message, Suggest ideas shows placeholder rows while it works, and if you're signed out it says how to sign in and links to Settings. Feed posts put the source and age on one line, drop images that fail to load, and use a lighter Discuss. Feed instructions has a × close, a focus ring on the text, "I'll update these after N more likes or skips", and Ctrl+Enter to save. Search (Ctrl+K, now shown on the sidebar row) uses the sidebar names, Automations and Skills and Connectors, closes when you pick a page or click outside, and small gray text across the app is lighter (#8b9096) so it reads on black. New tests check that a fresh Pulse opens on Feed with the tabs in Feed, Ideas order, the inline actions and keys on the focused row, the signed-out and loading states, the 70-character titles, and the palette names and closing.

The repo's bot rules (`CLAUDE.md` and `.cursor/rules`) add one more: fetch origin and work from the latest `main`, merge `main` into a branch before touching it, re-check right before merging and before tagging, never render or screenshot from a stale checkout, and name the starting `main` SHA in every PR body and report.

Workboard cards open only when you click them, not when the pointer rests on them. A second click on the open card's title row folds it, and so does Esc, unless a text field, menu, dialog or the palette has the keyboard; Esc on a card never halts anything. Clicking another card opens that one instead, and dragging a card still moves it without opening it. Follow up cards work the same way.

The bot rules drop done-and-green auto-ship: bots now get a PR green and ready to merge, with proof of work, and stop. Nothing is merged, tagged, or released, Claude's and Cursor's PRs included, without Jeremy's explicit say, and when he says merge it's squash and merge. The latest-`main` rule stays.

The Imagine page has one set of controls again. A row of unstyled debug buttons (sign in, use a code, Generate/Edit, the model picks, an image count stepper, Res, Aspect, Quality) sat above the chat box and duplicated the pills inside it. Those rows are gone. Sign in is a pill in the composer when Imagine has no Grok sign-in or API key, and account, mode, model, image count, edit sources, mask, and the video source live behind a new More pill. The composer's aspect pill now sets the aspect of every image, edit, and video request; before, stills ignored it and used only the separate Aspect button. The Speed/Quality pill alone picks resolution and quality, and the 6s/10s/15s and Video audio pills alone set duration and audio. The aspect icon is drawn larger, so the 2:3 shape no longer reads as a "0".

- Linux: `grokhub-linux-v2.10.91.tar.gz` and AUR `pkgver=2.10.91`.
- Windows: `GrokHub-Setup-2.10.91.exe` and `grokhub-windows-v2.10.91.zip`.

## 2.10.90 — 2026-10-04

Scheduled jobs run like crons: completely in the background, separate from your chats, and never in your way. Nothing shows a job while it runs: no status line when a clock job, its check or a `/loop` starts or ends, no glow, no Stop, and no row in the live-work strip, even on the hidden Background chat. Its result still lands on the Follow up card and the Home card when it finishes. A due job no longer waits for your chat turn to end; only a saved desktop replay still does, because it drives the desktop you are using. A `/loop` always runs as its own `grok -p` instead of borrowing your chat's Grok session when it was idle, which made your chat look busy and held your next message behind the loop. A report that finishes while you are replying in its own Follow up chat waits for your reply to end, so your live reply can no longer overwrite it. Halt (the hotkey, the tray, or Ctrl+Alt+H) still stops a running job, and jobs keep the unattended rules and low effort. New tests run a job on the fake Grok CLI that starts and ends in the middle of your reply without pausing or cutting it, send and finish a chat turn while a job works, check that nothing shows the job, and check its report reaches Follow up and Home.

Fixes from a debug run of the whole app. GrokHub no longer refuses to open, silently, after a crash or reboot: a leftover `cabin.pid` could name a pid the system had since given to another program (or a zombie), and the new launch took it for a running cabin and quit. Now only a live process with GrokHub's own name counts. When the Grok Build CLI download fails on first run (offline, or blocked by a proxy), Get Started shows the real error, such as `curl: (22) … 403`, instead of "install finished but grok was not found". Digest cards no longer say "The brief steers the next edition." twice. `cabin_leader_socket_is_not_the_cli_leader` passes with `GROKHUB_CONFIG` set to a temp dir, as `CLAUDE.md` tells local runs to do.

The quick chips under the chat box no longer offer "Continue Follow up · …" or "Continue Background". Hidden chats are skipped when chips suggest picking up another chat, so a Follow up card's chat stays on the card.

- Linux: `grokhub-linux-v2.10.90.tar.gz` and AUR `pkgver=2.10.90`.
- Windows: `GrokHub-Setup-2.10.90.exe` and `grokhub-windows-v2.10.90.zip`.

## 2.10.89 — 2026-10-04

Scheduled automations run in the background on their own process and stay out of your chats. A clock automation used to borrow your chat's reply slot: it switched you to the chat page when it fired, and a message you sent while it ran stopped it. Its Follow up chat ("Follow up · …") was also a History row. Now each run is its own background `grok -p` (or native engine run) on the hidden Background chat, one at a time, and it doesn't use any of your three `/bg` slots or show in the chat's background strip. You stay on the page you're on, and your messages don't stop it. Its report still lands on the Follow up card on the workboard and the Home card, where you read and answer it, but that chat is no longer listed in History. Follow up chats filed by older builds are hidden on load. Nothing is deleted.

Review fixes for the change above. A scheduled run is unwatched, so it runs at low effort again, as it did in the chat slot and as every other unattended run does; moving to the background had switched it to the effort you picked for your own chats. An automation whose instructions name a skill (`/snapshot …`, or words matching a skill's trigger) gets that skill's steps again. Deleting a Follow up card puts its chat back in History, as the "Its chat stays in History" status says, so the report and anything you replied there can still be found; startup now hides only the Follow up chats a card still links. Halt (the hotkey, the tray, or Ctrl+Alt+H) stops a scheduled run and marks the job stopped; Stop on your own chat leaves it running.

- Linux: `grokhub-linux-v2.10.89.tar.gz` and AUR `pkgver=2.10.89`.
- Windows: `GrokHub-Setup-2.10.89.exe` and `grokhub-windows-v2.10.89.zip`.

## 2.10.88 — 2026-10-04

A review of every open pull request, shipped as one release.

Background work stays out of the sidebar History. A `/bg` run forks the chat's Grok session, and a `/loop` or a quick-chip reply through the CLI writes a session of its own; on the next launch those sessions came back as extra chats in History. Their ids are now filed on the hidden Background chat, so startup adoption and the Grok session list skip them, and chats a loop already leaked are hidden on load. Nothing is deleted: the sessions stay on disk, and deleting all chats still removes them.

Hovering Send while no reply is running says just "Send" again. Since 2.10.87 the hover could read "Send · Read file" after a turn that used a tool, because the finished turn's tool cards are only cleared when the next turn starts, and it could also show another chat's pending permission title. The tool or wait name now only appears on Stop's hover while a reply is running here.

Fixes from reviewing 2.10.55 to 2.10.63 (#456). Imagine no longer signs you out after a wall paint or a failed generation refreshed your token: refreshes run one at a time, the app always picks up the new tokens for the account signed in now, and a failed refresh offers "Use API key" or Settings instead of looping on "Sign in again". The device "Verify" link opens through the app's own browser opener. On X11, desktop control types capitals and shifted symbols (`Hello!` no longer comes out as `hello1`), handles tabs and CRLF, lines screenshots up with clicks when no monitor sits at 0,0, and always lets go of a mouse button or modifier key when a drag or key combo fails. Stopping a steered reply clears its steer and background notes so they don't ride into the next chat, and `/retry` keeps them. Ask mode no longer mistakes a prompt that reads like `--always-approve` for the flag. The daily read takes links from the whole reply, never cuts one in half, and drops links that don't answer. The check is a HEAD request to public hosts only: it never fetches localhost or private addresses and doesn't follow redirects, so a cited link can't bounce it onto one. While GrokHub sits in the tray it no longer replays the last keypress or file drop every tick.

Tests from the last Cursor drafts are folded in (#483), with the ones that could pass against a stub strengthened. One of them, the nightly-review test, could have overwritten a real `suggestions.json` when run locally; it now writes only to a temp folder.

- Linux: `grokhub-linux-v2.10.88.tar.gz` and AUR `pkgver=2.10.88`.
- Windows: `GrokHub-Setup-2.10.88.exe` and `grokhub-windows-v2.10.88.zip`.

## 2.10.87 — 2026-10-03

"Minimal" is gone from the effort list. It was never a real level. Settings and the composer now offer None, Low, Medium, High and Extra High. A setting, session or `/effort` that still says "minimal" (or "mini") loads as **Low**, the smallest level that still reasons, so nothing breaks and reasoning doesn't switch off. The native engine also sends a saved "minimal" as `low`. The chat window loses its "Thinking" dot and its "Background" button. The glow around the chat box already shows a reply is running, and Stop's tooltip now says what it would stop (for example "Stop · Working on your reply"). `/bg` still moves a running reply to the background, and `/bg <task>` and `/bg stop` work as before. The repo's bot rules (`CLAUDE.md` and `.cursor/rules`) now say that finished, green work is a full ship: bots merge, tag, release and report the live link. They also add one-line rules for proof of work in PRs, literal test values, batched findings, green stacks with one rebase owner, a brief template, and finding the CI cause before at most one rerun.

- Linux: `grokhub-linux-v2.10.87.tar.gz` and AUR `pkgver=2.10.87`.
- Windows: `GrokHub-Setup-2.10.87.exe` and `grokhub-windows-v2.10.87.zip`.

## 2.10.86 — 2026-10-03

The Home card deck no longer flickers. On the empty chat screen the deck sits below the chat box, and since 2.10.66 the open fan moves above the box. Hover was checked against last frame's moving cards, and a card that crossed the chat box jumped about 260 px to the far side of it. With the pointer resting in the gap between the box and the deck, the deck opened and shut on almost every frame (98 times in 120 frames in a headless test), and moving the pointer up to the open cards closed the deck on the way. Hover now checks where the cards will settle, an open deck stays open over the pile, the fan and the chat box between them, and the cards slide without jumping. The "⋯" menu on Home cards (More like this, Less like this, Hide this automation's runs) is gone, and Home learns from what you do instead. Closing a card with × before you ever opened or used it counts as a strong "less like this" for its kind and topic. Opening, running and finishing a card are each remembered by how far you got and what you did (opened a chat, filed a todo, made an automation, finished the todo). A one-off card whose action you already ran or finished isn't offered again, while cards on the same kind of work and the same topics move up. Automation runs, schedules you set up and cards you pinned keep coming back. Everything stays on this computer, `card_prefs.json` keeps at most 100 uses, and older settings files and signal logs still load. Runs hidden from Home by an older build stay hidden until you undo it on the Automations page.

- Linux: `grokhub-linux-v2.10.86.tar.gz` and AUR `pkgver=2.10.86`.
- Windows: `GrokHub-Setup-2.10.86.exe` and `grokhub-windows-v2.10.86.zip`.

## 2.10.85 — 2026-10-03

Delete all (Settings → History) no longer brings deleted chats back. It first halts whatever is running, which saves the chats as they were, and then saves the fresh empty chat. Each save is written by its own background thread, so the older save could finish last and overwrite `threads.json` with the deleted chats. They would then come back on the next start. Saves now carry a sequence number. A save that's older than the last one written never replaces the chats or settings. It only writes its project list or secrets when no newer save has written them. This also made the `delete_all_history_clears_seeded_chats` test fail now and then in CI.

- Linux: `grokhub-linux-v2.10.85.tar.gz` and AUR `pkgver=2.10.85`.
- Windows: `GrokHub-Setup-2.10.85.exe` and `grokhub-windows-v2.10.85.zip`.

## 2.10.84 — 2026-10-03

A parity harness for the native engine, with nothing switched. `cargo run -p grokhub-agent --example eval` runs a fixed suite on the native engine and on the CLI path: a desktop probe, a small repo bugfix with a test, an Ask-mode refusal, background work plus Halt, an MCP tool call, compaction of a long transcript, and an Imagine call that only builds the request. Dry-run is the default. It uses scripted model replies for the native engine and the test-only fake Grok CLI agent (`grokhub-fake-acp`, not shipped) for the CLI path, with no network, no key and no stored credential. It writes `research/native-parity-v1.md`, a table of every item on both engines plus a GAPS section. **Live evals don't work yet.** `--live` refuses to start without `GROKHUB_EVAL_API_KEY` and a budget of $0.20 or less, and even with both it only says live mode isn't implemented in this build. So the report compares scripted runs, not real models, and its GAPS section says so. The Grok CLI stays the default engine. Plugin trust also no longer reuses a cached hash for files changed in the last two seconds.

- Linux: `grokhub-linux-v2.10.84.tar.gz` and AUR `pkgver=2.10.84`.
- Windows: `GrokHub-Setup-2.10.84.exe` and `grokhub-windows-v2.10.84.zip`.

## 2.10.83 — 2026-10-03

Native chats (Settings → Labs, still off by default) can now use plugin bundles in the Claude Code and Grok formats (`.claude-plugin/plugin.json`, `.grok-plugin/plugin.json` or `plugin.json`). A bundle can bring skills, hooks, MCP servers, agents and commands. Install one from Settings → Labs → Plugins with an https, ssh or `file://` git URL, or with an absolute folder path. The cabin copies it into its config folder. Nothing from the bundle runs during install: the git clone uses no templates, hooks or submodules, and the `.git` folder is removed. Bundles already in `~/.claude/plugins`, `~/.grok/plugins` or the project's `.claude`/`.grok` plugin folders are listed but never changed. Every bundle stays off until you trust its current contents and then enable it. Trust shows the exact hook and MCP commands it would run. Trust is tied to a SHA-256 of the bundle's files, so any change to the files or the version turns the bundle off until you trust it again. A bundle that holds a credential-like file can't be trusted, and paths that leave the bundle, including through symlinks, are refused. Plugin MCP servers are named `plugin__<bundle>__<server>`, go through the same permission rules as your own servers, and never replace a server you configured. A plugin can't add permission rules. Plugin agents work as subagent personas, and plugin commands are listed in the system text. A marketplace index is fetched only when you press Fetch (https only, no redirects, 1 MB cap), and fetching never installs anything. The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.83.tar.gz` and AUR `pkgver=2.10.83`.
- Windows: `GrokHub-Setup-2.10.83.exe` and `grokhub-windows-v2.10.83.zip`.

## 2.10.82 — 2026-10-03

With the native engine on (Settings → Labs, still off by default), the cabin's unattended work runs on it instead of the Grok CLI: automations, loops, the night review, phone and hub tasks, ideas, the digest lookup, chips and the greeting. Each runner keeps its prompt and how it reads the reply, so results still land in Follow up and on Home, grouped per source as before. An unattended run never shows a permission or question card and never waits for you. In Ask mode anything that isn't read-only is refused, the same rule unattended runs already follow. Auto asks the auto-review judge and refuses if the answer isn't a clear yes. Always works as it does for scheduled CLI runs. Halt and quitting stop every unattended run and what it started. Their tokens count toward today's usage, and the one-shot runs (chips, greeting, ideas, digest, review) don't leave sessions in History. Without a native sign-in a run fails with a clear message; it never falls back to the Grok CLI's sign-in. With the native engine off, every runner uses the Grok CLI exactly as before.

- Linux: `grokhub-linux-v2.10.82.tar.gz` and AUR `pkgver=2.10.82`.
- Windows: `GrokHub-Setup-2.10.82.exe` and `grokhub-windows-v2.10.82.zip`.

## 2.10.81 — 2026-10-03

Native chats (Settings → Labs, still off by default) now have memory and answer every Grok CLI slash command. `/remember` adds a note to the project's `MEMORY.md`, or to the global one in the GrokHub config folder when it starts with `global:` or `--global`. Secrets are redacted before anything is written. On the first turn of a session, the closest matching notes are recalled from a local SQLite full-text index and added to the system text, fenced as untrusted and capped at 4 KB. Recalled text can't change a permission, gate or mode. Newer notes win over older equal matches (30-day half-life). Recall only reads the global notes, this project's `MEMORY.md` and this project's flush notes, so one project's notes never show up in another. The index can be deleted at any time and is rebuilt from the files. Before a compaction, and on `/flush`, the pending conversation is saved to a per-project notes file in the config folder, never into your repository, without a model call. `/dream` makes one low-effort model call to tidy both `MEMORY.md` files, keeps a `MEMORY.md.dream.bak` copy, and leaves the files untouched if anything fails. Its tokens count in the chat's usage. All 74 Grok CLI slash commands now have a native answer: the cabin handles it, or says plainly that it's a CLI-pager or account feature. `/rewind` says it isn't available on native chats, because their sessions are append-only, and points to `/fork`. The full table is in the Shortcuts window. SQLite is compiled into the app (`rusqlite` with `bundled`), so there's nothing extra to install on Linux or Windows. The Grok CLI path and its slash commands are unchanged.

- Linux: `grokhub-linux-v2.10.81.tar.gz` and AUR `pkgver=2.10.81`.
- Windows: `GrokHub-Setup-2.10.81.exe` and `grokhub-windows-v2.10.81.zip`.

## 2.10.80 — 2026-10-03

Native chats (Settings → Labs, still off by default) can now read the web and make images and videos. `web_fetch` loads one public `http` or `https` page and returns it as markdown: headings, links, lists, code and paragraphs, with scripts and styles stripped. The raw body is capped at 5 MiB and the markdown is truncated after that. It only reaches public addresses. Localhost, loopback, link-local (169.254.*), private ranges, carrier-grade NAT (100.64.0.0/10) and their IPv6 forms are refused before the request, and again when the connection is made, so a DNS answer that changes in between can't reach your network. Redirects are followed by hand, up to a cap, and each hop is checked again. `web_fetch` sends a network request, so it isn't read-only. `WebFetch(domain:…)` rules decide it (deny, then ask, then allow). With no rule it asks when you're there, unattended runs refuse it, and Auto mode asks the auto-review judge. `image_generate`, `image_edit`, `video_generate`, `video_edit` and `video_extend` use the same request bodies as the Imagine page, with every parameter: up to 10 images, 1k or 2k, quality, a mask (retried as a reference image if the model rejects masks), text-to-video and image-to-video up to 1080p, edit and extend. That's more than the Grok CLI offers. These spend credits, so they're gated like other non-read-only tools. Results are downloaded into the session's own folder (`sessions/<id>/media`), shown as image and video cards in the chat, and removed when the session is deleted. Halt and Stop end a video wait. `/deep-research` on a native chat sends a research recipe as a normal turn: plan, search with `web_search`, `x_search` and `web_fetch`, cross-check, then a cited report. The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.80.tar.gz` and AUR `pkgver=2.10.80`.
- Windows: `GrokHub-Setup-2.10.80.exe` and `grokhub-windows-v2.10.80.zip`.

## 2.10.79 — 2026-10-03

Native chats (Settings → Labs, still off by default) can now hand work to subagents and keep a plan. `spawn_subagent` runs a child native loop on its own thread with its own history. An `explore` child is read-only: it only gets read-only tools, the gate refuses everything else, and it can't use a git worktree. A `general` child copies the parent's mode, gates and attended or unattended state, so it is never looser than the chat that started it. Starting a general child is gated like any other non-read-only tool, and every call a child makes goes through the same permission rules, auto-review judge and hooks as the parent. A child of a child can't spawn. A general child can run in its own `git worktree` under the GrokHub config folder. If the folder isn't a git repository that's an error, never a silent fallback to the main tree, and the worktree is removed if the child is cancelled. A `persona` adds to the parent's system text without replacing it. Background children show in the existing Tasks list, `get_command_or_subagent_output` and `kill_command_or_subagent` accept their ids, and `send_subagent_message` steers a running child. Cancel and Halt reach every child and grandchild and the commands they started. A child's tokens and cost count in the parent chat's usage. A background child's cost is added at the parent's next turn. `todo_write` keeps a todo list on the session. It shows under Tasks and survives compaction. `ask_user_question` shows a question card and waits while you're there. Unattended runs get an error instead of waiting. `enter_plan_mode` makes the chat read-only, and `exit_plan_mode` shows the plan in an approval card. Only your approval ends plan mode, and an unattended run can't end it. The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.79.tar.gz` and AUR `pkgver=2.10.79`.
- Windows: `GrokHub-Setup-2.10.79.exe` and `grokhub-windows-v2.10.79.zip`.

## 2.10.78 — 2026-10-03

Native chats (Settings → Labs, still off by default) now pick up skills, layered rules and hooks the way the Grok CLI and Claude do. `SKILL.md` skills are found in the project's `.grok/skills` and `.claude/skills` (from the repo root down to the folder) and in the user folders. A project skill wins over a user skill with the same name. The model sees a short capped list of names and descriptions, and the new `skill` tool loads a skill's body and lists its files without leaving the skill folder. A skill's `allowed-tools` is only shown, never used to grant anything. `AGENTS.md` and `CLAUDE.md` are layered global first, then the repo root, then nested folders down to the workspace, capped at 64 KB, after GrokHub's own rules. Hooks load from the same user and project hook settings the CLI reads, get the CLI's JSON on stdin and its environment variables, and answer the same way. A `PreToolUse` hook can refuse a call or turn it into a question. It can never let through something the permission check would ask about or refuse. A hook that times out has its whole process tree killed and decides nothing. In unattended runs a hook that asks counts as a refusal. Project hooks come from the repository, so on native chats they only run after you trust the folder in Settings → Skills and Hooks. Your own hooks always run. With the native engine on, that page lists the discovered skills and hooks. With it off, it shows the Grok CLI's listing as before. The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.78.tar.gz` and AUR `pkgver=2.10.78`.
- Windows: `GrokHub-Setup-2.10.78.exe` and `grokhub-windows-v2.10.78.zip`.

## 2.10.77 — 2026-10-03

Native chats (Settings → Labs, still off by default) can now use MCP servers. The client is a small synchronous JSON-RPC implementation in `grokhub-agent`, with no tokio and no new dependencies. It speaks stdio, streamable HTTP and legacy SSE. The official `rmcp` SDK was left out because it would have brought an async runtime into the app. Servers are read from `mcp.json` in the GrokHub config folder. Settings → Labs can import them once from the cabin's Grok home, copying only the server definitions, never auth files. The cabin's own `grokhub-desktop` server is never imported or started, because native chats drive the desktop in-process behind the existing desktop gates. Tools appear to the model as `server__tool` and go through `MCPTool` rules (deny, then ask, then allow). A tool with no rule asks when you're there and is refused in unattended runs. A server's read-only hint never skips the gate. Above 40 tools the model gets `search_tool` and `use_tool` instead of every schema. A server asking for input (`elicitation/create`) shows the existing elicitation card, and is declined in unattended runs. Stdio servers run in their own process tree, are killed on exit and restart, and a crash is reported instead of taking the chat down. Remote servers use the `Authorization` header stored on the entry. The browser OAuth flow isn't there yet. Settings → Labs lists each server's status and tool count, with restart. The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.77.tar.gz` and AUR `pkgver=2.10.77`.
- Windows: `GrokHub-Setup-2.10.77.exe` and `grokhub-windows-v2.10.77.zip`.

## 2.10.76 — 2026-10-03

Native chats (Settings → Labs, still off by default) can now run work in the background. A shell call with `is_background` returns a task id straight away. Its output goes into a 1 MiB ring buffer that keeps the tail. `get_command_or_subagent_output` reads new output and the exit status, and can wait up to 2 minutes. `kill_command_or_subagent` ends the whole process tree, using the same process group on Linux and kill-on-close job object on Windows as the foreground shell. When a task finishes, a short notice is added once at the start of the next model turn. `monitor` streams matching lines from a running task or its own command, for up to 10 hours. Monitors stop on Halt and when the session is deleted. `/bg` on a native chat starts a separate native engine on a fork of the session. It never shows a permission card. In Ask mode its non-read-only tools are refused, as for every unattended run, and Auto uses the auto-review judge. Steer on a native chat now lands at the next tool boundary without stopping the turn. Halt cancels every native run, including `/bg` engines, and kills all their background tasks and monitors. `scheduler_create`, `scheduler_list` and `scheduler_delete` create, list and delete GrokHub automations. There is no second scheduler. Creating or deleting one is never treated as read-only. The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.76.tar.gz` and AUR `pkgver=2.10.76`.
- Windows: `GrokHub-Setup-2.10.76.exe` and `grokhub-windows-v2.10.76.zip`.

## 2.10.75 — 2026-10-03

Native chats (Settings → Labs, still off by default) now track and manage their context window. The window size comes from the `context_length` in `/v1/models` (256k for `grok-4.7` when the field is missing). Token use is estimated locally with the bytes/4 estimator vendored from Grok Build's `xai-token-estimation`, with no tokenizer data. The result shows in the existing context line under the chat. At 85% the engine compacts on its own before the next model call. It asks for a summary using the prompt ported from the Grok CLI's `session_compact.rs`, then replaces older history with that summary. Any leading system message, the open todos and the last user turn are kept verbatim. A compaction marker is written to the session JSONL, so reopening the chat rebuilds the compacted conversation. If the summary call fails, comes back empty or is cancelled, the transcript is left exactly as it was. `/compact` on a native chat does the same on demand. On a Grok CLI chat it still sends the CLI's own `/compact`. When a request body gets close to the proxy's size limit, the oldest inline images are evicted first and replaced with the CLI's placeholder text. The summary call's tokens and cost count in the session usage. The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.75.tar.gz` and AUR `pkgver=2.10.75`.
- Windows: `GrokHub-Setup-2.10.75.exe` and `grokhub-windows-v2.10.75.zip`.

## 2.10.74 — 2026-10-03

Native chats (Settings → Labs, still off by default) are now real sessions. Each is an append-only JSONL file under the GrokHub config folder's `sessions/`: a header line (id, title, created, folder, model), then one line per message, tool call, tool result and usage record, flushed as it happens. Reopening a chat rebuilds the conversation from the file. If the last line was cut off by a crash, it's dropped and the file repaired instead of failing. History lists native sessions next to Grok CLI sessions. CLI rows still come from the same discovery code and stay read-only. Native rows can be resumed, forked into an independent copy, renamed, exported to Markdown and deleted. Delete stops a running native turn for that session before removing the file. Titles come from the first message locally, with no model call. Each turn's tokens and cost are stored and shown, labeled "SuperGrok pool" or "API credits". The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.74.tar.gz` and AUR `pkgver=2.10.74`.
- Windows: `GrokHub-Setup-2.10.74.exe` and `grokhub-windows-v2.10.74.zip`.

## 2.10.73 — 2026-10-03

Auto mode on the native engine (Settings → Labs, still off by default) now reviews instead of always asking. A call the permission engine would have asked about goes to auto-review first. Routine git, the read-only `gh` list and security-finding checks are decided locally on fast paths ported from Grok Build. Anything else gets a short low-effort `grok-4.7` judge call on a capped transcript tail, which answers allow, block or ask. When attended, a block or ask shows the usual permission card with the reason. Unattended, a block goes back to the model as "Auto mode blocked…". A judge error, a timeout (20 s) or an unreadable verdict fails closed: ask when attended, refuse when unattended. Deny rules, explicit ask rules, dangerous or unsplittable commands, desktop tools, Plan and btw never reach the judge. The judge's tokens and cost count in the session usage. Ask and Always are unchanged, and so is the Grok CLI path. The judge hasn't been run against the live API yet.

- Linux: `grokhub-linux-v2.10.73.tar.gz` and AUR `pkgver=2.10.73`.
- Windows: `GrokHub-Setup-2.10.73.exe` and `grokhub-windows-v2.10.73.zip`.

## 2.10.72 — 2026-10-03

The native engine (Settings → Labs, still off by default) gets a real permission engine on top of the 2.10.71 gate. Rules are allow, ask or deny, and deny wins over ask, which wins over allow. `Bash(...)` rules match each command segment by prefix or glob, so `git status && rm -rf /` is never auto-allowed and `Bash(git *)` doesn't match `gitleaks`. Wrappers like `timeout`, `nice`, `env` and leading `VAR=value` are peeled first. Commands the splitter can't classify ask: `$(...)`, backticks, heredocs and the like. `Read`/`Edit`/`Grep` path rules take `**` globs and can't escape the workspace. `MCPTool(server__*)` and `WebFetch(domain:...)` rules are parsed for later phases.

Dangerous commands (`rm -rf`, `sudo`, `dd`, `mkfs`, force-push, `curl … | sh` …) always ask, even in Always mode or with a remembered grant, and are refused in unattended runs. **Allow always** now remembers a grant per project. A short list of read-only commands (`ls`, `cat`, `pwd`, `head`, `tail`, `wc`, `grep`, plain `git status`/`log`/`diff` and similar) runs without a prompt, also in unattended runs. That's the one deliberate loosening against 2.10.71, and it matches the Grok CLI. **Settings → Permissions** lists and edits rules and grants, and can import the `permissions` block of a project's `.claude/settings.json` (read-only). The Grok CLI path is unchanged.

- Linux: `grokhub-linux-v2.10.72.tar.gz` and AUR `pkgver=2.10.72`.
- Windows: `GrokHub-Setup-2.10.72.exe` and `grokhub-windows-v2.10.72.zip`.

## 2.10.71 — 2026-10-03

The native engine (Settings → Labs, still off by default) can now change things, behind a permission gate. New tools: `write`, `search_replace` (exact match, `replace_all`, keeps CRLF files CRLF, one writer per file), `run_terminal_command` (bash on Linux, PowerShell on Windows; 120 s default, 300 s max; output capped), and the desktop tools screenshot, click, move, drag, scroll, type and key, called in-process with no MCP hop. Shell commands run in their own process group on Linux and in a kill-on-close job object on Windows, so Stop, Halt and timeouts end the whole process tree, not just the shell.

The gate: read-only tools always run. In Ask and Auto, anything else shows the usual permission card (Auto stays Ask until the Phase 5 reviewer). Always runs it. Plan and btw keep the read-only set. Unattended runs deny non-read-only tools with the same message as the CLI path. Desktop tools are absent while **Let Grok control the desktop** is off, and refuse while halted or on the lock screen. The Grok CLI path is unchanged. No live call has been made yet.

- Linux: `grokhub-linux-v2.10.71.tar.gz` and AUR `pkgver=2.10.71`.
- Windows: `GrokHub-Setup-2.10.71.exe` and `grokhub-windows-v2.10.71.zip`.

## 2.10.70 — 2026-10-03

A native engine arrives behind a switch. **Settings → Labs → Native engine (no Grok CLI)** is off by default, and while it's off chats launch the Grok CLI exactly as before. With it on, new chats run GrokHub's own read-only agent (new `grokhub-agent` crate, sync, `ureq`). It streams `POST api.x.ai/v1/responses` with model `grok-4.7` and the composer's reasoning effort, plus hosted `web_search` and `x_search`. Retries follow the Grok Build schedule (15 tries, 2 s doubling to 30 s, ±20% jitter, Retry-After). The loop runs until no tool call remains or 50 turns pass, and honors Stop, Halt, steer at the next turn boundary, and a guard against the same call three times in a row. The tools are read-only: `read_file` (PNG/JPEG go back as images), `list_dir`, `grep` (`.gitignore` aware) and `glob`, all confined to the workspace. Any write, edit or shell call is refused. Those chats carry a **Native** badge, and usage shows tokens and cost labeled "SuperGrok pool" (sign-in) or "API credits" (key).

Sign-in is shared. The Imagine sign-in becomes **Sign in with Grok**, and its keychain account moves once from `imagine-oauth` to `xai-oauth`. The bearer is your GrokHub sign-in, then your console API key, otherwise "Sign in with Grok or add an API key." The native engine never reads the Grok CLI's `~/.grok/auth.json`, `config.toml` or `GROK_HOME`, and never calls the CLI proxy. A source-scan test guards that. Parts of the prompt and retry logic are ported from xai-org/grok-build under Apache-2.0 (see `crates/grokhub-agent/NOTICE`). No live call has been made yet.

- Linux: `grokhub-linux-v2.10.70.tar.gz` and AUR `pkgver=2.10.70`.
- Windows: `GrokHub-Setup-2.10.70.exe` and `grokhub-windows-v2.10.70.zip`.

## 2.10.69 — 2026-10-03

Desktop control on KDE Wayland gets fallbacks. If the portal won't hand over libei, input stays in the same RemoteDesktop session through the portal's Notify* calls. If there's no portal session at all, it moves to an absolute uinput pointer (evdev 0.13, Linux only) that spans the union of the outputs from `kscreen-doctor`, so clicks land on absolute coordinates with no acceleration. ydotool is the last resort and is labeled imprecise. Capture tries KWin ScreenShot2, then `spectacle -b -n -o`, then the portal Screenshot, and crops to the requested monitor. xdotool is never used on Wayland.

A small broker owns the desktop session. It listens on `$XDG_RUNTIME_DIR/grokhub/desk.sock` (socket 0600, folder 0700), accepts only same-uid peers, and holds a file lock so two GrokHub processes can't drive the desk at once. The broker checks the **Let Grok control the desktop** switch (off by default), Halt (Ctrl+Alt+H) and the lock screen again on every request, and Ask still runs before any tool call reaches it. A request can't carry its own gate. **Settings → Desktop control** shows the active input and capture backends, and **Test** moves to each monitor's center and captures it. `packaging/udev/60-grokhub-uinput.rules` adds `TAG+="uaccess"`, so the seated user gets `/dev/uinput` without joining the `input` group. The mode stays 0660, never world-writable. The uinput mapping is only covered by unit tests so far. The ±2 px click accuracy still needs a live check on a KDE desktop.

- Linux: `grokhub-linux-v2.10.69.tar.gz` and AUR `pkgver=2.10.69`.
- Windows: `GrokHub-Setup-2.10.69.exe` and `grokhub-windows-v2.10.69.zip`.

## 2.10.68 — 2026-10-02

Desktop control on KDE Plasma 6 Wayland stops fighting the compositor. On a KDE session, GrokHub's desktop tools now use the xdg-desktop-portal RemoteDesktop session with libei for input (ashpd and reis, Linux only). Clicks are absolute moves inside KWin's per-output regions, with no pointer acceleration or calibration. KDE asks "Allow remote control" once. The restore token is kept in the OS keychain (service `GrokHub`, account `desktop-portal-restore`), never in a file, and each session's new token replaces the old one. If KDE asks again, the status says why.

Screenshots on KDE use KWin's `org.kde.KWin.ScreenShot2`, silently and at full resolution. The packaged `grokhub.desktop` now carries `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` so KWin allows it. Monitors come from `kscreen-doctor -j`. If the portal is denied or unavailable, input falls back to ydotool and says it's imprecise. xdotool is never used on Wayland. wlroots desktops keep grim and ydotool, and X11 and Windows are unchanged.

The **Let Grok control the desktop** switch (off by default) and Ask gating are unchanged. Halt (Ctrl+Alt+H) and the lock screen close the portal session, which revokes input at the compositor. This hasn't been run on a live KDE desktop yet.

- Linux: `grokhub-linux-v2.10.68.tar.gz` and AUR `pkgver=2.10.68`.
- Windows: `GrokHub-Setup-2.10.68.exe` and `grokhub-windows-v2.10.68.zip`.

## 2.10.67 — 2026-10-02

Home learns which cards help. Opening, dismissing, More, Less, Hide and Follow up now also update a small local preference file, `card_prefs.json`, in the GrokHub config folder. It keeps decaying weights (14-day half-life) per card type, per source group, and per topic keyword. Keywords are at most three single words taken from the card title at the time of the event, with stopwords, digits and times dropped. It's capped at 200 groups and 200 keywords, and holds no sentences or body text. If the file is missing, the type and group weights are rebuilt from the signal log, which stays text-free.

Home ranks event cards by a simple, deterministic score: a base for the card type, plus how often you open that type, source and topic, plus a little recency. Cards you keep dismissing sink, and a card scoring under 0.15 folds into a **More (n)** row under the deck ("Showing fewer of these; you've been dismissing them"). Failures, pinned cards and cards you worked on are never folded. Hide, Less like this and the once-a-day failure floor still apply first, and Home still shows at most three cards. A new type or source gets a small one-card novelty bump. When one reason clearly dominates, a short hint follows the why line: "You usually open these", "You often open {topic} cards", "New for you" or "Needs a look". **Settings → Behavior → What Home learned** lists the top liked and disliked groups and topics, with Forget on each row and **Reset all**. Reset keeps the signal log. No network or model calls are involved.

- Linux: `grokhub-linux-v2.10.67.tar.gz` and AUR `pkgver=2.10.67`.
- Windows: `GrokHub-Setup-2.10.67.exe` and `grokhub-windows-v2.10.67.zip`.

## 2.10.66 — 2026-10-02

Home cards explain themselves and take feedback. A card that stands for several runs shows **×N runs · latest h:mm**. The count restarts once you open the card, so it means runs since you last looked. Under the body, one muted line says why the card is there:
- "Your automation “name” finished and left a report in Follow up." (or just "…finished." when nothing was filed)
- "“name” failed and needs a look."
- "You saved this schedule."

Home paints at most three event cards, and the open deck stays above the composer instead of covering it. Cards are taller (96 px) to fit the extra lines.

Every event card has a **⋯** menu, which right-click also opens:
- **More like this.**
- **Less like this** keeps that card's group off Home for 14 days.
- **Hide this automation's runs from Home.** Hidden runs still update in place and in Follow up. Automations shows **Hidden from Home · Undo**.

A hidden or muted automation's failure still shows on Home at most once a day.

Card feedback goes to a local signal log, `card_signals.jsonl` in the GrokHub config folder, for the learning step that comes next. It records open, dismiss (and whether the card was opened first), more, less, hide, unhide and Follow up events. Each line holds only the time, card id, kind, group and source, never card text. At 2,000 lines the log rotates to `card_signals.jsonl.1`. Nothing leaves the machine.

- Linux: `grokhub-linux-v2.10.66.tar.gz` and AUR `pkgver=2.10.66`.
- Windows: `GrokHub-Setup-2.10.66.exe` and `grokhub-windows-v2.10.66.zip`.

## 2.10.65 — 2026-10-02

Home stops repeating cards. Each automation, schedule, offer or suggestion now keeps one card, with a stable id built from its source (or a hash of the normalized title when it has none) instead of the run time. A repeat run updates that card in place: newest title, body and time, back to unread, and a count of the runs it stands for. A failed run replaces the finished card for the same automation. Filing a run in Follow up points the automation's one card at the workboard instead of rewriting every older copy, which is how four identical "In Follow up on your workboard" cards showed up. Dismissing a card clears its whole group and keeps a hidden marker for 24 hours, so a repeat of that run doesn't come back that day. A failure still shows. Saved feeds are deduped once on load: duplicate runs fold into the newest card with their run count, and the file is only rewritten when something changed.

- Linux: `grokhub-linux-v2.10.65.tar.gz` and AUR `pkgver=2.10.65`.
- Windows: `GrokHub-Setup-2.10.65.exe` and `grokhub-windows-v2.10.65.zip`.

## 2.10.64 — 2026-10-02

Repo rules for new Cursor and Claude chats. `CLAUDE.md` and `.cursor/rules/repo-gates.mdc` write down the real gates: branch off `main` and never push to it, no secrets or credential files, the exact CI test and clippy commands, the version-bump file list, the 12,000-byte composer source-scan window, no merges without Jeremy's OK, and no tag or release without his "full ship". `.cursor/rules/merge-prs.mdc` now agrees with them: no merges, tags or releases unless Jeremy says so. `.gitignore` also covers `target/` anywhere, key and certificate files (`*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.crt`, `id_rsa*`), and `auth.json`, `secrets.json` and `credentials.json`, while still allowing `.env.example`. No tracked file is ignored. The working tree and full git history were scanned for committed secrets (gitleaks 8.30.1 plus targeted greps). Nothing is in the current tree. One historical finding was reported separately for a rotation decision, and history was not rewritten. The app itself is unchanged.

- Linux: `grokhub-linux-v2.10.64.tar.gz` and AUR `pkgver=2.10.64`.
- Windows: `GrokHub-Setup-2.10.64.exe` and `grokhub-windows-v2.10.64.zip`.

## 2.10.63 — 2026-10-02

Imagine has its own Grok sign-in and the full Imagine API. **Sign in with Grok for Imagine** on the Imagine page opens xAI in the browser (PKCE on a one-time `127.0.0.1` callback, or **Use a code instead** for a device code), and the tokens live only in the OS keychain (Windows Credential Manager, or the Secret Service on Linux), never in a file or a log. Imagine uses that sign-in first, then a console API key; it no longer borrows the Grok CLI login. If xAI doesn't allow the sign-in to use the Imagine API, Imagine says so and offers **Use API key**. Images: Generate or Edit, models `grok-imagine-image-2.0` (default), `grok-imagine-image-quality` and `grok-imagine-image`, 1–10 at a time, 1k or 2k, every aspect ratio, quality on 2.0, and edits from up to three source images with an optional mask. Video: text-to-video, image-to-video, edit and extend, with `grok-imagine-video-1.5` (1080p on text and image to video) or `grok-imagine-video`, 1–15 s and audio on or off. Results show in a grid with Save and Open folder.

- Linux: `grokhub-linux-v2.10.63.tar.gz` and AUR `pkgver=2.10.63`.
- Windows: `GrokHub-Setup-2.10.63.exe` and `grokhub-windows-v2.10.63.zip`.

## 2.10.62 — 2026-10-02

Grok can control the desktop precisely through GrokHub. Turn on **Settings → Let Grok control the desktop** (off by default) and GrokHub registers its own desktop tools with Grok Build as a local MCP server (`grokhub --mcp-desktop`, server `grokhub-desktop`): `list_monitors`, `screenshot` (per monitor or the whole desktop, with exact geometry and scale), `click` (button, double), `move`, `drag`, `scroll`, `type`, and `key` (combos like `ctrl+shift+t`). Coordinates are in the screenshot's pixels and map back exactly to real screen pixels per monitor, DPI-aware, instead of Grok guessing with xdotool or PowerShell. Windows uses xcap capture and SendInput (no extra programs). Linux X11 uses the X server directly (RandR, XTEST); Wayland uses grim and ydotool when they are installed and says what is missing when they are not. The registration goes into the cabin's own Grok home (`grok mcp add`), never `~/.grok`. Ask still asks before every desktop tool in a watched chat; unwatched Ask runs (background, automations, night, phone), Plan and btw deny them; Auto and Always allow them. Turning the switch off refuses the tools at once. Halt (Ctrl+Alt+H) stops desktop actions, the lock screen blocks them, and nothing reads the keyboard or clipboard.

- Linux: `grokhub-linux-v2.10.62.tar.gz` and AUR `pkgver=2.10.62`.
- Windows: `GrokHub-Setup-2.10.62.exe` and `grokhub-windows-v2.10.62.zip`.

## 2.10.61 — 2026-10-02

A message typed while Grok is still replying steers the reply. Enter stops the turn where it is, keeps what it already said and did in the chat, and carries on with your message and a short note of that progress, so finished steps are not redone. Alt+Enter, or `/queue <message>`, holds the message until the reply ends, which is what Enter used to do. With text typed during a reply, a row above the composer offers Steer and Queue, and lists queued messages with Steer now and Remove.

Work can run in the background beside the chat. `/bg <task>` starts one on a fork of the chat's Grok session. **Background** next to the Running pulse, or a bare `/bg`, moves the live reply off the composer so you can keep chatting; its answer posts on that chat when it ends. Sending in another chat now moves a reply still running there to the background instead of stopping it. Grok can start background work itself with a `BACKGROUND_TASK:` line. Up to three run at once, each listed above the composer with its time, last tool, and Stop. `/bg stop` stops them all, tray Halt and Ctrl+Alt+H stop them with the live turn, and the next turn on a chat is told what its background work found. With Ask on, `/bg` and Grok's `BACKGROUND_TASK:` lines are refused, because a background run can't ask for approval (switch to Auto). Unwatched runs under Ask (background, automations, night, phone) pass Grok Build `--permission-mode dontAsk` and `--deny` rules for shell, edit, and write. Deleting a chat stops its background tasks. They end when the cabin quits.

- Linux: `grokhub-linux-v2.10.61.tar.gz` and AUR `pkgver=2.10.61`.
- Windows: `GrokHub-Setup-2.10.61.exe` and `grokhub-windows-v2.10.61.zip`.

## 2.10.60 — 2026-10-02

Windows reads the day and the time from the system clock. The cabin used to ask the `date` program, which Windows does not have, so there the day stayed 1970-01-01 and every clock read noon Monday: usage never rolled over to a new day, the nightly review and session suggestions never came due (a fixed noon never reaches 9 pm, and a stuck day never changes), and scheduled automations fired at the wrong time.

Stop ends a headless Grok Build run on Windows. It used the Unix `kill` command there, so grok kept running after Stop. It now uses `taskkill` on the process tree, like the other Windows stops.

Devices shows a LAN pairing address on Windows and macOS. Neither has `hostname -I`, so the phone was offered `http://127.0.0.1`. The cabin now asks the OS which local address it would route from.

Dismissing sticks. Delete on the Ideas board remembers the idea, and later ideas on the same topic stay off the board, even when the nightly review or the ideas call words them differently. The review and the ideas call are told what you turned down. Taking an idea off the home feed with × keeps it on the Ideas board and counts as a no for new ideas on that topic. The cabin stopped remembering new idea titles after its first 64, so older installs could see a deleted idea come back; it now keeps the newest 64. A situation card you opened (for example "That job is still paused") is no longer posted again on the next tick, which is how the same card could show up twice in a row.

Workboard cards can be deleted. The card's ··· menu has Delete, with a second step so a stray click does not remove anything, and archived cards have Delete next to Restore. The card's chat stays in History. A card whose chat is still running asks you to stop it first.

Background work always runs at low reasoning effort: automations, `/loop` runs, phone tasks, the nightly review, ideas, the daily read, chips, the greeting, and the thread goal. What you type in a chat or on a card keeps the effort you picked.

Also fixed: `/project` followed by a word with accented or non-Latin letters (for example `/project aé€`) crashed the cabin. Quitting now waits for a save that is still writing, and `app.json` is no longer written by two savers at once. On Linux, the browser opener, video players, notifications, and the voice player are reaped when they exit instead of lingering as zombie processes. A heartbeat automation longer than a day runs daily, matching its next-run time and its label, instead of sitting overdue. The LAN hub compares access tokens in constant time and answers 503 past 64 requests at once instead of starting a thread for each.

- Linux: `grokhub-linux-v2.10.60.tar.gz` and AUR `pkgver=2.10.60`.
- Windows: `GrokHub-Setup-2.10.60.exe` and `grokhub-windows-v2.10.60.zip`.

## 2.10.59 — 2026-10-02

A soft glow can breathe around the chat composer while Grok replies. It stays off until Settings → Behavior → Composer glow (GPU effects). Turning it on uses the GPU renderer after a restart. If that renderer fails, or the last launch died while it was starting, the cabin comes back on OpenGL, turns the switch off, and says so once.

- Linux: `grokhub-linux-v2.10.59.tar.gz` and AUR `pkgver=2.10.59`.
- Windows: `GrokHub-Setup-2.10.59.exe` and `grokhub-windows-v2.10.59.zip`.

## 2.10.58 — 2026-10-02

Links in chat open in your browser. Every link, the MCP sign-in Open button, and the xAI sign-in go through one opener that takes only http and https and hands the address to the system without a shell (ShellExecuteW on Windows, `xdg-open` on Linux). The old Windows path ran `cmd /C start` with the address, so a server could slip a command in after an `&`.

Copy reads Copied on the button you clicked for a moment. The Mode, Permission and Effort menus open above their pill. Esc closes the plus menu, Add to folder, the avatar menu, Shortcuts and Upload. The Shortcuts sheet is a table grouped by scope. Double-click the empty title bar to maximize, and the window buttons say what they do. Tab and the arrow keys show a focus ring. Custom buttons, pills, tabs and switches carry names for screen readers. Connectors says Loading… while a list is still coming and None matched. when search hides everything. Inline code and tool output are easier to read, small labels are at least 12 px, and Windows code uses Cascadia Mono or Consolas. The run dot and the composer stop repainting every frame while nothing moves. Menus, sheets and cards share one set of corner radii.

Halt is now Ctrl+Alt+H. Ctrl+Shift+Esc opens Task Manager on Windows and never reached the cabin.

- Linux: `grokhub-linux-v2.10.58.tar.gz` and AUR `pkgver=2.10.58`.
- Windows: `GrokHub-Setup-2.10.58.exe` and `grokhub-windows-v2.10.58.zip`.

## 2.10.57 — 2026-10-01

The cabin window moves from egui 0.29 to 0.36 and stays on the OpenGL renderer. The frameless title bar, the tray, and the menus stay where they were.

A long History, a deep project tree, and a tall workboard column only draw the rows on screen. A row you have scrolled past keeps its place, so the scrollbar and the order do not change. Scroll it back and it has the same click, hover, drag, and right-click menu. The workboard card that is open, and the card you are dragging, still paint when they sit past the edge. The project asks for Rust 1.95.

- Linux: `grokhub-linux-v2.10.57.tar.gz` and AUR `pkgver=2.10.57`.
- Windows: `GrokHub-Setup-2.10.57.exe` and `grokhub-windows-v2.10.57.zip`.

## 2.10.56 — 2026-10-01

The cabin keeps going when a first look comes back empty or wrong. It tries one other path, then says what blocked it and the next useful step. It still asks before sending, paying, deleting, or publishing something you did not name, and it does not invent a source, a count, or a fact.

The home feed writes a daily read. Once a day, and only when the last one is not still unread, it looks up two short pieces: a news note and a longer story. Each one says why it matters to you and carries one real link from that lookup. If the lookup finds nothing worth your time, you get one honest card. A story may mention weather, mail, a calendar, or a bank. An offer to send, pay, delete, or publish on its own is not posted. News stays on the feed and does not ping the desktop.

One situation card can sit with that read. A workboard job left on "Paused. This is where to resume." for half an hour asks if you want it picked back up. A chip action you repeat at least three times, on With or Quiet, asks if you want that as a reminder. The same two signals can also leave an idea on the board, and that idea does not take the home pin.

Home feed cards no longer carry Up, Discuss, Delete, Accept, or Open. Click the card to finish with it: a digest or a suggestion opens its chat, an idea opens on the Ideas board, a finished run opens its chat or follow-up, and an automate offer opens the schedule box without saving until you press Add. The × in the corner clears the card. On an idea it only leaves the home feed. On a suggestion it stays gone. Quiet hours still hold a card and release it later. A situation card pings only when the cabin window is unfocused and quiet hours are off.

- Linux: `grokhub-linux-v2.10.56.tar.gz` and AUR `pkgver=2.10.56`.
- Windows: `GrokHub-Setup-2.10.56.exe` and `grokhub-windows-v2.10.56.zip`.

## 2.10.55 — 2026-10-01

Ideas need a reason now. One ask used to fan out into a pile of cards: the nightly review was told to answer "a repeated ask" with a skill and an automation, and the ideas call added up to four more, so installing a driver once could leave five cards about that driver. Now an automation, reminder, or skill idea needs work you repeat (two or more asks on the topic that are not one-time jobs) or lasting context (your memory, USER.md, an open workboard card). Installs, one-off fixes, and setups get nothing, even when they took several tries. The model sees your asks grouped by topic with counts and one-time jobs marked, writes a reason for each idea (shown on the card), and keeps to one idea per need. The cabin checks it again: one card per topic, none on a topic already on the board or turned down. The same check runs on the nightly review's skills and automations, and the generic "Session habit" skill that fired whenever you typed "when I" twice is gone. Untouched ideas that only answer a one-time job are cleared once at launch; cards you opened, changed, filed, or talked about stay.

The home feed gets one new idea per batch instead of the whole batch, and nightly skill ideas wait on the Ideas board.

Workboard cards are short now: a title, one line, and what the card has (a chat, notes, a new report). Hover a card, or click it, and it opens in place with its full text, notes, and a chat with the agent. The chat uses the Chat page's bubbles, markdown, thoughts, and tool rows, and streams the turn while it runs. Enter sends, Shift+Enter is a new line, and Stop ends the turn. Move, edit, notes, link, and archive moved into the card's ··· menu. Work on it starts the card's chat right there instead of sending you to Chat. Starting a drag folds an open card back, so it moves as one small card.

New Follow up row above the columns. A scheduled run that leaves you something to read or act on (a report, summary, or check you asked for, a question, or a problem) files a Follow up card with its own chat that opens on the report. A chore that just did its job (e.g. "Cleaned 12 files") or a short "nothing new" files nothing. Each automation keeps one open card. A later run adds its report to the same chat, marks the card NEW, and hands the agent the new report with your next reply. The run's card on the home feed opens that Follow up card. Follow up chats are listed in History like any other chat.

Also fixed: the shared Background chat for `/loop` runs could pick an idea card's hidden chat. Notes from your last Chat message could also ride along into an idea card's chat.

- Linux: `grokhub-linux-v2.10.55.tar.gz` and AUR `pkgver=2.10.55`.
- Windows: `GrokHub-Setup-2.10.55.exe` and `grokhub-windows-v2.10.55.zip`.

## 2.10.54 — 2026-09-30

Tool rows keep their names on a finished turn. Grok Build sends `tool_call_update` events without a title, which parse as the placeholder `Tool`, and each update overwrote the real name in both the live rows and the saved turn, so every tool row read `Tool · 32GB` instead of `run_terminal_command`. An update with no title, or only the placeholder, now keeps the name from the call, the same rule the tool cards already follow. A real new title still replaces the old one. The status and detail still update.

A replay of a real Grok Build 1.0.46 Auto-mode turn (reply, tool, reply, tool, reply) now checks that the finished turn keeps three separate replies and two named tool rows, and that they come back the same after History is saved and reloaded. The saved turn format is unchanged. Turns saved by 2.10.48–2.10.53 that already say `Tool` keep saying it, because the name was never stored.

- Linux: `grokhub-linux-v2.10.54.tar.gz` and AUR `pkgver=2.10.54`.
- Windows: `GrokHub-Setup-2.10.54.exe` and `grokhub-windows-v2.10.54.zip`.

## 2.10.53 — 2026-09-30

An idea card's chat shows only the agent's replies. With replies now stored beside their thoughts and tool runs, the card was about to show those too. The new action the agent proposes (`CARD_ACTION:`) is read from its reply only and the newest one wins, so a thought that mentions the tag no longer rewrites the card.

Apply or Delete on an idea removes that card's hidden chat and its Grok Build session instead of leaving them behind. Only the card's own hidden (background) chat is removed; a chat you can see in History is never touched. The chat box inside a card keeps focus after Send, so the card stays open while you wait for the answer. An automation idea without a clear time is sent to chat as a scheduled automation, not a recurring one, so one-off reminders read right.

Suggest ideas gives up on an ask that has not answered in three minutes, so the button no longer says Thinking… for good.

- Linux: `grokhub-linux-v2.10.53.tar.gz` and AUR `pkgver=2.10.53`.
- Windows: `GrokHub-Setup-2.10.53.exe` and `grokhub-windows-v2.10.53.zip`.

## 2.10.52 — 2026-09-30

A long agent turn stays smooth while it streams. Before, every stream delta rebuilt and rescrubbed the whole transcript's views and the token estimate, and every row of the live turn re-laid its markdown each frame. Now a delta that only grows the streaming reply leaves the transcript caches alone until the turn ends (an edit anywhere before the last message still rebuilds them at once), and live rows scrolled out of view reserve the height they last painted at. The row still streaming, and any row whose fold changes (a click, Minimize all), is always measured again, and a new turn never reuses the last turn's heights.

Saving History no longer copies every chat into a `serde_json::Value` tree first: `threads.json` and `chat.json` serialize straight from the chats and only take the 2.10.50 fit-under-the-cap path when they are over 512 MiB. Keys in `threads.json` now follow the struct's field order instead of alphabetical order; any version reads either.

- Linux: `grokhub-linux-v2.10.52.tar.gz` and AUR `pkgver=2.10.52`.
- Windows: `GrokHub-Setup-2.10.52.exe` and `grokhub-windows-v2.10.52.zip`.

## 2.10.51 — 2026-09-30

A failed turn no longer reads as an answer. A reply that starts with `Error:` (what a failed turn leaves in the chat) gets a faint red wash and a red hairline.

Help lines under section labels paint `backtick` spans as monospace code instead of literal backticks, and section labels are brighter than the muted help under them. Ideas uses the same page header as Automations, Skills and Workboards. An empty Workboard shows only the "No cards yet" tile, which now opens the add-card form, instead of a row of empty columns. The Grok Build skills list says whether it is loading, matched nothing, or found none.

Steadier chrome: the titlebar update chip keeps a fixed height instead of stretching to the bar, pill and tab labels are centred vertically, rail icons and labels no longer slide left on hover, and the projects list ends on a whole row. Live green uses a darker shade on light surfaces so it stays readable.

- Linux: `grokhub-linux-v2.10.51.tar.gz` and AUR `pkgver=2.10.51`.
- Windows: `GrokHub-Setup-2.10.51.exe` and `grokhub-windows-v2.10.51.zip`.

## 2.10.50 — 2026-09-30

History no longer disappears once it passes 32 MiB. A few chats with pasted screenshots could push `threads.json` over the store cap, and the next launch set the whole file aside as `threads.json.corrupt-<time>` and opened an empty History. Chat history (`threads.json` and `chat.json`) now has its own 512 MiB cap, and a save never writes more than a launch reads: past the cap, the largest messages in the saved copy give way to a short note that says so, biggest first, until the rest fits. If even that cannot fit, nothing is written and the file already on disk stays as it was. A file that is actually torn or unreadable is still set aside, as before.

If an earlier version already set your History aside, the next launch brings it back. Every `threads.json.corrupt-*` copy that still reads is merged into History next to the chats started since (the live copy wins when both have the same chat), then renamed `threads.json.recovered-*`. A copy that does not read stays where it is. Nothing is deleted.

Opening a History over 32 MiB in 2.10.48 or older sets it aside again; this version brings it back on its next launch.

- Linux: `grokhub-linux-v2.10.50.tar.gz` and AUR `pkgver=2.10.50`.
- Windows: `GrokHub-Setup-2.10.50.exe` and `grokhub-windows-v2.10.50.zip`.

## 2.10.49 — 2026-09-30

A tool ask no longer reads as "User cancelled". When Grok asked for two tools at once, the second permission card replaced the first and cancelled it, so the agent saw a cancel the user never made. Asks now queue: the card on screen stays until it is answered, then the next one shows.

Deny now sends the agent's own `reject_once` option, so a refusal reads as a refusal, and falls back to cancel only when the agent offers no reject option. Stop, the end of a turn, a stream error, and a replay still cancel the card on screen and every queued ask, so a real cancel still reads as a cancel.

The card's keys answer only on a bare key press with nothing over the chat: typing in the rename field, the find bar, a menu, or a confirm sheet no longer approves or denies a tool by accident.

- Linux: `grokhub-linux-v2.10.49.tar.gz` and AUR `pkgver=2.10.49`.
- Windows: `GrokHub-Setup-2.10.49.exe` and `grokhub-windows-v2.10.49.zip`.

## 2.10.48 — 2026-09-30

A finished reply stays the way it streamed. Each message Grok sends between tool calls is its own bubble, thoughts stay in their own rows, and tool calls sit where they ran. Before, the end of a turn folded every progress line into one long bubble. Messages split by a tool call no longer run together (`hit.GrokHub`), and a thought with several paragraphs no longer spills into the reply.

Back-to-back tool calls share one quiet row, `3 steps · Grep, Read file, Run terminal command`, that opens to each call. A single call says what it touched (`Read src/main.rs`, `Grep · 3 matches`), a failed call is marked, and the detached Work tree under a finished turn is gone. Copy and Reply sit under the last reply of a turn instead of under every progress line.

A step that runs a host command or starts Imagine still shows as a thought, not as the answer, and the text of a command is never split to add a paragraph break. A saved tool row keeps a short title and detail, and a failed tool partway through a turn no longer switches the quick chips to error.

Turns saved before this version keep their old single-bubble form.

- Linux: `grokhub-linux-v2.10.48.tar.gz` and AUR `pkgver=2.10.48`.
- Windows: `GrokHub-Setup-2.10.48.exe` and `grokhub-windows-v2.10.48.zip`.

## 2.10.47 — 2026-09-30

A long chat stays smooth. Frame time in the Chat pane no longer grows with the transcript: a 150-turn chat drops from about 37 ms to under 1 ms per frame. Each thought is keyed once when the chat changes, not scrubbed and hashed on every frame, and a row scrolled out of view no longer resolves its fold. The fork offer's turn count and token estimate are worked out when the chat changes, a live thought rekeys only while it grows, tool cards are no longer copied every frame, and a chat scrolled out of the rail skips laying out its title. Folds, the fork offer, and the rail look and behave the same.

An edit inside the transcript that keeps the message count and the last message's length still refreshes the chat and the token estimate, and a secret redacted inside a live thought rekeys that thought.

- Linux: `grokhub-linux-v2.10.47.tar.gz` and AUR `pkgver=2.10.47`.
- Windows: `GrokHub-Setup-2.10.47.exe` and `grokhub-windows-v2.10.47.zip`.

## 2.10.46 — 2026-09-30

Ideas are short cards you can read at a glance: the type (Skill, Automation, or Suggestion), a title, and one short line. Hover a card, or click it, and it opens in place with the details, the action Apply runs, and a chat with the agent about that card. The agent answers inside the card and can rewrite the action; nothing runs until you press Apply. Move away and the card folds back, keeping your edits and the chat.

Apply follows the type: a Skill is saved, an Automation is scheduled (or set up in chat when it has no clear time), and a Suggestion is sent in a new chat. The board keeps the newest 15 ideas and newer ones push the oldest out. A card you edited or talked through stays until you apply or delete it, outside the 15, up to 10 at a time. New ideas pop up on the home feed; dismissing one there leaves it on the Ideas board.

Suggested skills from the nightly review are Skill ideas now. The Skills page no longer has a Suggested section or an Add button.

Workboard card buttons work again: drag a card by the grip beside its title. A card takes notes, and Work on it opens a new chat with the card and its notes; notes changed later reach that chat on its next turn.

- Linux: `grokhub-linux-v2.10.46.tar.gz` and AUR `pkgver=2.10.46`.
- Windows: `GrokHub-Setup-2.10.46.exe` and `grokhub-windows-v2.10.46.zip`.

## 2.10.45 — 2026-09-30

The welcome line is a greeting again. The fast model call printed its own reasoning ("I'll use the user's name if known…") and the cabin painted that; it now reads only the reply, and a line that talks about the task never paints.

Quick chips stop echoing what you just typed. A typed prompt becomes a chip after you have typed it three times; a chip you picked still counts right away.

Skills are written only for a reusable procedure: at least two commands, a task rather than feedback on the last try, and a fix, build, or routine worth repeating. They are named after what they run (`fix-the-cause-cargo`), not the sentence you typed. Leftover auto-made skills that never ran move to `skills/.retired` on launch; a skill you wrote yourself stays, and nothing is deleted.

Ideas are written by the model from your own work: your notes, recent asks, open cards, and the automations and skills you already have. Each one is an automation, a reminder, a skill, or something to try, with the exact message that does it; Accept puts that message in the draft. Suggest ideas on the Ideas page asks now. The template cards ("A chip for the next step", "Set up: …", "Remind me later") are gone.

A memory file you switch away from is saved into the config directory it was opened from, like the other background saves.

- Linux: `grokhub-linux-v2.10.45.tar.gz` and AUR `pkgver=2.10.45`.
- Windows: `GrokHub-Setup-2.10.45.exe` and `grokhub-windows-v2.10.45.zip`.

## 2.10.44 — 2026-09-30

Arrows, checks, math signs, and Cyrillic or Greek paint instead of a tofu box ("Settings → Update"): an unsubset Inter Regular sits behind the latin statics as a fallback. Every text box placeholder is muted, so an empty field no longer reads as typed text, and the composer placeholder is a little easier to read. The rail footer is pinned: in a short window History clips before the avatar, which is the rail's door to Settings. The session row wraps, so a narrow pane drops Effort to its own line instead of clipping it, and the dividers are hairlines. Model and effort pills size to their label and show a down chevron. Light borders are one step darker so card edges show on the panel, and text selection uses its own colour instead of the hover fill. An install error that already names the install command does not print it twice.

- Linux: `grokhub-linux-v2.10.44.tar.gz` and AUR `pkgver=2.10.44`.
- Windows: `GrokHub-Setup-2.10.44.exe` and `grokhub-windows-v2.10.44.zip`.

## 2.10.43 — 2026-09-30

Replies render numbered lists, task lists, tables, quotes, rules, links, and strikethrough. Code blocks are coloured by language and each one has its own Copy. Links open only for http and https. Your own messages get Edit, which puts the text back in the composer; a draft you already typed stays on top. Ctrl+F finds text in the open chat: Enter and Shift+Enter step through the matches, the picked message gets a ring, and Esc closes it. Enter in the find box does not answer a permission card.

Automations remember how their last run ended. A failed run shows a red line on the Automations page with the reason and how many runs in a row failed, and the first failure posts one feed card. A halt is not a failure. A good run clears it.

Settings → Behavior has a daily token budget. The cabin warns once at 80% and once when it is used up. With Pause scheduled work over budget on, night jobs, loops, and anticipate wait until tomorrow; chat still sends. `/usage` shows the budget. Grok Build reports tokens, not prices, so the budget is in tokens.

Drag a workboard card onto another column to move it. While a file is dragged over the cabin, a card says what the drop will do, and a drop of several files says how many were left out. `/export html` writes a standalone page and `/export json` writes the raw transcript, next to `export.md`.

- Linux: `grokhub-linux-v2.10.43.tar.gz` and AUR `pkgver=2.10.43`.
- Windows: `GrokHub-Setup-2.10.43.exe` and `grokhub-windows-v2.10.43.zip`.

## 2.10.42 — 2026-09-29

Cabin tests that never ask the agent set `GROKHUB_GROK` to a missing path. The suite stays on the offline path and leaves the real Grok CLI idle.

- Linux: `grokhub-linux-v2.10.42.tar.gz` and AUR `pkgver=2.10.42`.
- Windows: `GrokHub-Setup-2.10.42.exe` and `grokhub-windows-v2.10.42.zip`.

## 2.10.41 — 2026-09-29

The Send/Stop disc sits inside the composer pill, beside the mic. The text field leaves room for the mic, so the disc is no longer drawn on the rounded end.

- Linux: `grokhub-linux-v2.10.41.tar.gz` and AUR `pkgver=2.10.41`.
- Windows: `GrokHub-Setup-2.10.41.exe` and `grokhub-windows-v2.10.41.zip`.

## 2.10.40 — 2026-09-29

Connectors lists Grok Build hooks and whether each MCP server is connected or needs sign-in. `/hooks` opens that Hooks section. Doctor still runs from the button. Sign-in stays a note that points at Grok Build.

- Linux: `grokhub-linux-v2.10.40.tar.gz` and AUR `pkgver=2.10.40`.
- Windows: `GrokHub-Setup-2.10.40.exe` and `grokhub-windows-v2.10.40.zip`.

## 2.10.39 — 2026-09-29

Skills → Workflows can pause, resume, and stop a run. `/workflow pause`, `/workflow resume`, and `/workflow stop` follow the permission pill. Empty `/workflow` and `/workflows` open Skills on the Workflows section. Grok Build does not list live runs here yet.

- Linux: `grokhub-linux-v2.10.39.tar.gz` and AUR `pkgver=2.10.39`.
- Windows: `GrokHub-Setup-2.10.39.exe` and `grokhub-windows-v2.10.39.zip`.

## 2.10.38 — 2026-09-29

Quick chips keep a hand cursor on the ×. Hover stays in the chip's slot. A background save writes threads into the config directory captured when it was scheduled, so the next cabin does not load that chat. A shell echo on Windows waits out PowerShell startup and still lands on the open chat.

- Linux: `grokhub-linux-v2.10.38.tar.gz` and AUR `pkgver=2.10.38`.
- Windows: `GrokHub-Setup-2.10.38.exe` and `grokhub-windows-v2.10.38.zip`.

## 2.10.37 — 2026-09-29

Welcome, quick chips, ideas, skills, and automations read why a turn happened. A reminder, a Friday routine, a failure, or a next step becomes a chip, a skill, and an automation in the cabin's own words. A chip, a greeting, or a nightly suggestion that repeats the sentence is dropped. Accept on an idea still opens the small talk. The note is already filled with the why and the help, and you can edit it before sending. It does not open on its own after every message.

The sidebar highlight stays with the pointer. The history search, the Projects header, and the new-folder plus are not rows. Crossing them moves the bar to the nearest page, project, or chat instead of jumping back to the open page. Leaving the sidebar rests the bar on that page.

A blocked workboard card resumes when the next ask has the same title. A different ask leaves that card and starts a new one.

- Linux: `grokhub-linux-v2.10.37.tar.gz` and AUR `pkgver=2.10.37`.
- Windows: `GrokHub-Setup-2.10.37.exe` and `grokhub-windows-v2.10.37.zip`.

## 2.10.36 — 2026-09-28

Shared buttons stay quiet. Hover scales to 1.035, rises 1px, and eases out over 120ms. One highlight glides across the quick chips, the session and permission segments, and the sidebar rail. On an empty chat the idea deck rests in the gap under the composer, centered between the chat box and the bottom of the window. Hover still slides it up over that composer, and it stays inside the chat pane. The attach control is a paperclip that leans open while the pointer is on it. Ask anything is a faint placeholder. The paperclip, that line, the mic, and send share the pill's vertical center, with more room inside the rounded ends.

- Linux: `grokhub-linux-v2.10.36.tar.gz` and AUR `pkgver=2.10.36`.
- Windows: `GrokHub-Setup-2.10.36.exe` and `grokhub-windows-v2.10.36.zip`.

## 2.10.35 — 2026-09-28

Buttons share one glass pill: a faint fill, a hairline ring, and a 1px highlight on the top edge. Solid, ghost, and danger are the same shape. Hover scales to 1.035, rises 2.5px, and eases out over 280ms. The open idea deck stays inside the home chat. It covers the composer there and does not paint over the sidebar, titlebar, or other windows.

- Linux: `grokhub-linux-v2.10.35.tar.gz` and AUR `pkgver=2.10.35`.
- Windows: `GrokHub-Setup-2.10.35.exe` and `grokhub-windows-v2.10.35.zip`.

## 2.10.34 — 2026-09-28

On the chat screen the idea and suggestion cards rest as a deck. The front card stays full size. Two cards sit just behind it, shifted down and slightly smaller, and a count shows how many are in the deck. Hover slides those cards up, over half a second, until each one is full size and stacked above the one in front. The open deck paints on top of the chat box. Hover a card that has slid up and it lifts a little further, and it stays there until the pointer returns to the deck. The Ideas page is still a scroll.

- Linux: `grokhub-linux-v2.10.34.tar.gz` and AUR `pkgver=2.10.34`.
- Windows: `GrokHub-Setup-2.10.34.exe` and `grokhub-windows-v2.10.34.zip`.

## 2.10.33 — 2026-09-28

On the chat screen, ideas and suggestions sit in a pile of three with a count. Hover opens every card one above the next, on top of the chat box, with no scroll. A card behind the front one lifts out to full size and stays up until the pointer returns to the pile. The front card stays put. The Ideas page is still a scroll.

- Linux: `grokhub-linux-v2.10.33.tar.gz` and AUR `pkgver=2.10.33`.
- Windows: `GrokHub-Setup-2.10.33.exe` and `grokhub-windows-v2.10.33.zip`.

## 2.10.32 — 2026-09-26

The Ideas page is a scroll of cards, with no brief box and no search. Accept opens a small centered talk that stays off History. The home feed pins at most three ideas once. Dismissing one there leaves it on the board. The board holds twenty. Untouched cards sink, and a learned setup can replace a stale one.

Quick chips learn from use and from the hour they were dismissed. A new empty home teaches. A fluent session stays quiet. A model is asked for chip ideas only in the middle pace, after two user turns, and at most once every thirty minutes.

Ideas, chat, Imagine, skills, automations, and the workboard each keep their own notes. One engine reads those notes and tells the other parts what to change. A later contradiction replaces the old instruction. Chips do not feed that engine. The nightly model review waits until eight new turns.

Upload shows every file. A selected file stays on a chip and the next message tells the agent where it is. The contents are not pasted into the box. The box stops growing after six lines. Learn-map sessions stay off History. Project chats nest under their folder.

- Linux: `grokhub-linux-v2.10.32.tar.gz` and AUR `pkgver=2.10.32`.
- Windows: `GrokHub-Setup-2.10.32.exe` and `grokhub-windows-v2.10.32.zip`.

## 2.10.31 — 2026-09-26

Startup keeps a chat that still has a Grok session, plan, or goal, even when the transcript has not been copied into the thread yet. Sessions saved under `~/.grok` that History no longer points at show up again. A save that shrinks history copies the previous `threads.json` to `threads.json.bak` and does not replace that backup with a smaller file. Opening one of those rows loads the session transcript.

## 2.10.30 — 2026-09-26

Auto and Always stay on `grok -p`. Routing them through agent stdio loaded `~/.grok` MCP servers, the child was SIGTERM'd, and the cabin retried that forever while the status stayed on Thinking. A killed turn is retried once.

## 2.10.29 — 2026-09-26

Ask, Auto, and Always keep one Grok process for the chat. A background task, monitor, or `/loop` stays up after the turn, and a new message prompts that session instead of killing it. Stop still ends it. Connector commands use `~/.grok` and wait through MCP startup. The cabin leader socket stays private. Night and phone tasks stay headless and are not killed at 300 seconds.

- Linux: `grokhub-linux-v2.10.29.tar.gz` and AUR `pkgver=2.10.29`.
- Windows: `GrokHub-Setup-2.10.29.exe` and `grokhub-windows-v2.10.29.zip`.

## 2.10.28 — 2026-09-25

The projects section `+` makes a folder. It does not offer New project, and a folder menu does not offer New project here. Folders, collapse, and the chats listed under a folder stay. Those chats stay out of History. Clicking a chat in History opens it and leaves that row where it is. A new message moves the row. Selecting, focusing, or opening a row is not activity. Pins stay last-pinned-first. A project click does not open the Workboard.

- Linux: `grokhub-linux-v2.10.28.tar.gz` and AUR `pkgver=2.10.28`.
- Windows: `GrokHub-Setup-2.10.28.exe` and `grokhub-windows-v2.10.28.zip`.
- Cursor cabin 2.10.28. VERSION 2.10.28. Draft only. Not tagged. Based on main 2.10.27 (`c2f6ff1`).

## 2.10.27 — 2026-09-25

A project is a chat. It lives in its folder in the project section and does not also appear in History. Clicking it opens that chat. Every chat that belongs to the folder or to a project inside it is listed under the open folder. A folder is the only collapsible container. Clicking it lists those chats and does not open a chat. Collapsing the folder hides them. Expanding it shows them again. History does not gain, lose, or filter rows because a folder was collapsed or a project chat was opened. Pins and transcripts stay. A project click does not open the Workboard.

- Linux: `grokhub-linux-v2.10.27.tar.gz` and AUR `pkgver=2.10.27`.
- Windows: `GrokHub-Setup-2.10.27.exe` and `grokhub-windows-v2.10.27.zip`.
- Cursor cabin 2.10.27. VERSION 2.10.27. Draft only. Not tagged. Based on main 2.10.26 (`624c490`).

## 2.10.26 — 2026-09-24

The project section and the chat section are separate lists. A folder looks like a folder. Clicking it shows the chats underneath and does not open a chat or hide History. A chat under that folder opens that chat only. It does not filter, hide, or remove the other chats. Projects stay in the project section, including inside a folder, and a project row is not a chat. New chats can be created under a folder or project and stay there. Creating a project does not wipe History. Background jobs stay off History and off the visible chat. Leaving a chat does not stop a hidden job or a live reply. Pins and transcripts stay. A project click does not open the Workboard.

- Linux: `grokhub-linux-v2.10.26.tar.gz` and AUR `pkgver=2.10.26`.
- Windows: `GrokHub-Setup-2.10.26.exe` and `grokhub-windows-v2.10.26.zip`.
- Cursor cabin 2.10.26. VERSION 2.10.26. Draft only. Not tagged. Rebased onto main 2.10.25 (`07fd28d`).

## 2.10.25 — 2026-09-24

Idea Accept files a workboard Todo (`BoardStatus::Todo` through `flush_board`) on the feed and on Ideas. The Todo title is the task, one line of work, not the feed card's canned line and not the raw message. An ordinary chat prompt such as "summarize the workboard" does not file a card. A live turn can still track a Doing run card, and that card is not a user Todo. Digest and idea cards share `updates.json` and stay out of the event paint cap of 4. The user dismisses an idea; Housekeep expires it at about two weeks or `expires_at`. One brief box in `app.json` steers the digest. Quiet hours hold digest, idea, and `automation_done` visibility. `poll_grok_loop` still records the finish. Minimize, tray show, and the next launch open a fresh empty chat. The previous transcript and pin stay in History. The feed stays hidden when nothing is undismissed. Draft only. Not tagged.

- Linux: `grokhub-linux-v2.10.25.tar.gz` and AUR `pkgver=2.10.25`.
- Windows: `GrokHub-Setup-2.10.25.exe` and `grokhub-windows-v2.10.25.zip`.
- Cursor cabin 2.10.25. VERSION 2.10.25. Draft only. Not tagged.

## 2.10.24 — 2026-09-24

Cabin History is the cabin's own chats, kept on the thread through headless `grok -p`. It is not `grok sessions list`. Creating a project does not wipe those chats. Background jobs such as “summarize the workboard” stay off History. They do not paint on the open chat, and leaving a chat does not stop them. A live reply keeps running when you switch chats. Leave a project chat and click the project again: the same chat opens. Pins stay. Transcripts stay. A project click still does not open the Workboard.

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
