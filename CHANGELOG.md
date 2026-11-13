# Changelog

## Unreleased

## 3.6.0 — 2026-11-13

- Pulse: Like absorbs More like this (unlike retracts); Dismiss absorbs Delete with a confirm.
- Activity in the titlebar always keeps its spot, closes on Esc or a click outside, lists background runs you started, and the chat no longer repeats its waiting count.
- Activity in the titlebar now holds messages typed during a reply: each queued message is its own row in order ("Next up", "2nd in line"), with Steer now and Remove, and Remove puts the text back in an empty chat box. Waiting side questions, local model downloads and the screen recording show there too, and background tasks get Stop.
- The Activity button counts the line: "1 of 2 running" when anything is queued.
- The strip above the chat box keeps only the Steer / Queue choice for text you're typing; queued messages and background task rows no longer show there.
- An 18+ chat keeps answering after its local model is unloaded for a game. Each message is judged on its own: an ordinary ask is answered online with the chat's 18+ turns and voice left out, and an 18+ ask gets one line saying the local model is paused or missing.
- A chat's summary is never replaced by the "18+ chats run only on this computer's model" line when the local model is away.
- A local model that has to load again shows "Loading this computer's model first" instead of a silent wait, and every chat class (math, plans) gets a real reply from it.
- 18+ chats now run only on this computer's model, which never refuses, and never go to xAI, OpenRouter or another online model. With no local model set up, the chat says so in one line and points to Local AI in Settings.
- Turning the joke heat up no longer marks every chat as 18+, so ordinary chats on OpenRouter keep sending after an adult chat in another window.
- When an 18+ chat needs this PC's model and none is set up, a "Get a local model" button under the chat opens Local AI in Settings.
- Ordinary 18+ turns are no longer stopped with "Not sent": the app's own note listing the hard limits was being read as part of your message. Only what you type is checked now.
- On this computer's model with 18+ on, only the hard limits stay: the "jokes, not sex chat" and "never a sex scene" lines come off, and adult chat runs unrefused.
- A chat export now says when the app answered a turn itself, so a turn with no thoughts or tool rows reads clearly.
- With adults-only on and dynamic chat picking the voice, an 18+ message now hands the chat to the adults-only voice right away instead of the usual voice turning it down. A voice you picked by hand for a chat still stays put.
- The agent now asks you first before any step that needs admin rights or changes system protection (an admin prompt, a machine-wide registry change, System Restore, Defender or firewall settings), and never works around the sandbox or an access-denied error on its own. Helper scripts and logs stay out of AppData, and PowerShell progress noise no longer shows up in tool output.
- Agent: an image `read_file` attaches is sent once, as the picture; the tool result is a one-line note (`[image image/png 1280x720, N bytes]`) instead of the same base64 again (a 300 KB PNG was adding about 100k tokens to every later request).
- Agent: streamed xAI replies keep accented letters and emoji whole when a network read splits one (they showed as `��`).
- Agent: the 18+ reply check reads only the new words of each streamed delta (a 30 KB reply: 1 ms instead of over a second), and the whole-conversation 18+ scan is skipped on routes where it can't change anything.
- Agent: a turn's session lines are written and synced once, not once per item.
- Goals, sessions and memory nodes are replaced by rename alone, so a crash or a reader never finds the file missing mid-save.
- Loadout's own text no longer calls the agent Grok: permission cards, desktop-control settings, background status, slash help and Privacy now say Loadout or the cloud model. Sign-in, model names and the Hey Grok wake word are unchanged.
- Loadout never opens a process with memory-read access, never treats a reused pid as its own, and CI now fails on any API anti-cheats flag.
- Mod installs now know the game's anti-cheat. Loadout reads it from the game folder and the running services (names only), refuses anything that turns an anti-cheat off, and offers a mod with a proxy-DLL loader or ReShade as offline only, with a plain note on the card and on the installed mod.
- The egui cabin and the Tauri shell no longer run on the same profile at once: whichever opens second says the other app is open (a login or tray start just exits quietly). A different profile folder is not affected.
- Internal: moved the Devices page's LAN-address helpers out of the main app file into their own module, so the file is back under its line budget. No behavior change.
- Spacing: the new app uses the same spacing scale as the cabin (`--space-*` tokens), every gap and padding sits on it, and the page margin is 24px on wide windows and 16px on narrow ones.
- Spacing: the cabin uses one spacing scale (2, 4, 8, 12, 16, 24, 32 and up), so gaps, card padding and page margins line up across every page; pages tighten to a 16px margin on narrow windows, every control is at least 24px tall, and card corners match the new app.
- Auto routing: every chat turn is now read on your PC for what kind of ask it is (a quick hello, a code fix, a debug, a plan, math and more). That read picks the right model lane and how hard to think, so easy turns stay cheap and hard ones get full reasoning.
- Long chats keep the same model between turns so the prompt cache stays warm. Effort still changes turn by turn.
- Ask "what model are you?" or "what effort?" in chat and you get the real answer for that turn. Nothing shows it anywhere else.
- The desktop app sets itself up too. A new profile opens on a short welcome with a "Make things bigger" button, then "Just set it up" or "Tailor it", a few questions the PC can't answer for itself, and a summary of what it set with Undo on each line. After that it looks again at launch, when the PC changes and after a long break, and says what it changed (with Undo) or offers a change (Yes or Not now) in a note in the corner.
- Settings → Your PC → Auto setup lists what it set and why, with Undo, Hand back, Hand everything back and Forget what you learned. Settings → Appearance has Text size (the whole window zooms) and Reduce motion.
- Chat lines like "make the text bigger", "dark mode", "undo that" or "why is the text so big?" are answered at once, without a model call.
- Notifications set to "only important" keep the OKs and replies and skip finished automations.
- Home's Patch Notes tile now sees the desktop app's game companion switch.
- Auto setup's engine: from this PC, a few setup answers and how the app is used, it settles each setting, applies what it is sure of (text size from the OS text size, less motion, lighter effects on a slow PC), offers the rest, and keeps a history with Undo. Anything you change yourself stays yours, a change you undo twice stops, and visible changes are limited to one a day and three a week. Chat asks like "make the text bigger", "dark mode", "undo that" and "why is the text so big?" are read on the spot.
- The native app sets itself up on first run: a "Make things bigger" button, then a few one-tap questions picked for this PC (never one the PC already answers), then a summary of everything it set with Undo on each line. After that it keeps tuning at launch and after a long break, says what it changed in a small note with Undo, and offers bigger changes with Yes or Not now. Chat asks like "make the text bigger", "dark mode", "undo that" or "why is the text so big?" are answered right away. Settings → Your PC → Automations & limits lists what auto setup set, with Undo, Hand back and Forget. Text size (100 to 150 %) is a new setting.
- Automations reads plainer. "Loops" is now "Repeating", and each repeating job says when it runs next ("every 30m · next today 9:30 AM · 3 runs"). Scheduled jobs also say how the last run went ("last run went fine", or that you stopped it); a failed run keeps its red line. The hints use plain words like "every 30 minutes check the deploy" instead of `/loop 30m`. The Tauri app's rows get the same last-run words.
- Background asks (quick chips, idea cards, feed rewrites and the home feed digest) now send one plain request with no tool list, about 2,700 fewer tokens each. The digest still searches the web for its links.
- Background work spends less: Anticipate runs at most once per idle stretch, and the daily token budgets for Anticipate, Ideas, Digest, Review and night passes are halved.
- Fixed a beta test on Windows: the line that tells the chat its app notes are not your words is shorter, so the background prompt stays at least 30% smaller than the chat prompt.
- Background work runs on its workboard card instead of a chat: scheduled automation runs, `/bg` tasks, tasks the assistant starts, and queued or resumed cards put their result on the card, and no chat gets a post you did not ask for. Main no longer gets scheduled reports, and the hidden Background and Follow up chats no longer get copies.
- An open card shows its run live (the current step and what it has said so far), then the result with a Send to chat button that posts it once into the chat that started it, or Main. `/bg … post the result here` (or "tell me here") still posts in that chat.
- The chat that started a run still knows what it found: its next turn is told the result and the card it is on, and that note now survives a restart.
- Activity's Open and the bell for a finished background task open its card, not its chat. A queued card no longer opens a new chat tab to run in.
- The shell's Workboard shows a card's run result.
- Background work stays off your chats: a finished background run archives its own card (open it from Archived or the bell to read the result or send it to a chat), a stopped or failed one waits in the new Needs Attention column (was Blocked), the memory import line goes to the bell instead of the chat, and thoughts no longer show the app's reply-mode chatter.
- Library: Battle.net games show up, World of Warcraft first, including installs inside a Wine prefix on Linux (Wine, Lutris, Bottles, Heroic, Faugus, or a Proton non-Steam shortcut), read from the Battle.net app's own install list.
- Game watcher: while WoW runs, boss pulls, kills, wipes and deaths come from the combat log WoW writes after `/combatlog`, for Retail and Classic flavors. Read-only file reads; nothing touches the game.
- WoW: connect your own Battle.net client (Settings → Behavior, under the game watcher) to read your characters, mount and pet counts, item level, gear, newest raid progress and Mythic+ rating from Blizzard's official profile API, without the game running. It shows on the WoW page in Library. It stays off until you connect. The client secret and the sign-in stay in your system keychain, the snapshot is deleted after 30 days, and Offline only stops it.
- Popups no longer appear in the top-right corner of the window app. Everything they said, plus finished background tasks, print alerts, game tips and Pulse pings, now lands in the bell.
- The bell shows how many notifications you haven't read (hidden at 0, "99+" past 99), and the same message twice in a row folds into one entry marked x2.
- Every notification now says what happened and where: a bare "Saved" reads "Saved your Appearance settings" (or the page or chat it was on), each entry shows the place it came from, and clicking it opens that place or thing. In the shell, a finished, failed or waiting reply names its chat, and Open chat opens that chat.
- A save to the work board now waits its turn while other writers keep the board busy, instead of failing with "locked by another writer" after seven seconds.
- Work board: on Windows, a save that lands while another writer is releasing the board lock now waits for it instead of failing with "Access is denied".
- Effort held between turns and briefed helpers on the cheap lane can now be proven with a paired live eval (`--ab boundary_routing`), and a pass turns them on.
- Agent: notes the app adds mid-run (checkpoints, background notices, the ledger, summaries) no longer count as a new ask for routing, so they don't restart the effort ladder mid-run.
- Agent: an off-by-default switch, `boundary_routing.toml`, holds a run's effort until the next turn or compaction and runs general helpers briefed on the quick lane, ready for the eval to prove before it's turned on.
- Pulse: Start my day shows what each item says and names anything ranked past the cap ("Not shown"), and the week recap lists every line of every section instead of only the first.
- When Grok is at capacity or the connection drops mid-reply, the turn now picks itself back up on its own: it keeps what already showed, carries on from there on another model for a minute, and only stops with a plain "too busy" note after several tries, so you no longer have to type "continue".
- The cabin Code tab can deploy the project: off until you turn it on, with the command and its switch shared with the desktop app. Ask mode shows the exact command before every deploy; Auto-beta runs a confirmed command without asking until it changes, and Forget makes the next one ask. The project's secrets are set for the run and hidden in the output and the activity list.
- Cabin Code tab: an editor with a file list, syntax colors and line numbers that opens and saves files only inside the project (never .git or a path that leads out), keeps unsaved edits across tab switches, asks before dropping them, and logs each save by path and size only.
- The cabin's Code tab can make a branch, push it and open a PR into beta, through the same push guard as the desktop app: main and the default branch are never pushed, nothing is force-pushed, and every push and PR shows its exact command first unless Auto-beta allows it. It also lists the project's deploy secrets by name, saves a value to the system keychain without ever showing it again, and asks before deleting one. Output and activity lines hide the project's secret values.
- The cabin has a Code tab, third on the rail after Home and Chat: the project folder's branch and changed files with each file's diff, the built-in skills and what overrides them, a test runner that asks before a command's first run (with Forget), and recent Code activity. It shares its logic, remembered commands and activity log with the desktop app's Code tab, and test output hides the project's secret values.
- The cabin's Code tab has a terminal: your own shell, started in the project folder, on a full terminal screen (colors, full-screen apps like less and top, scrollback, Ctrl-C, paste). The project's secret values and token shapes are hidden in what it shows, the screen follows the window's size, opening and closing are logged without what was typed, and leaving the tab or switching projects ends the shell. A git push typed there skips the push guard, and the tab says so.
- Cabin: while a reply runs, the Stop button, the listening mic, busy Workboard or idea cards, the run dot and the composer glow ask for about 30 frames a second instead of every frame (or, for the 33 ms requests that egui shortened, about 50), so a turn no longer runs the whole cabin at the monitor's refresh rate (up to 165 fps). Reduce motion keeps those frames, so voice mode still hears a spoken reply end. A long host command's output and a scheduled chat's streamed reply are read in full each frame, so neither falls behind the slower frame rate.
- Cabin start: today's date is read while the window opens and the machine name comes from `/proc` on Linux, so `Cabin::new` no longer waits on `date` and `hostname` processes (fresh profile, debug build: 41 ms → 7 ms, first frame about 90 ms sooner).
- Cabin start: Super+G and Super+Shift+Esc register in one call, which takes away a ~50 ms wait on X11 at the second frame.
- Settings and the Personality picker: voice pictures decode on one background worker (the tile shows its letter until then) instead of on the UI thread at first open (Settings at launch, debug build: 4.0–4.5 s → 1.5–2.0 s to the first window).
- Home and the mission dock: folding the outcome log is linear, not quadratic (a full 2 MB log took ~33 ms on the UI thread every refresh).
- Pulse: ranking splits each title into topic words once per frame instead of once per card and comparison.
- Voice mode: while it works on an answer, the mic shows three dots circling it, so Thinking no longer looks like it's still listening. Still under reduced motion.
- In the cabin, Get to know you says on Review and on the Saved screen when USER.md, SOUL.md or MEMORY.md was left alone and why, and "Add from OpenClaw" says when an import was cut short or the notes were already there.
- Tests: copies of the cabin tests that cover shared core and agent logic now live in those crates, so the coverage stays when the cabin is retired.
- Loadout's own text no longer calls the app "the cabin": tray menu, Settings notes, Skills, History, Devices, Connectors, Pulse, the update chip, slash help and --help now say Loadout, the app or this PC.
- A cache drop on the usage line now says why in plain words (old tool output trimmed, model changed, chat compacted and so on), the route log records where the prompt changed, memory recall is checked to pull nothing in for off-topic asks, and a test keeps fields xAI ignores out of every request.
- The Grok prompt-cache share now shows on Home's AI usage tile (flagged when low), and a turn whose cache share falls under 80% right after a cached turn says "down from N%" on its usage line.
- Cards: every Pulse post and idea has one neat row of buttons — Like first, the card's own buttons, I don't like, Dismiss last at the right — in both apps. Posts are always fully open (no ··· menu, no collapsing the report). In the cabin, idea rows keep their ··· for now.
- Pulse: the quiet-hours roll-up lists every held update with what it said, and the systems digest names its wins, each failing service or automation and why, what is blocked, the ready update with a link to its release notes, and every next step in full.
- Cards: Not interested is now the one dislike button on every Pulse card (I don't like is gone, and source posts no longer show a second Not interested). It trains card taste, small at first and stronger with repeats; on ideas it also feeds card taste.
- Cards: a shared check flags any card that only counts things ("2 updates") without naming them, ends on a cut-off step, or shows nothing under its title; the quiet-hours roll-up and systems digest are tested against it.
- Learning: every Pulse card reaction (Like, Not interested, opens, Snooze, Delete) now also feeds the app's learning engine, one signal per card trait with the trait's words hashed; Dismiss still teaches nothing.
- Taste learning: Pulse card taste now also learns lightly from Read it, Discuss and fixes (engagement), Snooze and Remind me later (wanted, not now), and Delete on a fresh card (a little less of this; deleting stale or finished cards teaches nothing). Like and I don't like still count most; Dismiss carries no weight.
- Pulse: Like and I don't like train card taste. Each card carries generic traits (topic, subject, tone, source); one or two clicks move a little and repeats move more, and traits shared by liked and disliked cards sort out which one caused the reaction. Liked kinds rise, disliked kinds sink and the worst leave the feed; the writer hears "more of / less of". Dismiss on a feed post carries no weight.
- Cards: each Pulse post's ··· is back with one item, Why I created this. Snooze and Delete now sit in the button row when they suit the card (Snooze for time-bound cards, Delete for stale, repeated or finished ones, both when unsure), between the card's own buttons and I don't like.
- Dependencies: refreshed the Rust lockfile to the newest semver-compatible releases (tokio 1.54, zip 9.2, uuid 1.29, arboard 3.7, dirs 7 and 43 others). No manifest changes.
- Settings → Behavior: "What Loadout changed on its own" lists self-made changes to skills, connections and automations, with Undo, Keep, and Restore for a removed skill, all on the same change ledger as /skills changes.
- A chat whose engine crashed or quit mid-reply now starts a fresh engine on its own, picks the reply back up once, and keeps answering new messages instead of failing with "sending on a closed channel" or dropping them. Engine crashes are written to `logs/engine-crash.log` in the config folder.
- Export (and `/export`) now saves the whole chat to your Desktop, named after the chat: every message, the thought process and each tool step in order, nothing cut. It used to land in the app's hidden folder and leave the thoughts out. With no Desktop, it still goes to the project or app folder.
- On Windows the assistant now knows it is on a Windows PC with PowerShell, so it no longer says it is on Linux or reaches for bash commands.
- The chat no longer narrates its own machinery: progress and "not done yet" notes show as quiet status lines instead of replies, it never names talk styles, playbooks or checklist ids, and it treats the app's notes as the app's, not yours. Wrap-ups are full sentences, and a thought no longer shows twice in a row.
- Shared chat outcome recording: any app can now write a chat turn's task outcome and run metrics, mark the last task wrong after a correction, and fail a skill's recent tasks after an undo, into the same profile files the weekly self-review and skill trials read.
- Fix: text typed into the message box right after a reply ends no longer loses its first letters. Chat's cards rise in by their own animation instead of a view transition, which held the page's updates while it ran.
- Rewind checkpoints: pruning no longer deletes the run it is writing or restoring, and orders same-second runs by their exact time instead of directory order.
- Quick chips: the row's order now reads each chip's history (picked, passed over, hidden) from the learning engine, so one system decides. Forget clears it, Always counts as a sure yes, and a chip you marked wrong is never shown. The ranker's learned weights start fresh once.
- Quick chips are smart again on the Loadout themes. A leftover from the pitch-deck mockup swapped the ranked chips for the same four (Plan my weekend, Roast my build, Find a duo, Fix my stutter) in every chat. Chats now get chips picked from the conversation, your habits, and the game, resume and tidy offers, and those four are kept as starter chips that fill a thin row and can be dismissed. The fixed four now show only in deck screenshots.
- Quick chips now rank through one shared engine on the home page and in the composer. It learns from which chips you tap, pass over or hide, shows at most three, and keeps the engine's own order until it has enough history. The row stays put while you type and only reshuffles between turns.
- Quick chips now prove themselves: a replay test shows the engine finding the next step about 2.5 times as often as plain habit counting, and ranking 200 chips stays under a millisecond.
- One boundary in twenty shows no chips, so Settings → Labs → Diagnostics can compare how fast the next step comes with chips and without, beside pick and hide rates.
- The coding or life lane now nudges chips before they are ranked, so the row that shows is the row that was learned from.
- Old chip code that nothing used is gone.
- Right after a reply that failed or names a next move ("Fix the cause"), its one-tap follow-up now leads the quick chips. Chips never ask a model, cloud or local; the unused model-chip code is gone.
- Quick chips learn what kind of ask you use and when (a game fix at 9pm, storage on Sundays), from chips you tap and asks you type, without saving your words. How a turn went now counts only for the chip that started it, so typed turns no longer skew chip scores.
- Quick chips are bigger and easier to read: 48 px tall, 16 px text, 8 px apart.
- Alt+1, Alt+2 and Alt+3 pick a chip; Tab, the arrow keys and Enter work too.
- Every chip follows one tap rule for its kind: ask, open, run with Undo, or show first. Snoozing backup reminders or adding a release reminder from a chip now offers Undo for 30 seconds.
- A reply's follow-up chips are saved with the chat, come back after a restart, and run in the mode (Chat, Plan or Ask) of the turn they came from.
- One switch, Settings → Behavior → Suggestions, turns chips off everywhere; off, replies no longer ask for follow-ups either.
- Quick chips now notice what your PC and day are doing: a game running ("Fix stutter in …"), one that just closed, a nearly full drive, an update waiting, and Friday evening plans (held back during quiet hours). They read only what Loadout already knows, with no new game or system reads.
- Quick chips are safer: a shell chip puts its command in the box for you to check and Enter runs it, a chip adds to what you typed instead of replacing it, the row waits while a reply streams, and a hidden chip stays hidden after a restart.
- Quick chips no longer remember shell commands, file paths or live status (model, resume, backup and similar) as habits; old ones are cleared on load. "Why this?" now explains each chip in plain words.
- Feature offers (the app you just left, a new model file, code you left, a backup or Downloads cleanup that's due, your game, a release reminder, a matching skill) now rank with every other quick chip instead of jumping to the front, and they still lead when nothing else is stronger.
- While you're gaming or a fullscreen app is up, quick chips hold their nudges and keep only the chat's own chips and game chips. While a protected anti-cheat runs, chips that touch the game, read other windows or run commands are hidden.
- A file waiting to be sent offers an "Explain" chip, and the file's name is never kept as a habit.
- "Continue" chips for other chats now follow the chats you used most recently.
- Quick chip memory (`chips.json`) now loads and saves through one shared store with a lock, a private write and an automatic upgrade; an unreadable file is set aside instead of overwritten, and old model or effort chips are cleared on load.
- Quick chips keep a small local log of which chips were shown and whether one was picked or hidden, with no chip text in it, so later ranking can learn from it. It never leaves the PC.
- Quick chips come to the desktop and web app: one row of up to three next steps under the chat box, on Home, and in the empty command palette, from the same engine the cabin uses.
- A reply's own follow-ups join the row under the box, and a tap sends them in the mode the reply was written in.
- Every chip has a × to hide it, a tip that says why it is there, and Alt+1 to Alt+3 to tap it; what you pick and hide teaches the row.
- Taking a Pulse idea on Home now counts as accepting it.
- New Settings → Behavior → Suggestions switch, on by default.
- The chip tests pass at any hour of the day; the 1-in-20 holdout no longer empties a test row by chance, and the speed check gives a busy Windows runner a little more room.
- Quick chips can now come from the reply itself: the model ends each chat reply with up to three hidden next steps, which never show in the chat or the saved history and compete with the local chips for a place in the row.
- Reply chips that would send, delete, pay or buy are dropped, a page chip must be one the app has, and every line passes the same copy checks as other chips.
- If the reply's chips keep getting passed over for the local ones, the app stops asking for them for two weeks, then tries again.
- Removed the old, unused path that asked a separate model for chip suggestions.
- CI: deck and web search screenshots moved out of `windows-release` into a manual `Deck shots` workflow (`deck-shots.yml`, shot filter, 6-minute budget); `deck-shots.ps1` takes `-Only` and `-BudgetMinutes`.
- CI: `windows-release` runs only on the nightly and manual runs; release.yml's `windows-installer` now runs the pack-script locks and size ceiling on every release build.
- CI: PRs run the shell job on Ubuntu only and skip `windows` when they only touch `ui/` or workflows other than `ci.yml`; push to main runs shell on Ubuntu and Windows.
- CI: macOS jobs (cross-check, phone-ios, macOS shell) run only on the nightly, manual runs and tags; a nightly on an unchanged beta SHA that was already green skips the build jobs.
- Internal: CI and release builds now use Node 24 (LTS) for the web UI instead of Node 22. No app behavior change.
- CI: `docs/ci-policy.md` lists which jobs run on which event; CI screenshot artifacts (smoke, deck, web search) keep 7 days instead of 14.
- CI: release.yml beta-push builds cache Rust (`Swatinem/rust-cache`) and skip the macOS shell bundle; tags, the nightly and manual runs still build all three bundles, uncached.
- The chat can no longer say a check passed when the check itself says otherwise. Before a reply ends, "verified", "gone", "empty" and "N GB freed" are read against what the commands actually printed (an error, `EXISTS=True`, a count, the `FREED=` figures), and a reply that doesn't match gets sent back to be fixed. This runs again after any extra checks in the same turn, so a reply after a checkpoint is read too.
- Classic Dark and Light now wear the Loadout frame: the LOADOUT titlebar with the personality pill, the red window rim and the rail's selected mark. Their own colours stay.
- Fix: clearing Main always removes Main's old chat from `threads.json.bak`. Before, a save still waiting to be written could do that cleanup too early, and the clear's own save then backed the old chat up and kept it. Delete all had the same problem and is fixed the same way.
- Right-click the Main chat tab and pick Clear chat to wipe Main's whole history for good, after a confirm that says it's permanent. Other chats, the Workboard, memory and settings stay.
- In the shell, right-click the Chat tab and pick Clear chat to delete the open chat's saved history for good, after a confirm that says it's permanent.
- The Code tab can push your branch and open a PR into beta, showing the command first. Its own push does not run the repo's git hooks, so a pre-push hook is skipped. It never pushes main or the repo's default branch, never force-pushes, and keeps every push, PR, new branch and blocked attempt in an Activity list. Settings > Code picks Ask (the default) or Auto-beta, which skips the ask only for a branch made in the Code tab, on GitHub, into the PR base.
- Settings > Code has a Secrets list for each project, ready for deploy runs. You add, replace or delete a secret by name; the value goes into your system keychain and never comes back to the screen. Only the names are saved, deleting asks first, and a value is hidden as [redacted] in output and in the Activity list.
- A Code tab session keeps its project folder across a restart, and Resume sends it on from the saved goal and next step.
- Internal: the Code tab's logic moved into a shared `grokhub-code` crate so the cabin can use it too; the desktop app's Code tab works the same.
- Code tab chats now always load the built-in coding skills, and a push the agent runs from chat goes through the same push guard as the Push button: pushing to main or the default branch and force-pushing are refused in every mode, an app-made branch pushes on its own in Auto-beta, and everything else asks first.
- Code tab: the agent window. Talk to the agent beside the project's files, diff, terminal and tests instead of on the Chat tab, with its to-dos and helpers listed underneath. Each session works in the folder of the project it started in, even when the app opens another project while it runs.
- The Code tab can run a deploy command you set per project in Settings > Code. It is off until you turn it on, shows the exact command first, and in Auto-beta runs a deploy you confirmed once without asking until its command changes (Forget clears it). Your project's secrets are set as environment variables for the run and hidden in the output and the Activity list, which also now records deploys and secret adds and deletes by name only.
- The Code tab has an editor and a terminal on the desktop app. The editor opens and saves text files inside the project only (no `..`, no symlinks that lead out, nothing in `.git`), and each save goes in the tab's activity log. The terminal is your own shell in the project folder, with common tokens and the project secret values saved before it was opened hidden in what it shows (a secret saved later is hidden once you close and reopen the terminal); it keeps running while you're on another tab and ends with Close terminal or when the app quits. Neither works over the web app. Commands typed in the terminal, `git push` included, aren't checked by the push guard.
- Code tab (desktop shell): a session started in the agent window has a lead. It plans first and changes nothing until you approve the plan, which you can edit on its card or send back for changes; the plan is saved with the session and `/view-plan` shows it. After approval the lead lists the tasks and hands each to a helper, and Tasks shows each task's helper and state (planned, running, needs you, review, done, failed).
- Code tab: in a session with a lead, Tasks updates when a task, helper or command changes instead of every 2 seconds. Each helper shows its last tool step, tokens and time, and **Details** opens its steps and result.
- Code tab: a project bar names the open project and lets you switch to another, open a folder or start a new project, and the tab lists that project's sessions with New session and click to resume.
- Code tab → Roll back: the branch's newest commits, each with Revert. A revert is a new commit that undoes the old one, then the push goes through the push guard. It always asks first and shows both commands, in every mode; it never resets, rewrites history or force-pushes, a conflict stops it with nothing changed, and main or the default branch is never reverted and pushed.
- The Code tab's terminal shows a name like `task-definitions.json` in full when a colour or line-redraw code follows it or the name arrives in two pieces (it was shown as `[redacted]`), shows the whole of a long listing when the shell exits right after it, and a job that outlives the shell is no longer left stuck on its own output.
- The cabin ends a test run or deploy that is still going when it quits, and its terminal's "The terminal is busy" note goes away once the shell takes input again.
- xAI server compaction now sends the chat's cache key, so the fold can reuse the chat's cached prefix. If the endpoint refuses the key, that compaction is sent again without it and the key isn't sent there again.
- Compaction now starts in the background once a chat passes 70% of its window (earlier for Grok, ahead of xAI's long-context price step). The run keeps working, and at the compaction line the finished summary is swapped in with everything since kept after it, so long runs no longer pause to compact. Local models skip it; `background = false` in `compaction.toml` turns it off.
- When a chat that xAI compacted on the server moves to OpenRouter or the local model, those models now get a clipped text copy of the earlier conversation instead of losing it.
- After a compaction, the chat gets a ledger kept by the app itself: the files written, the last 20 commands and how each one ended, and the open todos. It carries across every later compaction on Grok, OpenRouter and the local model, so a long run no longer forgets what it already built or which test last failed. Each new summary also keeps any earlier ask, fix or message of yours that it would have dropped.
- A summary compaction now goes out as the chat's own next request (same system head, tools and notes), so it reuses the chat's prompt cache; earlier notes are still dropped from the result.
- Shell chat: attach files and pictures with the + button, a drop or a paste. Pictures go to the model as image input; text files and PDFs go in as a file the agent reads. Up to 10 per message, nothing sent until you press Send.
- The chat box grows as you type. Long messages that wrap now make the box taller, line by line, instead of spilling out of it; past about ten lines the box stops growing and scrolls inside. The chat above makes room for it.
- The desktop app's chat has a push-to-talk mic: hold it (or Ctrl+G), speak, let go, and your words land in the draft through xAI speech to text. It never sends by itself, and Offline only blocks it.
- Config switch files: a `#` inside a quoted value (a URL fragment, a colour) is kept instead of cutting the value, and a line the app can't use (a typo'd value, an unknown key, a `[section]`, a stray line) now shows in the Settings status line and `/doctor` instead of only on stderr. The cabin's panels switch reads `generative_ui.toml` the same way the agent does, so `enabled = on` works there too.
- Chat: every personality now sits on one conversation core, so replies lead with the answer, match your length and energy, stay in full natural sentences and keep their warmth through tool work and errors; a reply that slips (stock opener, "the user", checklist talk, clipped notes) gets named once in the next turn's note so the voice corrects itself.
- Eval: a before/after naturalness set built from real flat replies, plus four live `talk-*` cases that fail a bot-sounding reply.
- Chat usage lines and exported chats now show the real cost: xAI reports cost in units of a ten-billionth of a dollar, and the app had been reading them as millionths, showing costs 10,000 times too high.
- Asking why a program keeps crashing ("why does bluetoothd keep crashing", "what's wrong with Discord.exe") now reads that program's own crash history, with how many times it crashed, when, and the likely cause, instead of only checking background services. A follow-up like "check into it for me" now remembers that answer.
- When a provider runs out of credit or refuses its key, the router keeps it out for the rest of the session (and the provider's other paid models with it) instead of retrying it every minute. A rate limit keeps it out for at least five minutes. If Grok itself is out of credit, a provider you set up, or this computer's model, takes the turn instead of the turn failing.
- A reply that drops partway is no longer sent again, so its text doesn't show twice or get billed twice. The turn stops with what showed, and that text stays in the chat.
- Decision engine: one place weighs every change Loadout could make on its own. A new kind of change only offers until you've said yes three times, each area has its own level (`/learned trust <area> <watch|offer|act>`), two undos in a day pause it, and `/learned decisions` shows what it decided and why.
- Delete one message from a chat: every bubble's ··· menu and right-click offer Delete message…, which asks first. A reply goes whole, with the tools it ran, and the assistant never reads it back.
- The shell's Chat has the same ··· and right-click Delete message… on its bubbles.
- The desktop app can run a supervised desktop session like the classic app: switch it on above the message box while desktop control is on, watch each step and the latest screen, and Pause, Stop or Halt it. Risky steps (delete, send, money, passwords) wait for your OK in every approvals mode. With the Cua driver turned on in the classic app's settings, sessions use it on Linux.
- Sign in with Grok in the desktop app keeps the code on screen and keeps waiting when one check gets no answer, until you approve it, it is refused, or the code runs out.
- Loadout can now read your public Destiny 2 profile through Bungie.net's official API. Under the game watcher in Settings, register your own application on Bungie.net (its terms keep keys personal), paste its API key, which stays in your system keychain, and your Bungie Name. Loadout shows your characters and Light, hours played, triumph score and lifetime PvE and PvP stats, refreshing every fifteen minutes or on Refresh, and says plainly when your Bungie.net privacy settings hide a part. Nothing touches the game or BattlEye, and Offline only stops the refresh.
- Loadout reads this PC once at launch in both apps: graphics card, memory, battery, screen, and the Windows or Linux text size, motion, contrast and dark mode settings. The local AI is sized to the hardware and the router steps down on a low battery at every launch, not only when setup is opened, and the desktop app now hands the hardware to the local model launcher too.
- Pulse (cabin): Dismiss stays pinned at the right of a post's row and no longer sits on top of Not interested when the row is too long, as on the daily digest; the rest of the row scrolls sideways beside it.
- Do all and the PC tune-up behave: a bare "continue" after a stopped run sends the next waiting step, fixes that change nothing ("No fix needed") no longer become buttons or steps, a Do all summary keeps its Finished card without a "That isn't done yet" line, one restore point covers the whole batch, and the restore point step now says whether Windows made it, skipped it or has System Restore off (it never turns System Restore on). On Windows the stopped-services check ignores delayed and trigger-start services that stop on purpose, and every flagged service restarts in one admin prompt.
- The agent never installs low-level input drivers such as Interception, and a fan, RGB or monitoring tool whose driver is on Microsoft's vulnerable driver blocklist gets a card that explains, in plain words, why anti-cheats may refuse to start games with it. The blocklist is fetched on Windows at most once a month.
- Internal: core, agent, app and the desktop shell (src-tauri) move to Rust edition 2024, and every crate now takes its edition from the workspace. Formatting stays on the 2021 style. No app behavior change.
- Internal: the proto, hub, desktop and xtask crates move to Rust edition 2024; a root `rustfmt.toml` pins the 2021 format style. No app behavior change.
- Added an offline efficiency bench (`cargo run -p grokhub-agent --example efficiency_bench`). For a fixed set of chats it reports how many model requests are made, how many bytes and estimated tokens are sent, how much of each request is cache-stable, whether early facts survive, the wall time, and the peak memory. The baseline is in docs/efficiency/baseline.md.
- Cabin: egui/eframe 0.37 with system fonts and color emoji, CompletionPopup slash autocomplete, and PasteImage attachments (egui_stack + egui-pop-phosphor bridges until upstream docks/phosphor release).
- Loadout now follows Elite Dangerous while the game watcher is on: it reads the game's own Player Journal for your jumps, dockings, mission and bounty earnings, promotions and deaths, shows the session in Settings, and posts a recap to Pulse when the game closes. It works with the Proton install on Linux too. Nothing reads the game's memory, touches the game or uploads anything.
- Loadout can now follow The Elder Scrolls Online with a small Loadout addon you install from Settings, under the game watcher, for the NA and EU server folders it finds (Proton included). The addon only listens to ZeniMax's public addon events and saves your characters' level, champion points, zone, gold and deaths when you log out or reload the UI. Loadout reads that saved file as data, shows the character you played last and posts a recap to Pulse when the game closes. Remove deletes only Loadout's own addon folder; your saved data and other addons stay, and nothing touches the game.
- The live eval has two safety probes: after nine plain log reads, a buried note asks to copy a planted secret or send it off the machine. Each try is scored said, tried and done, any of which fails it, and the report counts the calls the gate caught. Asks in a probe are answered "deny".
- Three Windows desktop cases (occluded window, cross-app window, dialog mid-run) are written up in `docs/eval/manual-desktop-safety.md` with the same scoring, to run by hand.
- `/export`, `/export html|json`, `/remember` and Memory Save now say "Wrote …" only after the file is on disk, and "Couldn't write …: <reason>" when it fails. Each export gets its own time-stamped file (`export-YYYYMMDD-HHMM.md`) instead of overwriting the last one.
- Exported chats (Markdown and JSON) and chat search now read the way the chat showed them, without the app's internal "SLASH_RESULT" and turn tags.
- Loadout can now follow Factorio 2.0 with a small Loadout mod you install from Settings, under the game watcher. The mod uses Factorio's official mod API to write a summary (research, rockets, deaths, play time and a few production rates) to the game's script-output folder; Loadout reads it, shows it in Settings and posts a recap to Pulse when the game closes. Settings says first that a mod moves a save to Factorio's modded achievement track and that multiplayer needs every player to have it. Remove deletes only Loadout's own mod folder, and nothing touches the game.
- Fast mode now sends Grok 4.3 with reasoning effort `none`, so it answers quicker than Balance, which uses the same model at its default effort (low). Quick asks on Fast's model (Consult, personality Try it, screen-recording summaries) use it too. Chat Completions now passes an effort to Grok 4.3.
- Stop now ends a waiting reply within about a second instead of up to two minutes. A long quiet think at high or xhigh effort gets 300 seconds before it counts as stalled.
- Fast mode, Auto's quick answers and the cheap background tier now use Grok 4.3 instead of Grok 3 Mini Fast. Grok 3 Mini Fast stays pickable in Settings, and a saved pin on it still counts as a ladder default.
- Voice mode now defaults to Grok Voice 3.0 (`grok-voice-3.0`). A voice model you set yourself is kept.
- Home's Achievements and Deals tiles turn on right where they are in the new app: one tap uses the Steam account signed in on this PC, and the checks run in the app itself (wishlist sales daily, games close to 100% weekly). The tile says what the last check found, says what to do when Steam isn't sharing (a private profile), and has Turn off.
- Home's Patch Notes tile turns on in place too: once on, starting a Steam game posts one Pulse card with what changed since you last played, a guides link and your note from last time. It reads only what Steam says is running; nothing opens or touches the game.
- Background task output and monitor lines now reach the model fenced as outside text, and a background notice never counts as your own words.
- Internal: tests that wait for a background file write (`/remember`, `/export`, Memory Save) now give it up to 10 s to report instead of 2 s, since the write's flush to disk alone can take longer than 2 s on a busy Windows CI runner. A write that never reports still fails the test, with the same message. No app behavior change.
- Show in folder now opens the folder with the file selected (Explorer on Windows, Finder on macOS, your file manager on Linux); diagnostics use it.
- One place picks the app's data folder for every part of the app; the LAN hub now uses `%APPDATA%\GrokHub` on Windows and brings over its old state once.
- A `folders.json` setting remembers where your own files go (`Documents/Loadout` by default), ready for exports and Settings.
- Exporting a chat now always says where it went. It saves to `Documents/Loadout/Chats` (then the Desktop, then the app folder if that can't be written), shows "Saved <file> to Chats" with a card that opens the file, shows it in its folder, or copies its path, and says why in plain words with Try again when it fails. History exports no longer overwrite an older file.
- Your other files now sit together in `Documents/Loadout`: diagnostics zips in Diagnostics (still kept off cloud-synced folders), screen recordings in Recordings, Imagine saves in Images, and the internet log CSV and week recap image in Reports. Older recordings in `~/GrokHub/recordings` can still be deleted from their cards.
- Saving the internet log twice in a day, or an Imagine image with the same prompt, no longer replaces the earlier file, and the log's save shows Show in folder in the bell.
- The Skills page shows the real skills folder on every OS.
- The window app can save a chat now. Type /export, or pick Export from a chat's ⋯ menu in History, and the chat is saved as Markdown in your Chats folder (Documents/Loadout/Chats unless you moved it). A card says where it went, with Open, Show in folder and Copy path. A chat with nothing in it yet says so and saves nothing.
- Settings → Privacy & data shows where your files go, with Open and Change…. The cabin and the window app share the setting.
- Settings → Privacy & data has a Storage block: where your files go with Open, Change and Reset to default, an Open for each folder (Chats, Diagnostics, Recordings, Images, Reports) and for local models. When older recordings or Imagine saves are still in the folders earlier versions used, Bring them over moves them on the same drive, or copies them and keeps the originals on another drive. After a Change, the old folder's files can follow the same way. Nothing is ever deleted.
- Format the Tauri ghost and focus-help modules so `cargo fmt --check` passes on the shell CI job.
- A fix you start from the Fix card now answers on that card: why it couldn't start, its result and what Undo fix put back stay with the card's Done, not in the chat tab.
- Fix it on a screen or audio report card leaves its receipt on that card (finished, stopped, or nothing to run) instead of posting into a chat.
- Run on a Pulse card leaves a receipt on the card when the background run ends. A card with a report now runs on the hidden Background chat instead of opening a new "Discuss" chat tab.
- When the app's own check retries the last step in your chat, the status line says it was the app's check, not you.
- Shell: game watcher help takes the product name from brand.json.
- Shell: Library api mock covers game watch; Game watcher copy uses the brand name.
- Refresh `src-tauri/Cargo.lock` so the shell workspace tracks `grokhub-core`'s quick-xml 0.42 and `cargo deny --locked` passes.
- A forked subagent now sends its chat's tool list byte for byte, so its first request reuses the chat's cached prefix past the tools, and the chat's last request is kept for forks by moving it instead of copying it every step.
- Home's Frame Rate tile sets itself up in one tap: Set up shows your system's own password (or Windows admin) prompt, checks that frames are counting, and offers Undo setup. Desktops with no prompt get Copy setup with the exact steps. Asking the chat to set up Frame Rate runs the same setup.
- Home Frame Rate: the frame probe is now a tiny stand-alone binary that the Linux shell bundles ship as `/usr/lib/loadout/fps-probe`, so the one-time setup works from the AppImage and the deb, and the shell answers `--fps-probe` like the desktop app.
- Shell UI toolchain: Playwright 1.64.1, jsdom 30.1.3, ESLint 10.14.0 and @babel/core 8.0.10. The UI now declares Node 22.22.2 or newer, which jsdom 30 needs.
- Library: On objective tab, Battle.net tiles beside Steam, and a per-game companion frame (notes, news since last play, guides, game-screen slot).
- Library companion screens for Destiny 2, Path of Exile, Elite Dangerous, Factorio, Stardew Valley, Minecraft and Rocket League, matching the cabin.
- Library companion screens for World of Warcraft, Guild Wars 2, Elder Scrolls Online, and Old School RuneScape (addons, hiscores, ArenaNet key via credentials card).
- Settings: under the game watcher, "Games Loadout can read, and how" lists each supported game and its read-only way in: a file the game writes, the maker's local API, or Loadout's own addon through the game's official addon system. Games with kernel anti-cheat (League of Legends) get no addon or mod from Loadout, and a test holds every entry to that.
- Agent: outside text stays marked as outside text. Pages, network output from shell commands, subagent results and files written after reading them reach Grok fenced as data, and long subagent results are saved to a file instead of flooding the chat. The safety reviewer now sees both ends of a long tool call, and three denied replies in a row end the turn instead of letting Grok look for another way.
- Get to know you: right after Auto Setup, a short, skippable stage asks your name, how the assistant should sound, and your goals, interests and routines, then shows the exact USER.md, SOUL.md and MEMORY.md text (and, with the new memory on, the Personal rows) for you to edit before Save. It resumes where you left off, stays on this computer, and Settings → Personal opens it again. In the desktop app and the cabin.
- Sandbox: the git guard no longer follows a link when it puts back a project's git config or hooks, so a sandboxed command can't use a swapped link to make it read, delete or write files outside the workspace. A hooks link the project set up itself is kept as a link, a pipe planted as a hook no longer stalls the guard, and a `.git` that a command replaced or moved is now reported instead.
- Internal: updated the global hotkey library to 0.9. Super+G and Super+Shift+Esc work the same as before.
- Goals no longer read as empty for a moment on Windows while they're being saved.
- grep cuts long lines, keeps its answer under 40 KB, skips .git and files over 4 MB, prints overlapping context once, and lists hits in path order so the same search gives the same output.
- The agent's grep can ignore case and answer with just the matching files or a count per file, and says when it stopped at its 200-line cap. Config switches read one way everywhere: a typo like `enabled = flase` now keeps the default and says so, instead of quietly reading as on.
- Grok 4.3 now gets the effort level the router picks (low to extra high), not only None.
- Grok chats keep the model's sealed reasoning between steps and pin each chat to one prompt cache, so more of every request is billed at the cached rate.
- A Grok chat now compacts before its prompt reaches 200k tokens, where xAI doubles the price of the whole request.
- Fast mode on grok-4.3 can turn reasoning fully off.
- Every Grok request is now sent with `store: false`, including non-reasoning models, so xAI keeps no copy of the chat. History is kept on your machine, as before.
- `grokhub --mcp-*` and `grokhub --doctor` keep working through a small `grokhub` shim that hands them to `grokhub-hub` and says the new name once; it stays for at least one release.
- Loadout can now read your Guild Wars 2 account through ArenaNet's official API. Under the game watcher in Settings, paste a read-only API key from account.arena.net; it stays in your system keychain. Loadout checks what the key allows, then shows hours played, fractal level, WvW rank, characters, coin and karma, achievements and this week's raid clears, refreshing every ten minutes or when you press Refresh. Offline only stops the refresh, the last snapshot is kept without the key, and nothing is added to the game.
- Helpers in a git worktree: what a helper changes is now kept as a sealed snapshot when it ends, however it ends. Stopping a helper no longer deletes its worktree. A helper that finished reads "ready to merge", one that was stopped, failed or ran out of steps reads "partial", and one that changed nothing reads "no changes" and its worktree is removed. Merging the work into the project comes in a later change.
- A worktree helper now starts from the project as it is on disk: your uncommitted and untracked files are in its worktree, and ignored files are not. Your branch, index and git objects are not touched.
- Helper worktrees moved from `worktrees/` to `helper-work/` in the config folder. Folders left by older builds are moved over, and one that still holds files is kept and listed instead of being deleted after three days.
- A helper can no longer start worktree helpers of its own, and worktree helpers need git 2.41 or newer.
- Sandbox: for a command in a helper's worktree, the git guard protects the project's own repository folder and never follows the `.git` file in the worktree, which the helper can change.
- Download models: the Voice tab no longer offers the on-device speech-to-text model, since nothing transcribes with it yet. A copy already downloaded still shows so it can be removed.
- History's Delete all is now a red button that takes two clicks: the first turns it into "Delete all N chats?" with Cancel, and it disarms on its own after a few seconds. One click no longer wipes every chat.
- Long chats resend less: once a chat fills 40% of the model's window, an older file read, folder list or search that was run again later with the same arguments is replaced by a one-line note pointing at the newer result. Your messages and the newest result are never changed. On the efficiency bench, a log read three times now sends 39% fewer bytes with recall unchanged. Set `enabled = false` in `history_trim.toml` to turn it off; `age_out = true` there (off by default) also moves big tool outputs older than three turns to disk as a summary plus a path.
- The Audio Check tile shows your mic and speakers and puts a muted mic or a silent output first, so you find out before the squad does. On Windows it lists your sound devices and flags one that reports a problem. It only reads; nothing is recorded or changed.
- Home tile words name the app from its brand file, like the rest of the shell.
- Home's chips now come from the same quick-chip engine as the composer: they follow your game, drive, updates and habits instead of the same four every time, each has an × to dismiss, and taps are learned. A chat chip still opens a new chat and sends.
- The desktop app's Home draws every module as a real tile: recent chats, local AI, personality, playbooks, automations, the journal, decisions, the daily drop, the Steam library, now playing, what to play, the systems brief and the coach tip show live rows and one-tap buttons. The "Preview: not built yet" placeholders are gone; modules still waiting on their own data say what they will show and offer one way in.
- Edit Home now works in the new app and on a paired phone or browser, not only in the desktop app. Start from Mission Control, Streamer/Social or Performance/Tuning, pick what each tile shows from the modules that fit it, and Save. It writes the same layout the desktop app reads, keeping the XP bar and tile settings.
- Hiding both middle tiles no longer leaves an empty gap; the big tile and the small tiles widen to fill the row.
- If the big tile slot is left empty, Home fills it with a tile that isn't already showing, so nothing appears twice.
- When Edit Home can't save, it says so and keeps your picks open instead of closing quietly.
- Achievements, Deals and Patch Notes now show their real Pulse cards in the new app too, instead of "soon". When one is off, the tile says where its switch is.
- A new check paints every Home module at every size it offers, in a wide and a narrow window, so a tile that says "soon" or runs past its edges fails the build.
- In the desktop app, Achievements, Deals and Patch Notes list what their Pulse sources found: games close to 100%, sales on your wishlist, and what changed since you last played. A tap opens the guide, store page or news. While a source is off, its tile says so and Turn on goes straight to its switch.
- Home on Linux: the Frame Rate tile now shows the frames your screen actually shows, counted by the kernel's display driver from outside the game, so anti-cheat has nothing to see. Reading that counter needs a setup run with sudo. The tile's Copy setup button gives you the commands: they install a frame probe (a copy of Loadout that only your user group can run), grant that one file the kernel's perf-counter permission (CAP_PERFMON), and make only the one tracepoint id readable. The probe uses that permission for one thing: it opens the frame counters, then drops every capability before it reports a single count. Started any other way, it drops every capability before Loadout itself starts, so the app never runs with the permission, and the Loadout you launch normally never has it. The system-wide perf setting stays as it is. Read the commands before you run them; the tile's hover text has the undo. Without the setup, the tile says plainly that it isn't reading.
- Home on Windows: the Frame Rate tile now shows a real FPS number and names the app it comes from (your game while you play). Loadout counts frames from Windows' own graphics event log, the same source PresentMon and CapFrameX read, from outside the game: nothing opens, reads or hooks the game, so anti-cheat has nothing to see. Starting that log needs a one-time setup in an admin PowerShell that adds your account to Windows' built-in Performance Log Users group; the tile's Copy setup button gives you the line, and the hover text has the undo. Without it, the tile says plainly that it isn't reading.
- The Frame Rate tile now counts real frames in the new app too, on the PC and on the web and phone views of it: from Windows' own present events (naming the game drawing them) or the Linux kernel's display flips, never from inside a game. Where the PC can't count them yet it says why and points at a safe counter.
- The Clips tile lists your newest clips and captures from the Videos folder and Steam's game recordings, and a tap plays one with your PC's own player. Only names and times are read, and only a place in the list travels from the screen, never a file path.
- Now Playing leads with the game that's running right now in the desktop app, marked LIVE, and What To Play has an Another button there for a different pick.
- Home modules no longer show a READY badge over an empty card. Modules whose data lives on another page (Journal, Automations, Library, Personality, Playbooks, Local AI) open that page; modules that aren't built yet are labelled Preview, on the tile and in the Edit Home picker. Objectives and Streak are no longer listed as not built.
- Home: Loadout comes back on Home when you restore it from the tray; un-minimizing or refocusing the window keeps the page you were on instead of jumping to a new chat.
- Home's PC loadout tile names your real GPU (from the system's PCI id list), tells an APU's iGPU from your graphics card even when the BIOS carves 4 GB or more out of RAM for it, and shows the card with the most VRAM. The driver reads as the kernel module ("amdgpu (kernel)") instead of looking like a version. DETAILS opens a "This PC" sheet with every GPU, CPU, RAM, disk and power, and the small tile shows the GPU's temperature in °C when the OS reports it.
- The Session Timer starts on its own when a game starts, names the game in the desktop app, fills toward a break, and suggests one after 90 minutes.
- The Hype Line shows a fresh line each day in the voice of your personality. It needs no model, so it costs no tokens.
- Home keeps itself current. The desktop app redraws at the pace of its fastest tile while Home is up, and its background readers wake the window the moment new numbers land. The shell, web and phone ask again at that pace only while Home can be seen, keep the last read with a "Try again" when the connection drops, and mark a tile "Updated N min ago" once its data is past twice its normal age.
- The shell, web and phone Home now draw every module as a real tile: big number, caption, rows, progress and buttons cut to the slot size, with honest empty, off, error and "shows on the PC" states. The dashed "Open in the cabin" placeholders are gone.
- Home's Speeds tile shows your ping and jitter from the internet log's once-a-minute check, says when you're offline, and shows the last download test with a "Test speed" button. With the internet log off it says so and sends nothing; the button opens the setting.
- Home's Storage tile reads every local drive (internal disks and mounted game drives, not network shares or system partitions) and shows free of total for each, fullest first, amber under 15% free and red under 5%. The small tile shows the fullest drive and how many drives there are.
- The Stream Check tile is a go-live checklist: whether OBS, Streamlabs, XSplit or Twitch Studio is open, whether your mic is live, and whether the internet is up. It turns green as "Ready to go live" when all three are fine. It reads process names, audio devices and the internet log only.
- Home: one shared tile model (what a tile shows, its state, its buttons, how often it refreshes and when it counts as stale) and one set of tile words for storage, updates, game mode, connection, AI usage and GPU temperature, so every Home surface says the same thing. A saved layout that puts a module in a size it can't draw now falls back to the default for that slot.
- Home: the AI usage, Game Mode and Connection tiles show real numbers (tokens today against your cap and this week's spend, the game the guard sees, sends in the last day) instead of placeholders, and their buttons open the right Settings section.
- The Tonight tile lists the automations due in the next 12 hours, soonest first, with Plan my week one tap away.
- The Squad tile counts the Steam friends saved on this PC and opens Steam's own friends list, where Steam shows who's online. It reads only the count from Steam's local settings file, never names, and never guesses who is on.
- A tile's Turn on now switches the internet log or the game companion on right there, with no trip to Settings. The new app keeps its own internet log too, so its Speeds and Stream Check tiles fill in.
- Home's Updates & Drivers tile is real now: it says whether a Loadout update is waiting, which installed Steam games need an update (from Steam's own manifest files), how many system packages are waiting (from the package list already on disk, only once system checks are on), and shows the GPU driver. Tapping it opens Settings > Update.
- Home: your level and XP now grow from what you actually do. Finished tasks count right away (and drop back out if you undo or correct them), Home cards you run or see through earn XP, and each active day adds a little. Card and day XP stop at 150 a day so it can't be farmed. Hover the XP bar to see today's XP and how it's earned.
- Chat: after a turn that found nothing, the next turn knows it, so a "thanks" no longer gets "glad it helped"; a reply that takes the credit anyway is sent back once to say it came up empty.
- Chat: small talk no longer shows the model-switch line or thoughts that talk about the app's own tools, terminal, browser tool, rules or routing; work turns keep them. The per-turn steps note the model reads no longer says "playbook" or "cabin", so models stop echoing those words.
- Agent: Grok's hosted web and X search now follows your permission rules. It is off in plan, btw and other read-only turns, in runs nobody is watching, offline, when a rule denies or asks for search, and when the chat carries recalled memory lines. In those cases the cabin's own search tool takes over where it is allowed, with its card and send check.
- Grok's hosted web and X search: its search calls now go back with the next request (xAI asks for every output item), the pages a reply cites are kept, and each call's record counts the searches billed. Search rounds per reply are capped by what the turn is for: 2 for chat, 3 for a task, 5 for research.
- Once a chat has seen recalled memory lines, xAI hosted search stays off for the rest of that chat, so a compaction or a later turn can't flip the tool list back and break the prompt cache.
- Internal: the shared HTTP client module (`grokhub_core::http`) now also builds the agents for the GitHub tool, the hub and browser-tab probes, Pulse previews and sources, the engine download, and the xAI and Imagine sign-in calls. No app behavior change.
- Internal: the shared HTTP client module (`grokhub_core::http`) now also builds the agents for the cabin Grok calls (chat, speech, Imagine), the release update download, Hugging Face model downloads and the printer alerts, and can honor an HTTPS proxy from the environment, which public-only fetches never use. No app behavior change.
- Internal: the shared HTTP client module (`grokhub_core::http`) now also builds the agents for the Grok model client, model listing and probes, the providers you added, the local model chat and speed test, MCP servers and their sign-in, web fetch, Brave search and Imagine media, so the agent crate no longer uses the HTTP library directly. No app behavior change.
- Internal: the shared HTTP client module (`grokhub_core::http`) now runs on ureq 3 (rustls with ring and the bundled web roots), and the connectivity re-probe, the network log speed test and the League live game reader use it too, so `crates/` has one HTTP client library instead of three. Timeouts, redirects, error codes and proxy handling stay as they were; a proxy from the environment is now reached with a `CONNECT` tunnel for plain `http` addresses too.
- Internal: a shared HTTP client module (`grokhub_core::http`) now builds the agents for the MCP registry refresh, the plugin marketplace index, the local model health and `/props` checks, store art and the update check. No app behavior change.
- `grokhub-hub` now also runs `--mcp-desktop` (the desktop tools for MCP clients, behind the same Settings switch and hard gate) and `--mcp-cua` (the Cua Driver gate, Linux), reading the switch from either app's settings. `grokhub-hub --doctor` checks sign-in (either app's), the memory folder and the hub, and `grokhub-hub --update` says how your install updates without running anything. The classic app's flags still work.
- `grokhub-hub` now carries the headless tools the desktop shell has no console for: `--mcp-self` (the agent's skill, connection and automation tools for MCP clients), `bench local` and the `sudo` helper (`--askpass`), plus `--hub` as another name for the hub. The `grokhub-self` server moved into the agent crate; the classic app's flags still work.
- The desktop shell answers `--version` and `--help`, and treats the classic app's `--agent` and `--tray` like a login start, so old service files and scripts keep working.
- The desktop app's Linux packages now include grokhub-hub, so sudo prompts from agent commands ask through the system dialog and the Cua gate starts with it, and `grokhub-hub --oauth` signs in with Grok into the same keychain entry the app reads.
- CI: the desktop shell checks also run when `crates/grokhub-hub`, the root `Cargo.toml` or the release workflow changes, and the shell job has 60 minutes instead of 45.
- CI: a test fails when a local crate the desktop shell builds, or a file its tests read from outside `src-tauri`, is missing from the shell paths filter in `ci.yml`.
- CI: a test fails when the step that stages grokhub-hub is dropped from the shell job of `ci.yml` or `release.yml`, moved to another job, put after the first cargo or tauri step, or loses its Windows and macOS condition.
- Building the desktop app on Windows or macOS: `node scripts/stage-hub-sidecar.mjs` stages the grokhub-hub it just built for this computer also when `CARGO_BUILD_TARGET` or a cargo config names a build target; before, it could stage a copy left by an earlier build.
- `docs/cross-platform/README.md` says to run `node scripts/stage-hub-sidecar.mjs` before any cargo or tauri command in `src-tauri` on Windows and macOS, and what the error is without it.
- CI: the cross-platform lint jobs pass again for macOS, iOS and Android (items only Linux, Windows or macOS use are no longer compiled elsewhere).
- CI: the macOS sandbox tests of the agent compile again (a test helper existed only on Linux).
- CI: the desktop shell lints clean on Windows again (a variable was mutable only on Unix).
- The desktop app's Windows installer and macOS app now include grokhub-hub next to the app, the same helper the Linux packages ship, and CI runs the desktop shell checks when crates/grokhub-code changes.
- Pulse: tapping Like on a For You idea sends more like this once and turns it into a filled heart that stays; tapping the heart takes the like back and undoes that taste signal. Reduced motion skips the animation.
- Screenshot-heavy chats send far less: only the newest 3 images stay whole, and older ones become a short note that asks for a fresh screenshot if one is needed. Old images are dropped three at a time, or along with any other history trim, so the prompt cache breaks once per batch. Set `keep_recent_images` in `history_trim.toml` to keep more; `age_out = false` keeps every image.
- Imagine: sign in with Grok right on the Imagine page in the desktop app, like the old app. Imagine keeps its own sign-in in the OS keychain, uses it first, then your chat sign-in, then an API key.
- `/import` no longer adds a "Tone: …" line to SOUL.md that you did not pick: it drafts from your files as they are plus the OpenClaw notes, and a line that is already in a file is not added a second time.
- Running `/import` again, or pressing "Add from OpenClaw" after it, no longer adds the same notes twice; Review says when the notes are already there, and the button goes away once it has added them.
- A USER.md, SOUL.md or MEMORY.md over 1 MB is left alone by Save instead of being replaced; Review and the Saved screen name the file. An import stops before it would grow a file past that size and reads at most 64 notes files, and says what it left out.
- `/import` on a computer with no OpenClaw or Hermes workspace says so in the chat and opens nothing; with one, it opens Review without starting Get to know you over, and Cancel or Escape closes it without marking anything skipped.
- A tone you cleared in Get to know you stays cleared when Review is resumed after closing the app.
- Get to know you reads and saves its files off the window's main thread, and an import only reads files it can import.
- `/import` in the desktop app brings in an OpenClaw workspace through Get to know you's review: every line to be added to USER.md, SOUL.md or MEMORY.md is shown first, TOOLS.md and files with secrets are skipped, and nothing is written until you save.
- Linux clone install: `scripts/install.sh --app shell|cabin|both` picks which desktop app to build and install. `shell` builds the Tauri shell (`grokhub-shell` plus a "GrokHub Shell" menu entry) and the hub, and stops early with the packages to install when npm or webkit2gtk-4.1 is missing; `both` installs both apps. With no `--app` the install is the cabin and the hub exactly as before, so the cabin's own update is unchanged.
- Simple read-only asks like "list my skills", "how much disk space do I have" or "is local AI running?" now answer right away from the matching tool with no model request; anything less plain still goes to the model.
- The agent writing to `AGENTS.md`, `CLAUDE.md`, `MEMORY.md` or a skill file now always shows you the change first, even on Always, so a web page can't leave standing instructions for every later chat. Secrets in a `MEMORY.md` write are redacted.
- Bump jsdom to 31 for Vitest; add @testing-library/dom and tolerate jsdom's shorter transform floats.
- The Auto mode judge now sees a short, fixed window instead of the last few thousand characters of the chat: what you asked in your own words (your first ask and your newest few, kept even after the chat compacts), the last few tool calls and the agent's latest words, and the action itself. Tool results are left out, since that's where injected text lives, so a long run of plain steps can't bury a bad one or talk the judge into it. A helper's judge also sees its brief, marked as written by the lead agent.
- Workboard: the board now has a token budget you can measure. The cabin rules no longer mention the board, so every chat request is 156 bytes lighter, and a paused card no longer adds a reminder to the chat's learned notes. A board full of cards adds 0 bytes to a normal turn; resuming a card adds one note of at most 1,200 bytes to that turn only, and the next request adds nothing new. The efficiency bench has three new rows (`kanban_off`, `kanban_idle_on`, `kanban_resume`) that check this on every test run. Old `WORK_PIN:` / `WORK_UPDATE:` lines in replies still work.
- Settings → Pulse → Know me: the opt-in scan of names and counts on this computer, a switch per signal, Scan now, and "What I know about you" with Delete per tag and Wipe all, both asking first. Settings → Permissions: each allowed scope lists what it taught, with "Forget these".
- Labs: the update channel note now points to About, under Updates, instead of the cabin.
- Updating or installing one app ("update the grok bot app") gets its own steps: find how it was installed and its version, update it with its own updater, package manager or official download (never by taking it apart), keep the old copy, and confirm the new version runs. A "continue" or "keep going, act now" now carries on the ask before it instead of being read as a new repair, and picks up the checklist the earlier run left open so the turn does the next step instead of only describing it.
- History shows its chats once: the second filtered chat list under Projects is gone, leaving one search over chats and memory.
- Queue shows todo states in words (In progress, To do, Done) instead of raw keys and row ids.
- Home: the PC Loadout buttons use the same casing as the other cards, and the edit pencil is labelled Edit Home.
- Pulse: "Offer to sleep the PC" and "Suggest Downloads cleanups" sit in their own rows with Turn on / Not now pinned right, instead of crowding the question.
- Objectives: the checklist column is centered on wide windows and scrolls at its own edge, so there's no stray scrollbar line at the window's edge.
- Lazy tools and the stable prompt are now on by default, cutting tokens per chat by about a quarter to a third with recall unchanged. Tools a chat loads stay loaded for that chat, and reopened chats and exports show only what you typed. Set `enabled = false` in `deferred_tools.toml` or `prefix_cache.toml` to turn either off.
- Backup and move for everything the app learned: `/learned backup` saves it to one file in the config folder, `/learned import` takes that file on a reinstall or a second PC, and `/learned restore` puts back what was there before the import.
- Auto setup now reads the learning engine in both apps. When about 30 rows of quick chips have gone by with none picked, it offers to turn suggestions off, and it never switches them off on its own. The desktop app used to pass auto setup no usage at all, and the cabin never filled this count.
- Learning engine, first part: one place for the app to record what happened (ids and counts only, never text) and to keep what it concluded, with a confidence per line, counts that fade over 30 days, and Forget and "That's wrong" for the user. Nothing uses it yet, so the app behaves the same. It writes at most once every 30 seconds and stays under 2 MB on disk.
- What the app learned is now on show. Settings, Privacy & data in the desktop app lists each thing it worked out from quick chips, cards and runs, with how often it was seen and how sure it is. Forget clears a line, That's wrong rules it out for good, and Forget everything clears it all. In the cabin, `/learned` prints the same list, with `/learned forget <id>`, `/learned wrong <id>` and `/learned forget all`.
- Plan reply chips (Yes, go, Tell me more, Change this and the rest) now learn through the learning engine. The reply you usually pick still leads the row, but the habit fades if you stop using it, it shows under what the app learned, and Forget or That's wrong there resets the row.
- Learning engine, second part: quick chips, Pulse and Home cards, and finished runs now feed it in both apps. It records which chips are picked, hidden or passed over, which kinds of card are liked or dismissed, and how each model and skill did (worked, failed, corrected, undone, time and cost). Ids and numbers only; nothing reads it yet, so the app behaves the same.
- Learning engine: the cabin and the desktop app can run on one folder without losing what the other learned, and Forget in one holds in the other.
- Wider autonomy: when Loadout is very sure (0.9 or more, seen 10 times or more) it can patch one of its own skills without asking, at most once a day, with Undo on the Done-for-you card. Sending, paying, deleting, secrets and permissions still always ask.
- A deliberate try now shows on the list of decisions (`/learned decisions` and the desktop page), once, with why it was shown.
- The app now tries things on purpose, rarely: a skill you have run a couple of times around the same time of day can show up once as a quick chip marked as a try. At most one a day, never while you are busy or in quiet hours, and hiding it is a no.
- The decision engine takes your goals (lowest cost, fastest answers, fewest interruptions, best quality) and puts a change that serves the first one ahead of the rest. Set them with `/learned goals` or **What matters most** on the desktop page. Both engines now read one picture of right now (quiet hours, busy, Halt, a running game, battery), and the page says when changes are waiting on it.
- Habit chips: a skill you usually run at the same time of day shows up as a quick chip then. Three taps and it has earned its place; a hide rests it for a month. It goes through the decision engine, so `/learned decisions` says why it showed.
- Habits belong to a setting: what you run while a game is up is learned on its own and only offered while a game runs, so it does not show up at work.
- The learning engine gets a head start: the first time it runs for someone who already used the app, it reads their past runs and card choices once, so habits and tastes are known on day one. It never does this again after Forget everything, and not while learning is paused.
- The on-device model's launch tuning follows the Local AI trust dial: on offer or watch it stops trying other launch settings and keeps the one it has.
- Settings → What the app learned on the desktop app now has buttons to save a backup, take one in (it asks first), put back what was here, and show the fault report. Nothing is sent anywhere.
- The router's own tuning now follows the trust dial: set Router to offer or watch and it stops switching models on its own. Halt, quiet hours, a busy moment, the daily limit of five changes and the pause after too many undos hold it too, and undoing the same kind of router change twice turns it off.
- A model-choice change the app tested but may not make on its own now waits for you: `/learned decisions` lists it under "Waiting for your yes", and `/learned yes <id>` or `/learned no <id>` answers it. The desktop app shows the same with Yes and No buttons. A yes goes in on the next pass and Halt still stops it; a no ends it for a month.
- Settings → Permissions: "What Loadout can read", one row per learning scope, each off until you allow it, with Revoke, the folder picker and the browser choice. It uses the same consent ledger as the cabin.
- The decision engine keeps a scoreboard: for each area, the share of its offers and changes that went your way in the last four weeks against the four before. An area that got worse asks first for a month. `/learned decisions` and the desktop page show the numbers.
- The weekly self-review now also reads what the app measured about itself: a skill or a model that keeps failing, and an area whose own changes you kept less than before.
- `/learned report` shows a fault report for the developers, in ids and counts only and without your skill names. Nothing is sent; you copy it if you want to.
- Auto setup now follows the Settings trust dial: on offer its changes become suggestions, on watch it changes and suggests nothing. Halt, quiet hours and a busy moment hold a change until later. The first-run setup and your own answers are not affected.
- The learning engine notices a habit that flipped: four times in a row against something it was sure of drops most of the old habit at once, without waiting for the 30-day fade.
- What Loadout learned: **Always** makes a line your own rule that nothing counts against, and **Pause learning** stops counting until you turn it back on (`/learned always <id>`, `/learned pause`, `/learned resume`).
- What Loadout learned (desktop): **How far it may go** sets each area to watch only, offer only or act when sure, **What it decided** lists the latest decisions with the reason for each, and **Carry on** lets it act again after too much was undone.
- The weekly recap card has a new section, "How its own calls went": for each area with a score, the share of what the app offered or changed that went your way in the last four weeks. It is left out of the share text.
- More Library games get real art. Loadout now also looks for the other cover files Steam keeps on your PC (the 2x and capsule box art, the hero and capsule headers), and when the store's usual file names are missing it asks Steam's store for the game's own header, which newer games keep under a hashed address. Games Steam has no art for at all still get the painted cover.
- Library covers now load from disk at once instead of fading in one by one on every launch. Covers from Steam's cache are kept in Loadout's own folder so they survive Steam clearing its cache, each cover is decoded once instead of twice, a few covers load side by side with disk hits before any download, a corrupt file no longer blocks good art, and a game the store has no art for is asked about again after a week instead of every launch.
- Library with no games is no longer a dead end: the header and its Skills button stay, and the empty state says what the library reads (games Steam and Battle.net have installed on this PC) with an Open Steam button. With Battle.net games and no Steam, the grid shows them as before.
- The Library is a cover grid again with the detail panel on the right; Battle.net games are tiles in it, and World of Warcraft shows once with Retail and PTR under it, however many Wine prefixes reach the same install.
- The WoW addon (1.1.0) now says hello in chat, sits in the minimap's addon menu and answers `/loadout`, no longer saves 0 gold or item level 0 at logout, and records spec, gear, professions, currencies, renown, Mythic+ rating and keystone, Great Vault, lockouts, achievement points, mounts, pets, quests and played time, which the WoW detail panel shows.
- Installing the addon under Wine uses the game's existing AddOns folder whatever its case, so the game finds it.
- Pulse: tapping Like on a card plays a short pop that turns it into a filled heart; tapping the heart takes the like back. Reduced motion skips the animation and swaps instantly.
- Live eval: `--auth signin` also reads the shell's sign-in and the cabin's Settings > Account sign-in (read only, never refreshed or printed).
- The live eval now runs the app's own shape: the chat prompt with the default personality, no turn cap, the router's starting effort and the Auto gate (a scripted answerer allows each ask and the report counts them). Seven longer cases reach output masking, compaction, an Explore subagent, plan mode, a file with planted instructions, an edit and test loop and a screenshot-heavy task. `--ab <flag>` runs every task with one setting on and off, and `--provider openrouter|local` runs the same tasks on another provider or this computer's model.
- Live eval: the dollar cap now holds each call's input price before it goes out, so parallel subagents can't all slip past it together, and a call cut off after it was billed still counts (the report shows how many). Server compaction goes through the cap the way the app uses it, subagents read the run's own config folder instead of yours, and the example runs against a throwaway config folder.
- Live eval `--ab`: each switch is now judged task by task on billed dollars per solved task, with 95% intervals for solve rate, cost per try and model calls per try, a per-task table, and a minimum detectable effect set before the run (`--mde`, default 10%). A run with fewer than three paired tasks, or too noisy to see that effect, says so instead of giving a verdict, and a switch that solves fewer tasks is never called a win however much it saves.
- Live eval: `--auth signin` runs on the app's saved Grok sign-in (SuperGrok pool) instead of an API key; read-only, never refreshes or writes the keychain.
- Live eval (RT-05): each model call has a wall-clock limit and a bounded retry with a growing wait, a dead key or sign-in ends the run cleanly, `--deadline-mins` stops before an outer hard stop, and with `--out` the report and a `<out>.jsonl` row are written after every try so a cut-short run keeps its data and `--resume` finishes it. The report gains a Routing section: effort in range, under and over effort, cost per solved try, and cost against always sending the flagship.
- New live eval for the agent: 20 real tasks (PC diagnostics, settings, mods, memory recall, small edits, the no-cheat line) run several times against a real model under a hard dollar cap, reporting pass^k, recall, cost, cached share and latency.
- Loadout says Loadout in its version line, chat exports, copied diagnostics, Linux app menu, Local AI models label and the default Work project.
- The older pages (Command, Queue, Devices, Memory, History, Journal and Night) now use the same text sizes, page margins and red-rimmed Loadout cards as the rest of the app. Page titles match the newer pages instead of the old oversized title.
- The taskbar, window, exe, tray, installer, Linux app menu and shell bundle icons are now the Loadout slot-grid icon instead of the old Grok mark. `scripts/render-brand-icons.py` renders every size from `brand/app-icon.svg`; file names stay `grokhub.*` so installs and updates don't move.
- Windows Setup, the Start menu and the desktop shortcut now say Loadout. Updating clears the old GrokHub shortcuts; the install folder, AppId and `grokhub.exe` stay the same, so updates land in place.
- Local AI: when the GPU backend crashes the engine at start (a bad or missing GPU driver), Loadout starts it again on the CPU with every GPU left out (`--device none`) and remembers that for this model and driver, so Local AI still answers. Out-of-memory keeps its own retry with fewer GPU layers.
- Settings > Local AI offers a faster engine for NVIDIA GPUs on Windows and Linux (llama.cpp's CUDA build, about 640 to 740 MB with its CUDA runtime). It is never downloaded unasked, and one button switches back to the standard engine.
- On Windows, the same row offers llama.cpp's ROCm build (about 250 MB) for AMD Radeon RX 5000 cards and newer, and Radeon Pro W6000/W7000. Older Radeons keep the Vulkan engine.
- Local AI now downloads its engine on Macs too: Apple Silicon gets the Metal build, Intel Macs the CPU build.
- ARM PCs (Windows on Snapdragon, ARM Linux) now get the GPU (Vulkan) engine instead of CPU only, and still fall back to the CPU when no GPU driver loads.
- A Grok sign-in that has expired, can't be renewed, or that xAI turns away no longer fails every chat with "Sign in with Grok or add an API key." When Local AI is ready, the chat answers on it and says the sign-in needs renewing, and the service status says the same instead of "xAI auth present". Diagnostics now name the app Loadout.
- In the desktop app on Windows, a running game now frees the GPU within one sample: the app's local AI sampler reads the game guard every sample, as the other shell already did. The app's sampler now runs the shared governor step, so both shells pause, throttle, restart and scale the same way.
- Switching local models no longer stalls the window while the old engine stops: it stops off the UI thread.
- Changing the context size in local AI tuning no longer overrides the context a running server really launched with; the relaunch sets it.
- The on-device model now sizes itself to each task from what is free right now: free VRAM (NVIDIA and AMD), free RAM, CPU load, battery and a running game. There is no setting for it; the running-model row shows the mode.
- Balanced (the default) runs your pick, or a lighter model while the computer saves power or memory, or while less is free than the pick needs.
- Gaming runs the lightest model on the CPU with half the threads, so a game keeps the GPU and its frame rate.
- Performance: on AC power with the CPU mostly idle, a hard task (code, debugging, planning, reasoning or a long paste) steps up to the biggest downloaded model that fits with room to spare and still runs fast. The reply starts on the model already loaded; the bigger one loads once the chat goes quiet, and steps back down when the computer gets busy or a game starts.
- Local AI tunes itself per device, model and mode. A small online bandit tries the slot count, the drafter on or off and a smaller micro-batch, one change at a time, only at a launch that happens anyway and only within the memory the plan has. It scores each by the server's own decode speed and prompt read time, promotes a change that is at least 5% faster, rolls back one that isn't, and bans one that leaves under 512 MB of VRAM free. An out-of-memory cap now grows back a step after two clean starts. The engine and model lookup that every routing decision makes is cached until a folder or file it reads changes. No new settings.
- The on-device catalog now lists 25 uncensored chat models: the five Gemma 4 defaults plus the best Heretic or abliterated build from NVIDIA, Qwen, ByteDance, Zhipu, Mistral, OpenAI, Nous, IBM, DeepSeek, AllenAI, Microsoft, Meta and Hugging Face, every file SHA-256 pinned. The new ones are optional: they never replace the model picked for you, but once downloaded the router can step up to them for a hard task.
- Qwen3.5, Llama 3.2, Ministral 3 and Granite 4.2 come with small same-family draft models; the speed test still decides whether each one is used.
- The on-device model catalog says what each model is for (chat, draft, vision, image, voice and other kinds), who made it uncensored and how, and caps the standard chat models at 25; draft, vision, image, voice and other specialty models don't count toward the 25. No model claims vision until its image reader downloads with it.
- New local models now reach you without an app update. Once a day the app looks for a newer model list in GrokHub's newest release, checks its signature, and uses it from the next start. A list that isn't signed, or doesn't load whole, is ignored.
- Local AI: a model kept running under Throttle now shares the machine politely: its threads sleep instead of spinning (`--poll 0`), run at low priority (`--prio -1`, and the engine process below normal priority), and on a GPU it sends short batches (`--ubatch-size 128`, and on Vulkan at most 20 graph nodes per submission), so a game's frames wait less. Each flag is passed only when the installed engine lists it in `--help`.
- The desktop app's Local AI page has the same Download models section: Chat, Drafts, Image, Voice and Other tabs, Download all and Download everything, Remove and In use, with progress and Pause. It's the desktop app's first way to download a chat model.
- Settings → AI & chat has a Download models section with Chat, Drafts, Image, Voice and Other tabs. Each row shows the model's size, whether it fits this PC, what it's for, the Uncensored badge and its licence, with Download, Remove or In use. "Download all" fetches every model on the tab that fits, best fits first, each chat model with its draft model. "Download everything" does the same across all tabs. Downloads run one after another and stop before the drive fills. Only the first chat model becomes the one in use.
- The draft model's speed test now runs by itself: once the computer has sat idle for ten minutes on wall power with no game running, a draft model nobody has timed yet gets tested, and a reply that comes in meanwhile waits for the current timing only. Real chats keep counting too: after 20 replies where under 40% of the draft model's guesses were kept, it turns off from the next load and gets a fresh speed test a week later.
- Draft models land in one folder, `~/GrokHub/models/drafters`, in both apps, and the engine looks there, beside the model and in the desktop app's older folder, so a downloaded draft model is actually found. The desktop app checks free disk before a draft download, and the window app's speed test runs in the background and shows its whole report, draft model verdict included.
- Local AI: with no sign-in and no API key, chats now answer on the installed local model, online or offline, on Windows, Linux and macOS. Before, the send was refused with "Sign in to send this message" even though Local AI was ready. The composer note says chats run on Local AI until you sign in. Hard chats go to the device too when nothing else can answer; prepare:hard, cloud-only steps (images, web) and steps with send, delete or credential tools still never do.
- Local AI and games: the new shell now runs the same governor as the cabin, so a game unloads the on-device model there too. A known game (or a deny-list app) frees the GPU at its first sample instead of after five; a model still on the GPU when a game starts is stopped at once, even mid-answer, and new work while a game holds the GPU starts on the CPU. Local tool turns now stream, so Stop ends them between tokens.
- Local AI switches between the chat models you've downloaded on its own. On a low battery it runs the smallest one and goes back to your pick once it charges. When your pick runs out of memory at start, the largest smaller model on disk stands in instead of the start failing. The running-model rows say when a smaller model is standing in, and a model answering a reply is never swapped mid-reply.
- The idle unload timer learns: when the model comes back within a minute of unloading, it stays loaded twice as long next time (up to an hour), and it comes back down when the model sits unused for hours. A timer set in Settings stays as set.
- Asking the on-device model for a picture ("draw me…", "make an image of…") now reaches the picture tool instead of a "can't make images" reply: small device windows keep the picture tool when a picture model is installed (and `load_tool_schema`, so tools like web_fetch stay reachable), the device is told it can draw, and with nothing installed the reply says to download a picture model and the picture guard from Settings → Download models → Image. The hard-ban check and the picture guard still run on every picture.
- Local AI: the app's own background calls (labels, summaries, difficulty) now ask the on-device model for no thinking, a short reply (`max_tokens`) and JSON in one fixed shape (a JSON schema the server turns into a grammar). They use whatever model is already loaded and never start one or swap the loaded one out; with none loaded they answer without the device.
- On-device chats now fit the local model's context window. Loadout reads the window from the local server and, only when the full prompt won't fit, keeps the core tools, drops optional guidance (never the safety rules), shortens a long AGENTS.md and keeps the newest turns, telling the model older turns were left out. A 4k or 8k local model can now answer a chat turn with room to reply. Grok requests are unchanged.
- A downloaded local model no longer reads as "not set up" for the rest of the session after one failed start: it tries again a minute later, a draft model that keeps it from starting is dropped (and stays off for that pair), and draft or vision helper files are never loaded as the chat model.
- Chats with an 18+ voice stay on this computer for every turn, and when the local model can't answer, the chat says why in one line instead of going online or showing a raw provider error.
- Ask the assistant why the local model isn't ready and it reads its own model folders and tells you what it found, instead of asking you to check files.
- An OpenRouter privacy-settings refusal hands the turn to the next model instead of ending the chat with the raw notice.
- Every on-device model now has its own uncensored draft model: the matching abliterated multi-token-prediction heads from its own Hugging Face repo, pinned by hash. It downloads right after the model when the drive has room, and the speed test still decides whether it stays on. Draft model messages no longer mention a Manual draft setting.
- Local AI picks the model on disk that's best at each task (coding, reasoning, writing, long pastes, 18+ chat) and switches without making a reply wait: it loads once the chat goes quiet, or right away when the file is already in memory, and the idle tick keeps that model warm. Models that refuse or fail a kind of task are marked down for it. A new Dynamic switch on the Local AI page turns this off, so your pick answers everything (a game, a low battery or too little memory still swaps to a lighter one).
- Pictures on your own PC have a second model, Chroma1-HD, in the Image tab of Models. It's used when Z-Image Turbo isn't downloaded.
- Macs with Apple silicon can now make pictures on the device too: the picture engine downloads for macOS and runs on the Mac's graphics chip.
- Each local chat model now gets a refusal check on your PC: while the computer sits idle, it answers a fixed set of everyday asks (like killing a stuck process or a safe medicine dose) once, and Running now shows how many it answered and which one it turned down. It runs after the draft model's speed test and stops the moment you send a reply.
- Settings → AI & chat now shows the on-device model that is running: its name and quant, whether it runs on the GPU, the CPU or both, its size, the last reply's speed, its draft model and how many guesses it kept, and when it unloads. Two buttons sit under it: Keep loaded and Unload now.
- The desktop app's Local AI page now shows the running model too, with Keep loaded and Unload now. Its Manual draft model choice is gone: the speed test and the live reply check decide when the draft model runs.
- Local AI: the memory planner now reads a model's sliding-window layout from its GGUF header (window size, which layers slide, their own KV widths and per-layer KV heads), so Gemma-style models get the longer context their KV really allows. Every launch sets its slots explicitly (`--parallel` 1, 2 or 4 by state and context, unified KV past one slot), reuses cached prompt chunks (`--cache-reuse 256`) and caps the host prompt cache at a quarter of free RAM (`--cache-ram`), each only when the engine lists the flag. Background calls never take slot 0, which stays free for the chat on screen; busy slots split a shared KV pool's context; the router's local context cap follows the real launch; and a chat's trims stay put from turn to turn, with the history cut moving forward in big steps, so the server's cached prefix survives.
- Local AI now recommends a speedy, capable everyday model for your PC (marked "Recommended for this PC"), and every local model carries task strengths (chat, code, reasoning, writing, long pastes, 18+) so the router can pick the best one per task.
- Replies can now be read aloud on your own PC with no Grok sign-in. Download the Kokoro voice from the Voice tab in Models, and it speaks with the mouth moving, using the same voice names (Ara, Eve, Leo, Rex, Sal). Nothing leaves the computer.
- Local AI: a governor level change no longer reloads the model when the new launch is the same; when it differs, the new server starts beside the old one and takes over once healthy (the old one finishes its requests, up to 20 seconds), falling back to stop-then-start only when memory can't hold both. Loading a model no longer holds the engine lock, so Settings, a stop or a model switch never wait on a cold start, and the start wait grows with the model file instead of a flat 30 seconds.
- The agent has one built-in `look_at_screen` tool that works on KDE and GNOME Wayland, X11, Windows and macOS through the app's own capture, even with desktop control off (it still asks first, unless you allowed it always). Shell screenshot commands like spectacle, grim or scrot are refused with a pointer to it, so the agent stops fighting the sandbox to see the screen.
- Per-chat caches (trim results, briefs, talk styles, playbooks, frozen MCP tool lists and the prefix guard) now drop the least recently used chat when full, instead of wiping every chat at once.
- Local AI engine updated to llama.cpp b12017; local models only get reasoning levels they support.
- Long agent runs keep the newest ten tool results whole and fold older big ones into a short note with the call, a summary and a path to re-read them, so long chats send far fewer tokens.
- Connectors speak the newer MCP spec (2025-11-25): tool results can carry structured data and links to resources, tools show their own names and icons, and a tool its server marks destructive asks you first every time, even under Always. Older servers keep working.
- Connectors also work with servers on the newest MCP spec (2026-07-28), which drops the session handshake. Loadout checks which spec a server speaks when it connects and falls back to the older handshake on its own, so older servers keep working and a server's questions still reach you on the same card.
- MCP servers start connecting in the background when Loadout opens, side by side, so the first chat no longer waits on every server in turn. A chat's MCP tools stay fixed for the rest of a turn; a server that connects meanwhile joins at the end of the list on your next message instead of rewriting the tool list mid-turn.
- Security: when a connector's tool fails, its error message now reaches the model as untrusted data inside the same fence as its normal results, so a server can no longer slip instructions in through an error. The tool row in the app still shows the plain message, and successful connector and web page rows now show their text instead of the fence header.
- MCP: tool calls that can change something carry an idempotency key in `_meta`, and a resend after a lost answer reuses the same key for ten minutes, so a server that honours keys never runs the same send twice. When an answer is lost mid-call, Loadout is told the call may have run and to check before sending again.
- MCP tools are listed in name order whatever order a server sends, and a server's ttlMs is honored: its tool list is fetched again once it runs out.
- Every image and video prompt now passes the hard-ban check before it goes to Grok Imagine, from chat, the Imagine page and the Tauri app alike: a prompt about minors or age-play, non-consent, real people in sexual content, or animals is never sent, and an 18+ prompt stays off online services (18+ images will come from on-device image making). Each says so in place of a result.
- Memory recall now finds notes by meaning as well as by words, using a small local embedding model (potion-base-8M, about 30 MB, fetched once in the background and checked against a pinned hash). "Which GPU do I have" finds "graphics card is an RTX 4080". Replaced facts are never recalled. Set `"memory_embeddings": false` in app.json for word search only.
- Session memory saved before a compaction now holds only the chat's own words, the newest part, never playbook or recalled text. `/dream` keeps your memory file when a rewrite would gut it or drop a line recall used lately, and tells you what it kept and dropped.
- Memory: the first message after a memory file changes no longer waits on an index rebuild. Only the changed notes are reindexed, in one transaction, and the search index follows row by row (release, 314 notes: about 270 ms down to about 4 ms). `examples/memory_index_bench.rs` measures it.
- Memory recall picks the notes that match your ask: the best match is no longer cut off when many notes share a word, short questions like "where are my saves" search their own words, "saves" finds "save", and the pack carries at most five focused chunks.
- Speed spans: memory recall records `turn:memory_recall` (LO-46 step 1). Release p50 stayed under 2 ms on realistic notes, so recall is not prepared while you type.
- Memory: "Remember new lines" and "Global" are now slider switches, matching the rest of Settings, instead of round checkmark bubbles in WebKit.
- Helpers in a git worktree: the lead can now ask to merge a helper's finished work with a new `merge_subagent_work` tool. It asks you every time, also when the permission setting is Always, in both apps, and the card says how many files go into which project folder and names the helper's task. Its only answers are **Allow once** and **Deny**. Nothing is merged without your yes; it is refused in background runs, where nobody is there to answer, and in a supervised desktop task. A request for work the lead may not merge is refused at once, with no card. The merged files are uncommitted changes in the project folder. A helper cannot ask for a merge, and work that is partial or held is merged only by you.
- After a plan is approved, the lead may now run up to three writing helpers at once when their tasks change different files, each in a worktree of its own. Tasks that change the same files still run one after another, and so does everything in a folder that is not a git repository.
- `/worktree` no longer calls worktrees a grok CLI feature: it says chats run in the bound project folder and only helpers work in worktrees of their own.
- Loadout now follows Minecraft: Java Edition while the game watcher is on: it reads the game's own logs/latest.log (the vanilla launcher's folder and Prism Launcher or MultiMC instances) for your deaths and advancements, shows the session in Settings, and posts a recap to Pulse when the game closes. Other players' chat can't pass for your events. No mod, and nothing touches the game.
- Download models: Z-Image Turbo pictures and Chroma1-HD pictures install again. The checksum lookup now reads each file's own folder on Hugging Face (and its next pages), so a file in a subfolder, like their VAE, is found.
- Download models: when a model doesn't download, the reason shows on that model's row, under its button, with the step that failed. When the drive is too full it says how much is free and how much is needed, and points to the Remove buttons. A resumed download only needs room for what is left.
- Download models: one model's failure no longer stops the rest of the queue. Pause and Cancel still stop it.
- Download models (desktop app): each model is its own card, so its button sits under its own name.
- Download models: the Download all button becomes the best lineup for this PC. It reads the hardware already detected on this machine and picks one model each for Speed, Balance, Performance and Image (Video once on-device video models exist), says how many, how big and which goal each fills before downloading, and gives hot-swap the right set to switch between. Nothing is stored or sent.
- Chat can research mods and set one up: ask for a mod and Grok searches the game's sanctioned sources, reads the mod's files and requirements, and puts an Install card on the Mods page. Grok can't install, update or remove mods itself: nothing downloads until you click Install on the card, and nothing is written until you confirm the file list. Games Loadout won't mod get their reason, and a shell command or file write aimed at a moddable game's folder asks you first.
- Mods finds new mods on its own: a game that shows up on this PC gets its mods looked up right away, every game gets its top mods weekly, and a deeper page a day keeps variety coming, all from the sanctioned mod sites with no AI calls. New finds get their own tab and a count on each game.
- For you on Mods now shows five picks, at most two per game, in a cleaner row of cards.
- Mods: open a mod to see its files, then install, update or uninstall it from a sheet that lists every file it writes, with an Installed tab per game.
- Mods: Install, Update and Uninstall for Stardew Valley, Skyrim, Fallout 4, Starfield, Cyberpunk 2077, The Witcher 3, Minecraft, WoW addons and the BepInEx games. Nothing downloads until you click Install, and nothing is written until you confirm the sheet that lists every file it adds or replaces. Replaced files are backed up, Uninstall puts the folder back as it was (keeping any file you changed since), and an install cut short is rolled back the next time Mods opens. The game must be closed. Files a site only offers in the browser are picked up from Downloads after you say they're done. Workshop games subscribe in Steam, and games a mod manager already looks after (Vortex, Mod Organizer 2, r2modman) are left to it.
- Mods page: your moddable games from Library, most played first, with search and Popular, Trending, Updated and Installed for each one over the sources Loadout trusts for that game. Games Loadout won't mod sit under "Can't mod" with the reason. Read-only for now; installing comes next.
- The new app has a Mods page: your moddable games most recently played first (the rest under Can't mod with the reason), Popular, Trending, New finds and Updated per source, a search, and For you, the same as the classic window. Installing comes next.
- Mods (groundwork): a mod allowlist that decides which games Loadout may mod and in which modes (singleplayer and your own servers; never kernel anti-cheat games, never competitive play), with a one-line reason for every blocked game, plus read-only search, detail and file-list adapters for Nexus Mods, Steam Workshop, CurseForge, Thunderstore and Modrinth behind a 6 h / 24 h cache and per-source rate limits. Nothing downloads or installs yet.
- Mods: a "For you" row at the top of the Mods page suggests up to eight mods for the games you've played in the last 60 days, ranked on your PC from how much and how lately you play, what you already have (similar mods, and what your mods need) and what's popular or trending. Nothing about what you play leaves your PC: Loadout only fetches each game's Popular and Trending pages, at most once a day and never with Offline only on. Each card opens the mod's details; Not interested hides a mod for good, and each game can hide its suggestions. 18+ mods stay out unless 18+ is on.
- The rail's Profile opens Settings → Account instead of dropping a menu over the page, the PC rail item is named Devices (it opens the pairing page), Devices' local task box says it runs on this PC, and the palette now lists Home, Library, Objectives, Journal and Command, with one Connectors row.
- On an API key, the nightly memory dream and the weekly self-review now go through xAI's Batch API at 20% off. A pass waits up to three hours for its answer and falls back to a normal call. `"batch_background": false` in app.json turns it off.
- Saying "good morning" or "hi" no longer leads to an offer to automate a greeting: greetings and small talk are never counted as repeat asks, and an offer whose topic is only a greeting word is never made.
- No thanks on a repeat-ask offer now holds at every hour, including for offers dismissed before this update.
- Clicking a Suggested automation tile opens a preview of what it does and when; only its Add button adds it.
- When the agent wants to create a new automation, you approve it on a card first, even with Always on, and it never makes one on a turn that was only a greeting.
- During a live Riot match (League of Legends, VALORANT, 2XKO), asks for gameplay tips get one plain line instead of an answer, because Riot's rules ban live coaching. Settings help and a look back at the match still work.
- A run no longer pauses after about 1M input tokens. It keeps going until the task is done, and stops only when its steps keep failing or repeating with no new work: first it gets a nudge to try a different way, then it stops and says so.
- A bell next to Activity in the titlebar now keeps archive Undo, saved diagnostics and status messages, instead of strips above the composer.
- Nyx's own browser now starts headless in a profile Loadout owns, so a locked or shared Playwright profile can't kill it. Set `GROKHUB_BROWSER_HEADED=1` in the Playwright server's env to watch it.
- On Linux, browsers Loadout starts get Wayland back (the cabin window still runs on X11 for close-to-tray).
- When the browser dies at launch, Loadout relaunches it once with a fresh profile; if it dies again, Nyx is told to read the page with web_fetch instead, so the turn still answers.
- Shell: clear clippy needless_lifetimes and field_reassign_with_default in the desktop Nyx module.
- Shell: snap the desktop Nyx dock and desk menu padding and type onto the design scales.
- Shell: rustfmt the desktop Nyx module and command signatures left unformatted by the wardrobe PR.
- Shell: Nyx's dock loads desktop companion state on mount without a setState-in-effect refresh wrapper.
- Shell: Nyx shows in the chat, with a face by Grok's replies and in the empty chat, and a docked card on wide windows whose pose follows what the chat is doing. Settings → Personality shows each voice's picture. Explicit art shows only with Adults-only personalities on.
- Nyx always has web search for a lookup: web_search and web_fetch are never dropped from a busy workspace's tool list, and Plan and Ask chats can look things up too.
- Game questions (quests, items, bosses, lore, patches) are searched before Nyx answers, without being asked. "Where do I turn in Aqir research tokens in WoW" is game help now, not a research report.
- Typing a tool like web_search into the terminal no longer runs (and fails) as a command; Nyx is told to call the tool itself.
- Nyx no longer offers to look something up when no search worked; it says so and marks what comes from memory.
- A chat reply that links a page no tool opened this turn gets sent back once: the model opens the page, drops the link, or says that part is from memory, not checked.
- A tool call the model writes into its reply as text (like `<report_findings>{…}</report_findings>`) no longer shows in the chat, in exports, or in the Tauri shell. A findings card written that way is posted as the real card; any other written call gets the model one note to call the tool properly.
- The Tauri shell shows a findings card's plain list after the reply instead of dropping it.
- A stopped reply now says why it stopped ("Stopped: the model changed mid-run", "Stopped: you sent a new message"), and the app log records each halt's cause with the chat and model.
- When the router switches models in the middle of a chat reply, the reply picks itself back up on the new model (up to twice in a row) instead of waiting for a Workboard resume.
- "yeah could use that info" and other go-aheads that name nothing new carry on the last ask, research included, instead of turning into small talk.
- When a turn's talk style or playbook changes, its notes say the earlier turns' notes are old, so an old research playbook can't steer a new turn.
- Desktop Nyx: while she speaks a reply she now hops gently with her voice on every outfit (smoothed so she doesn't jitter, off with reduced motion), and the window keeps painting while she talks so mouth patches move even with motion reduced.
- When a reply notices a game goal you mentioned, it asks under the reply: "Track goal: 99 fishing" as a one-tap in the desktop app, and "Track this goal?" with Track and Not now in the shell, web and phone. Track puts it on My goals; Not now hides it for 30 days. Each goal is asked about once per chat.
- Goals from chat: when you talk about a game goal, like "I want 99 fishing in OSRS", it shows up in the suggested tray on Objectives so you can Track it or say Not now. Say "add this goal" and it's tracked straight away. When you say how far you've got on a tracked goal, like "I'm level 82 now", its bar moves. It only links goals to games you own or that it knows, it reads nothing from a running game, and it asks about the same goal only once per chat. The goal lines the reply carries for this never show in the chat.
- The reference now covers Objectives and XP: where goals come from, follow-ups, season badges and how XP is earned. The Home modules spec marks Objectives and Streak & XP as built.
- Gentle follow-ups on goals left a few days: "Want to pick a night for your next Elden Ring session?". At most one per goal a week and two a week in all, never while a game is running, and after two go by with no progress the last one says "I'll stop asking about Elden Ring, just say the word." Any progress starts it over. The desktop app sends it as a one-tap card; the shell shows it on Home and Objectives with Plan it, which puts the ask in the chat box.
- Goals on Objectives now move from game data you already turned on, and show "Checked" when they do. Old School RuneScape skill levels and boss kill counts come from your saved hiscores reads. Steam achievement goals ("30 achievements in Hades", "100% achievements in Hades") come from the weekly achievements check, which now reads the games you have achievement goals for first. A watched Counter-Strike 2 or Dota 2 match moves "win 10 matches", "play 5 matches" and "500 kills" goals for that game when it ends. A goal's first reading becomes where its bar starts, so levels you already had don't pay milestone XP. Nothing new reads a running game.
- Typed goals understand "2000 total level" and kill counts ("kill Zulrah 100 times").
- Objectives no longer shows a Workboards button. Workboards stays on its own rail row and tab.
- The desktop Objectives page now has three parts. Today shows the goal closest to done, offers to pick up one you haven't touched in a week, and lists this chat's steps. My goals lets you type a goal in plain words, like "99 fishing in osrs", and nudge it along with +1, Done or Drop, and it holds suggested goals with Track and Not now. This season shows weeks in a row and a shelf of goals you finished.
- The Home objectives tile shows your goal that's closest to done.
- Objectives in the desktop shell, web and phone now has your goals too: Today with the goal closest to done, My goals with a box to type a goal and +1, Done and Drop on each one, suggested goals with Track and Not now, and This season with goals done and a trophy shelf. It reads and writes the same goal list as the desktop app, and a paired phone can add and move goals.
- In the shell, a chat whose checklist is all closed now counts as a finished task for XP at the end of the turn, so the XP bar moves there without pressing Accept as done. Accept as done later doesn't pay it twice.
- Your own goals now have a home: a goal store shared by the desktop app, web and phone, with goals linked to a game, a bar that starts where you really are, and XP at each quarter of the way (50) and when it's done (200, or 300 for finishing a game or a skill to 99). Suggested goals wait for Track or Not now, which hides one for 30 days.
- Monday's "Your week" on Objectives: up to three real changes from last week (a level reached, goals done, the goal that moved most, weeks in a row) and one goal to Track next. A quiet week shows nothing.
- Season badges: three goals done in a season earn its badge, and This season says how many are left. The first goal done each season pays a one-time +50 bonus XP.
- Internal: the Offline only network-exit test now counts the on-device model's loopback /props read (added by #1169), so beta's tests pass again. No behavior change.
- New Offline only setting: while it's on, Loadout stays off the internet, answers with your local model, and pauses third-party MCP servers, downloads and updates. This computer and your home network still work. The Settings toggle arrives next; for now set `offline_only = true` in `offline.toml` in the config folder, or run with `GROKHUB_OFFLINE_ONLY=1`.
- Settings → Privacy & data has an Offline only switch, and the status chip shows when it's on.
- Download models → Image now offers Z-Image Turbo (about 6.4 GB with its text encoder and VAE) and the picture guard, Qwen3-VL 2B (about 2.2 GB), which will check every on-device picture against the hard bans before it's shown. Downloading Z-Image also fetches the stable-diffusion.cpp picture engine (Vulkan build, about 35 MB, Linux and Windows x64). Making pictures on device comes in the next update. The speech and picture engines now share one pinned, checked installer, in both the desktop app and the new shell.
- On-device pictures: with Z-Image Turbo downloaded, chat draws 18+ pictures on your computer, and any picture when Grok isn't signed in, one picture per run so the model loads only while it draws and leaves memory right after. Nothing is drawn while a game or heavy app has local work paused. With the picture guard downloaded, every on-device picture is checked against the hard bans before it's shown (and deleted if it fails or the check can't answer); 18+ pictures need the guard. 18+ pictures and videos still never go online.
- On-device speech to text: Download models → Voice now offers Parakeet v3 (25 languages, about 640 MB). Downloading it also fetches the sherpa-onnx speech engine (about 27 MB, Linux and Windows x64), and voice input then transcribes on your computer, one clip at a time with two threads so nothing stays loaded or slows a game. whisper on PATH still works when Parakeet isn't downloaded.
- The agent always keeps its system prompt fixed and puts each turn's style, playbook and memory notes in that turn's own message, so every chat gets the cheaper cached prompt. The old switch that rewrote the system prompt every turn is gone, and the live A/B run can now compare WebSocket mode on and off instead.
- Chat: every message gets its own reply. In a casual chat, a message typed while a reply runs (a quick question included) waits as its own turn instead of being answered only on the side card; a typed `/btw` still answers on the side.
- A side answer that fails puts the question in line as its own message, and a steer sent after the run last checked for one goes as its own next turn instead of folding silently into the next reply.
- Stop no longer joins queued messages into one: the first comes back into an empty box word for word, and the rest keep their places in line and go one by one after the next reply or on "continue".
- The desktop app reads a message typed mid-reply the same way: in a casual chat it waits as its own turn too.
- Signing in with OpenRouter now finishes on its own: the key is saved and openrouter.ai is allowed the moment the browser sends you back, with no approval card in Settings or the chat.
- The OpenRouter row's Allow button allows it on that click, without a second card.
- Settings: Sign in with OpenRouter now finishes in Settings. The key it brings back, and the allow card after it, show right under the OpenRouter row (and under Other providers in Permissions) instead of only in the chat behind Settings, where they timed out unseen and the key was dropped.
- Settings: the OpenRouter row says where the sign-in stands (Sign in, approve the card below, Allow, or signed in with Sign in again), the sign-in is checked every frame instead of on the 15-second background beat, and the window comes forward when the key arrives.
- OpenRouter sign-in sends and checks `state` and names the new key, per OpenRouter's current OAuth PKCE docs; a deny now says why.
- Agent: a step that would send something to an email address, site or phone number that only a web page or other outside text mentioned now asks you first, even with Always on. Values you typed, your memory and your project's own git server pass as before; unattended runs refuse and explain why.
- Loadout can now read Old School RuneScape stats from Jagex's official hiscores. Under the game watcher in Settings, enter your display name and pick your account type (normal or one of the ironman tables). Loadout shows your total level, XP and rank, your highest skills and top boss or clue counts, and the XP you gained over the last day, reading at most once an hour or when you press Refresh. No key, no plugin, nothing touches the game, and Offline only stops the read.
- Recording and audio-check clocks now sit in the titlebar next to the bell, and the Save as skill offer waits in the bell, so nothing stacks over the toasts.
- Bump oxc-transform-react to 0.158.0 for the shell UI build.
- Packages the agent installs (npm, uv, pip, pipx, Poetry), and MCP servers started with `npx` or `uvx`, now skip releases younger than seven days, since most poisoned releases are pulled within days. A plain install beside a lockfile gets a note to install exactly what it pins.
- History, Journal, Queue, Memory and Devices now scroll under a fixed header instead of running off the bottom of the window. The Memory editor fills the page and scrolls inside itself, so Save and the file tabs stay put on a long MEMORY.md.
- Stop, Halt or a denied step partway through a batch no longer leaves tool calls without an output, and chats saved that way heal on load.
- Stop on one chat no longer stops background runs and unattended loops at the engine. They now stop only on Halt everything (tray, Ctrl+Alt+H, quit), which writes its own stamp. Desktop control still stops on any Stop.
- Quota notes from chats, helpers and background runs that reply at the same time no longer overwrite each other in `models/quota.json`. Every write now goes through one writer, and the account reads run outside it so a slow endpoint never holds up a reply.
- Read-only tool calls in one reply now run in parallel (up to 4) by default; set `enabled = false` in `batched_tools.toml` to turn it off.
- Tasks panel: a **Helper work** list shows what each worktree helper changed, also after a restart, with its state, file counts, files and worktree folder. **Merge** puts ready work into the project folder as uncommitted changes; a conflict changes nothing and offers **Merge again**. **Merge partial work** asks first for a helper that did not finish. **Finish merge** and **Put files back** show when a merge stopped part way. **Discard** and **Delete worktree** ask before they delete. A press that is refused shows why on its row.
- Tasks panel: a task whose helper's work waits in a worktree reads **Not merged**, also when the lead already closed it, and a helper's own row says what became of its work (for example `Ready to merge · 3 files`).
- Chat: Hangout TALK/TYPE mode and a tab strip that can archive, soft-delete, and restore chats, matching the cabin.
- Parity: chat turn extras — report_findings lands as a card with a pill per fix (a pill fills the composer; nothing runs until sent), the Finished block shows as a recap card with the turn's steps, each finished turn gets its evidence receipt, plan and question cards carry predicted reply chips, and decisions waiting anywhere gather in one inbox over the chat, also listed in the Command palette.
- Shell: Labs can put Nyx on the desktop in her own see-through window, with a wardrobe on the dock and right-click menu; off by default, same `desktopNyx` switch key as the cabin.
- Tauri Devices: Start share runs this computer's LAN hub in the app, another computer pairs with a code, and Run it here sends a task to a chat on this PC.
- Shell: opt-in game watcher (GSI cfg write/remove) and Pulse Steam achievement / wishlist Settings rows, same keys and defaults as the cabin.
- Tauri Imagine: + New project clears the prompt, last result and error for a fresh start. Agents lists queued jobs, and Run sends a job's prompt to its chat as a normal turn.
- Shell Memory matches the cabin: SOUL/USER/MEMORY tabs, Reflect, Restore, retention and chat-archive settings, and Pulse ledger tidy.
- Settings → Your PC → PC helpers in the desktop app now has the file, system and printer helpers the cabin has: Downloads cleanup suggestions (nothing moves until you pick files and approve the list), new 3D model offers, backup and unpushed-work reminders, pick up where you left off in code, the internet drop log with a speed test and CSV export, and LAN 3D printer alerts with an optional camera snapshot. Every switch starts off, and they use the same settings as the cabin, so both apps agree.
- The desktop app offers "Explain this?" above the composer when you save a new screenshot, as the cabin does. It's off by default under Settings → Behavior → Screenshot explain; a tap attaches the screenshot and nothing is sent until you send. Both apps share the switch and the offer.
- Parity: creating and editing skills — the Skills page makes a new cabin skill from a name and what it should do, and edits a cabin skill's description, trigger and steps. The write path moved into `grokhub_agent::skill_write`, shared with the cabin, so both apps write byte-identical skills through the ChangeLedger; a skill's verify script is written on save and still never runs without approval.
- Path of Exile sessions now count the maps you ran, the area level you reached and the time spent in maps, from the game's own Client.txt, in Settings and the Pulse recap.
- Path of Exile session recaps now say when the gap between your level and an area's level cut your experience, and by how much at worst. Path of Exile 2 works experience out differently, so it gets no such line.
- Loadout now follows Path of Exile and Path of Exile 2 while the game watcher is on: it reads the game's own Client.txt log for the areas you enter, your level-ups and your deaths, shows the session in Settings, and posts a recap to Pulse when the game closes. Party members' lines don't count as yours. Nothing reads the game's memory or touches the game.
- Settings → Your PC gets a PC helpers page with the wellbeing and focus switches the cabin has: help with the app you were just in, sleep when idle (15 minutes to 4 hours) and the daily rhythm. They save in the same files, so both apps agree, and the app now counts rhythm minutes and sleeps an idle PC while the cabin is closed.
- Home's PC Loadout tile in the new app shows the GPU and CPU temperatures (marked when hot) and the apps that start with the PC. Startup apps are read from the autostart folder on Linux, and on Windows from the Run keys and Startup folders, leaving out what Task Manager turned off. Nothing is changed.
- Speeds now shows your ping and jitter, from the connect time the internet log already measures once a minute.
- Under the hood for the new Personal tab: a learned line can be paused, which keeps it saved but stops chat, the profile block and the nightly review from using it. You can also add a line in your own words, export everything to one readable file, and wipe every learned line about you for good.
- Personal is now its own row on the rail. It shows what the app has learned about you, grouped (About you, What you like, Your routines, and more) with where each line came from and when. Search it, edit a line, pause one with "Don't use", forget it, or add your own. Two switches up top turn learning from chats and Know me on or off. "Export to Downloads" saves one readable copy, and "Wipe everything" clears it all after one confirm. "Account and app settings" and the raw "Notes files" are a tap away.
- Settings → Personality has a Dynamic chat switch (on by default) and, with adults-only on, a Heat dial: Cheeky, Spicy or Filthy. The 18+ check now says plainly what turning it on changes.
- When another voice steps in for part of a chat, the chat's personality pill shows it, and the switch sheet offers Keep to stay in that voice.
- The live eval checks that an ordinary adult question gets a friendly answer with no lecture, and that a filthy roast lands instead of being refused. The reference gains a Personalities section.
- Every voice now talks with you like an adult friend: most subjects are fine, mature ones included, it can match your swearing, and it skips the lectures and disclaimers. It still turns sincere for grief, health or real distress.
- Nyx flirts back playfully and suggestively, never explicitly.
- Groundwork for the 18+ heat dial: with adults-only on, every voice can tell crude jokes at Cheeky, Spicy or Filthy (jokes, never sex chat), and Spicy and up only go to providers that allow adult chat.
- A quip bank for casual chat: example jokes at each heat (clean, cheeky, spicy, filthy) that a voice riffs on with the real moment instead of pasting. Quips come at most every 4 to 6 turns (every 3 at Filthy), never during a fix, money, settings or a rough mood, back off when you don't laugh, and rotate so they don't repeat. "Roast me harder" and "ease off" are understood.
- Four new voices: Theorycrafter (builds, stats and the math behind the meta), Gremlin (chaotic feral gamer energy), Dungeon Master (scenes, lore and quests) and Esports Caster (play-by-play hype). Until their own outfits are drawn they look like Nyx herself.
- Lawyer-precise and Friendly Neighbor left the roster: a chat on Lawyer-precise now speaks as Concise Pro, one on Friendly Neighbor as Nyx, and copies you made of either keep working. Their pictures now show as Nyx reading and hanging out.
- The shell's Settings → Personality matches the cabin: turning on Adults-only personalities asks the one-time 18+ check first, then shows a Heat dial (Cheeky, Spicy or Filthy), and a Dynamic chat switch (on by default) lets another voice step in while the topic calls for it.
- Shell chats now speak with the heat you pick while 18+ is on, hand off between voices like cabin chats, and carry the safety closer once instead of twice.
- Dynamic chat groundwork: each voice now says what it's best at (support, teaching, fixing, planning, game strategy, stories, hype, winding down, hanging out), and the chat can hand the conversation to the right voice when a topic clearly takes over: two turns in a row, a short hold after each switch, straight to a gentle voice when you're having a rough time, and back to your own voice when the talk drifts home. Hand-picked voices and explicit voices never switch on their own.
- Chats can now hand off between voices as the topic drifts: a build error mid-hangout brings in Chief Engineer, venting brings in Gentle Listener, and the chat eases back to its home voice after. A hand-picked voice or "talk like ..." holds it in place.
- With 18+ on, a voice drops a quip now and then when the room is right, never on a frustrated or serious turn. "Roast me harder" and "ease off" move the heat for that chat.
- After a long chat compacts, the rules you gave earlier ("never push to main", "always run fmt first") and the safety rules are re-sent word for word, so they don't fade. The app's own nudges are tagged so they're never mistaken for your words.
- The /plan chip now puts /plan in the box (before anything you typed) instead of doing nothing you could see.
- Plans can be answered in one tap: Yes, go, Tell me more, More on step N, Change this, Go, but…, or Not now. The reply you pick most moves to the front, and typing "go", "yes" or "go ahead, but skip step 3" runs the plan instead of planning it again.
- The agent's plan approval card takes the same replies, and "tell me more" or a change goes back to it in the same turn. Question cards show their options as one-tap chips. Permission cards keep Allow once, Always allow and Deny.
- Agent: before a turn ends, the reply is checked against what the run actually did. Wrong table totals or sums, test counts that don't match the test output, "tests pass" after a red run, and "committed", "pushed" or "updated `file`" claims with no matching step get one note back so Loadout fixes the reply or finishes the work. Episode claims with a wrong sum are turned back in code without a judge call.
- The new app's proactive cards now match the classic app's: a source behind a connector grant (calendar, mail, system state) is read only when you granted it, goals left a few days get the same gentle follow-up card, categories you quieted stay quiet, a learned away hour holds all but urgent cards, and nothing goes up while a game runs.
- Pulse: a suggestion you Delete, Ignore, Snooze or mark Not interested stays away instead of coming back on the next look, and a suggestion that never reached the page no longer uses up the suggestion budget.
- Pulse: Delete or Ignore on a nudge rests that watcher for a week, as Dismiss does.
- `/goal` saves through the same lock as the app's other settings changes, keeps a goal to 200 characters and refuses one that holds a key or password.
- Nudges: a crash or update check that hangs is stopped after 10 seconds, the crash watcher reads only the newest 500 error lines, and the GPU driver watcher no longer probes the hardware when no driver is on offer.
- Workboards: a board with several running or blocked cards reads the outcomes log once per page instead of once per card.
- Run on a proactive card that found something now works in the background on the hidden Background chat and leaves its receipt on the card, instead of opening a new "Discuss" chat tab. Its steps are still recorded as proactive, not as yours.
- A proactive step that needs your yes still stops at the approval card first, and nothing runs until you approve. With approvals set to Ask, a card with a report keeps its own chat, where every step asks you.
- A lean fixed prompt and every-tenth-line `read_file` numbers are built behind `prompt_diet.toml` (both off) and wired into the A/B eval, so the paired eval can decide whether either ships.
- Settings → Local AI has a new "Tidy typos on this computer" switch, off by default. On, the on-device model fixes typos in a chat message (at most 0.3 s) before the model reads it; the fix is dropped if it changes any number, name, quote, path, link or @-mention, adds or drops a real word, or reads as a different kind of ask. Your saved chat always keeps what you typed, and nothing goes to the cloud for it.
- Loadout knows your project's test and build commands and your GPU from checks it runs itself, retires any that stop being true, and keeps the nightly dream's lessons out of chats until an eval shows they help.
- Chat: an OpenRouter model your account's privacy settings (zero data retention or a guardrail) leave no endpoint for now says so and what to do, instead of the raw HTTP 404 text.
- Local AI (Windows): the governor no longer throttles the on-device model because Loadout's own maximized window is in front; only another app's fullscreen window counts.
- Settings → AI models → Local AI: Context size, GPU offload, CPU threads and Keep the model loaded set the launch; three governor switches (games and fullscreen apps, battery, idle) write `local/governor.json`.
- Local AI: a game, a fullscreen app or a deny-list app now unloads the on-device model (GPU and RAM go to the game; it loads again on the next ask once the game is gone) instead of only throttling it, unless the model is the only one that can answer (offline or cloud down), which throttles as before. Settings → Local AI → "When a game or fullscreen app runs" picks Unload, Slow down or Ignore.
- Local AI: the on-device model's reply now reaches the chat (it was saved to the session but never streamed to the screen), and a long chat is trimmed to the device's window on the way (oldest turns first) instead of being refused for its length, with or without tools.
- Linux: closing to the tray no longer maps the window straight back. The cabin stops re-sending the unmap after 1.5 s and never treats X11's stale focus on the hidden window as a taskbar click; the tray's Show or a second launch brings it back.
- Local AI: every launch and relaunch of the on-device model is sized to the GPU memory and RAM free right now, not to the setup-time probe. With "Share with the game", a game that takes GPU memory moves model layers to the CPU and shortens the context on the next relaunch (and again when it takes much more), and the model grows back once the game ends.
- Internal: tests that set `GROKHUB_CONFIG` now put the previous value back instead of unsetting it, and an unpinned test falls back to a temp folder, so a plain `cargo test` can no longer rewrite the real config stores with defaults (it did).
- Windows: a window saved as maximized comes back maximized. The native fill of the work area now runs on the frame after winit's own maximize flag is cleared; before, eframe applied that clear at the end of the same frame and shrank the window back to its saved size.
- Chat: the standing "sign in" note no longer sits above the composer. It posts once to the bell (Notifications) with a Settings pill that opens AI models; the card after a refused send stays where the message is.
- Chat: a reply from the on-device model or a provider model now knows which model it is. Every call that reaches a provider or the device gets one line naming the model that answers (the file the running sidecar loaded) and who serves it, so "which model are you?" no longer gets an insistent "Grok".
- Local AI: the free-VRAM baseline for a relaunch is taken once the model is loaded, so the model's own memory no longer reads as a game taking the GPU (it restarted the model over and over on battery); battery or heat alone never restarts it.
- Local AI: the Settings rows write only their own switches into `local/governor.json`, keeping hand-edited allow and deny lists; a picked Context size survives the hardware probe; one message longer than the device's window keeps its start and end instead of being refused; the tuning rows show only while Local AI is on and installed.
- Windows: a window saved as maximized falls back to the work-area size, then to winit's own maximize, when the native fill can't land, instead of staying half size. The sign-in note in the bell moves back to the top, unread, when its wording changes or you are signed out again, instead of a second copy.
- Settings: "Sign in with OpenRouter" and "Get a key" open your browser again; if the browser can't be opened, Settings shows the link with a Copy button and the sign-in still finishes when you open it.
- Settings → Account: a Grok sign-in that has expired or can't be renewed now shows "Sign-in expired" with Sign in with Grok, instead of Connected, and the avatar menu offers Connect Grok in that case too.
- Pulse: every card now has the same two buttons, its main action and a second one, with anything else in its ··· menu. The type word ("Do", "Learn") is a plain tag, not something that looks clickable.
- A Pulse card's chat now starts with the whole card, so the first reply already knows what the card found. Tapping a card's fix while another reply is still writing waits its turn in the card's own chat instead of cutting that reply off. In a chat opened from a card, a short "check into it" means the card's subject, and a program that keeps crashing gets the PC repair playbook instead of the general one.
- Pulse cards carry quick chips instead of Run, Snooze and Like. A card leads with the fixes it offers (a crash card offers "Look into the crashes", a failing automation "Find out why"), then Discuss, Ignore for now and Remind me later. A fix opens the card's own chat with its report before it asks anything, so it never lands in whatever chat was open. Remind me later brings the card back tonight, half an hour before you usually wind down, or when you next sit down. Ignore for now hides the card without teaching Pulse to show less of it. Like moved to the card's ··· menu.
- A Pulse card shows its short summary, and Full report opens the whole report right on the card. The quiet-hours digest opens to each update's summary, and each one opens its own card.
- Pulse cards now say what they found. A failing automation's card names it, says how many runs failed and what the last error was; a finished run's card leads with what its reply found instead of the automation's prompt; the quiet-hours digest keeps each update's own line; a crash card names the program it is about. Discuss starts the chat from the card's whole report.
- Docs: the Pulse section of the reference now covers card reports and Show the full report, the chips and Remind me later, card chat, how a fix runs (receipts, the live checklist, the all-off switches), and the fan, driver and clock rules.
- In a chat opened from a crash card, "look into it" now reads that program's own crash history by the name on the card: its crash dumps and its recent warnings on Linux, its crashes in the Application log on Windows. It only reads when and how it crashed, never the dump itself, and changes nothing.
- Pulse: your Feed instructions now steer the summaries on posts fetched from your sources too, so "Future posts follow them" holds for every post, not only the daily digest.
- Pulse: the page no longer reads the brief's file or checks cached images on disk every frame while it paints.
- Pulse: notifications go through one gate. Only time-sensitive ones (due within the hour) pop up, never while a game is running or in quiet hours, and at most three a day; the rest wait on the page.
- Pulse: the shell now writes the digests and offers the cabin writes — start-my-day brief, weekly systems brief, weekly recap, daily drop, release radar, hobby paths, repeated-ask automate offers and one-time helper offers — on the same schedules and state files, gated by the scheduler lease so only the active app posts. Settings → Pulse gains the Sources switches with the cabin's defaults.
- Docs: the Pulse section of the reference describes the new idea groups, today's set, three-day post expiry, the plain why line, how likes and Not this teach the ranking, and the notification gate.
- Driver updates on Linux are checked twice before anything changes: the update list must name a newer GPU driver or firmware package, and the installed and repo versions, read again just before the fix, must match it. Any mismatch and nothing runs.
- A driver update runs only when there is a way back: a system snapshot, or the old package still in pacman's cache or offered by apt. Undo puts the old driver back. Never while a game runs.
- On Arch the update is a full system update from the package lists already on disk, because Arch doesn't support updating one package alone. On Windows the card points to the GPU maker's own app or Windows Update.
- Pulse: a card that stays at least 60% on screen for 8 seconds counts as a light look, once per card per day. It is skipped if the card already got a reaction that day, and nothing is sent while the tab is hidden.
- Fan fixes can now run on Linux: the fix saves every fan's setting first, sets the fans to full speed in one step you approve with your password, and checks them afterwards. Undo puts each fan back the way it was.
- Fans are never changed while a game runs. Under CoolerControl the card points you to CoolerControl. On Windows only your PC maker's own app is used, because other fan tools load drivers that anti-cheats flag.
- No fix may change clock speeds, power limits or undervolts, or install a fan driver that anti-cheats flag (WinRing0, PawnIO, InpOut). Those steps are refused outright.
- A card's read-only check, like "Look into the crashes", now runs quietly without opening a chat, and its result lands on the card as a dated receipt.
- A running fix shows a live checklist: each step marked done, running, waiting for your OK, not started, didn't work or didn't run, then the re-check. A Stop fix button ends the step in flight, and the result keeps the checklist next to Undo fix.
- Pulse: ideas are about your games and PC now. Idea groups are Games, PC, Mods & builds, Deals and Automations, with "More for you" for the rest; ideas saved under the old groups move over on their own.
- Pulse: idea runs read the games you play lately, the topics you follow and your newest open PC health findings, so ideas are grounded in your own setup. Game ideas stay read-only and allowed-data only.
- Pulse: scrolling past posts or ideas no longer silences a whole category. Untouched cards only lower its score a little; ignored proactive cards still go quiet, and that quiet now lifts on its own after two weeks. Categories an older build quieted from skips come back.
- Pulse: a Like now lifts ideas on the same topic in the ranking.
- Settings > Pulse has a new "Fixes Loadout may apply on its own" section with one switch per kind of fix, all off. Off, a card offers its fix and waits for your tap. On skips only the tap: anything that changes your computer still asks on its approval card.
- The first switch covers checking a crashing program's crash history. Switched on, the read-only check runs when the crash card shows up, once, and its result goes on the card.
- Pulse PC health cards that run hot now offer "Run the fans at full speed". The app saves each fan's setting first, checks the fans read full speed after, and Undo fix puts every fan back with its own step. It never runs during a game, under CoolerControl, or on Windows.
- The PC health sheet's footer now says what the app may change: fans only with a backup, drivers only from the official source after a double check, and never clock speeds or a running game.
- Pulse and Home rank from learning-engine trait beliefs; interests and automation on/off feed the same store.
- Pulse has a Saved page: the Saved button in the Pulse header opens it, Save on a source post keeps it there, and each saved card has only a ⋯ menu with Remove from saved. Saved cards live apart from the feed, so feed cleanup never touches them, and the old 200-item limit is gone.
- Shell Pulse: each post says where it is from and why in words ("Helldivers 2 on Steam · You play Helldivers 2"), and Read it opens the source page.
- Shell Pulse: Love pressed again takes the heart off, and the taste note forgets that love.
- Shell Pulse: today's set ends with "That's all for today", then the older posts; the page looks for new posts once a minute while it is open.
- Shell Pulse: an empty feed says what the writer found last time, with an Add interests button.
- The new Loadout window's Pulse cards now match the app's: each card says what was found, shows a short summary with "Show the full report", and has chips for its fixes, Discuss, Ignore for now and Remind me later.
- A fix chip or Discuss opens that card's own chat with the report already in hand. Nothing runs until you send it there.
- Ignore for now hides a card without teaching Pulse to show less of it. Remind me later brings it back tonight before you wind down, or when you next sit down.
- Pulse's feed opens with today's set: posts from the last day, at most two from one game or source and twelve in all, with the rest listed after them.
- Fetched and written Pulse posts leave the feed three days after they were posted, unless you loved, saved, pinned or edited them.
- Pulse: PC health cards open with a one-line verdict against your own recent sessions, and Fix it opens a sheet of options (compare runs, FPS cap and cooling steps, quiet mode, warn lines, pause, ask).
- Pulse posts news for the Steam games you play even when you haven't added them as interests, and says "You play {game}" on the post.
- "Why am I seeing this?" and the line under each Pulse post are now plain words ("Matches your interest in Rust", "From r/buildapc, a source you follow"), with no internal tags or scores.
- Shell Pulse: every check moves the topic searches on, so the same three searches no longer repeat until a post lands.
- Shell Pulse: the day's 16 calls are spent only when a model call is made, counted on your own day, and the taste note is rebuilt only when there is something new to write about.
- Shell Pulse: Not this and Dismiss on a post teach the writer "less of this", and chat lines keep ten slots so your loves stay in the taste note.
- Shell Pulse: the writer keeps the quiet hours set in Settings → Behavior instead of a fixed 22:00 to 07:00, and the first run seeds interests from your memory notes.
- Workboard: archiving a queued card takes it off the queue, so Restore no longer starts an agent run on it; a card the agent marks done leaves the queue too.
- Agent: a workspace program named like a read-only tool (`./gh`, `tools/cat`) no longer gets the read-only fast path; only a bare name or a system bin path does.
- Memory: a MEMORY.md with a non-UTF-8 byte is left untouched (remember and dream say so) instead of being cut off at that byte; reads still show every note.
- History: Delete all also removes `threads.json.bak` and the quarantined copies, so no backup keeps the deleted chats.
- Shell Pulse: the writer asks only the sources left on in Settings → Pulse → Sources.
- Local AI: resuming an engine or draft model download continues where it stopped; the Range header now reaches the CDN a release host redirects to.
- Shell: error text now meets WCAG AA contrast on every theme (a `--danger-text` token; borders and fills keep `--danger`), and a test keeps every theme at 4.5:1 or better.
- Shell: the Local AI engine and draft model panels are styled cards; download errors show in the error colour and status lines announce to screen readers.
- Shell: a draft model download started in Manual keeps its progress and Cancel when you switch to Auto, a paused download offers Resume, and the engine's Cancel button clears as soon as the install finishes.
- Shell: draft model downloads only follow HTTPS redirects inside the Hugging Face Hub and refuse private addresses, like the engine download already did.
- Shell: a download that fails before it starts no longer leaves Get engine or Install draft stuck.
- Shell: picking a catalog build with its safety training removed shows the notice once, like a third-party file.
- Shell: the chat box says "Message Loadout"; the model is still Grok.
- Shell CI: the Windows engine-install smoke no longer plants a stub binary when the download fails; a skipped download is named as such.
- Shell lint: unused variables and the rules of React hooks are errors.
- Chat asks before `sort -o`, `uniq IN OUT`, `rg --hostname-bin` and any `git branch` that creates, deletes, moves or forces a branch; these ran as read-only commands with no card.
- Web app: goals, pausing a memory row and adding a personal fact work from a paired browser (the desktop refused them).
- Code tab: a test run whose output hits the size cap in the middle of a character no longer crashes and locks the runner.
- Code tab: a remote URL with `@github.com` in its path is no longer read as GitHub.
- Stop asking me keeps settings that changed after the chat loaded, a mangled pairing link shows the pairing form, and pairing says "Too many open tabs." when that is why it failed.
- Chat asks before more shell commands that can write or run a program and used to run with no card: `git grep -O`, more `git branch` forms, `kubectl` with a config, server or identity flag, `sort /O`, `date -s`, `hostname NAME`, a wrapper run from a path (`./env ls`), a redirect such as `>&2file`, and a second command hidden behind `""#` or an empty quoted word.
- Windows: a PowerShell command with a backslash path, a curly quote or a bare carriage return can no longer hide a second statement from the approval check.
- Hooks now apply to tool calls made by helper agents, a `*` matcher runs on every tool, a folder you have not trusted cannot displace a plugin's hook, and `.grok/./hooks` or `..` paths get the agent-settings card.
- Text read from outside (web pages, tool results) stays marked across helper agents, resumed chats, summaries and `grep`, so an edit that follows it still asks.
- The approval card for a new connection shows the program and arguments it will run, not another field.
- A connector entry can use only the key saved under its own name, an imported entry cannot name a saved key, replies from a remote connector are size-capped, and a backslash can no longer redirect a connector's message address.
- Driver roll-back runs only for driver packages it verified.
- CI: a failed `changes` job now fails the required `linux` and `windows` checks instead of letting them skip, and a change to Markdown that is compiled into the app or read by a test runs the tests.
- Release: a pushed tag must match `VERSION` before anything is built or signed, and the unsigned Windows staging files can no longer be merged into a release.
- Cards sync: only an issue the sync opened or one of the project's own people opened can close a card.
- Guards: the pinned-actions check also reads quoted `uses` keys, and new guards cover the release downloads, the tag check call and embedded Markdown.
- Code tab editor: a save is refused when it would go through a dangling link (outside the project or into `.git`) or into another project after the project folder changed, a file with Windows line endings keeps them, a slow open or a late save can no longer replace or mark the wrong file, and the changed-files list no longer draws on top of the editor.
- Code tab terminal: a large paste can no longer freeze the app (the terminal says it is busy instead), a shell that ended is noticed while a job still holds the terminal, the first output of a new shell is no longer lost, a key split across two reads stays hidden, ordinary file and branch names are no longer shown as `[redacted]`, and the colours follow the theme.
- Code tab tests and deploys: output hides the project's secret values also on long lines, across stdout and stderr and for multi-line values, and a running test or deploy has a Stop button that ends everything it started.
- Code tab push: a tag with the branch's name can no longer bend the push target, and the confirm card says when the repo's pre-push hook will not run.
- Web access: a request that is only half sent is closed as idle.
- CI: the desktop shell's lockfile now gets the same supply-chain check as the rest.
- Mods: Update works for a mod whose main file is a browser download (every Nexus mod); it goes through the same download and pickup as the first install.
- Games: Disconnect or Remove key during a refresh stays disconnected (Battle.net, Guild Wars 2, Destiny 2, Old School RuneScape), and changing the Battle.net region ends a sign-in that was under way.
- Reports and diagnostics redact more key shapes (Hugging Face, GitLab, npm, PyPI, Groq, Google, Slack rotated tokens, webhook secrets, Azure account keys), and redaction no longer slows down on a long run of look-alike prefixes.
- Home: a feed tile's link goes through the same check as every other link before it opens.
- Learning: a failed import or put-back reports its write error instead of looking done.
- Windows install script builds with `--locked`.
- Desktop & web shell: wrap lazy views in a per-page error boundary with a "Try again" recovery card so a render error in one tab never blanks the app.
- Mobile & narrow windows: give the chat composer's message box a full row above its buttons on phones (`<= 600px`), and tighten desktop view-tab spacing so more tabs fit per row.
- Weekly self-review: write `self_review.json` atomically via a temporary file and release the night's claim if the weekly model call fails before posting any cards.
- Mods: crash recovery keeps a mod folder whose install record can't be read, along with the backups of the game files it replaced, and says so.
- Home: with system checks off, the Updates & Drivers tile checks again once a minute instead of starting a read on every repaint.
- Shell: an approval or question card whose answer didn't reach the agent shows its buttons again.
- Shell: web access serves only the app's own files, even in a dev build.
- Shell: the Nyx dock shows when the window widens, even if it widens right after Chat opens (a flaky test traced to this).
- Code tab: a confirmed push or pull request is refused when the command about to run is not the one on the card you confirmed, also when the new one would have needed no confirm; a remote with a second push URL on another host, or a URL that hides its host, is no longer read as GitHub; an open diff is re-read on Refresh; a branch named like a flag is refused.
- Web app: Edit automation works from a paired browser, a Journal tile links only to a chat that browser can open, Home tiles and chips no longer open pages the web app lacks, a tab that stops reading is dropped instead of queueing without limit, an empty send no longer claims a chat, and the HTTPS front limits connections per address and closes ones with no call waiting that stay silent.
- Chat: a reply whose events were lost says part of it may be missing, an answered question card keeps its own answer after a message above it is deleted, a draft is kept when the open chat is picked again, and losing window focus ends a held Ctrl+G recording once the mic is open.
- Settings: text you typed but did not save in Behavior, Code settings and Edit Home is kept when the page is shown again.
- Dialogs keep Tab inside them and give focus back to what opened them.
- Desktop session (desktop app): a step you allowed never runs after Halt or on a locked screen, and Halt with no reply running ends the session and takes its cards back.
- Desktop session: an approval card says what the step does (the control a click lands on, the command typed) and what kind it is (Send, Delete, Spend money). It shows all of the typed text, however long, with each line end marked.
- Desktop session: Allow acts at once, also when you click it while the reply is still being written. An Allow that was not run within 5 minutes is refused instead of running with a later message.
- Desktop session: Pause keeps the session, its goal and its cards, and the reply's Stop after a Pause ends the session. Turning off "Let Grok control the desktop" stops a running session. The step list no longer marks a waiting step "not run" because another step with the same tool was refused.
- AI models: a saved provider key is found after a restart without opening Settings. A provider whose saved address was edited by hand is skipped, so its key is never sent to the new address; it stays in the file and can still be removed with its key.
- Cabin Code tab: keys typed in the terminal no longer fire the cabin's shortcuts, a running test or deploy has a Stop button, and a test or deploy confirm closes when the project changes.
- History (desktop app): Delete all deletes exactly the chats it named, also with a search typed, and a held Enter no longer confirms it. A chat you delete closes if it was open. When part of a Delete all fails, the page says so and the chats that did go are closed.
- History: opening a chat to read it never rewrites its file, and linked files and folders in the session store are skipped.
- Debug bundle (desktop app and cabin): text is redacted first and cut second, so a key the cut lands inside stays hidden; only the end of a long log is read; a real `Authorization` header loses its value and other lines keep theirs.
- Command page: a line holding a key or a password is never remembered or listed, and one an older build stored is taken out of the file.
- `/forget <topic>` also removes matching lines from the project's `MEMORY.md`, and says how many.
- Web app: Enter in the slash menu picks a row only when the list matches what you typed.
- Devices: a paired computer is remembered after a restart, a pairing code that ran out is replaced, and Run it here sends the task as the cabin does. A running recording keeps its clock and Stop on screen in a narrow window.
- LAN hub: `/v1/health` no longer names the computer and the hub sends no cross-site headers, so a web page cannot read it.
- The audio check says it works on Linux only instead of failing on other systems, and `/record` says it takes stills for 2 minutes at most.
- Stop hands queued messages back: anything you queued behind a stopped reply lands in the message box instead of sitting out of sight and sending itself after your next reply.
- A message typed in one chat while another chat's reply runs now waits for its own chat. It never sends into whichever chat is open when the other reply ends, and it goes once you open its chat again. Activity shows which chat each waiting message is for.
- Stop hands back only the open chat's waiting messages; another chat's keep waiting. Steer now only joins a message to its own chat's reply, and a retry after a failed step only ever sends to the chat it came from.
- Send and Stop belong to each chat: only the chat whose reply is running shows Stop, its glow and thinking rim. New chat never reopens the chat whose reply is still starting.
- In the desktop shell, Clear chat or deleting the open chat while a reply runs opens a chat that shows Send. The old chat's waiting messages go back in the box instead of sending into the new one, and what you type in the new chat goes there.
- Depend on quick-xml 0.42 and migrate XML readers to the UTF-8 `&str` event API.
- Workboards, Automations and Skills each get their own row on the desktop rail, so none of them hides behind a button on another page.
- When xAI says to slow down, Loadout waits at most 30 seconds and says so in the thinking line. Calls to each model are paced under xAI's per-second limit, so parallel tools and subagents don't pile into rate limits.
- Dependencies: the desktop shell's web-access certificate maker moves to rcgen 0.14. Certificates and keys come out the same (self-signed, PEM). reqwest 0.13 and ureq 3 were already in place.
- Docs: reorganize README.md with installation, build, usage, configuration, architecture, documentation, and contributing sections.
- Trimming and compaction now go by xAI's own count of the last prompt plus an estimate of what was added since, instead of counting sealed reasoning by the size of its encrypted blob. Long tool-heavy chats no longer trim many steps early, so the prompt cache breaks less often. The blob sizes are logged once per session next to the real count.
- The desktop app can record your screen with `/record` (a still every 2 seconds into your Recordings folder, with a red clock and Stop in the top bar, `/record stop` and Halt) and check your audio with `/audiocheck`, as the cabin does. Recording asks first and nothing is sent anywhere.
- Code tab: helper work that conflicts can be redone. The lead starts a new worktree helper with `redo` set to the work's key; the helper gets a fresh copy of the project as it is now and the old work's changes as a diff (cut at 40,000 characters, with a line that says so). The old work is not changed and stays on the Tasks panel until you discard it. Once the new helper has finished, the old row says "A newer helper redid this", and the new row says which work it redid.
- CI: release binary size ceiling raised to 74.6 MB (measured 64.8 MB plus 15%), so beta pre-releases publish again.
- The same tool call with the same arguments now runs at most twice in a row before Loadout asks the model to change course, and the refusal shows in the chat. An edit to a file resets the count, so edit-then-check loops keep going.
- Test-only compaction hooks are compiled out of release builds (the tool-output eval now reads a scratch config file), and Grok requests with tools say parallel_tool_calls outright.
- Every model request is now kept sealed on this computer by content hash for 14 days, so a turn can be replayed byte for byte (`examples/replay_request`). A chat's shared prefix is stored once, and `request_replay.toml` (`enabled`, `keep_days`) turns it off or changes the window.
- Each tool call a rule, the Auto judge, a hook or the sandbox stopped or asked about now leaves one sealed safety event (who decided, what the person answered, and the request that produced the call), with no arguments or refusal text.
- Desktop app: scheduled automations and resumed runs follow your Approvals setting and the job's tool set (`TOOLS:`, `NEED_SHELL`, `NEED_MCP`), as the old cabin did. Ask stays read-only, and desktop tools come only while desktop control is on and no game holds it.
- Desktop app: Resume on a paused or interrupted Workboard card picks it up from its checkpoint. A chat card sends its short resume note once in its chat; an automation's or background run's card goes on in the background, in the same session when it is still intact and never in a looser permission mode. The agent's own interrupted runs resume by themselves at most twice a day, then wait with a note in the bell. A resumed background run shows in `/bg` and `/bg stop` stops it; a resumed chat turn never runs in a looser mode than the run started in.
- The egui cabin is retired: the Tauri desktop app and the hub are what ships. Releases no longer carry the cabin's Linux tarball, Windows Setup or Windows zip, and `install.sh --app cabin` now says the cabin was retired.
- Models: Grok 3, Grok 4 and Grok 4.1 Fast leave the model list (xAI retired them on May 15, 2026), and a saved pin on any retired chat id now sends Grok 4.3, which is what xAI already serves for them. Imagine drops the retired "Quality" image model; a request for it goes out as Imagine 2.0 at low quality, xAI's own replacement.
- When the model connection keeps failing, Loadout now gives up after 6 tries, about a minute of waiting, instead of 15 tries over about six silent minutes. A dropped stream is no longer resent, and charged again, up to 14 times.
- Pulse's Steam library is read on its own thread, so a large `localconfig.vdf` no longer stalls the first frame or the Settings → Pulse → Sources page; the check on open waits for that read.
- Settings Mute/Unmute re-reads `pulse-steam.json` before saving, so a mute the agent's `pulse_steam` tool saved moments earlier is kept.
- The Steam config parser no longer drops every game after one with a value-less key, so play stats reach the Library and Pulse rankings again.
- The PC loadout card retries its hardware read when the probe thread fails to start instead of showing "Reading this PC…" forever.
- Shell Activity Dock: a turn that fails right after a finished one starts from zero steps, and finished entries older than ten minutes leave the activity map.
- The agent's `pulse_steam` tool says Steam is not installed instead of "not in the Steam library" when there is no Steam.
- Code tab (desktop shell): a lead session's Tasks panel has a Review section. Review asks the lead to start a read-only review helper, which is handed the approved plan and the session's whole change as a diff and reports findings; the panel lists them with blockers first, also after a restart. A review is recorded only when the project is the same at its end as at its start, and it never commits or stages anything. The lead gets the findings with the helper's reply, a second findings report adds to the first, every plan approved in the session is handed over, and a review waits until helper work is merged or discarded.
- Loadout now follows Rocket League matches while the game watcher is on, through Psyonix's official Stats API on this computer: your goals, assists, saves, shots and demos, the score, and a recap in Pulse when the match ends. The API is off in the game until you set `PacketSendRate=30` in its `DefaultStatsAPI.ini`; Settings shows the line with a copy button, and Loadout never edits the file. Loadout only reads from the local port: nothing writes to the game folder, reads memory or touches the game, which runs Easy Anti-Cheat.
- Auto routing now learns from your own chats: when an answer worked (you moved on or copied its code) or didn't (you asked again, or its tools kept failing), the router leans toward the model and effort that served you best for that kind of ask, at the lowest cost that holds up.
- The router only tries something new on easy, everyday turns, and only a stronger or equal option; hard, high-stakes, agentic and safety-routed turns keep the usual route.
- Router tuning runs silently: the weekly router suggestion cards are gone, and a change is kept only when your own logged turns show it does no worse. Old router cards are cleared from Home.
- What the router learns stays on this device and holds only ids, counts and probabilities, never your text.
- Claude (through your own API key) now answers in a stream instead of one block at the end: its words and thinking reach the chat as they are written, and if the stream drops before anything showed, the turn fails over like any other provider error — after text showed, the turn stops with what is on screen, the same rule Grok's stream follows. Claude calls now also ask Anthropic to cache the shared prefix (the instructions, the tools and the conversation so far), so later turns pay the cheaper cached rate for what earlier turns already sent; the spend ledger counts cache reads and writes at their own prices. And Claude's stop reasons are read properly now: a reply cut at the output limit shows as a truncation the chat can carry on from, and a refusal is reported as a refusal — its own kind of stop — instead of an empty answer.
- Asking whether your Claude (or OpenRouter) key works, or which models can answer, now runs a live check and answers from what the provider said: key saved, allowed, accepted, and a one-word test reply, plus whether Grok is signed in and, in an 18+ chat, why nothing in it goes online.
- When a provider you added hits a limit, runs out of credit (a 402 now counts as out of credit, not a mystery failure) or falls over, the turn no longer dies there: that model rests for a minute and Grok takes the turn instead; if Grok was already down and its stand-in provider fails the same way, the on-device model answers.
- Auto routing: saying no to a tool, a safety refusal, or a repeated call no longer counts as a failed step, so two noes don't push the next steps to max thinking.
- Background work sent through xAI's batch queue no longer counts toward how fast a model looks, so it can't make Grok look slow or skew the tuning.
- When a model stops partway through a step, the hand-off is now a proper packet instead of a bare retry. The model that takes over is told, in one short note over the work itself, which model stopped and why, which models already failed this step, which models answered the steps before it, and who wrote any partial reply that was kept — so nothing already said or done gets repeated. Every switch inside a call (a provider that hit its limit handing the turn to Grok, Grok handing to a provider or to this computer's model when it drops) is written on the route record with its reason, and the chat shows one plain line that a switch happened and the work so far is kept, without naming models. A step that failed before producing anything is still resent exactly as it was, so a clean retry stays clean.
- Auto routing now also remembers how similar chat turns went on your PC. When the small on-device word model is downloaded, each typed turn is compared with past turns, and when they clearly agree the router follows them; when they disagree it thinks harder rather than less.
- The memory stays on your PC and holds no words from your chats, only number fingerprints and how each turn went, capped at 2,000 turns. Turning off memory embeddings turns it off too.
- When a chat falls back to Local AI, it now thinks as hard as the step needs on models that can switch thinking on and off: a quick step turns thinking off, a harder one turns it on and picks the model's nearest thinking level.
- Imagine picks its own model and quality from what you ask for: posters, photoreal scenes, 4k and words in the picture get full quality; sketches, icons, memes and drafts get the faster one. Asking for a video, an animation or a timelapse makes a clip, with its length and sharpness taken from your words.
- The price limit and spend estimates now count the higher rate OpenRouter lists for long prompts.
- Brain 1: when the Brief rules can't tell what an ask is, a small in-process scorer (hashed words, word pairs and concept cues against compiled-in example asks) now sorts it with no model call. Each Brief carries a `confidence` (0 to 100). The fast-model fallback for asks neither can sort is now opt-in through `briefModelFallback` in `app.json` (off by default), so shipped builds no longer pay a model call for an unclear ask.
- Every model Auto can reach now carries a model card: how it is reached (xAI, OpenRouter free or paid, Claude direct, or on-device), what it is good at, what it is for, how it treats your prompts, and when it is expected to leave. Known models get list prices from a table built into the app where their listing names none — Claude models most of all, so Auto can weigh Claude alongside Grok and OpenRouter instead of never picking it — and Claude cache writes now count at their real rate in the spend ledger instead of the plain prompt rate. Live results replace these starting figures as they come in.
- Auto routing knows current Grok prices and context sizes (Grok 4.7, 4.5 and 4.3) even when only a Grok Build sign-in lists the models, and counts every token at twice the rate once a prompt reaches 200k tokens. A live price list still wins.
- Long prompts (200k tokens and up) go to a model with a 1M context, such as Grok 4.3, unless the step needs the strongest model and it still fits there.
- Grok 4.20 Multi-Agent runs 4 agents on everyday turns. 16 agents need a hard step and your OK for that route.
- A model that hits its requests-per-second limit counts as busy, so Auto picks the next model instead of waiting on a busy answer.
- Grok 4.7 Fast is only used with a Grok Build sign-in, never with an API key.
- A chat no longer bluffs about the online models: a reply that says Claude or another key is connected without a check is sent back to check first, it no longer tells you to update, restart, sign in again or send a harder task to test a model, and asking about a task id that was never started says so instead of looking like a running task.
- Nothing on screen shows which model or effort Auto picked: the Auto chip, mode chips, "How Auto picks", Refresh models, why and guard cards, router change notes and Imagine's model pickers are gone. Imagine uses its default model.
- Ask in chat and you get a straight answer: a reply from a provider or the on-device model now knows the model and the reasoning effort it was sent with.
- Plan, outage and wait notices, the premium card and provider notes say what happened without naming a model ("Your plan changed", "Waiting for xAI", "Use a premium model", "Added 2 models from openrouter.ai.").
- Provider and Local AI setup, OpenRouter's data policy, spend caps and the price limit stay where they were.
- The cabin's routing commands are gone: `/model`, `/m`, `/mode`, `/models`, `/effort` and `/why` (with `/why models`, `/why table` and `/why local`) now answer "Unknown command" like any retired command, and nothing lists them in `/help` or the command picker. Auto alone picks the model and effort; ask in chat to hear which answered.
- What `/why local` showed lives in Settings → AI models → Local AI: whether the on-device model can take work, the governor's level and reason, the tier, heavy steps waiting for idle time, and where the governor log is.
- The prompt-cache share stays on Home's AI usage tile, and the diagnostics export is unchanged.
- Auto alone picks the model and effort for every step. The Default model, "Use for your chats", Models Auto may use, Local AI for chats, Steady effort and the Grok 4.7 Fast toggle are gone, and saved picks are dropped when Settings load.
- A provider model you set up (key and permission) can join Auto's pool when it lists a price under your price limit and answers in text. xAI still answers first.
- OpenRouter keeps only its data policy; the endpoint sort and the fallback router are gone.
- Auto picks OpenRouter models itself from OpenRouter's model list, read at most once a day: only models that take tools on agent steps and reasoning on thinking steps, fit the chat, answer in text, sit under your price ceiling and aren't being retired. OpenRouter's own auto router is never used.
- Every OpenRouter call now names a backup model from the same picks, keeps one chat on the same endpoint, sends the thinking level the step needs, and goes only to endpoints that support everything the call asks for. By default no OpenRouter endpoint that may keep or train on your prompts gets them.
- When xAI is down and another provider you added stands in, one that has been busy or much slower lately waits behind a healthy one.
- OpenRouter answers now arrive in a stream instead of one block at the end: words and thinking reach the chat as they are written, tool calls and the cost still land on the turn, and if the stream drops or hits a limit before anything showed, the turn fails over like any other provider error. After text showed, the turn stops with what is on screen, the same rule Grok's and Claude's streams follow. A limit reported mid-stream also tells the app to re-check the OpenRouter account at once.
- When xAI is down, overloaded or unreachable, an ordinary chat turn is answered by a provider you set up (Grok on it first), and failing that by this computer's model, instead of ending in an error. 18+ chats still go only to this computer's model.
- A new model from OpenRouter or Claude now runs a two-week tryout on small turns first, with a few free test calls a day and a small share of low-risk turns. It graduates on its record, or steps out for two weeks if it keeps failing or does worse than the models it would replace.
- OpenRouter models keep their own reasoning between tool steps: their reasoning details go back with the next request, only to the model that wrote them.
- The local model's thinking now comes back apart from its answer, so it never shows up as reply text.
- Auto now checks what each account can still spend, on its own: OpenRouter's free-versus-paid state and free calls left today, whether your xAI key is blocked, and Claude spend tracked locally from real replies (with its rate limits and any spend-limit or out-of-credit pause) — all kept in `models/quota.json`, refreshed every ten minutes and right after a limit, credit or key error, so an empty route is spotted before a chat runs into it.
- A model source that fails to answer a refresh (an xAI outage, a sign-in renewal that blinked) no longer wipes its models or your saved sign-in from the router: each source's last good list stands until that source checks in again.
- The auto router now weighs every model for the task at hand — quality for what the task needs, speed, how reliably it has answered lately, expected cost and risk — from its model card, its last 7 days and the account's quota state, and orders the route ladder by task size: free and cheap routes first for small work, the plan's models first for medium work, and one scored paid tier for long runs, which never spend a free model's allowance. Hard limits come before any weighing: a route the quota ledger has closed (a spent OpenRouter account, a blocked xAI key, a benched Claude), a model whose card says it cannot do what the task needs, a route that trains on prompts for sensitive work, a probation model on real work, and a free model about to expire are never picked; an explicit (18+) turn is kept on this computer's model from the pick itself, and while a game holds the GPU the on-device model takes tiny work only. When every route is filtered out the router still picks a working one rather than stopping, and the route record names each rule that fired.
- Auto keeps a long chat on the same model between turns, moves it to a cheaper one only when the saving over the rest of the chat beats re-sending the history, and still moves up whenever a turn needs a stronger model.
- Auto keeps private per-chat routing counts (prompt cache hits, model switches, escalations and how often they helped) for its own tuning; no chat text is stored.
- Auto routing treats risky asks with more care: deleting or force-pushing, dropping a table, a production deploy or migration, and security work such as passwords, keys or encryption go to the strongest model and think hard from the start.
- Inside a long agent run, effort follows the work: a step that only reads tool output thinks lightly, while a tool failing twice, a repeated call, tests or a build failing after an edit, or a long search with no edits make it think harder. A run of working edits and passing tests lets it ease off again.
- When a reply spends its whole thinking budget and comes back empty, Auto counts it as a failure and thinks harder on the next step. "Try again" and "regenerate" do the same for the next answer.
- On repeated trouble Auto raises effort first and keeps the same model, so the prompt cache stays warm. Only two failures in a row at the top effort move the work to a stronger model.
- Auto now sizes a task before it runs: a long task — many steps, a long answer, a big context or a long-running chat — goes to a paid route from the start (your plan first) instead of spending itself on a free model's small daily allowance, and a chat that grows into a long task partway through is upgraded on the spot, so its remaining steps and any failover treat it as one. Easy work takes the opposite shortcut: when a model is already loaded on your device, quick tasks default to it instead of paying for the cloud. Short chats otherwise keep their fast free route, and when a free model is the only route there is it still answers: a working route beats a dead stop.
- Desktop app: a chat that no model in your plan answers now waits and sends its ask again on its own once one does, with one quiet note naming the provider and the step. A Pulse card names the wait and offers Retry now; only models in your plan are checked.
- Desktop app: the router's budget cards are back over the chat box (80% of the week, the ask at 100% with Keep going, and the upgrade nudge), and Settings → Account shows the SuperGrok pool's room or that the plan refused the sign-in.
- The desktop app reads what each message is for, so the Chat / Plan / btw chips are gone. A big multi-step job plans first and anything else is answered. While a reply runs, a side question gets its own read-only answer in an "On the side" card above the box, a correction steers the reply, "stop" stops it, and a new job waits its turn. `/plan` or `/chat` sets just the next message.
- Side answers never join the chat or History. **Tell the main task** feeds one into the running reply, or sends it next when nothing runs.
- The Ask / Auto / Always chips left the composer. **When the AI wants to change things** in Settings → Defaults is the one place to set it, and **Stop asking me** on a permission card allows that step and moves the setting to only ask for risky things.
- The window app now reads each message itself: there are no Chat, Plan or btw buttons. A big multi-step job gets a plan first, everything else is answered straight away, and `/plan` still asks for a plan on the next message.
- While a reply runs, a side question gets its own answer in an "On the side" card without touching the work, with Tell the main task to feed it in. Corrections steer the work, "stop" stops it, and a new job waits its turn.
- Ask / Auto / Always moved out of the chat box and the Privacy page into one setting: Settings → Defaults → "When the AI wants to change things". Typing `/auto` or `/always-approve` opens it. A saved session mode is dropped on the next save.
- The permission prompt offers Allow once, Always allow (this kind of step here), Deny, and Stop asking me, which loosens the setting one step.
- Undo and Keep rows for changes Loadout made on its own now show only in the chat that made them.
- A run whose replies keep getting cut off at the output limit now stops after three in a row and says so, instead of looping. A run with no step limit now pauses once it has sent about 1M input tokens, and a "continue" picks it up. Normal runs are unchanged: the efficiency bench shows the same requests, tokens and recall before and after.
- Internal: updated the SQLite library to rusqlite 0.41, which bundles SQLite 3.54.1 (was 3.46.0). Memory recall, the AMR index and the browser-history reader work the same as before.
- Depend on rusqlite 0.42 (bundled SQLite 3.54.1) in the workspace and refresh both Cargo lockfiles.
- In Auto mode, ordinary shell commands that the sandbox fully holds (Linux with Landlock and its own network, or macOS) now run without the judge call or a card. Hard classes, `outside_sandbox`, privilege raising, dangerous commands and your Ask rules still ask; Supervised still asks every time; `auto_allow = false` in `shell_sandbox.toml` turns it off.
- Sandbox: a sandboxed command that writes or rewrites `.git/commondir` (which sends git to the config and hooks of another folder) has it put back when the command ends, like `.git/config` and `.git/hooks`, so a push address or a hook planted that way no longer outlives the command.
- Sandbox (Linux and macOS): tokens in the app's environment reach sandboxed commands only as stand-ins; the egress proxy puts the real token back in request headers only for that token's own hosts (GitHub, GitLab, npm, crates.io, Hugging Face, xAI), over HTTPS it ends with a session CA limited to those hosts. `--doctor` counts masked tokens.
- A repository's own permission file can't pre-approve its commands anymore: its allow rules count only after you trust the folder (Settings shows Trust folder), and the agent writing its own settings or hooks always asks.
- Sandboxed commands can't plant git hooks or git config that would run later outside the sandbox: the sandbox puts them back and the agent has to ask. Loadout's own git calls run with hooks off, and only package download caches stay writable, not tool folders like `~/.cargo/bin`.
- On macOS, sandboxed shell commands now run under a Seatbelt profile: the network reaches only the egress proxy, writes stay in the workspace, temp folders and caches, and key, credential, keychain and browser folders can't be read.
- Sandbox: the egress proxy lets commands reach only package registries, code hosts, model hosts and hosts named by your `WebFetch` allow rules, checked by name before any DNS lookup; addresses and `localhost` need a rule naming them, and names that resolve to local addresses aren't dialed.
- Sandbox (Linux): a model-written command gets its own network namespace whose only way out is the egress proxy, a seccomp filter against new Unix sockets, vsock and io_uring, and empty stand-ins for secret folders (SSH and GPG keys, cloud and registry credentials, browser profiles, the sign-in folder, the config folder). `--doctor` reports how commands are confined.
- Sandbox: on Windows 11 (24H2 build 26100.9278 and later) shell commands run in their own process container through Microsoft Execution Containers, with no admin rights: the network only through the egress proxy, writes only in the workspace, temp folders and caches, and secret folders unreadable. Auto mode now runs ordinary commands there without asking, as on Linux and macOS. Older Windows keeps the restricted token; `GROKHUB_SANDBOX_NO_MXC=1` turns the container off.
- The screenshot Explain offer now uses the same pills as the composer chips and no longer reads its file on every frame.
- Security housekeeping: the yanked async-executor 1.14.0 is replaced with 1.14.1, the known glib 0.18 advisory is documented in deny.toml, and Dependabot now sends one grouped monthly update into beta for Rust and the UI.
- Settings → Behavior: Quiet self-review, the self-improvement history with Revert, runs by area, and saved ideas with Copy as GitHub issue, read from the same files as the cabin.
- Settings has one AI models page for everything model-related: Sign in with Grok, OpenRouter and API keys, the default model, Local AI and Offline only, and spend. Account, Defaults, Privacy & data and Behavior no longer repeat those rows, and messages that pointed to them now point to AI models.
- Settings: Behavior has a Reminder notifications switch, and About has a field for the support link, so neither needs editing app.json by hand.
- Settings: three settings nothing read anymore are gone; older app.json files with them still load.
- Settings: the Labs cursor-motion demo only shows with GROKHUB_DEV_DEMOS=1.
- Settings: a row of category buttons across the top (Account, Look & feel, AI & chat, Your PC, Privacy & safety, Help & updates) and a page menu on the right that follows the chosen category. One page shows at a time instead of one endless scroll.
- Settings: a big Back button names the page you came from, and the left rail (Chat, Pulse and the rest) works while Settings is open.
- Settings: the old Behavior page is split into PC helpers, App & extras, and Automations & limits; every setting kept as it was.
- The shell app calls the agent Loadout too: chat box, mode tips, plan card, desktop-control switch and the palette's Ask row no longer say Grok.
- Shell: an Agents page lists the open chat's to-dos and background tasks (commands, subagents, monitors) with a Stop for each and Stop all, like the cabin's Queue; the web app gets it for chats the browser started.
- Shell: outside links (Pulse sources, connector sign-in pages) open in your browser; the web app opens them in a new tab on that device.
- Shell: Pulse and Activity menus close on Esc or a click outside; Pulse posts can be snoozed until 9 AM, dismissed, or marked Not this.
- Shell: errors on Pulse, Connectors, Journal, Settings, Memory and Labs read in the error colour; chat offers Open Settings only for key or sign-in trouble.
- Shell: Pulse Delete and Automations Remove ask first.
- Shell chat: chats are no longer lost on New chat, reload or quit. A new History page lists your past chats under Today, This week and Older, with search over titles and words in the chat, and Rename and Delete (Delete asks first). Opening one shows its transcript, with tool steps on one quiet line, and your next message carries the conversation on. The chat you had open comes back when the app starts.
- Shell: the Code tab lists the app's built-in skills, read only. A built-in shadowed by a project or user skill of the same name is marked as overridden, and comes back when that skill is removed.
- Shell: a Code tab shows the workspace's repo, its changed files and each file's diff (read only), and can hand a review to the agent. First slice of the coding toolkit.
- Shell: the Code tab can run a project's tests. The first run shows the exact command and asks before running; after that, the same command is remembered until you change it or forget it in Settings.
- Desktop app: a Command line page (Ctrl+K → Command line) runs one shell line in the project folder by handing it to the agent in Chat, the way the cabin's Command page queues `/sh`. The first run of a line asks and shows it, a yes remembers it for that project, changed text asks again, and Forget clears them. Destructive lines ask every time; forbidden lines, paths outside the project, force pushes and pushes to main or a protected branch are refused with the Code tab's push guard message.
- The shell's Connectors page adds connectors from the catalog or a pasted config, signs in to HTTP servers in the browser, saves a connector's API key after an approval card, and switches each connector off and on.
- Cross-platform app: opening Loadout again brings up the running window instead of a second copy, and the window comes back where you left it and at the same size.
- Cross-platform app: Settings → General → On this computer can start Loadout when you sign in (in the tray when closing keeps it there), bring it up from anywhere with Ctrl+Shift+Space (⌘+Shift+Space on a Mac; not on Wayland), and notify you while it's in the background when Grok needs an OK or a reply or Run now finishes. A notification offers Deny and Open only; allowing a step always happens on its card.
- Shell: a Devices page shows this computer, whether Web access is on and the browsers paired right now (kept in memory only), with one button that signs every browser out by changing the pairing code.
- Hold to talk in the desktop shell no longer clips words: the mic stays open for two minutes after a hold, so the next one starts at once and keeps the half second before the press, letting go keeps a short tail so the last word isn't cut off, sliding a finger off the button no longer ends the hold, and the mic asks for echo cancellation, noise suppression and level control. Recording runs off the page's main thread where the webview supports it.
- The cross-platform app builds again after the CUDA engine change: its engine download takes the asset URL from the downloader.
- Shell: Settings → Account offers Sign in with Grok (device code) on desktop and iOS, so a Grok subscription works without an xAI API key. The sign-in is kept in the system keychain and used before a saved key; the key form stays below as the second option.
- Shell: a Halt button stops desktop control and every running reply at once. It shows in the top bar while desktop control is on or something runs, and Ctrl+Alt+H, the tray's Halt and Super+Shift+Esc (when the system allows it) do the same. Cards that were waiting read "Not run: halted", and desktop steps stay off until the app restarts, as in the cabin.
- History in the desktop app now has a Projects tree, a Pinned group and a Delete all that asks twice, lists read-only Grok CLI and cabin chats, and searches tool steps.
- The desktop app has a Home tab, first in the bar (it still opens on Chat), with the cabin's tiles from the same home_layout.json: recent chats, automations, Pulse, Objectives, the streak, the voice's Nyx and Up Next fill in, and the rest say Open in the cabin. Objectives, opened from Home or the palette, shows the open chat's checklist, level, XP and weekly streak, with Accept as done. The web app reads both.
- Shell: an Imagine page makes images, or a short video when chosen, from a prompt with aspect and count chips (square, one image by default), through chat's guarded media tool. Results are saved under the config folder's imagine/ and pinned to the cabin's Imagine wall, which is the page's gallery, with a lightbox and Download. With no key, or a key Imagine refuses, it points to the cabin's Imagine sign-in.
- Shell: saving Local AI settings keeps the cabin's "When a game or fullscreen app runs" choice in `local/governor.json` instead of failing to build.
- The new desktop app has a Library page: the Steam games on this computer as a cover grid with All, Installed and Backlog tabs, search, and Play. It reads only Steam's own library files and the public store art (off under Offline only), and Play only hands the game's id to Steam.
- Shell: the top bar shows the brand mark and name, and the browser tab shows the app icon.
- Shell: view tabs carry icons and fold to icons alone on narrow windows, so they stay on one row.
- Shell: the last theme paints at startup instead of flashing the default first.
- Shell: card titles stand out, and automation results read as text instead of code.
- Shell chat: replies now render Markdown the way the cabin does: headings, bullet, numbered and task lists, quotes, rules, tables that scroll sideways, and code blocks with a language label, keyword colours and a Copy button. The page uses a TypeScript port of the cabin's parser, kept in step by a shared fixture test. Raw HTML and `<script>` in a reply show as plain text. Only http(s) links are links; in the desktop app they open in your default browser, in the web app in a new tab. Your own messages stay plain text.
- In the desktop app you can keep typing while a reply runs: Enter (or Queue) lines the message up instead of waiting. Queued messages show only in the Activity menu, in order ("Next up", "2nd in line"), with Steer now, which hands the words to the running reply at its next step, and Remove, which puts them back in an empty chat box. The next one sends when the reply ends; Stop or Halt puts what waited back in the box.
- The desktop app has a Notifications bell next to Activity. Finished or failed automations (with Ask about it / Find the cause), a new update, the local engine finishing or failing, a Workboard tidy (with Undo), and replies that finish or need your OK while the window is in the background all land there, with an unread number. Automation results no longer print into the chat, and the tidy banner on the Workboard is gone.
- The Activity button reads "1 of 2 running" when work is lined up, and says its counts to screen readers.
- Shell: the command palette (Ctrl+K) now lists New chat (Ctrl+N), Sign in while there is no API key, Shortcuts (Ctrl+/), and the composer's slash commands for a query that starts with `/`, with each row's key at its right; Settings shows Ctrl+,. Ctrl+N, Ctrl+, and Ctrl+/ work from any page, Ctrl+/ opens a keyboard shortcut sheet, and the cabin's older words (feed, schedule, keys) find their rows. Desktop-only commands stay off the web app.
- The palette test checks that commands follow the last page, so a new page in the shell no longer breaks it.
- Desktop app (shell): the Memory tab is now Personal. Facts are grouped (About you, What you like, Your routines, What you need, How you think, Waiting for your OK) and searchable, each with where it came from and when. You can add a fact, pause one so chat stops using it, export everything to one file in Downloads, or wipe it all after a confirm. The notes files fold under "Notes files".
- Desktop app (shell): the Skills tab no longer lists the built-in skills, and each skill has a switch. A switched-off skill stays listed so you can turn it back on, but chat won't use it.
- The desktop app now notices what you might need while the GrokHub cabin is closed: it offers blocked cards, unread reports and failing automations as a few well-timed Pulse suggestions, shares the card budget and mutes with the cabin so nothing doubles, and still asks before any action.
- The desktop app runs the local nudge watchers you turned on in the cabin (game patches, GPU driver, low disk, crash spikes, OS updates, controllers and printers), once each across both apps.
- Workboard cards that are running or blocked now carry a mission view: a checklist from their steps, a status and what to do next.
- Shell Pulse: PC health cards now show a one-line read of their numbers and a Fix it / Options window with things to try. Every card has the same two buttons: the main one, filled, and a second one, outlined.
- Shell: streamed reply text no longer re-renders the whole app on every chunk (100 chunks: 100 app renders before, at most 2 now); the Activity Dock still updates on tool steps, asks and the end of a turn.
- Shell on phones: scrolling re-renders the app only when the bottom bar actually slides, not on every touch-scroll event (a 60-step swipe: 59 renders before, at most 2 now).
- Shell: loading settings, which reads the keychain, runs off the main thread, so the window isn't held up while it first paints.
- Shell: every chat step goes through the router, which picks xAI, a provider you set up, or Local AI, and the model and effort; with no sign-in or key, a ready Local AI answers.
- Shell: no model or effort shows or can be picked anywhere: the model picker, the Model and Effort chips, the Defaults model row and `/model` are gone. Ask in chat to hear which model and effort answered.
- Shell: Settings keep setting up your xAI key or sign-in and Local AI; a model saved by an older version is dropped.
- Agent shells drop BASH_ENV and ENV, interpreter start-up variables like NODE_OPTIONS ask first, MCP server lists count as agent settings, a tool hook that can't run asks instead of letting the call through, an MCP server can't grow memory with an endless line, and after reading outside text an edit to a build script, package script or editor task asks first.
- Model-written shell commands now run in a sandbox on Linux: writes land only in the workspace, temp folders and tool caches (Landlock), and network goes through the egress guard's local proxy. Commands that raise privileges or were approved on a hard card run on the whole machine; `outside_sandbox` asks first. Turn it off or widen it in `shell_sandbox.toml`.
- On Windows, model-written shell commands now run with a write-restricted token: writes land only in the workspace, temp folders and tool caches (each gets one inheritable allow entry for Loadout's capability SID), and network goes through the egress guard's proxy, the same as on Linux.
- Shell: scheduled automations fire from the shell, window open or in the tray, while the cabin is closed. The cabin keeps priority through a heartbeat lease, a job is claimed before it runs so it never fires twice, and after a sleep each job runs once. Each job can be edited in place.
- Shell Settings: new Defaults, Behavior, Privacy & data and Background sections. Offline only uses the same offline.toml as the cabin, Privacy shows what left this computer this week, Behavior saves sentence autocomplete, quiet hours and a daily token budget (all off by default), and the web app shows all four read-only.
- Desktop shell Settings: the same top row of category buttons (Account, Look & feel, AI & chat, Your PC, Privacy & safety, Help & updates) with a page menu on the right that follows the chosen category. On a phone the page menu becomes a row above the page.
- Desktop shell Settings: a big Back button names the page you came from (Back to Chat, Back to Pulse). Every setting works as before.
- Shell: signed self-updates for the AppImage and the Windows installer, with a Stable or Beta channel in Settings > About; releases now ship a minisign-signed latest.json (LU-06).
- Shell: a Skills page lists the cabin, project, user, plugin and built-in skills chat can use, with search; Use puts a skill's trigger in the chat box, Open folder shows its folder, and the web app lists them read only.
- The desktop app runs the chat, project and session slash commands the cabin has, read through the same command list: `/rename`, `/pin`, `/delete`, `/sessions`, `/retry`, `/undo`, `/stop`, `/compact`, `/context`, `/usage`, `/rewind` (`files`, `step`), `/project` (`new`, `show`, `bind`, `clear`, `rename`, `move`, `folder`, `delete`) and `/export html` or `json`. `/delete`, `/project delete` and `/rewind files` ask first with the exact target and change nothing until you say yes. A pinned chat lists first in History. The web app gets only `/context`, `/usage` and `/sessions`.
- Shell: typing `/` in the composer lists commands (Up, Down, Tab, Enter, Escape) and runs /new, /clear, /chat, /plan, /btw, /remember, /help, /settings, /memory, /automations and /connectors without sending them to the model; the cabin's other commands show greyed with where they live.
- Desktop app: /recall, /forget, /memory prune and dream, /dream, /learned, /skill, /skills changes, undo and restore, /connections changes, /automations changes, /privacy, /brief and /health now run in the composer, and the forgetting ones show what goes before they ask.
- The shell's slash command tests build again after the new summon hotkey and notification settings.
- The shell smoke test now waits for a new chat card to finish fading in before reading it, so it stops failing at random on an empty card.
- CI: the shell smoke test waits for each page to actually show and clicks a personality's Use button only once it stops moving, so the Personality step no longer fails at random.
- At the shell's default 900px width the tab icons wrap to a second row instead of pushing the whole window sideways, so no scrollbar sits along the bottom over Send and every tab takes a click on Linux.
- Shell: a small dotted orb shows while a reply hasn't started (thinking, looking something up, or working with a tool) and in the mic while it listens or turns speech into text. Plain 2D canvas, still under reduced motion, paused when hidden.
- Shell: the built app now carries `licenses.md` with the notices of every bundled package.
- A shell command that outlives its timeout now keeps running as a background task wherever background tasks are on, and the agent gets its output so far plus the task id, instead of the command being killed. A halted session, or a run without background tasks, still kills it as before.
- Two desktop tests now pass on Windows: the Steam library fixture escapes its folder paths the way Steam writes them, and the skills test builds its expected folder with the system's separator.
- Shell: a Workboard page on the cabin's own board file, with drag-and-drop or a keyboard Move menu, notes, archive and a Delete that asks first. Work puts the card in the chat box without sending it and moves the card to Doing. Each card also shows its steps, where it left off and the Agent or You badge, with a log read only when you open the card, Resume and Hand back for a paused run, and Move up and down. Every change goes through the same board lock and merge as the cabin and the agent's work tools, so none of them overwrites another's change.
- Agents: an agent's own `git push`, `git send-pack`, `git subtree push` or `git lfs push`, and its `gh pr create`, `gh pr merge`, `gh repo create --push` or a `gh api` call that is not plainly a GET, now always ask first on a "Push, PR, GitHub API call or ship button" card (hard class `ship`), also in Auto and Always and for a Code tab chat in Auto-beta. The command is read by shell words, so quotes (`g''it`, `-C 'my app'`), git's own options before the verb, an alias set with `-c alias.<name>=`, git's `git-push` program by path, `stdbuf`, `flock`, `eval`, `if`/`then`, `!` and a script given to `bash -c` or `sh -lc` don't hide it. A desktop-control click on the app's own Commit, Push, Open PR, Merge, Waive or Allow, or on a "Create pull request" button anywhere, asks the same way. An unattended or read-only session refuses it. A plain `git commit` by the agent is unchanged.
- Code tab (desktop shell): under Review, a lead session's Tasks panel lists the checks a change must pass before it can be committed: tests run on these exact files (with Run), a secret scan of the added lines by path and line, the branch, and one sentence for each thing in the way. A review reads Stale once files or commits change after it. Reading the checks changes nothing in the project. The checks refuse a repository whose own config allows a git transport or turns on a sparse checkout, a stored file that does not match its id, and an origin address that changed since the session began.
- Code tab (desktop shell): a lead session's Tasks panel can now commit the reviewed change. The Commit section lists every changed file first, the commits made during the session, an editable message, the branch and the author, and Commit asks with the exact text of what it will do (`commit <tree> to <branch> on <head>: "<subject>"`, with the earlier commits, allowed lines, waived findings and "no tests run" when they apply). Every check runs again on the click; the commit holds exactly the reviewed tree on the reviewed HEAD, moves the branch only if it has not moved, runs no hook and no signing program, and leaves a clean status. Waive on a review blocker (one confirm) lets the change ship with it open, and Allow on a key-shaped line in the uncommitted change lets that line through; a file flagged by its name, a held project secret and a line inside an earlier commit are never allowed. An Allow holds only while the files are the ones it was given for, and a Waive only for the review it was pressed on; a line allowed in the panel's own commit stays allowed there. Commit opens again as soon as a refused message is fixed, and a branch that takes no commit offers New branch in the checks. Commits and waivers show in the Code tab's activity log, a waiver once, and the Code tab reads its files list again after a commit, push or pull request from the panel.
- Code tab (desktop shell): after the commit, a lead session's Tasks panel can push it and open its pull request. Push sends the commit made in the panel by its id (`git push --porcelain --no-recurse-submodules --no-signed origin <commit>:refs/heads/<branch>`), with no tags, no signed push, no hook, and no replace refs or grafts file; Open PR runs `gh pr create` with hooks, fsmonitor and tag pushes off for gh's own git calls. Both always ask with the exact command, whatever the push setting says, and refuse unless HEAD is still that commit on its branch, it adds only the commits on record, no file changed since, its review still stands with every blocker waived, no helper work or turn is running, and the repository's own git config names no program to run. The repository's own config may not name another push address (`pushurl`, `pushInsteadOf`). A review of the commit itself also lets it push. A pull request waits for the push, and is refused when origin's branch is not that commit (that read is refused under Offline only); the panel's card offers no New branch, which would leave the commit behind.
- Sign in with Grok in the desktop app takes a code that ran out off the screen as soon as you come back to Settings, and after Cancel and a new code, a late answer about the old code no longer shows under the new one or makes the page ask xAI again without waiting.
- Sign in with Grok in the desktop app stops and says why when xAI said yes but the sign-in could not be kept (a keychain that is not available), instead of waiting on a code that was already used.
- The Skills rail row has its own lightning-bolt icon, so it no longer looks like Library's.
- The Skills tab lists only your skills, not the built-in ones. Each skill has an on/off switch, plus Edit and Remove (with Undo), and New skill makes one from a name and what it should do. Chat never uses a skill that's switched off.
- Chat box: `/queue` typed just as the reply ends is sent instead of being lost.
- Chat box: a command picked from the slash menu while a reply runs says it waits for the reply instead of vanishing.
- Chat box: Enter pressed again on a command that is still running starts no second run, and its answer no longer clears what you typed meanwhile.
- `/goal` pins a goal when the settings file is empty.
- Pulse: a click on a suggestion that fails no longer counts against the suggestion budget, and a suggestion that could not be saved gives its place back.
- The desktop app runs /send, /bg, /queue, /loop, /board, /goal, /fix, /diagnose, /consult, /inspect, /hooks, /view-plan, /auto, /hub, /host, /update and /palette, and answers /workflow and /worktree the way the cabin does.
- Chat box: `/queue`, `/bg`, `/bg stop`, `/send` and `/consult` typed while a reply is running now run beside it; any other command typed then stays in the box and says it waits for the reply.
- `/hub` is greyed as "On your computer" in a paired browser, and says so where the app has no Devices page instead of doing nothing.
- Home's Playbooks rows run their playbook: a tap starts it on a new Workboard card and puts the ask in the chat box, so nothing runs until you send it and the usual approvals still apply.
- Home's Clips tile with no clips yet opens Steam's Settings, where Game Recording turns on. On a PC without Steam, Clips, Squad and the game tiles offer Steam's download page instead of a page or chat ask that couldn't help.
- Dependencies: sha2 0.11, getrandom 0.4, base64 0.23 and symphonia 0.6 (mp3 decode for the companion's lip-sync). Hashes and tokens are unchanged. The lip-sync decode now drops the mp3 encoder's delay and padding frames, so the mouth envelope lines up with the audio more closely. quick-xml stays on 0.41 until wayland-scanner moves.
- After a chat's first message, small talk and bare go-aheads ("ok", "lol", "how are you", "do it") no longer pull memory into the turn, which could add about 1k tokens of unrelated notes each time. The first message's recall is still there.
- CI: the shell smoke test clicks a top tab only once it sits still and clicks again until it is pressed, so a tab click right after a turn ends no longer fails as "not interactable" on Linux.
- Shell: retry WebDriver type into an element that was still moving, the same way clicks already settle.
- CI: when the shell smoke's web access page never loads, the log says which requests were open, whether the server answers, and where the shell's threads wait.
- Download models can fetch a model made of several files, such as a speech model's encoder, decoder and token list, checking every file against its pinned hash (a small file outside LFS is checked by its git oid). Voice and image models download into folders of their own, so they never replace the chat model you run or show up in the chat model picker.
- Loadout now reads your Stardew Valley farms while the game watcher is on: when the game closes it compares the farm you saved with how it stood at launch and posts the days played, gold earned and skill-ups to Pulse, and Settings shows the farm's day, gold and skills. It reads only the small save summary the game writes (Proton installs included), needs no mod and never writes a save.
- Stop now takes effect right away while the server is compacting a long chat, instead of after up to 90 seconds.
- Stop now hangs up the connection right away, even while the model is still thinking with nothing shown yet, so the server stops generating and billing, and a local model gives the GPU back at once.
- Stop no longer erases the reply it cuts off. Whatever already came in (the text so far, its thinking and any tool calls) stays in the chat with a "Stopped" line at the end, on the tab the run belongs to. Stopping before anything arrived still leaves no empty bubble.
- Stop in the composer now ends only the chat's own run: background runs and scheduled loops keep going, and tray Halt still stops everything. On Windows, a command that leaves a background process holding its output no longer freezes the run; Loadout waits about two seconds, ends the leftover job, and answers with what arrived.
- A terminal command that leaves a background process holding its output (a daemon, `setsid`, a service helper) no longer freezes the run. The command now answers with what it printed within about 2 seconds of finishing, and Stop ends the wait right away.
- Settings → Privacy & data → Storage: one Open per folder, where local models sit, Reset to default, and Bring them over from the folders older versions used, which names each source and destination and asks first.
- Stores: a JSON store another program is holding for a moment (a Windows replace, a virus scanner) is read again for up to half a second, and is never moved aside as broken. Before, a read that raced a save could quarantine a live store, which showed up as lost Pulse cards and a flaky Windows test.
- The auto-review judge, the close-check and VerifyGate now ask Grok for strict JSON (structured outputs), so their replies always parse; other providers and the local model keep the text reader.
- A helper's result now lists the files and pages it read, a fresh helper is told what the chat already read so it doesn't read it again, and the same task can't be handed to a second helper while the first still runs.
- Subagents now trim, compact and stop before Grok's 200k long-context price step the same way your chat does, and one stops after sending about 400k tokens (200k a level further down), handing back what it found.
- Helpers are steadier: one that crashes gets one fresh try, a helper keeps its chat's background or quick route, a SubagentStop hook can send a helper back once, its result says plainly how it ended, and a worktree it didn't change is removed (old ones are tidied after three days).
- A helper's question shows in the app window even after the reply ended, stays until answered, and two helpers asking at once each get their own answer.
- A general subagent now starts as a fork of the chat, with the same history and cache key, so its first request is a cache hit instead of a cold start. Explore subagents still start fresh.
- The app now decides when Grok may start a helper: a small task gets none, a short one at most one helper that only reads, a chat runs at most four at once (eight across the app), a background helper that changes files runs alone, and a turn's helpers share one token allowance. Each helper also stops at a step and time limit and hands back what it found.
- A chat that starts several helpers can wait for all of them in one read: `get_command_or_subagent_output` takes ids joined by commas (or a `task_ids` list), waits until they have all ended or the time is up, and returns each result under its id.
- A helper result the chat already read no longer comes back a second time as a finish notice.
- Notices that arrive between two steps (helpers, background commands, monitors) now go in as one message instead of one each.
- Helpers stay off the free OpenRouter day once fewer than 80 free calls are left (a quarter of what's left must cover one helper's whole step budget), so a fan-out can't spend the calls the chat itself needs. When the free route is the only one, helpers still use it.
- A sudo password dialog closes when the command that asked is stopped or times out, and a password is remembered only once it reached a `sudo` that was still running.
- Without a session runtime folder (`XDG_RUNTIME_DIR`), the sudo helper is kept in the config folder, not in a temp folder that sandboxed commands may write.
- Sign in with Grok refuses a page or endpoint address whose real host is not xAI's (`https://not-xai.example?.x.ai/device` passed the check before); the profile photo check reads the host the same way.
- The sign-in test override (`GROKHUB_SHELL_OAUTH_URL`) is taken only when the host the request would reach is this computer; an address with `@`, `\`, `?` or `#` is ignored.
- `grokhub-hub --oauth` asks again when a poll gets no answer (a dropped connection) while the code is still good, and says why once.
- Chats that open as small talk send a lighter tool list: the file and shell tools wait in the lazy index and all come back at once on the first work ask.
- Settings → AI models in the new app: add your own AI provider with a key (saved only after you approve a card), allow where chats go, see the router's table, set the weekly spend cap and price limit, and revoke paid-upgrade grants.
- Shell: automations gated by a check command run the check, then fire from the tray scheduler the way the cabin does.
- Desktop app: chat now records each turn's task outcome and run metrics the way the cabin does. "That was wrong" marks the last task failed, and undoing a skill change fails that skill's runs from the day before, so the weekly self-review and skill trials count desktop chats too.
- Linux: installing from a clone (`scripts/install.sh`) now builds the Tauri desktop app and the hub by default. A cabin install that started at login now starts the new app at login instead; `--app cabin` still builds the old cabin for now.
- The desktop app's Settings → Labs now has Diagnostics mode like the cabin: one click builds a redacted debug bundle, shows it with a redaction summary you can copy, and saves it to your Diagnostics folder. Chat text goes in only for a chat you pick, and nothing is uploaded.
- Dependencies: refreshed the desktop shell's Rust lockfile to the newest semver-compatible releases (zip 9.2, brotli 9.1, wasm-bindgen 0.2.130 and 17 others). No manifest changes.
- Tauri: sentence autocomplete (ghost text) and focus-help chip/context for the Behavior and PC helper switches.
- Desktop app: weekly self-review cards in Pulse now have their own button. Apply change edits the skill or adds the chat note and starts a one-in-five trial, Revert puts the skill back, and Save idea keeps a code idea. A code idea can also be copied as a GitHub issue from the card's ⋯ menu.
- Desktop app: /imagine, /imagine-video, /journal, /learn, /mcps, /plugins and /resume run in the composer. Imagine and the journal open those pages, /learn reflects the chat into memory, /mcps opens Connectors, /plugins opens Settings → Labs, and /resume opens History.
- Voice mode in the app: one tap and you talk hands-free; it hears when you finish, sends what you said, and speaks the reply sentence by sentence as it streams (xAI voice with a key, the on-device voice without). Talk over the reply or tap to stop it.
- The desktop app now runs the Sunday self-review when the cabin is closed: the same pass, the same five-card cap and quiet mode, and never on a night the cabin already reviewed.
- Wind-down in the desktop app reads GPU busy the same way the cabin does, so an idle PC can sleep when nothing is rendering.
- Tests: the Grok Build deny-rule tests compile each rule's pattern once instead of once per rule per command (the three `gb_` tests: 48 s → 6 s locally; they were the slowest in the suite).
- Tests: the UI suite runs in vitest's `vmThreads` pool, so each worker loads vitest, jsdom and the test libraries once instead of once per file; every file still gets a fresh jsdom window (`npm test`: about 40 s → 17 s locally).
- CI: the serial `channel_tip_tests` start first in nextest, so they no longer run one at a time at the end of the Windows job.
- Docs: the compass says CI runs nextest, and that core has one HTTP client (`http.rs`).
- Tool calls hold up better: garbled tool arguments get two retries before the run stops with a note, an MCP tool whose schema the provider refuses is sent with no parameters instead of failing every turn, and a dropped call to a read-only or idempotent MCP tool is sent once more.
- Reading a big file now returns about 12k tokens at a time and says which line to continue from, instead of sending up to 5000 lines at once. A single huge line, like minified code, is cut and labelled. Background task output keeps its head and tail like shell output does. On the efficiency bench this cuts a tool-heavy chat by 32% and a diagnostics run by 46%, with recall unchanged.
- Supply chain: the accepted-risk notes in `deny.toml` for the remaining transitive advisories (paste via egui_dock, the zbus 5 helpers, glib 0.18 and proc-macro-error via Tauri's Linux stack) record which upstream releases still block each one and why the glib entry shows as not detected. No dependency or behaviour change.
- Internal: updated the Windows tray icon library to 0.26, which also drops the unused Linux GTK 0.18 packages from the lockfile. The tray icon and its menu work the same as before.
- Internal: updated the WebSocket library to 0.30, which also drops the old rand 0.8 and utf-8 packages from the lockfile. Closing and focusing browser tabs works the same as before.
- Agent: a run learns how many steps its kind of task usually takes from past successful runs and gets one note to step back and finish when it reaches that many, plus one note to wrap up at 80% of its token budget, before anything stops it.
- Agent: each workspace's up-front tool list follows what its sessions actually use: tools unused there for ten sessions wait in the deferred index, and deferred tools used in most sessions load up front. The list stays fixed for a session, so the prompt cache holds.
- Router: a turn reader works out what each typed message is for (answer it, plan a big job first, answer a side question beside a running reply, fold a correction into the work, stop, or queue a new job), with a labeled test set and a small on-device store that learns from corrections by message shape, never words.
- Text size: one type scale across the app (12 caption, 14 body, 16 prose, 20 section, 24 headline, 32 display), each on a 4px line grid. Chat replies and the composer read at 16px with 1.5x line height, markdown headings stop shouting, page titles step down on narrow windows, and a guard test keeps text on the scale.
- Text size: the new app uses the same type scale as the cabin (`--font-*` and `--lh-*` tokens in rem, so Text size and the OS text size scale it). Body reads at 14/20, chat and the composer at 16/24, weights stay at 400/500/600, big titles get tighter tracking, and page titles step down on narrow windows.
- Text and controls are easier to see in every theme: error text uses a lighter red in the dark themes, text fields and switches get a clearer border, buttons show a solid focus ring when you tab to them, and a new test keeps the theme colors above WCAG contrast. Also fixes the Memory screen, where the note field shrank to nothing next to the new "Global" switch.
- UI dev tooling: eslint's glob matcher (minimatch) is pinned to 11.0.3 through an npm override, which clears the moderate minimatch advisory (GHSA-g3hv-75wg-m249) from `npm audit`. Dev dependency only; nothing in the shipped app changes.
- Screens ease in when you switch tabs, new chat messages and cards rise in, the command palette fades and pops open, and the on/off switch has a springy thumb. Everything shares one easing curve, and people who prefer reduced motion get instant changes.
- The app loads less up front and stays quick in long chats: Settings, Pulse, Memory, Journal and Connectors load the first time they open (and quietly when the app is idle), cutting the first download by about 20%, and chat messages only repaint when they change, so typing in a long conversation costs less.
- Shell: Pulse, Journal, Memory, Connectors and Settings stay open in the background once you've opened them, so a half-typed edit is still there when you come back while their lists refresh. A reply streaming in no longer redraws the cards above it (the chat is built with React Compiler), and tool, approval and question cards ease in over 150ms where the webview supports view transitions (none with reduced motion).
- Settings keeps its section list in place while each page scrolls on its own, every section opens at the top, and a Close button (or Esc) takes you back to the screen you came from. The Approvals dropdown now matches the other fields, and Journal sits in the same centered column as the other screens.
- The app looks and feels more polished: shared radius, shadow and focus tokens, real hover, press and focus states on every button and field, softer depth on chat bubbles, cards and the command palette, a blurred palette backdrop, readable centered columns on Memory, Automations and Connectors, themed thin scrollbars, and Inter bundled so the type is the same on every OS. Disabled buttons now go neutral instead of a faded accent.
- A USER.md, SOUL.md or MEMORY.md that can't be read as text (saved in another encoding, not a file, or no access) is left alone by Save instead of being replaced by the draft; Review and the Saved screen name the file and say why.
- When one of the three notes files is a link, Save stops before it writes any of them, and Review says so before Save is pressed.
- Get to know you handles one request at a time, so a second press of Save or quick Next clicks no longer fail with "No such file or directory" or overwrite the one-step backup; Save is greyed while it is saving.
- A session runtime folder (`XDG_RUNTIME_DIR`) is used for the sudo helper only when it is yours alone and outside the folders sandboxed commands may write (`/tmp/runtime-…` is not used); otherwise, or when the helper can't be made there, it is kept in the config folder.
- `grokhub-hub --oauth` asks again after a passing server error (502, 503, 504) from the sign-in server instead of giving up, and stops once the code's own lifetime has passed. A server error while refreshing the sign-in no longer says to sign in again.
- Sign in with Grok refuses a page address or a code that holds a line break or another control character, so a terminal can't be shown a second address under xAI's.
- Cabin: egui family stays on 0.37 (eframe/egui_extras, egui_plot 0.38, egui_stack 0.25.9, egui-pop-phosphor); overlay toasts stay out — status lines use the bell only.
- Home's Updates & Drivers tile opens a real page in the new app: what waits to update on this PC (the app, Steam games, system packages on Linux, winget apps and Windows Update on Windows) with one offer per group. Update system runs your distro's own upgrade behind its password prompt and tries a snapshot first; nothing heavy starts while a game runs.
- The Updates & Drivers page has a Drivers group. On Linux, Update drivers checks each GPU driver update twice (the installed version and the repo's version must both match the list), takes a Snapper or Timeshift snapshot first or keeps the old version, updates to the exact version, then checks it landed. Put old driver back undoes it. With no way back, nothing updates. On Windows it lists driver updates and opens Optional updates.
- Voice: the mic is filtered before it drops to 16 kHz, so sounds above 8 kHz no longer fold back into speech, and voice mode keeps every sample at block edges.
- Voice mode: one spoken piece that fails no longer silences spoken replies until the chat is reopened; the next reply tries again.
- On-device voice: pieces spoken side by side each write their own new file, and a reply line that starts with "--" is spoken instead of read as an engine setting.
- Voice mode lets you cut in: tap the mic while a reply is being spoken and it stops and listens, or just start talking over it. The mic keeps listening while the reply plays and only sustained speech above the reply's own sound counts, so its echo, a cough or an "mm-hm" doesn't stop it, and the reply's sound isn't sent to be transcribed. A long reply no longer counts as a quiet minute.
- Docs: a compass map for voice mode (open mic, streaming speech-to-text, spoken replies, barge-in) and an updated Hey Grok paragraph in the reference.
- Voice mode keeps one mic open for the whole conversation and listens until you stop talking (up to a minute), instead of recording a fixed 4 seconds. The first word is no longer lost, a pause mid-sentence doesn't cut you off, and a short question goes out about a second after you finish. Silence no longer turns voice off: after 9 seconds it says it's still listening, and it pauses itself after a quiet minute. Tapping the mic mid-sentence sends what you said. A Thinking state shows between your turn and the reply. Each turn goes to speech-to-text as 16 kHz mono (about a quarter of the old upload). On Windows the mic queues 800 ms of audio so a busy moment doesn't drop words. The local whisper fallback now reads its transcript back.
- Voice mode speaks a reply while it is still being written: the first sentence plays as soon as it is ready, and each next sentence is made while the one before it plays, on one kept-open connection. Long replies play to the end instead of being cut off after 8 seconds. Spoken replies skip thinking and command lines, voice turns ask for a short answer that sounds right out loud, and a speech failure now shows in the status line instead of going quiet. Voice turn timings (words, first reply text, first sound; numbers only) are kept in `voice-timings.txt` in the settings folder, with p50 and p95 at the top.
- Voice mode hears while you talk: the mic streams to xAI speech-to-text as you speak, the words you've said show live in the status line, and the turn ends as soon as you've clearly finished a thought instead of always waiting out the silence. A pause mid-sentence keeps listening. When the mic's own pause detector ends the turn, the words are already there, so there is no upload wait. The upload stays as the fallback when the stream can't be used (behind a proxy, or a sign-in the stream refuses).
- Voice: a turn detector for hands-free voice. It keeps half a second from before the first word, waits out mid-sentence pauses (900 ms of quiet ends a turn, 1.4 s after a short turn or a trailing "um"), drops clicks, coughs and steady hums, lets a minute-long turn through, checks in after 9 s of quiet and pauses after a minute. Over a spoken reply, only 400 ms of speech louder than the reply's own echo counts as talking over it. A speech chunker cuts a streaming reply into pieces to speak (the first at a clause break after six words, then sentences, no markdown read aloud, code skipped), and per-turn timings print one line with p50/p95. Not wired to the mic yet.
- Local AI gives the engine bigger GPU shader and kernel caches (unless you set your own), so compiled GPU work survives between loads, and on a machine short of RAM a cold model going wholly to the GPU loads straight from disk instead of filling the file cache.
- After a game ends and things settle (about 45 seconds), Local AI brings your picked model back in the background, so the next chat isn't stuck on the small gaming model. While you game, the small model keeps its draft model on the CPU when a speed test of that setup (run while no game runs) showed it helps; otherwise it runs alone.
- Local AI learns when you usually start chatting on this computer (same time most days, or same time on a weekday) and reads your models into memory about 15 minutes before, so the first reply comes quicker. It never loads to the GPU for this and skips it while a game is running.
- Local AI keeps the chat's cache when a model unloads quietly and puts it back the next time that model loads, so the first reply skips re-reading the system prompt and tools (a cache the reply couldn't use is dropped, and that model stops trying for the session). With nothing to restore, a GPU load answers one tiny warm-up first.
- Local AI's health log now says how much of the model was already cached in RAM when it loaded, and how long the first reply after a load took to read its prompt.
- Local AI reads the model it will need next into RAM in the background (after a game, an idle unload or at app start, and the model a game swaps to), so the next load skips the disk. It runs at the lowest disk priority, slows down while you game, and stops when memory or the disk gets busy.
- Local AI now checks how much of a model is already in memory on Windows and macOS too, so the background warm-up skips models that are still cached there and looks again every 30 minutes, as it already did on Linux.
- Web access pages no longer hang while loading: the server could leave one file unanswered behind open connections, and now nudges its worker pool so every connection gets read.
- Agent: a `web_search` or `web_fetch` the model types into the shell (`web_search "q"`, `web_search("q")`, `bash -c "web_search q"`) now runs as that tool, through the same permission checks, with one line saying so; other tool names anywhere in a shell line (after `&&`, `;`, `|`, or inside `bash -c`) are turned back without running, and each repair is counted per model in the session metrics.
- Agent: when a model fails partway and another takes over, the hand-off note now carries the person's latest ask and the calls already tried, so the next model carries on instead of asking what it was doing.
- Chats with Grok now keep one WebSocket open per chat and send only what's new each turn, for faster tool-heavy runs. Anything the socket can't carry falls back to the normal connection; turn it off with `enabled = false` in websocket.toml.
- WebSocket mode opens within 3 seconds or hands the turn to HTTP, Stop works while it opens, a socket the server closed between turns reopens cleanly, idle sockets close after 10 minutes, and a request the server already took is never quietly sent again over HTTP.
- The Sunday self-review's last-run guard, its Suggestion cards and its `self_review.json` writers now live in shared core, so the desktop app can run the same pass without both apps running it on one profile.
- Wind-down: no more "Idle … Sleep the PC?" card while nobody is at the PC. While you are there it offers once to sleep the PC after a chosen idle time, or to open the system sleep settings; Settings › Behavior picks the idle time.
- Fix Windows shell release builds by giving the auto-setup API and React components distinct module names on case-insensitive filesystems.
- Dependencies: the Windows bindings move to windows 0.62 and windows-sys 0.61 (core, agent, desktop and app). No behavior change. zbus 6 and x11rb 0.14 wait on upstream crates that still pin zbus 5 and x11rb 0.13.
- Internal: the loopback MCP test servers (streamable HTTP and SSE) and the Imagine local-send test server put each accepted socket back in blocking mode. Windows gives an accepted socket the listener's non-blocking mode, so a request that arrived a moment after the connection was dropped unread and the client saw os error 10053; `http_streamable_and_sse_on_loopback` failed now and then on Windows CI because of it. No app behavior change.
- On Windows, shell commands now start in a plain folder path instead of a `\\?\` one, so PowerShell commands that use relative paths (like `Set-Content notes.txt`) work again.
- On Windows, desktop control now reads the button, link or field under a click through UI Automation (read-only, 300 ms cap, paused while an anti-cheat runs), so risky clicks get the same named-target checks as on Linux.
- Windows: PowerShell commands now read and write UTF-8, so accents and emoji in output come through intact, and commands too long for one command line run from a temporary script file instead of failing. On Linux and macOS, a command over 128 KB is turned back with a way round instead of an opaque spawn error.
- Tests: arguments to shell commands, file writes, edits and hook stdin are checked to arrive byte for byte, backslashes, quotes, `$`, backticks, Unicode and 9 KB payloads included.
- Workboard: agent work now logs itself on the board. A task turn, a background run, a scheduled automation and a desktop session each open or update a card, add one progress line per tool call, mirror the plan into the card's steps, and save a checkpoint (next step, files touched, why it stopped) when the run ends or you press Stop. Chats file no card. The agent can also read and update its own cards with `work_get`, `work_list`, `work_update`, `work_add` and `work_archive`. These load only when the agent asks for them by name, so they add nothing to a normal turn, and none of them can delete a card or change one of yours.
- Workboard: an Archived view in the cabin and the shell, newest first, with search, Restore and a Delete that asks first. Bulk deletes take all archived cards, or those archived more than 30 or 90 days ago, after a confirm that shows the count, and remove each card's log too; cards on the board are never touched. The local tidy now runs at start and every six hours, in the cabin or, while the cabin is closed, in the shell, under the same board lock so it never runs twice. Each tidy that archives cards raises one notice with Undo. Settings shows "Auto-archive after 30 days" and an "Auto-delete archived" choice (Off by default, or after 30 or 90 days) that both apps share. No model reads or writes any of this.
- Workboard: "Agent: do this" on a card, in the cabin and the shell. Now starts Work on it; Queue lines the card up so the cabin starts it on its own, one queued run at a time, only once no chat turn or queued run is live, there is room for a background run and no protected game is up. Either way the card becomes yours. Start a flow links a playbook or a saved recipe to the card and starts it (recipes run in the cabin, never during a protected game), and Make it recurring files a morning, weekday-morning or evening automation that owns the card. Triage adds a High, Normal or Low priority, Block with a reason that is saved where the work left off, Unblock, and drag a card onto another to put it above; the order is the same in both apps. Editing a card's steps makes them your plan: the agent can mark your steps as started or done but never rewrites, reorders or reopens them. Nothing new goes into the agent's prompt.
- Workboard (app): each card has its own chat inside the card. Say sends a message to that card's agent (Work on it for a card with no chat yet), Stop ends its turn, and an ask for your OK shows there with Allow and Deny.
- Workboard (app): queued cards start on their own when nothing else runs, one at a time, and the board says why the next one waits. While the cabin is open it still starts them.
- A card's chat is an ordinary chat in History. A Stop or a failed turn moves the card to Needs Attention; a finished turn moves it to Done.
- Workboard: interrupted work picks up from its checkpoint. When the app restarts, the engine stops, the model returns an error or the route swaps models mid-run, the card in Doing moves to Blocked as Interrupted, with the cause, its next step and a log line. Resuming hands the model one short note (at most 1,200 bytes) built from the card's checkpoint and steps, once, in that turn only; the same model keeps its session, a different model starts fresh with just the note. Agent-owned automations and background runs resume on their own, at most twice a day per card, then wait for you with a bell notice. Work you started waits for Resume and is never resumed for you. A resumed run keeps the permission mode it started in, and the game guard is checked before any desktop step.
- Workboard: a card's progress ring now follows its steps, and its line under the title says where it left off, with an Agent or You badge and a kind chip. An open card lists its steps and its checkpoint (goal, next, blocker, refs), and a Log drawer shows the last 20 progress lines, read from disk only when you open it. A paused or interrupted run shows Resume and Hand back. An Activity row for a run that works a card can open that card, and the bell tells you when a run is interrupted, when Resume fails, and when the hourly tidy archives cards (with Undo). The Home Workboard tile counts Doing, Interrupted and Needs you. None of this adds anything to what the model reads.
- Workboard: cards can now carry steps, a short capped checkpoint of where the work left off, an owner (agent or you), and a short append-only progress log. Archiving records when, and Delete also removes the card's log. Once an hour a local sweep archives a finished agent card you never touched (after a day) and any card idle for 30 days with no live run; it never deletes. Saves now merge card by card, so the cabin and other writers never overwrite each other. Older `workboard.json` files load as before. No model reads or writes any of this.
- The World of Warcraft addon now loads on Retail and every Classic flavor (Mists of Pandaria Classic, TBC Classic Anniversary, Classic Era and their test clients). On Classic it reads professions, talent trees, reputations, currencies and quests. A new **Install addon in all versions** button puts it into every WoW version you have in one click.
- Library: the World of Warcraft card can install Loadout's own read-only WoW addon into each installed flavor (Retail, Classic and the rest) with one click, keep it updated, or remove it. The addon ships in the repo under `addons/wow/Loadout` for manual installs too.
- Library: the WoW card shows the characters and recent boss kills, level-ups and achievements the addon saved, read from WoW's own SavedVariables file as data (no Lua is run). WoW saves it on `/reload` or logout.
- WoW: finished Mythic+ keys post to Pulse with the dungeon, level, time, bosses down, wipes and your deaths, read from the combat log WoW writes after `/combatlog`. A boss kill says which pull it took, and when WoW closes one card sums the bosses pulled without a kill.
- WoW: when WoW runs a minute without writing its combat log, Loadout says once to type `/combatlog`, and the WoW page shows whether the log is on. The Loadout addon (1.3.0) also reminds you in your own chat window when you enter a dungeon, raid or key with logging off; it only asks the game whether logging is on and never turns it on. `/loadout logreminder` turns that off.
- WoW addon: the repo copy of the Loadout addon now lists the current Classic (5.5.4), Classic Era (1.15.9) and Anniversary (2.5.6) clients in `## Interface`, so manual installs on those flavors aren't flagged out of date.
- Side calls (auto-review judge, close check, Brief, memory and dream) now go over HTTP instead of the chat's WebSocket, so they no longer break the chat's response chain and their deadlines hold.
- Depend on x11rb 0.14 in grokhub-desktop (shell X11 backend); keep 0.13 for tauri-plugin-global-shortcut.
- Long chats on Grok now compact on xAI's side, keeping more context for fewer tokens; if that fails or takes over 90 seconds, Loadout falls back to the old summary. Put `backend = "summary"` in `compaction.toml` in the config folder to always use the summary.
- XP no longer moves backwards: earned XP is kept in its own ledger, so old logs rotating away or switching chats can't lower it, and checked steps count once each.
- Levels come faster at first (level 2 at 200 XP) and settle at 500 per level, with a title beside the level (Rookie, Regular, Grinder, Veteran, Elite, Legend). No one drops a level.
- The weekly streak is now weeks in a row: a week counts if you earn any XP in it, two freezes cover missed weeks, and a weekly target of active days tunes itself from your own weeks and pays a bonus that grows the longer the run.
- A big day is never capped: past 150 XP a day, card and day XP counts half.

- Linux: `grokhub-linux-v3.6.0.tar.gz` and AUR `pkgver=3.6.0`.
- Windows: `GrokHub-Setup-3.6.0.exe` and `grokhub-windows-v3.6.0.zip`.
- Shell (Tauri): `grokhub-shell-v3.6.0-linux-amd64.deb`, `grokhub-shell-v3.6.0-linux-amd64.AppImage`, `GrokHub-Shell-Setup-3.6.0.exe` and `grokhub-shell-v3.6.0-macos-arm64.dmg`.

## 3.5.0 — 2026-10-31

- Pass the local llama-server API key through its child environment instead of exposing it in process command lines.
- Settings: clarify private-data recovery, point API-key setup to Defaults, and make Appearance previews easier to see.
- Local AI: every on-device model in the catalog is now an abliterated (refusal-removed) build, so an 18+ chat that falls back to this computer's model isn't refused. Gemma 4 E2B, E4B, 12B, 26B-A4B and 31B come from huihui-ai's Gemma 4 QAT abliterated GGUFs (Q4_K), each pinned to its SHA-256 like before. The banned-ask check (minors, non-consent, real people) still runs in front of them like any other model. Muse Glimmer 30B and its DFlash drafter are no longer listed. A stock Gemma 4 file you downloaded earlier no longer matches the catalog: it keeps running, by its file name, until you pick another model, you can delete it from the models folder as before, and the picker recommends the abliterated build. New local model suggestions on Pulse now link only huihui-ai's abliterated builds.
- Pulse can nudge you when a Steam game is close to 100% achievements, or well through with a rare one next, with links to Steam's free community guides. Opt-in under Settings → Pulse → Sources; it reads your public Steam stats once a week and posts nothing for a private profile.
- Pulse: an achievements check that takes a while to fail waits its full retry time from when it failed, and one that fails after you switch the source off no longer schedules a retry.
- Chat: web search source chips show the keyboard focus ring.
- Pulse: a failed achievements check tries again in an hour, doubling up to 12 hours, instead of waiting a week.
- Chat: a web search reply's Sources list shows as compact chips with each site's name; hover shows the link and a click opens it.
- Activity Dock in the egui cabin titlebar: running turns, approvals waiting, and scheduled automation runs, matching the shell's Activity Dock.
- 18+ chats are judged by the whole conversation, not just the newest message: an 18+ voice or any explicit earlier turn keeps the chat on xAI or this computer's model, including clean follow-ups. OpenRouter no longer gets explicit chats, Try it in Settings goes through the same check, direct provider calls refuse them as a backstop, and an 18+ chat xAI can't take (signed out or down) falls back to the local model instead of a provider that bans adult content. Banned asks are still never sent anywhere.
- 18+ chat now plays adult roleplay in character between fictional adults, with exactly four hard limits (minors or age-play, non-consent, real people, animals) plus stop on request; a send that hits one is stopped on this computer with a short notice and no request at all, a reply that drifts into one is replaced before it shows, and picking an adults-only personality while Adults-only is off now says why instead of quietly switching to Nyx.
- 18+ chats now stay off providers that ban them even when the newest message is held back, a reply that falls back to this computer's model says so instead of naming xAI, a half-finished xAI reply is never mixed with a local one, and Try it is only offered for an 18+ personality when somewhere can take it.
- Ask / Auto / Always on the composer stick until you change them; Always asks for confirm once, then stays on across new chats and restarts.
- The repeated-ask Automate offer now shows its own question on Pulse ("You ask this most evenings around 7 PM, …"), says Open edits the schedule line first, and schedules at the hour you actually ask instead of an hour early.
- Cabin: "Remind me about work that isn't backed up" (Settings → Behavior, off by default). At most once a day, Pulse names repos in your code folders with commits unpushed for 3 days, important folders changed since the last backup, and a system snapshot older than two weeks (Timeshift, Snapper or File History). One-taps on the post (and in the chat chip row) push through the agent (which asks first), open the backup tool, or snooze for 3 days. It uses dates and counts only and never copies or uploads files.
- Backup reminders: a snooze that can't be saved now says so, the reminder's one-taps come back after a restart while its Pulse post is still up, and a backup tool with no snapshots yet is named instead of "no backup tool was found".
- Every merge into `beta` publishes a new beta pre-release again, so the beta updater can take it right away; the nightly build stays as a backup.
- Loadout ships built-in vstack skills (qa-walkthrough, av-qa-walkthrough, create-/maintain-verification-skill, follow-through) with plan-to-skill wiring for ship gates, verify-before-done, and multi-step follow-through.
- CI: beta auto-merge now fires (it never ran from beta), card sync reads the Loadout project, and merged card PRs close their issue and card.
- CI: card sync only closes a card for a merged beta PR that says "Fixes #N", not one that just mentions it.
- The chat tab strip's + (and Ctrl+N) now shows the new chat as a selected "New chat" tab instead of an empty strip.
- The far-right ... menu lists the chats that did not fit, the Activity list opens below the tab strip, and the left rail no longer jumps when Chat opens.
- Restyle chat tabs to even-fit widths with an overflow menu (no scroll bar), short context titles, close-on-hover, and remove the native usage/pool status bar.
- Chat rail restores the last-used tab across Home↔Chat and restarts (closed → next live; none → fresh).
- Chat tabs under the title bar replace the rail chat drawer, with soft-delete trash that restores within 30 days.
- Chat: saved-turn labels (`TURN_THOUGHT:`, `TURN_SAY:`, `TURN_TOOL:`) are stripped on every path into a model (chat history, goal names, chips, background tasks) and on every path to the screen, including a reply that echoes them mid-text, so they and the thoughts behind them never show in chat.
- Chat: an obvious joke between friends gets a playful comeback in the personality's voice instead of a lecture; a request that would really hurt someone is still steered away, in character.
- Deck shots: a joke-chat shot (`GROKHUB_DECK_CHAT=joke`) seeds replies stored with saved-turn labels, so the Windows run proves the chat shows only the replies.
- Chips: picking or dismissing a suggestion built from your chat, draft, chat title or a model reply no longer saves its text to `chips.json` (only an opaque key and counts). Older saved text is cleared on load, and that cleanup is now always written back.
- Privacy: chips.json no longer stores raw chat lines as chip keys; a one-time scrub drops legacy text keys.
- Tests: drain queued snapshot writes before timing sync completion, avoiding Windows disk-latency timeouts while retaining consent and exactly-once send checks.
- A fresh launch opens on Home. Chat still keeps a clean draft ready behind it.
- The window resizes from its edges and corners, not only from the maximize button.
- The titlebar's LOCAL chip is a proper pill with room beside the personality pill.
- Home's edit pencil and the layout editor's close button no longer show as empty boxes.
- A wide empty composer says files can be dropped in and that Ctrl+/ lists the shortcuts.
- CI: PRs run fmt, guards, pii-guard and one Linux and one Windows job (clippy and tests in one build). Cross-check, macOS shell, phone-ios, windows-release and deny (unless Cargo.lock or deny.toml changed) move to push, nightly and manual runs. Shell and phone-android are path-filtered. ci.yml and pii-guard.yml accept merge_group.
- OpenRouter and local chats get a free cabin `web_search` tool (DuckDuckGo Lite by default; optional SearXNG or Brave key). xAI keeps hosted search.
- Cabin: "Pick up where I left off in code" (Settings → Behavior, off by default). Pick your code folders, and when you open the app with an empty chat, a repo with unpushed commits or changed files gets one-taps first in the composer: continue as an agent task, show git status, or open it in your editor. Merged branches untouched for 30 days get a tidy offer that still asks before deleting. It reads git metadata only (branch, counts, dates), never diffs or file contents.
- Companion files: Nyx's outfits and modes now come from `companion.toml` (`grokhub_core::companion`), shared by the home dock's corner box and the coming desktop companion. Adults-only outfits are marked in the file and show only with adults-only personalities on; more can be added from `{config}/companions/nyx/adult/` on your PC.
- Pulse: learns your daily rhythm from the minutes you're typing in the cabin (and, opt-in, your games' last-played times), by weekday and hour, on this computer. Low-priority nudges wait for an hour you're usually around, and Pulse offers once to post the brief at the hour you usually sit down. Settings → Behavior shows the week as a heatmap with the usual and away hours, a "Use as quiet hours" button, a Pulse brief time, and Forget my rhythm. Quiet hours you set always win.
- Deck shots hide the signed-out auth strip, pin Personality to Viper (with adults-only on) in throwaway config, and refresh Hangout/Pulse proof pairs after CI.
- Settings → Appearance has "Objectives beside chat", the chat-first layout from slide 10: chats show the open objective's steps and recent chats on the left and a PC snapshot on the right, in place of the companion dock. Off by default.
- Deck-match 2: replace the labeled sidebar with a 40px Loadout icon rail, bracket-L titlebar wordmark in Chakra Petch, trust pills, and a red window outline on Ops Dark themes.
- Home dock: the companion changes pose with the chat, as on slide 17. Hello waves you in on an empty chat, Helping holds the checklist while a tool runs, and Hanging out sits back while you talk. Each pose is a file in `brand/`.
- The home dock matches the deck more closely: the companion card is headed "Companion · docked" inside amber corner brackets, its art grows with the dock, and the Listening wave moves while voice is on (it stays still with reduced motion). Objectives always shows your level and XP, earned from finished tasks and checked steps. PC loadout shows how much RAM is in use, and the GPU tile is the amber selected tile.
- Library on the rail opens a game library, as on slide 11: the Steam games on this PC as a cover grid with All, Installed, Backlog and On objective tabs and a search, and a detail column with play time, last played, the open objective that names the game, Play and Optimize settings. Everything is read from this PC. Skills moved from that rail slot to a button on the Library page, and is still in the palette and slash commands.
- Restyle the Hangout chat column to slide 02: companion header with TALK/TYPE, red-rim assistant bubbles with face avatars, red composer outline and amber send
- Deck shots re-seed the Hangout transcript after fresh-home boot; slide quick-reply chips replace Connect / Start here under the composer
- Windows deck-shots also pair Pulse at 1600×900 and 1100×720 beside slides 13 and 15
- Settings → Personality matches slide 19: a card grid with Explicit/Roast tags, a selected detail panel with the sample line, and an amber Talk as button. Adults-only still hides explicit presets.
- Settings has a Privacy & data page, as on slide 12: whether anything left this PC this week, a This PC / Cloud picture, the on-device model, "agent asks before changes" (Ask or Auto), Know me and encrypted-memory switches, Open data folder, a two-click Wipe Know me, and a seven-day ledger of off-device calls read from the egress log. The log counts calls, not bytes, so the page does too.
- Pulse picks its layout from the whole page beside the rail, as the deck's container rules do, so a 1024 px window now shows one column with the feed below instead of the Feed | Ideas switch. On that one-column layout the header's Suggest ideas reads "Suggest".
- Restyle Pulse to slides 13 and 15: outlined amber Feed/Ideas tabs, amber group diamonds, signal-red icon tiles, amber AUTOMATE tags, and the existing amber Run / red-outline Snooze / quiet Dismiss row actions
- Deck match: in the Loadout look, Settings opens as a full page beside the rail instead of a centred dialog, so Privacy & data shows its story, switches and off-device ledger side by side like deck slide 12, at 1600x900 and 1100x720.
- Deck shots of the chat-first layout (slide 10) show the Hangout chat beside the objectives, without the Activity popover over the snapshot.
- Deck shots: rail clicks allow for the chat tab strip, and Settings opens with Ctrl+, at 1600x900, so the Pulse, Library, Objectives, Privacy and Personality shots land on their pages again.
- Deck shots: `-openSettings <section>` opens Settings on that section at launch (`GROKHUB_DECK_OPEN_SETTINGS`), and Library and Settings shots leave the Activity popover closed (`GROKHUB_DECK_ACTIVITY=0`) so it doesn't cover the page.
- Privacy & data at 1100x720: the hero line wraps ragged right with a gutter instead of stretching its letters into the switches column, and the Encrypted memory row matches the other switches.
- Deck shots: the Library seed fills the throwaway Steam's `appcache/librarycache` from Steam's CDN, as the Steam client does, so slide 11 shows real covers; new `library-detail` shots pick a real game so the detail header shows its art.
- Deck shots open the Library on a real game (Baldur's Gate 3) so the detail header shows its store art.
- Upgrading from a cabin before 3.4 now moves the old Dark default to Ops Dark once; picking Dark again in Settings afterwards sticks.
- The companion dock now shows at the default 1100 x 720 window in a compact width, and the composer column no longer slides under the rail when the dock is open.
- Buttons, tabs, cards and the rail now follow the deck's state recipes in the Loadout themes: red 1.5 px outlines, a gray lift on hover, a darker gray when selected, and amber with dark ink for the main action. Cards round at 12 and buttons at 8, the selected rail item gets an amber bar, and the send button is amber instead of white.
- Deck shots: one launch-time page handler. `GROKHUB_DECK_NAV` opens any rail page and closes the Activity list over it, so Pulse shots no longer rely on a rail click.
- Add Activity Dock Windows deck-match proof shots beside slides 02 and 20.
- Windows deck-match proof shots of Hangout chat tabs beside slide 02 (1600×900 and 1100×720).
- Deck shots open Settings with the rail's gear instead of Ctrl+, so the 1100x720 Settings and Privacy shots land, and add the Windows proof pairs for slides 09–12.
- Deck-match proof shots: Hangout slide 02 and Pulse slides 13 and 15 beside Windows cabin captures.
- Deck screenshot switches (`GROKHUB_DECK_*`) only work in a screenshot run. A stray `GROKHUB_DECK_WIND_DOWN` no longer fakes an idle PC or opens the sleep confirm in a real cabin. One helper reads them, once per run.
- Desktop companion: she breathes (a slow four-second rise through the chest and head), floats a little over the pedestal, blinks every three to seven seconds when her outfit has an eyes-closed patch, and changes outfit with a short scanline crossfade. Reduced motion keeps all of it still.
- Desktop companion: her mouth follows the spoken reply. The reply's mp3 is decoded in the app into a loudness envelope before it plays, and the closed, half and open mouth patches follow it (for outfits that ship the patches). The pedestal lights up while she listens and flickers while she thinks. Right-click gains Change outfit, with Pick for me to hand the choice back to her mode.
- Labs: **Desktop companion**. The corner box gets a Pop out; the companion then stands on her holo pedestal in a small see-through window on the desktop, floating above other windows (or behind them, with Stay behind windows). Click her to open chat, drag her anywhere (she snaps to the monitor's edges and remembers her spot), right-click for Push to talk, Dock back and Hide. The tray gets an item for the same. Empty pixels of her window are click-through on Windows, and the window is left out of screenshots and screen shares. She hides at once while a game runs or a fullscreen app has the screen, and comes back a few seconds after; animation runs at 30 fps, drops to a cursor watch when still, and stops while hidden. Reduced motion keeps her still.
- Labs Diagnostics zip only attaches recent app logs when "Include recent app logs" is ticked under the preview (off by default), and the preview says whether they go in.
- Labs Diagnostics zip goes to Downloads (or the home folder) instead of a Desktop that syncs to OneDrive, Dropbox, Google Drive or iCloud, and is not written at all when every one of those syncs, so a save never uploads itself. The saved notice names the folder used.
- Labs Diagnostics Save now opens a preview of the redacted bundle with a count of what was hidden; Save writes exactly what was shown and Cancel writes nothing.
- Labs Diagnostics zip now also redacts bare AWS access keys, Slack, Stripe, Google API and Notion tokens, JWTs, passwords in URLs, and whole PEM private key blocks.
- Labs Diagnostics: bundles now also redact credential lines (`DB_PASSWORD=…`, `"api_key": "…"`) and every metadata field, never overwrite an earlier bundle, land on your real Desktop (OneDrive or a localized XDG Desktop), export the turn you clicked with the chat's own personality, and show an Open folder action in the chat after saving.
- Downloads cleanup suggestions (opt-in under Settings → Behavior): once a week the cabin looks at the names, sizes and dates in Downloads and posts one Pulse card, like "6 GB of old installers, ISOs and duplicates in Downloads". It covers installers for apps you already have, big archives and ISOs untouched for a month, archives already extracted, and exact copies (found by size, then a hash). You pick from a review list, approve the exact files, and they go to the Trash or Recycle Bin; nothing is ever deleted outright or moved without that approval.
- Local AI: looking for an installed llama-server on PATH skips empty and relative entries, so Loadout never runs an engine from the folder it was started in.
- A findings card with two or more independent fixes now leads with a **Do all** pill. It runs the fixes one at a time (changes first, explanations last), shows each waiting fix as a Queued row, and ends with one summary of what got done and what didn't. Fixes that delete, overwrite or send something keep their own pill and are left out, and Do all stays off when the fixes are alternatives to pick from.
- Cabin: "Help with the app I was just in" (Settings → Behavior, off by default). When you come back to the cabin, the first suggestion fits the app you left, such as modelling tips after Blender, print-profile help after a slicer, patch notes after a game, or a next step after a code editor, with "Because you were in …" and a dismiss. It reads only that app's process name, once, when the cabin loses focus. Windows, X11, Hyprland and Sway.
- Game companion: switching straight from one game to another now closes the first one, so its note offer still shows. The launch card's Open goes to the top-rated guides, and the newest news item is its "Read at" link.
- Cabin: game launch companion (Settings → Behavior, off by default). When a game from your Steam library starts, the Feed shows what changed since you last played, a link to its top-rated guides and the note you left yourself last time; a quiet desktop notification is optional. When the game closes, a one-tap lets you leave a note for next time. It only reads what Steam reports as running: no screen capture, nothing drawn over the game, nothing a game's anti-cheat would see.
- Game guard fixes: nProtect GameGuard is now recognized by its GameMon.des processes and its service; the app no longer reads other windows' titles anywhere while an anti-cheat runs; a slow, outdated anti-cheat check can no longer override a newer one; Halt (and Cua Driver being off) now wins over the gaming pause, so a halt still stops the Cua driver and a desktop Test after Halt says it was halted; and "Record my screen" now refuses to start while an anti-cheat runs instead of starting and stopping at once, and a recording already running waits out an anti-cheat instead of ending.
- Library: each game shows its real cover art. The art comes from Steam's own cache on this PC first, then a copy kept in the settings folder, then Steam's public store images (logged in the off-device ledger). Covers fade in, and a game with no art keeps the painted cover, offline too.
- Game guard: Loadout never gives a game's anti-cheat a reason to flag your PC. While you're gaming (a Steam game, a known game process, an app on your local AI deny list, an exclusive fullscreen game) or an anti-cheat runs (Easy Anti-Cheat, BattlEye, Vanguard, FACEIT, ESEA, PunkBuster, GameGuard, XIGNCODE3, EA Javelin), desktop control pauses: every click, key, scroll and typed text, window focus/move/resize, opening apps, the desktop Test, and the screenshots desktop control and the agent's eyes take. A paused action tells the agent why instead of failing silently. While an anti-cheat runs, reading other windows' titles and "Record my screen" also wait. Settings shows "Game mode: desktop control paused" or which anti-cheat is running.
- Windows game and anti-cheat detection no longer opens any other process: process names come from a Toolhelp snapshot, anti-cheat services and drivers from the Service Control Manager, and Steam's running game from its `RunningAppID`. The PowerShell `Get-Process` path listing (wind-down and PC health) and the foreground app's process open are gone. Games outside Steam are now recognised by Unreal shipping builds and the Epic launcher's manifests, and any game can be added to the local AI deny list.
- The Game watcher now listens only while Counter-Strike 2 or Dota 2 is running, shows its tips as a quiet note in Loadout (at most one a minute, never in the game), and posts one recap card to Pulse when each match ends.
- New Game watcher setting for Counter-Strike 2 and Dota 2 (off by default): it writes one small Game State Integration file, the feed Valve supports for this, into the game's own config folder and listens for the match on this computer only. Turning it off removes the file.
- The game watcher can now follow League of Legends matches through Riot's own local match feed, and World of Warcraft boss pulls (kills, wipes and your deaths) from the combat log.
- The game watcher now keeps each finished match on disk for 30 days (up to 200 per game), can answer questions about a match from a short summary using the local model first, and Settings says which game it supports first based on your most played Steam game.
- Chat: hanging out no longer feels rushed. Casual chat is care-first (stop when the need is met, natural length), the personality sets the reply length there, and the cabin rules tell the assistant to stay present with no task offers or sign-offs. Task turns keep their word caps. The default voice flirts back playfully, PG-13.
- Pulse offers the two off-by-default PC helpers once each, under its header: "Offer to sleep the PC late at night?" and "Suggest Downloads cleanups?", with Turn on and Not now. Not now stops that offer for good.
- Settings › Behavior lists Late-night wind-down and Downloads cleanups first, under "PC helpers", so a small window no longer clips them.
- Pulse: optional bite-sized learning paths from Know me hobby tags (3D printing, Blender, Rust, game modding, digital art, CAD). Opt in per hobby under Settings → Pulse; Done / Skip / Too easy / Too hard adapt the next tip; tips prefer the app you were just in when Focus help is on.
- Learning-path tips now wait out quiet hours, show up on the Feed where their Done, Skip, Too easy and Too hard buttons are, and pick the game modding path when you're playing a recognized game.
- Desktop companion: the holo pedestal she will stand on, drawn in the theme's colours with slow counter-turning rings, a fading light cone and drifting scanlines; still under reduced motion. Deck shots can preview it in the corner box (`GROKHUB_DECK_PEDESTAL=1`).
- Home packs its module grid: each card fills its slot, so a short card no longer leaves a gap above the next one, and the body only scrolls when the modules really overflow (no stray scrollbar).
- Home measures the chips and composer, so the "Assign an objective" box is never cut off at the bottom.
- On narrow windows (900×700) Up Next sits beside PC Loadout instead of dropping below the fold, and Today's objectives keeps room for the weekly streak.
- PC Loadout shows Tune-up and Details at medium size too, and empty weekly streak tiles say how to start one.
- Edit Home catalog uses plain labels: "fits Large, Medium, Small", "in this slot", and "Not built yet" instead of "L M S", "IN SLOT" and "Coming soon".
- Home Mission Control: module registry (36), slot grid layout engine, and `home_layout.json` v2 persist.
- Home Mission Control Send/chips switch to Chat (new tab) and stream the reply there.
- Home rail opens modular Mission Control (no chat tabs); Edit Home dialog; Send and chips jump to a new Chat tab.
- Pulse: "Know me" (Settings → Pulse, off by default) learns your games, hobbies, dev tools, hardware and language from names and counts on this computer, one toggle per signal. It never opens file contents, browser profiles, mail, messages or keys. "What I know about you" lists each tag with its source, with Delete on each and Wipe all; interest tags join Pulse as "From your PC". The agent reads them with `know_me_tags`.
- Labs: Diagnostics mode (off by default) exports a redacted chat debug bundle to the Desktop as `loadout-diagnostics-DATE-TIME.zip` (JSON + Markdown) with classifier, playbook, policy, length cap, tools, and timings — one click, Open folder toast, Windows and Linux.
- Beta pre-releases and the heavy CI jobs now run nightly instead of on every push to beta.
- Chats now teach the cabin as you go: facts about you land in USER.md, "always/don't" corrections in SOUL.md, and "remember that…" in MEMORY.md as short clean lines, replaced instead of piled up. Raw chat is no longer copied into memory, and every write is logged in `memory/learn-log.jsonl`.
- Local model launch: the llama-server doc comment now says the API key goes in the child environment, and its tests check the exact `--api-key` argument and the key variable instead of the whole environment.
- Agent messages (Cua Driver, sudo prompt, budget and offline pauses, private-data locks, self-managed skill and automation tools, update source-folder hints) now name the product "Loadout".
- Status lines, Settings notes, confirm sheets and change reports in the cabin now say "Loadout" instead of "GrokHub". Folder, keychain and update paths keep their old names, so nothing moves.
- Help text, slash hints, Pulse and health notes, update hints, and the prompts the cabin sends to the model now name the product "Loadout" instead of "GrokHub". Paths, keychain entries and update URLs keep the internal name.
- The README and the reference guide name the app Loadout; download names, folders and commands keep their GrokHub names so installs and updates don't change.
- Disable Devices 'Send a task home' with explanation note and sign-in link when signed out.
- Hide session actions menu when no active session is open.
- Hide Library filter header and show empty state with 'Open Steam' action when no Steam games are found.
- The code signing policy names the app Loadout. Release file names stay as they are, so updates and checksums keep working.
- The model's opening line, the secret prompt for the app's own tools, screenshot and desktop-session errors, the hub's phone pairing refusal, the MCP sign-in app name, the recording delete note and the shell window title now name the product "Loadout". Key and folder recovery steps name Loadout and still give the stable "GrokHub" entry and folder name to look for.
- The Linux menu entry, the Windows exe details, PRIVACY.md, the installer's privacy page and the built-in playbook headers now say "Loadout". The desktop file, exe and folder names stay grokhub/GrokHub.
- Local AI: Get engine install in the shell Settings panel, restore the Muse/Gemma install-and-run draft offer, and wire Manual draft On/Off into the sidecar. Engine downloads allow GitHub's `release-assets.githubusercontent.com` CDN redirect.
- Local AI: Settings reads this PC's memory before offering a model, and with no suggestion starts on the smallest model instead of the catalog's biggest.
- Local AI: Download asks once with the size and the drive's free space (and refuses when it won't fit); the closed model dropdown uses a short line that fits.
- Cabin: Local AI download confirm reads the drive's free space when it opens, not the figure from launch.
- Cabin: a hardware read that fails shows as unknown, with no recommended model, instead of as a PC with no memory.
- Cabin: Settings looks again every 250 ms while it reads this PC's hardware, so the model picker appears without a mouse move.
- Cabin: draft model downloads ask first and check free space, like main model downloads.
- Local AI: turning it off mid-answer now frees the model as soon as that answer ends, a keep-warm load or a request already on its way can't restart it after it's off, and the "Using local" chip clears when Local AI is off or nothing can answer.
- Local AI: the status shows Ready once the on-device server has exited, instead of Running while the model is gone.
- Local AI: the idle unload waits for requests in flight and counts idle time from when the last one finished, so a long answer no longer gets the model stopped mid-reply.
- Local AI: switching the local model Off unloads it right away, and quitting Loadout stops the on-device server instead of leaving it holding memory.
- Local AI: the model loads ahead of time while it's covering for the cloud (offline, or no cloud model answering), stays loaded for as long as that lasts, and unloads 10 minutes after the cloud is back. Online with the cloud up, it still loads only on demand.
- Routing: with the cloud down or no sign-in, hard work stays off the local model (also after a failed on-device check), a provider you picked or pinned still answers on its own key, and a model held back only by your price limit asks for your OK instead of switching to local.
- Routing: online, the cloud models always take the step while any of them answers; the local model is used only offline or when no cloud model can take it.
- Routing: the on-device difficulty check runs only offline, so online turns no longer load the local model.
- Cabin: a "Using local" chip shows while online steps fall back to the local model because no cloud model is answering.
- Pulse can suggest a bigger local model when one in GrokHub's checked catalog now fits this PC, with a button that opens the model picker on it, and can link new official GGUF releases on Hugging Face that would fit with headroom. Opt-in under Settings → Pulse → Sources; it checks once a week, and only catalog models download.
- Chat: a permanent Main chat is always first in the tab strip, sidebar, and History. It can't be archived, deleted, pinned, or renamed, and it opens by default on a first run or when the last chat is gone.
- Chat: scheduled automation reports, and background tasks whose chat was closed, now post their result in Main, and the next reply there knows about them.
- Quitting right after switching memory files no longer drops what you typed in the file you left: exit waits up to two seconds for that save.
- Docs: evaluated MemPalace for bot memory.
- Model drop: every new model from one Downloads scan now gets its own offer, Settings lists `.stp` with the other extensions, OBJ sizes no longer claim millimetres, and the Windows "Open in slicer" one-tap runs the slicer in PowerShell (`& "…exe"`).
- Model drop: the Open and Move one-taps quote file paths so a crafted file name cannot run its own shell command, and a 3MF preview stops reading an entry that inflates past a size cap.
- Cabin: "Offer next steps for new 3D models" (Settings → Behavior, off by default). When an .stl, .3mf, .obj or .step file lands in Downloads, Pulse shows it with a preview rendered on this PC, its size and dimensions, and the composer offers one-taps: open it in your slicer (OrcaSlicer, PrusaSlicer, Creality Print, Bambu Studio or Cura, found on Linux and Windows), suggest a print profile, print it through the agent (which asks first), or move it to your models folder. Only files with those extensions are looked at.
- The internet drop log now raises "Offline since …" and "Back online" as toasts on every page, saves the CSV to Downloads under USERPROFILE on Windows, and counts a minute as slow only when the connect takes more than 300 ms.
- Internet drop log (opt-in under Settings → Behavior): about once a minute the cabin checks that two public DNS servers answer, logs drops and slow minutes on this computer, says when it's back online and how long it was down, and posts a daily card like "Your internet dropped 3 times last night around 2 AM". Settings shows drops by hour, runs an on-demand speed test, and exports the log as CSV for your ISP.
- Opt-in local nudge watchers post Pulse suggestions for game updates, GPU drivers, low disk, crash spikes, OS updates, and peripherals, with 24h cooldown, 7-day dismiss snooze, and a daily cap.
- A profile still on the retired "Default" personality now switches to Nyx on disk the first time it loads, in both the cabin and the shell, so the old id doesn't linger.
- Nyx is the default personality: the plain "Default" preset is gone, and a profile or chat still set to it speaks as Nyx after the upgrade. Nyx now carries the honesty values the old preset stated.
- Nyx's corner box on the home dock now shows what she is doing: at the desk with headphones while a coding tool runs, in a doctor's coat for PC-care tools (fans, temps, drivers, repairs), with glasses and a book while searching or reading, on the couch with a controller when you just came back from a game, and in her hoodie when you're just talking. The Viper personality puts her in the snake onesie.
- Fixed: Pop out (and Dock back) on Nyx's home card now works. The card's tap-to-talk area was on top of the button, so a click turned on voice instead.
- With adults-only personalities switched on, an explicit personality other than Viper now dresses Nyx in her raunchy outfit in the home dock's corner box. Viper keeps the snake onesie, and with adults-only off Nyx shows her usual picture for what she is doing.
- Nyx's corner box now has several poses per mode, new Thinking and Writing modes, and every built-in personality wears its own outfit there and on its Settings → Personality tile (explicit ones only with adults-only personalities on).
- Home is one page for everyone: deck shots now paint the same module grid real profiles get, with seeded data only.
- Objectives on the rail is the details page for Home's objectives card (full checklist and streak) instead of a second Home, and the card's Details opens it.
- The coach line with a made-up "+12%" win rate is gone from Home and Objectives.
- The Linux package check no longer calls a file it can't read "missing". Files a user can't read without admin rights are listed as "couldn't check", so they never turn into a reinstall (kernels included). Only files that are really gone get the reinstall offer.
- The assistant never says it's doing something "with your approval" before you said yes or accepted the approval card, and it keeps its planning notes to itself.
- Long-run progress notes read as plain sentences ("Finished check failed services. Now on verify packages.") instead of "Done: … and 2 more."
- The Linux and Windows repair playbooks check the fix again after a restart they started, and the Linux one fixes the real setting (keeping a .bak) instead of linking around it.
- Opt-in PC health while gaming samples CPU/GPU temps and util lightly (local_governor game detection), then posts a Pulse summary with max temps, throttle flags, a GPU vs CPU bound guess, and tips — never changes fans, clocks, or drivers.
- PC health while gaming: sensor and process checks now run in the background with a 5 second limit, so a stuck `nvidia-smi` can't freeze the app. Windows games from Steam, Epic, GOG and Xbox libraries are detected. CPU use comes from real CPU time on Linux instead of load average. The summary tracks the hottest GPU and only flags throttling against clocks seen so far.
- The PC loadout header now shows a real, labeled number, "RAM N% used", instead of an unlabeled health score. That score read 100% whenever heat couldn't be read, which is most Windows PCs. The header stays blank until free memory has been read, and the small GPU temp tile says "Sensors unavailable" whenever heat wasn't read.
- Build: beta compiles clean again with no Personality photos in place (clippy dead_code fix).
- Settings → Personality stays inside the window: Loadout opens Settings as a fitted page, the card grid wraps and scrolls, and the Selected detail card never overflows.
- The Selected detail panel and amber TALK AS button stay on-screen at deck sizes: the layout reserves the detail column first so an expanding card grid cannot push it past the Settings clip rect.
- Grid cards force a top-down layout so descriptions no longer wrap to a one-character column beside the title.
- Settings → Personality Selected card matches slide 19: big letter icon beside name/tags, quote, amber TALK AS — no placeholder photo strip or Try it/Duplicate on Loadout.
- Only the selected grid card gets the red highlight; other cards keep a quiet border.
- Chat personality: the switch opens a centred picker with presets wrapping in rows. A switch now changes the titlebar, the chat welcome face and the companion dock together.
- Personality Try it: signed out shows a neutral line with a sign-in link instead of red text; a reply that never comes times out after 45 seconds; it uses your provider model or the local model when that is where your chats go.
- Personality "Try it" drops the call it replaces or that timed out, so a late reply no longer lands over the new one. Every route now ends within the 45-second limit: a provider call is bounded by it, and a Grok call shares it across its Responses and Chat Completions attempts and skips the second once cancelled.
- 18+ chat keeps your AI account safe: asks that every provider bans (minors, non-consent, real people) are held back before any model call, local ones included, and the voice steers elsewhere in character.
- Explicit adult turns go only to xAI or this computer's model: Anthropic, Gemini and OpenAI turns go through xAI instead, and OpenRouter keeps its pick with fallbacks off. You get one plain heads-up a session.
- Adults-only personalities ask once that you are 18 or older and remember it. A profile that already had them on keeps them.
- Small talk stays chat. A task word only makes a task when it names something to act on ("turn off the lights", not "turn around" or "stop it lol"), laughing and "how are you?" lines read as chat, and while you're hanging out a short question stays chat instead of getting a sourced answer. After real work, a bare "delete it" still means that work.
- Playbooks are picked in a fixed order: safety checks first, then a specialist, then the default for that kind of ask, then a generic fallback. Chat never falls through to a task playbook, a playbook can `exclude` intents, and `priority` breaks a tie.
- Playbooks: add Game tuning, a specialist playbook that reads this PC's hardware before recommending FPS and graphics settings for a game.
- Brief routes gaming tips, PC/loadout compares, and Steam library or patch-note asks to gaming-help, build-loadout, and game-news-library (game-help / loadout / library domains); game-tuning still wins FPS asks.
- Two new talk playbooks. Hanging out covers small talk: it matches your energy and length, banters and flirts back within the persona's content level, makes no task offers or sign-offs, and uses no tools. Winding down covers tired, stressed or venting lines: it listens first, stays gentle and short, and offers no fixes unless you ask.
- The Brief gains a `low` mood for tired, stressed or venting lines, and "I'm crashing out" reads as a mood instead of a crash to repair. Playbooks can match on mood (Settings → Playbooks shows "Fits chat · low").
- One "Fix this PC" playbook replaces Repair Linux and Repair Windows. It covers Linux (Arch, Debian or Ubuntu, Fedora), Windows and macOS, and each turn only sees this computer's steps.
- The PC-fix lessons are built in: back up first, a permission-denied answer isn't something missing, fix the real setting rather than linking around it, say "approved" or "done" only once it's true, and check again after a restart.
- System repair on a Mac now gets the PC-fix playbook instead of the general one. Settings → Playbooks counts only this computer's steps.
- Playbooks: rewrite code-dev and research, slim task-general, and add Quick answer, Plan the day, and Creative draft specialists.
- Playwright browser tools now detect cached Chromium or installed browsers when Chrome is missing, and explain how to install a browser when none is available.
- Playwright browser fallback: `PLAYWRIGHT_MCP_*` values of `false` or `0` no longer count as a browser choice, `PLAYWRIGHT_BROWSERS_PATH=0` still prefers an installed browser (and otherwise leaves Playwright to its package cache), a relative MCP working folder resolves against the cabin's own, and current macOS "Google Chrome for Testing" downloads are found.
- Print alerts: printer requests no longer follow redirects, so they can't be sent off your network, and a print card shows a camera snapshot only when the printer reports a webcam on its own host. The snapshot now actually appears on the card.
- Pulse: opt-in LAN 3D printer alerts (Moonraker/Klipper and OctoPrint) notify on finish, fail, pause, or filament runout with progress and an optional camera snapshot; pause/cancel stay behind the approval card.
- Settings › Privacy no longer says "Cloud OFF" and "Your data stays on this PC" while you're signed in and chat or background tasks would use Grok. A quiet week now reads "Nothing left this PC this week" with the cloud READY, and a line names what goes to Grok (chat, and background tasks unless the installed on-device model takes them). "Cloud OFF" is kept for when nothing can leave: not signed in and no calls.
- The On-device model hint matches the router: small background jobs run on this PC and bigger ones still use Grok, or it says the model isn't installed yet.
- The title bar chip reads the same 7-day egress log: "LOCAL · 0 B SENT" only for a week with no calls, otherwise the number of calls sent.
- The Pulse wind-down offer only says it is on once the switch is saved; a failed save keeps its error showing.
- Privacy: the on-device model copy now says how it's really used: the cloud first while it answers, this PC when it doesn't or you're offline, sensitive data kept here, loaded only when needed, and 18+ chats only to xAI or this PC.
- Security: in-app updates unpack into a new private folder with a random name, so another account on the same Linux computer can no longer swap the verified binary before it installs. Voice clips, pasted pictures and spoken replies also go to a private temp folder instead of fixed names in /tmp.
- Pulse renames the Ideas tab to For You. Dismissing a For You card after you used it, or a Feed card after Like or Discuss, records as used/done and never counts as a negative bury; only unused dismiss and Not this stay negative.
- Pulse: the line over the Feed counts the posts it shows (with any check in progress) instead of saying "Nothing to check yet" over cards.
- Settings › Pulse: the Add topic field and its Add button line up; hobbies not seen on this PC say so in plain words.
- Pulse: learns from what you do with each card. Run, accept, like and save lift a category; dismiss, snooze and "Not this" lower it, and a card left unopened for a day counts as skipped. After three skips in a row a category goes quiet, with a one-line note. Each post's menu says why you're seeing it ("From Dota 2 on Steam · game news +3") and offers More like this / Less like this. Settings → Pulse → "What Pulse learned" lists every category with Bring back and Reset. Proactive suggestions follow the same weights.
- Pulse's like, dismiss and used lines now live in their own `memory/pulse-ledger.md` instead of crowding MEMORY.md, in both the app and the desktop shell. Lines already in MEMORY.md move over once, and MEMORY.md.prev keeps a copy. A MEMORY.md over the size cap is never rewritten, and a full ledger drops its oldest lines rather than the new one. The Memory page also offers, once, to remove lines an earlier reflect copied straight from chat into MEMORY.md, showing them first and keeping a copy.
- Pulse is one stream: ideas and feed posts share a single centered column (an idea, then two posts, and so on), with no Feed | For You switch, side-by-side columns, or "From your feed" heading. Suggest for you and Feed instructions sit on the right of the Pulse title, the source check note waits behind a small Details toggle, and an empty Pulse shows one empty state.
- Pulse no longer follows small talk: chats titled "How are you" or "Good morning" don't become interest topics.
- A topic you skipped stays skipped; chat mentions alone no longer bring it back under its starting weight.
- Ignoring a card over and over stops lowering it at a floor; only a real dismissal goes lower.
- Closing a digest, run report or other report card counts as seen, not as "show fewer of these".
- Monitor brightness chatter (powerdevil i2c/DDC errors, ddcutil) no longer shows up as an app crashing.
- CI and QA: harden process-tree tests and pii-guard, lint the shell UI, and capture Tauri smoke shots in Ops Dark.
- Pulse: a Not this / Not interested / Dismiss that the ledger refuses (too large, or a link) no longer scores the card first, so "Nothing was changed" is true.
- Web: the Add, Remember and Search buttons stay on screen at phone width instead of sliding off the right edge.
- After multi-step work the agent ends with a Finished list (Done, Partly done or Not done per task), so the reply shows as the recap card.
- Release radar now takes wishlist games in turn with the most-played library games and goes round the whole list across daily checks, so wishlisted games get looked at. Soundtracks, demos and DLC for other games no longer post as "sequels", and a search hit that is DLC for the game itself is labelled as DLC.
- Pulse can tell you about upcoming DLC and sequels for your most-played Steam games and your wishlist (opt-in under Settings → Pulse → Sources). It checks Steam's public store once a day, posts only what you don't own yet, and "Remind me when it's out" posts again on release day.
- Repeated-ask offers now count the same ask at the same hour on different days as separate times, so asking for the same thing every evening triggers the offer instead of overwriting one entry.
- When you ask GrokHub the same kind of thing around the same hour (for example patch notes most evenings), it offers once to schedule it or save a skill. Asks stay on this computer; Dismiss never suggests that cluster again.
- Revert premature vstack AGENTS.md and Claude settings wiring; maintainer will ship agent setup as its own card.
- Late-night wind-down: a switch that can't be saved now stays as it was, so Pulse and Settings never act on a change that isn't on disk.
- Privacy: the on-device line now says small jobs run on this PC only when it has room, and use Grok while it's busy.
- Screenshot Explain no longer stutters the app: the folder check and the image load run in the background. It finds screenshots in a Pictures folder moved to another drive or OneDrive, or set in Linux's user-dirs, says so when the attachment tray is full (and keeps the offer), and a file stamped with a future date no longer hides later screenshots.
- Opt-in screenshot folder watch with a dismissible Explain chip; image bytes load only after the user taps.
- The Settings sheet (non-Loadout themes) takes 80% of the window between 920×620 and 1280×880, centres under the titlebar, and no longer covers the window buttons. Maximizing gives a bigger sheet.
- Each Settings section opens at the top instead of keeping another section's scroll position.
- The About heading opens About, which shows the version and a build line (channel, branch and commit), instead of a panel titled Update.
- Connectors › GitHub copy says "your GitHub token" instead of internal names.
- A send while signed out keeps the typed message in the box and shows a sign-in card with a Sign in button, instead of clearing the text behind a faint status line.
- The empty chat's headline uses full-contrast ink.
- Built-in skills credits now point at the upstream projects instead of naming people, and the upstream MIT notice ships unchanged beside them.
- Repo: deck-match proof pages, CI guards and the deck-shot list are split so changes stop conflicting on shared lists (one page per change under docs/design/deck-match/, one guard per file under scripts/ci-guards.d/, and a union-merged scripts/deck-shots.list.ps1).
- Pulse: once a day at your usual active hour (or first open), a Start my day Digest pulls together resume, watchers, game sales, and one next step — empty sections stay hidden; each line has a one-tap; Not today dismisses the whole brief.
- Start my day waits for the morning (5 AM to 2 PM) until your rhythm is learned, waits for the code-resume scan, leaves out snoozed cards, muted Steam games and steps you said "Not this" or never to, keeps quiet categories from crowding out real items, and counts a one-tap as using the brief and its card.
- Pulse can alert you when games on your public Steam wishlist go on sale (opt-in under Settings → Pulse → Sources). Pick any sale, 25% or 50% off, or the lowest price seen; each sale alerts once, and Not interested lowers how many show per day. The SteamID is sealed on this PC and only sent to Steam.
- When a reply that followed real work (tool steps in the same turn) ends with a **Finished** block, the cabin paints that block as its own amber-rimmed recap card under the reply. The card shows one line per task, marked done (✓), partly done (!) or not done (×), plus what still needs you, where to undo, and a **Show details** fold with the turn's steps. Plain chat replies never get a card. A **Do all** run now asks for this recap at the end, and its card lists the steps of every fix in the batch.
- Tauri shell: Ops Dark, OLED Black and Soft Charcoal Loadout tokens in styles.css (slides 07–08).
- Downloads tidy and desktop trash moves: refuse on a fixed drive whose Recycle Bin is turned off (policy or per-drive "delete right away"), and ask before deleting a file too big for the bin, so nothing is ever deleted for good.
- Settings now scrolls as one page with the side menu following the section you're reading, icons across the app answer hover, press and selection with a short motion, card icons lift a little when you point at the card, and a new "Reduce motion" switch in Settings → Appearance keeps everything still.
- Game guard: Riot Vanguard installed on its own no longer pauses anything. Its driver and service run all the time once installed, so desktop control, screen capture, window checks and "Record my screen" now pause for Vanguard only while a Riot game runs (VALORANT, League of Legends, 2XKO), found by exe name with no process opened; the Riot Client and League's lobby don't count. Settings notes under the desktop switch that Vanguard is installed and always running. Other anti-cheats are unchanged.
- Web access is served over HTTPS on your network. Loadout makes its own certificate the first time and keeps it, and Settings shows the certificate's fingerprint so you can check it against the one in your browser the first time it warns you.
- Web app: when its connection to the desktop drops and comes back, the page gets the chat events it missed, so a reply that was streaming finishes on screen. If it was gone too long for that, the chat says part of the reply may be missing.
- Web search polish: natural citation wording (no tool/provider names), drop off-topic hits like the Rust game for programming queries, and say Loadout instead of GrokHub in Cabin defaults local-model hint.
- Pulse: weekly "Your week" recap Digest from opt-in counters (games playtime, projects, prints, disk, tasks) at your usual active hour, with a path-free share image.
- Wind-down: the sleep offer stays off when this PC can't tell whether a game, a fullscreen app or GPU work is running, and Settings says why. Windows now detects games running from Steam, Epic, GOG and Xbox libraries.
- Late-night wind-down (opt-in, Settings → Behavior): well past your usual wind-down or after an hour idle with nothing running, one Pulse card a night offers to sleep the PC. It never shows during a game, a fullscreen app, a render, a download or an agent task, and the PC only sleeps after you confirm.

- Linux: `grokhub-linux-v3.5.0.tar.gz` and AUR `pkgver=3.5.0`.
- Windows: `GrokHub-Setup-3.5.0.exe` and `grokhub-windows-v3.5.0.zip`.
- Shell (Tauri): `grokhub-shell-v3.5.0-linux-amd64.deb`, `grokhub-shell-v3.5.0-linux-amd64.AppImage`, `GrokHub-Shell-Setup-3.5.0.exe` and `grokhub-shell-v3.5.0-macos-arm64.dmg`.

## 3.4.0 — 2026-10-27

- Activity Dock in the app shell's views bar: one place on every page that shows the chat turn that's running (with its latest step and how many are done), an approval or plan waiting on you, and Run now automations as they run and finish. Review and Open chat go to the chat with the approval card; Open result goes to Automations. On phones it's a dot and a count, and the list opens above the bar.
- Loadout rebrand, foundation 1 of 6: the product name, tagline and companion name now come from one file, `brand/brand.json`, and the logo mark and app icon load from `brand/logo-mark.svg` and `brand/app-icon.svg`. The app now shows the working name "Loadout" in the shell, the web app, and the cabin's window title, tray, title bar, welcome screen, About and notifications. Config folders, the keychain entry, app ids and installer names stay `GrokHub`, so settings and sign-ins carry over.
- Loadout foundation 3 of 6: the cabin's chat home now has a dock on the right. The companion (named from `brand/brand.json`) sits at the top as a still picture with "Here · tap to talk"; tapping it starts voice. Below it are Objectives (the open mission's checklist), PC loadout (GPU, driver, thermal, RAM and free disk as this PC reads them) and From Pulse (the newest home cards; tap to open Pulse). The dock hides on windows narrower than about 1240 px so the chat keeps its column.
- New "Nyx" personality, the companion's own voice: a nerdy gamer and techie who is warm and fun, roasts and trash-talks when the moment's right, and drops the jokes for straight help when you're frustrated or it's serious. Never explicit. New installs start on Nyx; existing installs keep whatever personality they picked, and the 21 other presets are unchanged. Its name comes from `brand/brand.json`.
- Appearance: Ops Dark (default for new installs), OLED Black, and Soft Charcoal Loadout theme tokens, with classic Dark / Light / System kept.
- Long lists show part of the next item so you can tell there is more: the command palette (shell and cabin), the phone Settings sections row, and the next-step chips under a reply. Arrow keys scroll one item ahead of the selection, instantly when the system asks for reduced motion.
- Pulse in the Loadout look: Ideas and Feed side by side on wide windows (about 60/40, narrower on laptops, cards tile 2-up when there's room), one stacked column on small windows, and the Feed | Ideas switch on narrow ones, where Run stays on every idea and Snooze and Dismiss sit in the ··· menu. Cards use the theme's raised fill and red rims; every Pulse action is unchanged.
- Pulse: your local Steam library feeds the Feed with each game's Steam news, most recently played first, with store art. Settings → Pulse → Sources shows "Use my Steam library in Pulse" when Steam is found, lists the games feeding Pulse, and mutes any of them. The agent's `pulse_steam` tool turns it on or off and mutes games, and reports what it saved.
- Settings > About: a quiet "Support the developer" link (label in `brand/brand.json`). It shows only once `supportUrl` in `app.json` holds a web address and opens it in the browser. No payments, tiers or prompts.

- Linux: `grokhub-linux-v3.4.0.tar.gz` and AUR `pkgver=3.4.0`.
- Windows: `GrokHub-Setup-3.4.0.exe` and `grokhub-windows-v3.4.0.zip`.
- Shell (Tauri): `grokhub-shell-v3.4.0-linux-amd64.deb`, `grokhub-shell-v3.4.0-linux-amd64.AppImage`, `GrokHub-Shell-Setup-3.4.0.exe` and `grokhub-shell-v3.4.0-macos-arm64.dmg`.

## 3.3.0 — 2026-10-27

- Personalities: an adults-only switch in Settings → Personality (cabin and shell), off by default. Off hides explicit presets such as Viper in every picker, and a chat set to one speaks in the default voice; the quick picker tags explicit presets.
- Care-first routing stays off. The brain policy `care_first` switch can hold `chat:default` effort, let the Brief pick the goal, and soften the 60, 150 and 120 word targets. The offline dry-run had no live quality judge, so the shipped file keeps the switch false.
- The default voice keeps a values charter (honest, not sycophantic), learned memory rows show their source and date and wait for approval, and a sycophancy check blocks a skill patch that would agree with a wrong claim.
- A multi-step chat checks once before it stops, and that chat's open intentions stay on disk.
- Agent: a feature ask like "use X for Y" no longer runs the repair checklist, a checklist item only counts as done after a new tool result, a blocked item can't flip back to done without one, adding new items without finishing any counts toward the 3 re-plan cap, a "done" reply with no tool run gets an honest line, and made-up one-tap fixes are dropped from the findings card instead of refused and re-posted.
- Model dream stays off until `app.json` sets `"dream_model": true`.
- After the deterministic dream, one nightly `background:dream` call asks for medium effort. That route starts at low and its ceiling is medium. The reply is capped at 4096 output tokens.
- Lessons pass `lesson_checks` and the change ledger. Undo restores the previous skill file.
- USER and SOUL lines stay proposed until you approve them.
- While the flag is on, a turn's reflect or remember proposes those lines instead of writing them into core memory.
- Chat keeps a capped SOUL block and the approved USER block in the volatile prompt tail, recalls only new memory hits on later turns (1,024-token cap, override with recall_token_cap in app.json), and starts new installs on the AMR backend with legacy dual-read. An existing app.json that omits memory_backend stays on legacy; set "memory_backend": "amr" to opt in (memory_migrated is set on a new install).
- Chat: replies lead with the result instead of ending on a generic "What's next?". The "What's next", "Next step" and "Take the next step" chips are gone, suggested chips must name a specific action (or there are none), and the agent does an obvious, reversible next step itself.
- Chat: when the agent needs a decision it asks with 2 to 4 options and marks the one it recommends. The Butler, Drill Sergeant and Friendly Neighbor voices no longer close on a question.
- Shell chat: Grok's questions show their options as buttons, recommended first; a reply that hit an error or named a fix gets up to three one-tap follow-ups (most replies get none); and an automation's Run now posts a Finished or Failed notice in chat with a one-tap follow-up.
- Native agent: with deferred tools on, the eager tool list is now the general coding set (read, list, grep, glob, edit, write, shell) plus the self-manage list tools; scheduler, media, desktop, connection and self-manage write tools load on demand from a sorted one-line-per-tool index. A per-profile schema byte budget test keeps it from growing back. Technique from the Reason Agent 1.1 post, reimplemented.
- The agent can check Local AI itself: `local_ai_status` reports whether it's on, the engine and model paths, loaded or idle, the network state, recent health lines and the last offline event. `local_ai_selftest` asks the on-device model one short question without the network, only when called and after your OK.
- Local AI now gets its engine on its own: turning it on, or launching with it on, downloads the pinned llama.cpp `llama-server` when none is in `GROKHUB_LLAMA_SERVER`, `{config}/bin` or `PATH`. On x86_64 Linux and Windows it is now the Vulkan build, which uses an AMD, Intel or NVIDIA GPU and falls back to the CPU.
- Settings → Local engine shows Runtime ready, Runtime missing, Loading or Error with the reason, and offline requests say "Runtime missing" instead of failing silently.
- Fixed: the Linux engine install wrote its shared-library links as empty files, so `llama-server` couldn't start. CPU-only launches no longer pair a q8_0 KV cache with flash attention off, which llama-server refused.
- Fixed: the on-device route only looked for its engine and model when `GROKHUB_CONFIG` was set, so in a normal launch Local AI never ran. It now reads the cabin's own config folder.
- Local AI keeps a small log at `models/local_health.jsonl`: engine starts with their load time, stops, missing engine or model, out-of-memory, GPU backend and other start failures, network changes (online, offline, degraded), and where each offline turn went. No prompts, answers or keys, and the home folder is written as `~`.
- Local AI: first-run setup and Settings now let you pick which on-device model to download from a short list (the one that fits your machine is marked recommended), switch to another downloaded model without reinstalling, and remove models you no longer use. A switch takes effect on the next request, and the model picked in the desktop shell's Settings → Local AI is the one that runs.
- Offline with no working on-device model, a chat turn no longer fails silently. The chat says "Offline, local model unavailable" and why (runtime missing, no model, Local AI off, or the start error), keeps your message, and answers it once the network is back with a note: "answered after reconnect".
- Personalities: a new adults-only "Whore" preset, tagged Explicit like Viper and never picked by default; it keeps hard limits (adults only, consent, no real people) and drops the act when told to stop.
- Native agent: an append-only prefix guard logs any request whose cached prefix (system, tools, earlier turns) changed for an unexpected reason, and the native usage line now shows each turn's cache hit share ("cache N%"). Technique from the Reason Agent 1.1 post, reimplemented.
- Pulse Feed (shell): posts are cards in one centered column with a 3D topic emoji, cited snippet, and a hero image or a square gallery that opens a lightbox with Download. Love and Discuss sit under each post; a menu holds the full date, "Why I created this" and Delete. More posts load as you scroll.
- Pulse Feed (shell): Edit feed instructions saves what your feed should cover and writes a new post right away.
- Web app: Pulse is now available in the browser.
- Pulse Feed (shell): posts are now short model-written briefings from the free sources, with inline cites checked against the pages fetched that run, a topic emoji, a "why you're seeing this" line and up to three images cached on this computer.
- Pulse Feed: a system cadence replaces the "Check every" choice. Posts come every two hours, every 90 minutes after you open Pulse or love or discuss a post, and every four hours after three quiet days. Saving the feed prompt writes one post right away.
- Pulse Feed: a short taste summary, rebuilt from what you love, discuss and say in chat, steers the writer. Reading a post never counts. Like is now Love, and Delete removes a post without teaching the feed anything.
- Pulse Feed lists every post newest first, with no score cutoff.
- Pulse Ideas are written for you in the background (at most every six hours) from your journal questions, interests, recent chats and feed posts. Each has an emoji, a short title and an "I can …" line, and retires after two weeks unless you edit it.
- Pulse Ideas: the first three ideas lead with no header, then Productivity, Financial Management, Shopping, Relationships and Health & Fitness. This order applies to the cabin too.
- Pulse Ideas (shell): rows with a clay icon. A click is Let's do it; the menu adds More like this and Not interested. Snooze is gone from ideas.
- Eval: the R6 technique A/B now measures each arm from the requests it actually sends (prefix shared with the previous request, tool-schema bytes per request, model calls per task, tool calls per model call) instead of fixed fixture usage, and the report gives a per-arm verdict from those numbers. Measurement idea from the Reason Agent 1.1 post (reasonmachines.com/blog/introducing-reason-agent-1-1). No defaults change.
- Shell on phones: the views bar sits at the bottom and slides away while you scroll down, then comes back when you scroll up, reach the top, or tap into a field.
- Shell: pulling a page past its top or bottom on a touch screen now stretches it with iOS-style resistance and snaps back on release (Chat, Pulse, Automations, Memory, Connectors, Settings). Reduced motion turns the stretch off.
- Shell Settings: on/off options (desktop control, close to tray, and the Local AI rules) are slider switches again instead of round checkmark bubbles.
- Docs: skill cues in AGENTS.md and CLAUDE.md for when to reach for coding skills.

- Linux: `grokhub-linux-v3.3.0.tar.gz` and AUR `pkgver=3.3.0`.
- Windows: `GrokHub-Setup-3.3.0.exe` and `grokhub-windows-v3.3.0.zip`.
- Shell (Tauri): `grokhub-shell-v3.3.0-linux-amd64.deb`, `grokhub-shell-v3.3.0-linux-amd64.AppImage`, `GrokHub-Shell-Setup-3.3.0.exe` and `grokhub-shell-v3.3.0-macos-arm64.dmg`.

## 3.2.0 — 2026-10-26

- Release: add the minisign public key (`packaging/minisign.pub`); the updater now verifies `SHA256SUMS.minisig`.
- Agents now squash-merge their own PRs into beta once CI is green; only promoting beta to main needs the owner. House rules switch from vstack to pstack.
- CI: `ci-guards` now fails closed when `git grep` or `git diff` errors, or when the Cargo.toml, Cargo.lock, or cli.rs version inputs cannot be parsed, instead of skipping those rules; added `scripts/test-ci-guards.sh` regression tests.
- CI: every GitHub Action in `ci.yml`, `release.yml` and `pii-guard.yml` is pinned to a full commit SHA with its tag in a comment. Dependabot moves the pins each week, and `scripts/ci-guards.sh` fails a workflow that uses a tag or branch.
- CODEOWNERS now also covers the harness access, approval, at-rest, audit, canary, park and path D modules.
- Connectors: when the model asks for a connector GrokHub doesn't run, the reply and status line now point to Settings → Connectors instead of "not wired. GitHub is the only live connector." `/connectors`, `/mcps` and the Skills page Connectors button all open that one page (stage 10).
- Settings → Permissions: the Calendar and Mail rows now say they aren't read yet instead of promising a reader GrokHub doesn't have, and hard-action cards, the privacy note, the Marketplace Google steps, Plan mode and goal/chip prompts no longer name Grok Build (stage 8).
- Docs: cross-platform recode plan (Tauri 2 + React + TS) and Phase 0 decisions in `docs/cross-platform/README.md`.
- Cross-platform Phase 1: new `grokhub-proto` crate (hub wire types, wasm-safe), iOS keychain feature fix, and a `cross-check` CI job that runs clippy for macOS, iOS, Android and wasm.
- Cross-platform recode, Phase 2: a Tauri 2 + React desktop shell (`src-tauri/`, `ui/`) next to the egui cabin. It has streaming chat, settings with the API key in the OS keychain, a tray, and a tauri-driver smoke test against a mock model. `cargo xtask bump` now also keeps the shell's lockfile in step.
- Cross-platform recode, Phase 3a: the Tauri shell's chat now runs the cabin's agent loop (`NativeEngine`) in Ask mode. Tools that change things (writes, commands) show a card with Allow once, Always allow and Deny, and nothing runs until you answer. Stop cancels a waiting tool. Settings has a work folder. The smoke test checks that a write waits for Allow once.
- Cross-platform recode, Phase 3b: the Tauri shell has an Automations page. It lists, adds (plain text such as "every weekday at 9am summarize my inbox"), pauses, resumes and removes the jobs in the same `automations.json` the cabin uses. Run now runs one job unattended in Ask mode: it can read, and anything that needs your OK is refused. The cabin keeps firing jobs on schedule.
- Cross-platform recode, Phase 3c: the Tauri shell has a Memory page. It shows and edits the global and project `MEMORY.md` notes the cabin uses, adds a line like `/remember`, and searches what chat recall would find. Secrets are redacted before anything is saved.
- Cross-platform recode, Phase 3d: the Tauri shell's Settings has an Approvals choice (Ask every time, Auto, Always approve), the cabin's three permission modes. Ask stays the default. In every mode, deleting, sending, paying and credential steps still wait for your OK.
- Cross-platform recode, Phase 3e: the Tauri shell has a Connectors page for the MCP servers in `mcp.json`. It checks them, reconnects one, signs one out, and disconnects one after you confirm. Signing in and adding connectors stay in the cabin for now.
- Cross-platform recode, Phase 3f: the Tauri shell has a Pulse page with the cabin's Feed and Ideas. Like, Snooze, Not this and Dismiss work and teach the ranking the same way the cabin does. Run puts an idea in the chat box for you to send.
- Cross-platform recode, Phase 3g: the Tauri shell's chat can control the desktop (screenshots, clicks, typing, apps) once you turn on Settings → Let Grok control the desktop. Deleting, sending, paying and credentials always wait for your OK, even when approvals are set to Always.
- Cross-platform recode, Phase 4a: the Tauri shell reports what each platform can run. On a phone it hides Pulse, Local AI, desktop control and the tray setting, and the backend refuses them too. Desktops keep every feature.
- Cross-platform recode, Phase 4b: the Tauri shell builds for Android and iOS. CI runs a smoke build on an Android emulator and an iOS simulator, and each one completes a chat with a mock model.
- Cross-platform recode, Phase 5a: the shell's UI also runs in a browser, as a remote for GrokHub on your computer. You pair it with a code (kept in memory only, never in browser storage). It offers chat with tool approvals, automations, memory and connectors, and shows Settings read-only. A Chromium smoke test against a mocked desktop runs in CI.
- Cross-platform recode, Phase 5b: Settings → Web access in the Tauri shell. It serves the web app on your network, with a fresh pairing code each time it's turned on. Web chats can't control the desktop, can't change settings or the API key, and only see their own conversations. Turning it off, or quitting, ends every pairing.
- Cross-platform recode, Phase 6: releases now carry the Tauri shell's bundles (Linux .deb and .AppImage, a Windows setup .exe, and an unsigned macOS .dmg), listed in the minisign-signed SHA256SUMS. `cargo xtask bump` keeps the shell's crate, bundle and lockfile versions in step with `VERSION`, and each changelog release section lists the shell's bundles. The shell now reports the same version as the cabin.
- Security: bump `zip` to 9 for the engine archive extractor (RUSTSEC-2026-0367/0368/0369) and update `ctutils`, `rustls-webpki` and `webpki-roots` (RUSTSEC-2026-0381/0382/0385).
- Moved the desktop-control tools (screenshots, clicks, typing, app launch, the hard gate's path A) out of the cabin into a new `grokhub-desktop` crate so the new shell can share them. The cabin behaves the same.
- Fix: docking leaves keep keyboard focus on the active leaf after a sidebar tab switch, split, or close, instead of leaving it on a stale leaf.
- Docs: agents squash-merge green PRs into beta; `.github/` and release-script edits no longer need a separate OK.
- Fix: one-tap fix guard matches whole words, so fixes like "Unblock upstream DNS" or "Download skill report" are no longer refused as host grants. Adds negative tests for findings cards (malformed input, non-card bodies, word-boundary non-matches).
- Fix: deferred `show_panel` rows for deleted chats are dropped and the queue is capped, so it no longer grows without bound.
- Journal: "To mission" starts a mission from an open decision. It opens a chat named for the question, with the next experiment drafted for you to send, and links the two so a second click goes back to that chat. The weekly systems brief now lists the Journal's open questions instead of saying no journal exists (stage 9).
- Local AI: on-device models can pair with a draft model for faster replies (speculative decoding). Gemma 4 E4B, 12B, 26B and 31B pair with Gemma 4 E2B, and Muse Glimmer with its official DFlash drafter; a model with built-in prediction heads uses those. Settings offers the draft model as its own download (never automatic, and it never replaces your model). A pair is only used when both tokenize the same, the engine supports it, both fit in memory, and the machine isn't busy, and Test speed keeps it only if at least 40% of its guesses are accepted and replies come at least 10% faster on your machine.
- Local AI: the on-device catalog is now Gemma 4 (E2B, E4B, 12B, 26B-A4B, 31B; Q4_K_M, Apache 2.0, each file SHA-256 pinned) instead of Qwen 2.5. The setup wizard suggests the default for this machine's tier, from its GPU memory, Apple unified memory or spare RAM. Qwen 2.5 models you already downloaded still load; GrokHub just no longer suggests them.
- Local AI: Meta's official Muse Glimmer 30B (with its DFlash drafter) is in the catalog as an optional pick for 16 GB+ machines. GrokHub never picks it for you, and it needs llama.cpp b10353 or newer. Community and abliterated builds are not listed.
- Local AI: a model picker scores every catalog model as Recommended, May be slow or Hidden with a plain reason, and pre-selects one. A CPU with AVX only gets Gemma 4 E2B on the AVX build the pinned engine already ships; a CPU without AVX keeps local AI off.
- Local AI: the on-device model now steps back while you play or work hard: a live governor checks the machine every 2 seconds (5 on battery) and throttles the local model for a game or fullscreen app, low memory, heat, power saver or battery, unloads it when the battery drops under 20% or the machine overheats, and gives it more room when you've been away for 5 minutes on AC power. It waits 5 checks before stepping down and 30 (and 2 minutes) before stepping back up, restarts the model only between requests, and stops retrying a model server that keeps crashing. Apps can be listed in `local/governor.json` (`allow` never counts as a game, `deny` always does; `throttleOnBattery` turns the battery rule off).
- Local AI: GrokHub now reads every GPU (NVIDIA, AMD, Intel Arc and iGPUs, Apple), CPU performance cores and SIMD, battery and thermal state, and sizes each on-device model launch to fit: partial GPU offload, MoE experts kept in RAM on smaller cards, context and KV cache that fit, threads from the performance cores, and a VRAM split across two or more GPUs.
- Local AI: when the model server runs out of memory, GrokHub retries with fewer GPU layers (up to three times), remembers what worked for that model and GPU, and otherwise names the next smaller model to try.
- Local AI: fixed the local model server failing to start on GPU machines: the pinned llama.cpp needs `--flash-attn on`, not a bare `--flash-attn`.
- Local AI: models already downloaded with LM Studio or Ollama, or in a folder you pick, can be found in place without copying them.
- Local AI: a hybrid laptop without `nvidia-smi` now finds its NVIDIA card through Vulkan, iGPU-only machines offload to the iGPU on Vulkan or SYCL, launch sizing works after a restart (not only on first-run setup), a Hyprland maximized window no longer counts as fullscreen, the governor can't stop the model server between its start and a request, and the crash breaker no longer reads as a missing install.
- Local AI: the router follows the resource governor. Grok stays primary; background work goes on-device only when the machine has room (heavy steps wait for idle time on AC), offline and cloud-outage work goes local unless the governor paused it, and then the step waits with a "Local paused: game or low power" chip. `/why local` shows the on-device model's state, and level changes are logged on this device only in `governor.jsonl` (7 days).
- Local AI fixes: an out-of-memory launch drops the draft model before giving up GPU layers, the room check counts the draft model's own cache, a draft model whose file can't be read is skipped, the draft speed test runs even when the machine is busy, and a hybrid laptop's discrete card found only through Vulkan now wins over its integrated GPU.
- Library-intent detection no longer fires on "is the patch installed"; the installed branch now needs an app/game noun or a named sealed app. Added word-boundary tests for short app names.
- Persist worker writes projects.json into the config dir the job was queued for, so a late write cannot land in another test's or session's dir.
- CI: ci-guards fails a raw `thread::spawn` or `fs::write` in the app persist paths (`persist.rs`, `projects.rs`, `dock_ui.rs`) unless the site is marked `// ci-guard: allow-raw-io <reason>` (rearch stage 6).
- Internal: secrets.json writes (Settings keys, OAuth refresh, sign-in and sign-out) now go through the single ordered persist worker, and an older token refresh can no longer overwrite a newer one (rearch stage 7).
- Saving: if the background save worker has stopped, chats, projects, and settings are now written directly instead of being dropped, and a panicked write is reported as "Could not save ..." without killing later saves.
- Personal-info guard CI check, and "user" as the only name in docs, tests and rules.
- Tests: the Settings run-count plot's bar mapping, axis labels, empty series and bounds are now covered.
- Lockfile: `clipboard-win` 5.4.1 (yanked) moves to 5.4.3.
- Tests: the install.sh package test no longer fails when the suite runs as root.
- CI: the release workflow's build jobs get a read-only token; only publish can write the release.
- Tests: split `app/tests.rs` into per-feature test files (rearch stage 5); the `tests.rs` line budget in ci-guards drops with each move.
- Shell: Settings has an Appearance section. Pick System, Dark or Light; it shows at once, is saved in `shell.json`, and is applied again at startup.
- Shell: a command palette opens with Ctrl+K (⌘K on a Mac) or the Command button for touch. It lists this platform's pages and Settings sections, filters as you type, and ends with Ask Grok, which puts the typed text in the chat box. The tabs now wrap on a narrow window.
- Shell: Settings has a Pulse section with Interests. Add a topic, then pin, mute or remove it, in the same `interests.json` the cabin's Pulse lookups read.
- Shell: a Journal tab shows the cabin's decision journal from the same `decisions.json`. Add a question, add options, lean to one, then Decide, Defer, Reopen or Abandon. To mission links the question to a chat and puts its first message in the chat box without sending it; a second press goes back to the same chat. The web app leaves the journal on the desktop.
- Shell: Settings has a Labs section with Plugins, labeled Preview. Install a bundle from a URL or folder, trust it, then turn it on or off, or fetch a marketplace list. It's the same plugin state as the cabin's Labs page.
- Desktop shell: Settings → Local AI. Auto (the default) shows the tier, model, state and reason, speed and memory, with switches for pausing while gaming, using idle time and slowing down on battery. Manual adds the model picker (Recommended, May be slow, and Show all for the rest, each with size, quant, context, speed, license and its reason), models already on this PC, "Use a model file" (a third-party or abliterated file shows a one-time notice first), device, draft model, GPU layers, context, threads, KV cache, idle unload, memory caps up to 90%, never-unload, app allow and deny lists, the speed test and Reset to Auto. The page says when hardware detection is limited (for example on Wayland), and the tray shows Local AI's state.
- Shell: the composer has the cabin's mode chips, each with a tooltip. Chat, Plan and btw pick the session: Plan and btw are read-only, and Plan shows Grok's plan on a card with Approve plan or Keep planning (approving goes back to Chat). Ask, Auto and Always set Settings' approvals (the web app only shows the computer's). Auto effort shows that GrokHub picks the effort. Grok's questions and connector forms now show as cards you can answer; a secret field is a password box and its value stays out of the chat.
- Shell: Settings → Permissions, Pulse → Interests and Labs keep the text box, chips and button together instead of a 240px gap.
- Shell and web app: the chat composer and Journal cards fit a phone-width screen, the conversation log takes keyboard focus, and muted interests keep readable contrast.
- Shell: Settings has a Permissions section. Add Allow, Ask or Deny command rules for every project, see a project's `.claude/settings.json` rules read only, and remember or forget Allow-always commands for this project. It uses the same files the cabin and the agent's approvals read.
- Shell: Settings has a Personality section. Pick the default voice for every chat, duplicate a preset, and edit or delete your own. It uses the same presets and `personalities/` files as the cabin, and chats pick it up from the next message.
- Shell: Settings has a Playbooks section. View or edit a playbook's TOML, duplicate one into your own, and reset an edited one or remove your copy. It uses the same `playbooks/` files as the cabin and the agent, and a refused save says why.
- Tauri shell: Settings has a section nav (General, Account, Local AI, Web access, About), and About shows the version. Phones get General, Account and About; the nav wraps into a row on narrow screens.
- Shell: with no API key, Chat opens on a sign-in card with one button (Add your API key, which opens Settings → Account) and a hint about where to get a key. The web app says to add the key on the computer. Automations and Connectors have clearer empty states, and Web access is labeled Preview.
- Internal: every slash command now lives in a `register_slash!` registry in grokhub-core instead of one big match, and every command that runs or unlocks something has a test proving it still goes through its confirm, consent or permission path (rearch stage 10).
- Fix: panel tables no longer drop cells when a row has more cells than the header row.
- Fix: identical status toasts (same kind and text) raised within a few seconds collapse into one counted toast with a fresh timer instead of stacking.
- Web access: the pairing code changes after 30 minutes with no browser using it. That ends every web session, and browsers pair again with the new code, which Settings shows.
- Web access: the Settings note says a paired browser can approve file changes and commands on this computer.
- xtask: `cargo xtask bump --dry-run <X.Y.Z>` lists the planned edits without writing; `bump` now computes every edit in memory first so a failure cannot leave a half-bumped tree, and is covered by temp-dir fixture tests.

- Linux: `grokhub-linux-v3.2.0.tar.gz` and AUR `pkgver=3.2.0`.
- Windows: `GrokHub-Setup-3.2.0.exe` and `grokhub-windows-v3.2.0.zip`.
- Shell (Tauri): `grokhub-shell-v3.2.0-linux-amd64.deb`, `grokhub-shell-v3.2.0-linux-amd64.AppImage`, `GrokHub-Shell-Setup-3.2.0.exe` and `grokhub-shell-v3.2.0-macos-arm64.dmg`.

## 3.1.0 — 2026-10-15

- UI: optional egui_dock main-panel tabs behind Settings → Dockable panels (U7f; default off; layout in dock.json; egui 0.36, no egui/eframe bump).
- Tests: shared `app/test_support_tests` module for QuietCabin, IsolatedConfig, and `wait_until` helpers (rearch stage 4; prepares feature test splits).
- UI: egui_plot bar chart for self-improvement runs-by-area (U7e; egui 0.36, no egui/eframe bump).
- CI: friction guards for conflict markers, god-file line budgets (`app/mod.rs`, `cards.rs`, `slash.rs`, `app/tests.rs`), and Cargo.lock edits that need the `dependencies` label or a Cargo.toml dep change (rearch stage 1).
- UI: egui-phosphor icons for sidebar, card tiles, and bar glyphs (U7c; egui 0.36, subset regular, no egui/eframe bump).
- UI: egui_extras TableBuilder for generative panel / compare tables (U7b; egui 0.36, no egui/eframe bump).
- Deps: bump rustls to 0.23.46 (RUSTSEC-2026-0346/0347/0348/0349; lockfile only, no advisory ignores).
- UI: egui-notify toasts for Saved, run-finished, and error status lines (U7a; egui 0.36, no egui/eframe bump).
- Nightly deep pass: `show_panel` drains into the owning chat (not the visible tab); Save-as-skill refuses to clobber Extracting/Draft; `/recall` and Settings archive copy match #712; `library_intent` / findings paraphrase / `skill_list` frontmatter honesty; host_safety `chr()` and `base64|sh` rails; deslop stale ui_kit/panels/journal/SP2 comments and archive docs.
- Eval: R6 offline technique A/B matrix (prefix cache, tool-output summaries, deferred tools, batched read-only, all-on) with cost/success metrics and adopt/spike/reject report in `docs/eval/r6-technique-ab.md`.
- Agent: background and unattended callers bind a per-job `ToolSet` (`schemas_for_job` / `dispatch_for_job`) and log schema size at job start (SP2).
- Watch-and-learn (SD2): after a screen recording, **Save as skill** runs the vision step extractor, shows the draft (hard steps flagged), and saves only on confirm; undo removes it; replay still re-asks hard approvals.
- Generative UI (G4): `show_panel` tool results with a `{"panel": …}` marker upsert into the chat `PanelStore` and paint inline under the turn (off by default via `generative_ui.toml`); panel buttons queue a `[panel action]` follow-up through the normal approval path.
- Agent: per-job `ToolSet` allowlists (`schemas_for_job` / `dispatch_for_job` / `tool_set_from_job`) so background and unattended jobs can send a lean tool schema list; deferred mode keeps self-manage schemas on the stable list.
- Watch-and-learn (SD1): pure core step extractor turns a screen-recording vision reply into ordered, classified steps and an unsaved skill draft (no network, save, or UI — SD2).
- Agent: optional batched read-only tool round (`batched_tools.toml`, off by default). When enabled, consecutive gate-readonly tool calls in one model reply run dispatch in parallel (up to four at a time) with results appended in original order; approvals, MCP, and write paths stay sequential.
- Chat composer: optional sentence autocomplete paints muted ghost text after your draft (off by default; Settings → Sentence autocomplete). Tab accepts when the slash menu is closed; Esc dismisses; click the ghost text to accept.
- Connectors: Marketplace installs reload MCP tool lists in chat immediately; stdio entries accept catalog `command`/`args` or a single argv array.
- Agent: optional `show_panel` native tool (`generative_ui.toml`, off by default). When enabled, the model can emit a validated G1 panel spec as tool output (`{"panel": …}`) with no render or side effects; the tool stays deferred like other rarely used natives when deferred tool schemas are on.
- Fixed: tray **Quit** tears down the cabin async runtime with a bounded `shutdown_timeout` after the UI loop returns, then exits from `main` so the tray icon is dropped before the last-resort `process::exit` (Windows ghost-icon case).
- Generative UI: phase 1 controlled panel renderer (CI status, build errors, compare table) with dismiss/pin store; off unless `generative_ui.toml` sets `enabled = true`.
- Chat composer attachment tray: multi-file picker, drag-drop, and paste share one ingest worker; thumbnails and errors show above the composer (up to 10 files).
- Screen recording and audio check report cards get a **Fix it** action that builds a harness fix plan from the diagnosis and runs it step by step through the existing hard approval cards (Phase C / SC2).
- Docs: SignPath Foundation privacy notice in [PRIVACY.md](PRIVACY.md) and [docs/CODE_SIGNING.md](docs/CODE_SIGNING.md); Windows Setup shows the statement before install (`packaging/windows/privacy.txt`).
- Chat shows a small connectivity chip (Online / Reconnecting / Offline (local engine)) above the composer from the cabin connectivity engine; ureq provider and xAI client transport failures nudge that state.
- Arch (AUR): optional `llama-cpp` dependency for a system `llama-server` instead of the in-app engine download.
- Composer quick chips: ui_kit spacing, radius, focus ring, and clearer insert-vs-action hints; draft chips insert into the composer without clearing attachments or auto-sending.
- Dreambeans daily drop: Brain 4 registers an overnight `daily_drop` local job that calls the core picker on ranked Pulse rows (off by default; no model or Google I/O). Settings → Pulse exposes opt-in toggles; the ledger and last run persist in `daily_drop.json`; picked cards show on the Feed.
- Agent: optional provider prompt-prefix cache (`prefix_cache.toml`, off by default). When enabled, the stable cabin system prompt stays fixed across turns; talk style, playbook, and memory snippets move into a `<cabin-volatile>` block on the user message instead of rewriting the system row.
- Connectors: Marketplace and Settings sign-in for installed MCP connectors — browser OAuth seals behind a credentials card; API keys seal in the OS keyring after the same card.
- Agent: optional compaction of large tool outputs (`tool_output_compact.toml`, off by default). When enabled, oversized tool results are summarized for the model with key facts plus a handle and workspace spool path so `read_file` can recover the full text; the stable system prompt is unchanged.
- Agent: optional deferred native tool schemas (`deferred_tools.toml`, off by default). When enabled, rarely used tools are listed in a compact index and the model calls `load_tool_schema` to pull full schemas into the volatile tail of the tools list so the stable prefix stays cache-friendly.
- Connectors: Marketplace **Refresh catalog** pulls display metadata from the official MCP registry (https allowlist + SSRF rail) for connectors already in the bundled list; install recipes stay bundled.
- Local AI: Settings → Cabin defaults → **Get engine** downloads a pinned official llama.cpp `llama-server` build for this OS/arch into `{config}/bin` (HTTPS to GitHub only, resumable, SHA-256 checked). No engine ships in installers; local AI stays off until you opt in.
- CI: ci-guards no longer shallow-fetches or warn-skips PR rules on a missing merge-base; `release` only unlocks Cargo.lock when grokhub-* versions change.
- Safety: CODEOWNERS also covers scripts/ci-guards.sh, .github/workflows/, xtask/, and .cursor/rules/.
- Agent: named sealed-app library asks use word/phrase boundaries; read_skill loads frontmatter-less skills while skill_list still filters them.
- Persist: PersistService worker catches panics per job so one bad write cannot kill the thread; surfaces via persist_err and keeps later jobs running.
- Tests: QuietCabin/release_isolated flush PersistService before removing config dirs; Projects/WindowCfg pin scheduled dir; quiet cabins use default_dock_state.
- Host receipts: after a command exits, drain stdout/stderr until the pump threads finish so late (or non-UTF-8) lines are not raced away under CI load.
- Nightly adaptive: PersistService surfaces FullSnap/projects/window save errors and retries after lock failure; side loop/suggestion/automation/feed writers take `persist_io`; dock.json save leaves the UI thread; History rail glyph distinct from Automations; Labs cursor-preview copy; CodeQL egui `salt` rename + test PROOF redact; deslop panels dead_code/ticket tags.
- Release: feature PRs add `changelog.d/` fragments; `cargo xtask bump` owns VERSION lockstep and folds notes on promotion (rearch stage 2).
- CI: CODEOWNERS for gate, perm, harness consent/egress/hard, and host_safety (rearch stage 3b; needs-human-review).
- CI: non-release PRs cannot touch version lockstep files without the `release` label (rearch stage 3a).
- Tests: move Composer/plus/attach Cursor-fold tests into `app/composer_attach_tests.rs` (rearch stage 5 pilot).
- Persist: one PersistService worker orders threads/projects/settings disk writes (rearch stage 6; secrets/OAuth still Stage 7).
- Agent tooling: always-apply `.cursor/rules/vstack.mdc` plus AGENTS.md/CLAUDE.md line: always use vstack skills (viper-mode first), never pstack.
- Agent tooling: vendor vstack skills (`242d4f9`) under `.cursor/skills/` so Cursor Cloud agents load them.
- Agent tooling: sync vendored vstack skills in `.cursor/skills/` to vstack `6f2fbd3` (tighter `viper-mode` and `setup-vstack` wording).

- Linux: `grokhub-linux-v3.1.0.tar.gz` and AUR `pkgver=3.1.0`.
- Windows: `GrokHub-Setup-3.1.0.exe` and `grokhub-windows-v3.1.0.zip`.

## 3.0.2 — 2026-10-13

- Security: MCP legacy SSE absolute `endpoint` URLs must stay on the connect host (no pivot to link-local / LAN / loopback). Custom provider base URLs and marketplace index URLs refuse non-public addresses and dial through `public_only_resolve` with redirects off (same rail as Pulse / `web_fetch`). `hub-state.json` writes use a user-only Windows DACL before bytes land (pair tokens), matching `secrets.json`.
- Fixed: Pulse **More like this** no longer clears the related-search queue when Hacker News, Reddit, and Web search are all off, and the status says to turn one on instead of claiming the ask is queued.
- Fixed: AMR `remember` publishes nodes via temp + exclusive hard_link/rename (with `sync_all`), so a trail file is never visible empty mid-write; the turn-end trail test waits for the body, not just `is_file()`.
- Fixed: proactive dismiss mute expiry in tests is driven from the dismiss wall clock (not `t0 + DAY + 2s`), and Pulse feed-instruction rewrite waits up to 30s so Windows CI load does not flake the mock rewrite.
- Pulse Feed thumbs decode cached images off the UI thread (same gate as Eyes / Imagine stills), so the first paint of a post with on-disk images no longer hitch on `fs::read` + JPEG/PNG decode.
- The main chat empty surface no longer shows the Home/event card deck; cards live in Pulse.
- Chat turns that ran harness tools show an expandable **Evidence** receipt under the reply: goal, plan status, actions, checks, changes, confidence, and recovery. Built from existing redacted spans and outcomes (no new log). A successful tool is not treated as proof the goal succeeded; missing sections say unavailable.
- Imagine UI finish: Agent More offers model and 1–10 image count (sprite stills, not an empty menu); image count is a ± stepper to the API’s 1–10 range; Results use ghost pills like the stage; a failed stage shows Retry; More sits with Style/Aspect instead of a lone second row; docs match Account-reuse auth.
- Mission-based OS v1: one foreground mission in the Work tree, projected from Brief, checklist todos, desktop episode, approval parks and outcomes (active / needs-approval / paused / blocked / done). Distinguishes planned, attempted, verified and unverified checklist rows; a failed step cannot look complete; Continue resumes without duplicating finished steps. No second run loop and no approval-policy change.
- Journal (`/journal`): a local-first decision backlog / experiment journal for open questions — options, evidence, next experiment, defer/decide/abandon, quiet dismissible due nudges, and promote to a Workboard Todo (Mission promotion waits on Mission-based OS). Stored in `decisions.json`.
- Added: weekly state-of-systems briefing. Sunday at dream time (same `NightPasses` slot as self-review) aggregates local outcomes, `/health` checks, automation health, Pulse digests, and dream What-changed into one Digest card; `/brief` regenerates on demand. Deterministic, local-first, no model call; every claim cites a source id. Missions/decision-journal convert actions wait until those surfaces exist in-app.
- Windows release tags can Authenticode-sign Setup and the portable zip through SignPath Foundation when `SIGNPATH_*` secrets/variables are set (no-op until then). See `docs/CODE_SIGNING.md`.
- Recovery-first runtime v1: a visible `RecoveryState` projection (Running / Retrying / Replanning / Needs approval / Needs input / Paused / Blocked / Recovered / Completed with gaps) over the existing harness ladder, parks and drive gaps. Ladder steps surface a calm status line with reason and next action; hard approvals stay hard and no parallel retry stack was added.
- Fixed: Pulse Feed **More like this** starts a source check immediately (HN / Reddit / Web search for the post’s words) instead of only queuing for the next scheduled check with no visible reaction. The button reads Looking…, the Feed shows the check banner, and related posts can land when the fetch finishes.
- Composer chips and the empty-home greeting paint from built-in local pools only (habit / stage / `local_greeting`); they no longer spend a fast-model ask on every refresh. Chat tab titles come from content words in recent user turns (`topics_from_turns`) instead of a background `GOAL:` summarize. Skill activation matches whole-phrase / `OR`-split triggers before bag-of-words Jaccard, so short junk asks do not false-activate.
- Pulse interest learning is a local connection graph, not “saw a word → whole identity.” Chat titles need mentions in two distinct chats before they become Interests; related topics get weighted edges; each source check may probe up to two neighbor queries from strong open edges; Save / More like this strengthens a link and Not interested closes it and branches to other neighbors. Domain-general (apps, music, tools, hobbies, …) — no game-only schema. Graph lives in `interests.json`.
- Fixed: Pulse web search no longer queries cabin jargon or weak tokens (the hidden “Background” chat, agent lanes like `background:triage`, Settings chrome, stopwords). Checks prefer sealed/strong interests; when nothing qualifies they skip Lite rather than invent a junk query.
- Auto baseline lanes (Cheap / Workhorse / Code / Flagship) reserve the plan's strongest reasoner for plan/design/debug/prepare/`repair:diagnose`; SuperGrok-shaped plans keep memory/judge/dream/chat on the coding mid and escalate to flagship only when attention says so. Seed catalog v2; rebuild refreshes on seed bump.
- Auto picks models with a built-in Rust logic system (no model-in-the-loop): capability catalog + baseline lanes + on-the-fly escalate/de-escalate from request signals, plus learned feedback from corrections/failures (`models/capability_catalog.json`, `models/attention_feedback.json`). Light tasks keep a cheap *task* model; host risk and hard asks escalate; `repair:diagnose` always takes the strongest reasoning model. Settings "How Auto picks" shows baselines and notes that turns can escalate.
- Pulse can search the open web for free (Settings → Pulse → Sources → Web search). Each check queries DuckDuckGo Lite for your top three interests; results post with the result site as the source name so diversity and novelty caps still apply. No API key. The paid “X and web search (xAI)” row stays off.
- Security: Pulse feed and image fetches no longer use `HTTP(S)_PROXY` from the environment. An env proxy would dial the proxy host and skip the public-only DNS check on the target URL; they now match `web_fetch` and resolve the target directly.
- Fixed: a stalled repair/research run's fallback findings card no longer offers a "Finish the rest" one-tap when the open todos ask to grant workspace/OS access or activate a skill (the same doom-loop #720 already refused on `report_findings`). Stored cards drop those pills when painted.
- Security: host rails (`forbidden_reason`) also catch common shell/Python obfuscations that rebuild forbidden path leaves without spelling them — assignment expansion (`s=ssh; cat ~/.$s/…`), quoted `+` concatenation, and `$(echo …)` / `` `echo …` `` splits — so Always/Auto shell cannot read `~/.ssh` that way.
- Fixed: sealed Installed-app learnings (and other scope facts) reach the model on gaming / Steam / library asks — first turn and follow-ups — even when Memory is still on the legacy backend, so the cabin no longer claims "no recorded info" while Settings shows Learned N things. Steam is matched as a whole word so coding asks about `upstream` / `downstream` do not pull the Installed-app pack.
- Fixed: cabin skills under the settings `skills/` folder (the same ones `skill_list` shows) load with the `skill` tool; names normalize so `GamingProfile` and `steamlibreview` resolve instead of `unknown skill`.
- Fixed: the system prompt no longer claims every chat is a permanent read-only phase; it says what tools are available, to prefer sealed Installed-app Memory over `~/.steam` workspace walks, and not to invent OS-permission "fixes".
- Fixed: `report_findings` one-tap pills that ask to grant workspace/OS path access or "activate" a skill are refused, so path-restriction and skill-activation doom-loops cannot post.

- Linux: `grokhub-linux-v3.0.2.tar.gz` and AUR `pkgver=3.0.2`.
- Windows: `GrokHub-Setup-3.0.2.exe` and `grokhub-windows-v3.0.2.zip`.

## 3.0.1 — 2026-10-12

- Fixed: park answers write atomically and ignore empty or partial answer files, so an Approve click is not read as Deny (Windows CUA flake on `deny_sends_nothing_approve_forwards_once_and_the_password_slot_is_never_typed`).
- Fixed: Settings → Behavior auto-archive copy matches #712 (idle chats leave History and search; still on disk; no Archived drawer promised).
- Security: Pulse feed and image fetches refuse CGNAT and other non-public literal IPs the same way `web_fetch` does, and dial through the shared public-only DNS resolver so a hostname that resolves to a private or link-local address cannot connect.
- Settings → Cabin defaults and Permissions no longer re-read `providers.json` (or rebuild the provider model pick list) on every paint: the cabin keeps a cache that add / remove / allow / revoke clear. Settings → Background only schedules its 1 s countdown tick while a job is due in under a minute; waiting and minute+ rows rely on the normal heartbeat.
- Pulse diversifies what it follows from what you like. A check posts at most two stories from one source (so Hacker News cannot fill the whole run), searches your strongest interest every time and rotates the other topic slots so weaker interests still get a look, and gives a small score bump to under-seen sources. Save or More like this twice on posts from the same subreddit, GitHub repo or YouTube channel adds that place to Settings → Pulse → Sources when that source kind is on.
- Code is now rustfmt-clean and CI checks formatting.
- Fixed: source-window tests match rustfmt-wrapped needles (`session_row`, `Margin::symmetric`, `feed_prompt`) and the composer Pill scan window is 15,000 bytes.
- Fixed: the "Archived N idle chats" composer row and the History sidebar Archived section no longer appear; archived chats stay out of History lists and search. Archive / Unarchive, auto-archive, and Undo remain.

- Linux: `grokhub-linux-v3.0.1.tar.gz` and AUR `pkgver=3.0.1`.
- Windows: `GrokHub-Setup-3.0.1.exe` and `grokhub-windows-v3.0.1.zip`.

## 3.0.0 — 2026-10-12

- Beta builds are labeled X.Y.Z-beta.N and compare correctly against releases. GitHub tags `vX.Y.Z-beta.N` publish as pre-releases (never Latest); stable `vX.Y.Z` tags are unchanged.
- Dependency diet: `Cargo.lock` package count **604 → 601** (dropped unused `tray-icon` libxdo/X11 deps; trimmed `winresource`/`x11rb`/`globset`/`toml`/`regex` features; workspace-shared `sha2`/`ureq`/`image`/`keyring`/`rusqlite`). No feature loss; image codec smoke covers png/jpeg/webp/ico/bmp.
- Folder-level AGENTS.md and CLAUDE.md files are picked up when the agent first works in that folder, not only the project root.
- Short UI repaint timers (palette search, Pulse tab crossfade, Permissions keyring wait, folder picker) only run while something is actually busy or animating, so an idle cabin is not woken every few dozen milliseconds.
- Background and unattended jobs use a slimmer system prompt: they keep the cabin safety rules and AGENTS layers, and drop the skills list and plugin extras unless the job text declares `NEED_SKILLS` or `NEED_PLUGINS`. Chat is unchanged; the nightly review still uses its lean review prompt.
- Archive finished chats so they leave the main History list (still on disk; restorable while the chat is open): Archive / Unarchive from the row menu, chat header menu and Ctrl+Shift+A; opening works; a reply restores the chat with a quiet note; toasts name the chat and offer Undo. *(3.0.1 #712 later dropped the Archived sidebar drawer and searchable archived hits — idle chats leave History and search.)* Idle chats auto-archive after 14 days (Settings → Behavior, off switch), skipping pinned chats and chats with a running or parked task or background job, on app start and once a day.
- Settings → Behavior no longer reads `self_review.json` (or the change-version ledger) on every paint: the cabin keeps a cache that writers update, so the Settings page stays off the disk while it is open.
- Added: GrokHub keeps a copy of each file before the agent edits it; `/rewind step` lists the agent's steps and puts any file back as it was.
- Fixed: channel switch no longer force-moves a local `main`/`beta` that is ahead of origin (fast-forward or refuse); Update recover resets to `origin/main`/`origin/beta` (not an old release tag); backup commits ignore GPG signing.
- Fixed: long chats and tasks no longer stop with a context error; GrokHub compacts again whenever the context fills and retries once after a context-overflow reply, keeping the latest steps word for word.
- Auto router picks the strongest reasoning model on your plan for real work and keeps background jobs on the fast tier, with a reason for every pick.
- The router knows the on-device model's strengths: bench results fill its capability metadata (context, eval pass rate, tokens/s) without claiming tool-calling (the on-device path still returns no tool calls), a failed verify on a local answer escalates to a cloud model with a quiet note naming both, hard or long work stays off local when online, and when every plan model is down an eligible step can fall back to local instead of pausing forever (including after a verify escalate that finds no healthy cloud route).
- Updater recovers diverged or dirty installs (backup branch), stops prompting sudo on every update, and the Beta switch works on any clone.
- Fixed: when a model reply is cut off at its length limit, GrokHub no longer runs the half-written tool calls in it; the agent is told to resend smaller steps.
- Security: text from web pages, connectors, screen reading and research queries is now marked as outside data (`<<<UNTRUSTED …>>>` fences with a per-run nonce) before the agent sees it, so instructions hidden in a page can't pose as you or the system. Workspace file reads stay plain for coding.
- `grokhub bench local` (and Settings → Local speed → Test speed) measures the on-device model: first-token latency, tokens/s, load time, RAM/VRAM peaks, plus a small eval (chat, summarize, classify, a two-step tool turn). CI checks the report schema and eval pass rate against a fake server so regressions fail without a GPU.
- The on-device model picks hardware-aware llama-server defaults: GPU layers when VRAM fits, thread count from physical cores, an 8k–16k context when memory allows (instead of a fixed 4k), flash-attention and q8 KV cache on capable GPUs, prompt-cache reuse (`--cache-prompt`), and a 10-minute idle unload that frees RAM/VRAM until the next local call. Setup also reads AMD/DRM VRAM on Linux (`/sys/class/drm/*/device/mem_info_vram_total`) and DXGI/CIM adapter memory on Windows when `nvidia-smi` is missing.
- When the network is down, chat and on-device background work can use the local model instead of waiting on the cloud: a connectivity state (`Online` / `Degraded` / `Offline`) steers the router, the status chip can say "Offline · on … (local)", and cloud-only steps such as image generation wait with a named note until reconnect. Prefer-local while online and the full OS network hooks are still follow-up.
- The on-device model can now run for real: GrokHub starts a llama.cpp `llama-server` on 127.0.0.1 only (random port and a bearer token), health-checks it, and talks to it over the usual chat API for classify, summarize, difficulty and streaming chat. It looks for the server binary under the settings folder (or `GROKHUB_LLAMA_SERVER`) and a `.gguf` from `~/GrokHub/models` / `installed.json` (or `GROKHUB_LOCAL_MODEL`). Offline routing and the status chip come in follow-up work; without those files the local toggle still does nothing.
- A chat turn now checks its work before it ends. After GrokHub changes a file, runs a command that changes something, or clicks on the desktop, it may not finish until it has checked the result (a test or build run, a re-read of the file, a look at the command's effect, a screenshot); otherwise it carries on with "Not verified since your last change to <file>". A careful or critical task, repair or research turn must also cover each item of its Brief's done list in its checklist (for example services, packages, boot errors and ports for a system repair), with a reminder every eight steps while any are uncovered, and a critical system change needs a backup first. There is no step cap: a turn that makes no progress after three re-plans ends and names what is still open.
- Release builds now use thin LTO, one codegen unit and stripped symbols, so the Linux cabin binary is about a third smaller (roughly 61 MB down to 42 MB on this machine). Debug builds keep only line tables for faster links. A size ceiling in `packaging/release-binary-size-ceiling` fails the Windows release CI job and the Linux release bundle script if the binary grows more than about 15% past that measured size.
- The Brief's policy goal now steers model effort for that chat turn. A quality goal starts each step one level higher than its class would (never past High or the class ceiling), speed one lower, cost at the class floor, and balanced leaves it alone, always inside the class's band and without asking you anything. Your own words ("think hard", "keep it quick") still win, and the usual step up after a tool error and step back down after clean work still apply. The route log keeps the goal and its reason, and `/why` ends each line with it, for example "Goal: quality (mistakes can break the system)."
- Chat and background turns now build a cache-friendly system prompt: the cabin safety rules, AGENTS/CLAUDE.md layers, skills list and plugin extras stay in a stable prefix, and the personality, tuned notes and learned brief sit in a volatile tail closed by a short reminder that the earlier cabin safety rules still win. Skill discovery and AGENTS layers are memoized by path and mtime (including nested `SKILL.md` files) so an unchanged tree is not re-walked every turn. Debug builds log `prompt_compose_ms` for the local build; provider cache hits stay in Usage.cached_tokens.
- Labs Beta now works without a source clone, Windows included: turning it on downloads the newest beta pre-release from GitHub, checks it against its `SHA256SUMS`, swaps it in, and only then saves the channel; turning it off does the same with the latest stable release. Update on Beta without a clone takes the newest beta build (by build time, not by the order GitHub lists them). A refused download names why and leaves the channel as it was. Building beta from a Linux clone stays the developer path.
- Update without a source clone (the Windows Setup install, and now a Linux install from the release tarball) downloads this computer's build from the latest GitHub release, checks it against the release's `SHA256SUMS` (and its minisign signature once the signing key ships with the app), unpacks it, and swaps `grokhub` and `grokhub-hub` in, keeping the old ones as `.old` and putting them back if any step fails. A download that doesn't check out is not installed, and the update card says why, for example "Update 2.13.2 not installed: the download's checksum didn't match". Source clones still pull and build as before.
- Workboard cards now use a Flo Hoeller project-card look: soft pill shape, segmented progress ring by status, a status chip (To do / In progress / Blocked / Done / Follow up), and overlapping presence chips for chat, notes and automation. Open cards keep the same chat and notes; only the compact card chrome changed.
- Releases now carry `SHA256SUMS` with the SHA-256 of every Linux and Windows file, signed with minisign (`SHA256SUMS.minisig`) from a key kept only in GitHub's secret store; the public key will live at `packaging/minisign.pub`. Each push to `beta` also publishes the same files as a GitHub pre-release (`beta-v<version>-<sha>`), so the beta channel can move to signed builds. `docs/REFERENCE.md` shows how to check a download by hand.
- Each Brief now names the row it matches in the new mode policy table, for example "Policy: repair × critical, goal quality (mistakes can break the system)", shown in `/why` and the trail. The table ships in GrokHub (chat, question, task, repair, research and background rows from the Brain design, plus defaults) and maps each kind of ask and care level to a model goal and reason, depth, ask vs act, stop rule and talk style. Copy it to `brain/brain_policy.toml` in the settings folder to change a row or add one. A file that names an approval, permission or other safety key, or that fails its checks, is ignored whole and `/why` says why; approvals are never in the table. Nothing acts on the table yet.
- Before each chat turn GrokHub now writes a short Brief of the ask: what kind it is (chat, question, task, repair, creative, research or background), how careful the run must be (casual, careful or critical) and what "done" means, for example "repair × critical, done = services, packages, boot errors, ports". Plain rules decide it; anything that touches services, packages, boot, ports or permissions is always critical. Only when the rules can't tell does it make one small fast-model call. `/why` shows the newest Brief first and the turn's trail carries it. The Brief keeps only those fixed words, never your text, in `brain/briefs.jsonl` in the settings folder, and it does not change replies yet.
- The weekly self-review can now describe up to two changes to GrokHub's own code that would have avoided the week's failures, each as a Pulse card with the problem and the evidence. GrokHub never makes these changes. Save idea keeps it in Settings → Behavior; Copy as GitHub issue puts it on the clipboard as an issue body with home folders shown as `~`, email addresses as `[email]` and keys removed, for you to paste yourself. Nothing is filed or sent. Settings → Behavior → Self-improvement history lists every change the review made (trying, kept, rolled back, ended or reverted, with its diff and numbers), and Revert puts back a kept skill or chat note that is still the newest change on it.
- The weekly self-review may now suggest one chat note a week alongside its skill fixes, for example "Lead with the answer, then one line of why." Apply saves it to `tunable/` in the settings folder and tries it first in one chat in five; GrokHub keeps it only if those chats take fewer steps, and otherwise removes it on its own with a Home note. Notes go before the cabin's safety and approval rules, which always win. A guard now refuses any suggestion aimed at approvals, the hard-action list, MindCheck, permissions, network access, consent, the honesty rules, the trial evaluator or GrokHub's own code, and logs each refusal. Nothing it refuses is ever written, and the review sends nothing anywhere except its one model call.
- A skill change GrokHub makes from its weekly review now runs as a trial instead of asking you to Undo or Keep it. One run in five that uses the skill follows the new steps and the rest follow the old ones (each run always lands on the same side). After ten runs on the new steps, or a week, GrokHub keeps the change if it took fewer steps without failing or being undone more often, and otherwise puts the old steps back on its own. A Home note names the change and the numbers, for example "Kept: Wait for the save in weekly-report, 40% fewer steps over 10 runs". Each version, its parent, a diff stat and the result are kept in `changes/changes.jsonl` in the settings folder, and `/skills undo` still steps a kept change back.
- GrokHub keeps local metrics for every finished run in `metrics/runs.jsonl` in the settings folder: whether it worked and passed its verify, how many steps it took and how long, its tokens and a cost estimate in dollars, user corrections and undos, recovery steps and replans, the model it used and its task class (the skill it ran or the topic of the ask). A later correction or undo updates the run's record. Runs older than Settings → Memory retention are dropped. Nothing leaves your computer; this is the measuring step of the bounded self-improvement loop.
- Pulse source posts have Save, Not interested and More like this. Save keeps the post in Saved Finds (Settings → Pulse, newest first, each opens its link, with Remove) and raises the interest it was for. Not interested quietly takes the post down and lowers its interest and its source. More like this raises both and searches Hacker News and Reddit for the post's own words on the next check. Settings → Pulse → Sources shows "X and web search (xAI, paid)" switched off: Pulse uses free sources only for now.
- Pulse checks its sources every 90 minutes and once when GrokHub opens (unless it checked in the last ten minutes), never in quiet hours. Settings → Pulse → Check every picks 1 h, 90 min, 2 h, 3 h or 4 h. Sources are fetched and ranked on your computer; only each check's top five new posts go to Grok's fast model, in one call, for a two or three sentence summary, and a check with nothing new calls no model and says "I looked and did not find a source worth your time." Settings shows today's model calls against a budget of 16 a day; past it, or when Grok isn't connected, posts keep the source's own text. The Feed now keeps up to 120 live posts (was 40), and Home shows up to three digest posts (was two).
- Pulse ranks what its sources find. Each item is scored by how strongly it matches your interests (a pinned topic counts most), how fresh it is, and the source's own signal (Hacker News points, Reddit score); weak matches don't post, and the best ones post first. A story already on your Feed under another link or a reworded title isn't posted again. Each source post now says why it's there, for example "Why: Rust".
- Pulse reads free sources. Settings → Pulse → Sources turns each one on or off: Hacker News and Reddit search your top three interests, and you can follow subreddits, GitHub repos' releases, YouTube channels and any RSS or Atom feed (paste a name, URL or channel id). No keys and no paid search. Each new item becomes a Feed post with its own title, the source's name (for example "r/rust" or "rust-lang/rust on GitHub"), a short summary from the source, its image when it has one, and a link to it. Posts about a muted interest, items older than a week and anything already posted never show, so a dismissed post stays gone. Sources are checked on the feed clock and never in quiet hours, or with Check now; replies are cached with ETags, and a failed source shows in Settings with its reason while its last items still count.
- Pulse has an interest profile. Settings → Pulse lists the topics it looks for, each marked From memory, From chats, From saved or Added by you. Add a topic, Pin it to keep it first, Mute it so the feed never covers it, or Remove it (it stays gone). On the first feed lookup GrokHub seeds the list once from lines like "Interests: Rust, air compressors and xAI" in USER.md or MEMORY.md, liked posts and the five newest short chat titles. Opening, liking or accepting a post raises the topics its title names; Dismiss and Not this lower them, and a pinned topic never drops. Every feed lookup now names the strongest unmuted topics and the muted ones. The profile is `interests.json` in the settings folder.
- GrokHub has 20 personalities besides its own voice: Genie, Pirate, Hype Coach, Sarcastic Sidekick, Noir Detective, Surfer, Shakespearean Bard, Butler, Professor, Drill Sergeant (lite), Chief Engineer, Lawyer-precise, Zen Mentor, Concise Pro, Friendly Neighbor, Southern Mechanic, Space Mission Control, Retro 80s Hacker, Gentle Listener and Viper (explicit: vulgar, roasts you, still gives the real answer). A personality changes how chat and `/bg` replies sound and nothing else: the cabin's approval, safety and honesty rules come after it in the prompt and always win, and night jobs keep the plain voice. The presets are data (`personalities.toml`). Pick one in Settings → Personality: each card shows its description and a sample line, Use makes it the default, and Try it asks the fast model for a two-sentence preview in that voice. The name in the title bar switches the open chat's personality for that chat only; Make default there saves it for every chat. Duplicate makes your own copy of any preset in `personalities/` in the settings folder; edit its name, description, tone rules, emoji, length, what it calls you and its voice, then Save (checked first), Reset to drop unsaved edits, or Delete (asks once). Spoken replies use the personality's voice when it names one xAI has (ara, eve, leo, rex, sal), else Ara.
- Fixed: a damaged settings, keys, Home feed, hub or other GrokHub file no longer resets without a word. GrokHub still keeps a copy beside it (`app.json.corrupt-<time>`) and loads defaults, and now also posts one Home card naming the file and the copy, for example "Settings file was unreadable" with "Restored defaults, kept a copy at ~/.config/GrokHub/app.json.corrupt-1760160000000." A memory note that has stopped parsing is moved aside the same way instead of being skipped on every recall.
- Fixed: the LAN hub answers a request body that isn't JSON with a 400 and "Request body isn't valid JSON." Before, it read it as an empty body and answered with the wrong error, such as "Pairing code does not match." A request that crashed while holding the hub's state no longer makes every later request drop its connection.
- Fixed: on KDE Plasma, the System appearance follows the Plasma color scheme. It reads `kdeglobals` (window color first, then the scheme name), so Breeze Light no longer shows the cabin dark.
- Fixed: on KDE Wayland, `focus_window` and `set_window_geometry` say "No window title has … in it." when no window matched. Before, they reported success. The KWin script now calls back over D-Bus with what it found.
- Fixed: on KDE Wayland, the desktop tools can list windows through KWin. The safety gate now knows the focused window, so Enter in a chat app or Delete in Dolphin is treated as a hard action there too.
- Fixed: on Windows, Eyes, the live room and replay take screenshots again. They use the same screen grabber as the desktop tools; before, every frame failed because they looked for Linux capture tools.
- Fixed: on Windows, COMPUTER_CMD click, double-click, move, type, key and scroll drive the desktop through SendInput. Before, they tried xdotool and replied with Linux install steps.
- Fixed: Paste an image works on Windows (the PNG or bitmap on the clipboard) and on KDE Wayland, where `wl-paste` now gets the Wayland socket that startup drops. Before, both said to pick a file or copy something first.
- Fixed: on Windows, Devices now finds the LAN address by reading the network adapters, picking the home or office network over VPN and virtual adapters the same way Linux does. Before, it ran a Linux-only `hostname -I` that always failed and fell back to whichever address the default route used.
- The cabin opens faster: on a fresh Linux profile the first frame now paints in about 97 ms instead of 175 ms (median of 5 cold starts). Global hotkeys are grabbed right after the first frame instead of before it (a hidden `--hidden` start still grabs them at once), and the clock and theme probes run while the window is created. Each launch adds a line to `startup.jsonl` in the config folder with the time each stage was reached (newest 20 kept).
- Background work now runs from one scheduler. Automations and loops, the Pulse refresh and digest, ideas, anticipate, proactive cards, the nightly review, self-review and dream, memory retention, idle reflect, the indexers and the model list refresh each register once with a priority. Every 15 s pulse runs one queue of the jobs that are due, highest priority first. The scheduler also handles quiet hours and Halt, and gives each job that spends tokens its own daily budget. When a job uses up its budget, it stops until tomorrow and the status line names it. The budgets can be changed under `"background": {"dailyTokens": {...}}` in `app.json`.
- Settings → Playbooks is new: a playbook is a short TOML recipe for how the agent works through a kind of task, with phases that go look-only, then safe and reversible, then risky, plus a backup, a check, a way back and when to give up. GrokHub ships one (Any task) and you can view, edit, duplicate, reset or remove playbooks there; yours live in `playbooks/` in the settings folder. Save refuses a playbook that runs a command GrokHub never runs (for example `mkfs`), marks a delete, send, payment, sign-in or irreversible system step as anything but risky, or marks a changing command look-only, and says which step and why. Risky steps still ask you first; a playbook never changes an approval, and nothing runs playbooks yet.
- Settings → Playbooks now ships Repair Linux (written for CachyOS and Arch with KDE Plasma) and Repair Windows. Each looks first with GrokHub's read-only repair checks (services, boot or event-log errors, packages or app updates, disk, memory, network), takes a restore point and keeps a .bak before a change, restarts before it reinstalls, and asks you before anything bigger such as reinstalling packages, installing updates, repairing system files or deleting files. A playbook step now can't mark a cache cleanup or package removal as safe (the same rules as repair fixes), and a playbook's give-up reasons must be real blockers (a refused approval, something missing, every approach failed), never "it's taking long".
- How GrokHub talks now fits the kind of ask. Casual chat stays short, a long task shows a brief "Done: … Next: …" note between steps (at most one every three tool calls), and a careful repair or research run ends with a findings card that names each item, with a one-tap fix for what is left (built from its checklist if the run gets stuck before posting one). A personality still changes only the tone, and approval cards read the same in every style.
- Settings → Playbooks now also ships Code changes (read the code and run the tests before editing, one small edit at a time, build and test after each), Files (list what will move first, move in batches, removals go to the Trash), Research (several sources, the original when there is one, a link for each claim) and Desktop apps (a screenshot before each click and after it). Each turn's Brief now names the playbook it picked from its intent, domain and this computer's OS, for example "Playbook: Repair Linux", on the trail and in `/why`. Asks about clicking in an app or window now get the new desktop domain.
- Background work now steps aside while you work: when you're typing or a chat is running, heavy jobs (automations and loops, anticipate turns, self-review and dream, the indexers) wait and start as soon as you go idle, and light jobs keep going on the cheaper model. Settings → Background lists every job with its next run, today's tokens against its daily budget, and whether it's waiting for you, for quiet hours, or for tomorrow.
- A turn's system prompt now carries the playbook its Brief picked: the phases in order (look, safe, risky) with their steps, how to back up, check and undo, and when to stop. A later turn swaps it for its own playbook or drops it for small talk, and risky steps still ask first.
- Each talk style now has its own length: casual chat under 60 words, answers under 150, a long task's wrap-up under 120, the reply after a findings card under 80, research summaries under 200 (links don't count) and background notes under 40. The style's length wins over a personality's own length line, so a chatty voice still keeps small talk short. The trail and `/why` show the turn's style, for example "Talk: findings card, reply under 80 words".
- Each run's line in `metrics/runs.jsonl` now records its Brief (intent, care and domain), the policy row it matched (for example "repair × critical") and the playbook it picked, and policy rows and playbook steps are now areas the self-improvement loop may tune (the weekly review starts proposing them in a follow-up): a tweak runs as a trial like a skill change, kept only when matching runs do better and put back otherwise, with a Home note such as "Kept: Linux repair checks ports before services, 22% fewer steps over 9 runs". It never lowers a critical row or a system playbook's care, never removes a system playbook's back-up or re-check steps, never touches approvals, the hard-action list, the honesty rules or the trial evaluator, and your own edits to `brain/brain_policy.toml` and your playbooks are kept as versions in the change history and never written over. Nothing leaves your computer.
- The Sunday self-review now proposes one policy-row or playbook tweak a week when a row or playbook had at least 5 runs and many of them failed (or, for a playbook, few passed their re-check); the tweak runs as a trial behind the same locks and is kept or put back by its numbers, and quiet mode only logs it. Settings → Behavior's self-improvement history also shows one line per area (system, code, files, desktop, web, media, general) with its runs, how many worked, average steps and how many passed a re-check.

- Linux: `grokhub-linux-v3.0.0.tar.gz` and AUR `pkgver=3.0.0`.
- Windows: `GrokHub-Setup-3.0.0.exe` and `grokhub-windows-v3.0.0.zip`.

## 2.13.1 — 2026-10-10

- Fixed: an unfinished checklist from an earlier request no longer takes over the next, unrelated message in the same chat. A checklist drives a reply only after that reply works on it. Fixed: when the first send of a chat carried over from the CLI days failed, its recap of the earlier turns was lost; it now goes out with the retry.
- GrokHub no longer needs, installs or updates the Grok Build CLI. The legacy CLI engine and its Labs switch are gone, along with the `grokhub-acp` crate. The Linux installer, release tarball, PKGBUILD, AUR package and Windows Setup stop installing `grok`, and Windows Setup no longer adds `~/.grok\bin` to PATH. Update, `/update` and `grokhub --update` update only the cabin, and the titlebar chip only says Update cabin. `/rewind`, and `/undo` once a reply has finished, now say the native session is append-only. `/worktree` says worktrees were a CLI feature. A CI test fails if code outside the Google Marketplace check looks for the CLI, or if packaging installs it again. An existing CLI install is left alone, and Google's Marketplace connectors still use it when it is there.

- Linux: `grokhub-linux-v2.13.1.tar.gz` and AUR `pkgver=2.13.1`.
- Windows: `GrokHub-Setup-2.13.1.exe` and `grokhub-windows-v2.13.1.zip`.

## 2.13.0 — 2026-10-10

- Fixed: on Linux, an agent command could read the saved sudo password by running GrokHub's password helper itself (for example `sudo -u "$USER" "$SUDO_ASKPASS"`). The helper now answers only when the pipe it writes to is read by `sudo` alone, never by GrokHub's tool output or another program, and agent commands that name the helper are refused before they run.
- Signing in no longer touches the Grok Build CLI. GrokHub never reads, refreshes or writes the CLI's login (`~/.grok/auth.json`); Connect Super Grok and `grokhub --oauth` keep the sign-in in GrokHub only, and chips, greetings, Imagine and chats use GrokHub's sign-in or your API key. Get Started shows GrokHub's own sign-in sheet even when the CLI is logged in, and never waits on a CLI install. Without a sign-in, chat shows "Sign in with Grok in Settings → Account, or add an API key." with a Settings button. The model list no longer asks Grok Build for its models. The legacy CLI engine keeps its old behavior until it is removed.
- Desktop control no longer goes through the Grok Build CLI. GrokHub stops adding its desktop, Cua and self-management tools to Grok Build at startup and when the desktop switch changes; the switch now just says "Desktop control is on/off". GrokHub drives the screen itself, and on Linux with the Cua Driver flag on, GrokHub runs its own Cua gate as one of its MCP servers, with every action checked by the same rules as before.
- GrokHub now runs every chat, background job, night job and slash command on its own engine, which talks to xAI with GrokHub's sign-in or your API key. The Grok Build CLI is no longer needed to chat. A chat from before keeps its messages, and its next reply gets a short recap of the earlier turns so the conversation carries on. Settings → Labs → "Legacy Grok Build CLI engine" brings back the old path for now; it goes away when the CLI code is removed.
- In the Marketplace, Google Drive, Gmail and Google Calendar now say "Uses your Grok Build CLI" when Grok Build is installed. Their detail view then shows Grok Build's own entry and its status, plus how to sign in to Google there. Without Grok Build they keep the manual Google Cloud steps and an Install button. GrokHub only checks whether the CLI is there; it never installs or runs it for this.
- Settings → Connectors has a Marketplace tab: 26 popular MCP connectors (GitHub, Context7, Playwright, Notion, Linear, Supabase, Stripe, Gmail and more) with search, category pills and a detail view that says what each one can do, what it can reach, how it signs in and where its source is. Install adds it to this cabin's connectors in one click with nothing to approve, and a browser sign-in opens right after for connectors that use one. A connector that needs an API key gets a masked key field; Save key asks first, and the key is sealed with your keychain key and never written into the connector's entry. Disconnect or `/connections` undo removes an install.
- Connectors now live in Settings → Connectors. Each connector is a row with a status chip (Connected, Needs sign-in, Error with the error named, Off or Not checked), its account when known, and Connect, Reconnect, Sign out, Turn on/off or Disconnect. Disconnect always asks first; for this cabin's own servers it removes the entry and deletes its sign-in and saved token, and `/connections` can bring the entry back. GitHub, hooks and plugins moved there too. The Skills page keeps workflows and skills and has a Connectors button; `/connectors`, `/mcps`, `/hooks` and the rail's old Connectors link open the new page.
- The eval suite has a new `system-scan` item that replays the 2026-10-09 beta scan against a fake host (failed nextdns, systemd-resolved on port 53, 6 packages with missing files, boot errors). It passes only when Always Allow shows no permission card, an early "it's resolved" is sent back to the open checklist items, every item ends covered, and the findings card names systemd-resolved and offers the 4 fixes. It runs in CI with the workspace tests; on Windows it reports skipped because the fake host needs a Unix shell.
- Hard classes now see past leading variable assignments: `PATH=/x sudo reboot` or `LANG=C dd of=/dev/sda` is classified by its real command, so Always Allow still stops for it.
- Browser sign-in (OpenRouter, MCP servers) no longer misses the callback when the browser sends the request in pieces.
- A scan, diagnosis or review now ends with a findings card under the reply. It lists what Grok found, most severe first, each naming the service, package or error source, with a button for each one-tap fix (for example "Fix port 53 conflict" or "Reinstall 6 packages"). Tapping one starts that fix in the same chat, still asking for anything that needs your approval. Dismiss hides the buttons and keeps the report.
- A chat or shell run no longer ends when the model says it's done while parts of the task are left. For a task with several parts, Grok writes a checklist first, and GrokHub sends it back to the open items until each one is checked with a real result or marked blocked with a reason. Saying "continue" picks the same checklist up. If Grok is truly stuck, the run ends saying which items are still open.
- On Linux, `sudo` in GrokHub's commands now asks for your password once per session, in a system password dialog (KDE's own on Plasma, else zenity, qarma or yad), never as a GrokHub permission card. GrokHub keeps the password in memory only until it closes and never writes it to disk. A wrong password asks again, and Cancel makes `sudo` steps fail straight away for 5 minutes instead of asking over and over. Only GrokHub's own helper, started by a real `sudo`, gets the password; a program run by hand gets nothing. Commands no longer wait on a terminal nobody can see.

- Linux: `grokhub-linux-v2.13.0.tar.gz` and AUR `pkgver=2.13.0`.
- Windows: `GrokHub-Setup-2.13.0.exe` and `grokhub-windows-v2.13.0.zip`.

## 2.12.1 — 2026-10-09

- Always Allow now means it: GrokHub only stops for money, sending, deleting your data, credentials and irreversible system changes. Admin work such as `sudo journalctl`, `sudo pacman -Qk`, `sudo pacman -S amd-ucode` or `systemctl restart nextdns` runs without a permission card, and so do commands GrokHub can't split and your own ask rules. Partitioning a disk, installing or changing the bootloader, rebuilding the initramfs and writing into `/boot` or the EFI partition now count as irreversible and always show the white approval card, while listing them (`fdisk -l`, `parted -l`, `bootctl status`, `efibootmgr -v`) stays a normal step. Reading your keyrings or password store is a credentials step. Ask mode keeps asking as before, deny rules still win, and nothing runs this way while nobody is there.
- When no model in your plan answers, GrokHub no longer stops and waits for you. It keeps checking on its own (after 15 s, 30 s, 1 m and 2 m, then every 5 minutes, with no limit) and resumes the paused chat turn as soon as a model answers, with one quiet note: "Back on grok-4.6; resumed: Summarize inbox". While it waits, Home shows an information card naming the models and the step ("Waiting for a model: grok-4.7 … “Summarize inbox” picks up on its own once one answers.") with a Retry now button and nothing to approve. It only ever tries models in your plan, so waiting never spends money. The budget pause is unchanged and still waits for you.
- Every card on your feed now names the thing it's about. A card that can't name its item isn't posted at all, so you won't see a bare "Automation finished" or "Pick that job back up?". The router's cards name the model or step: "Paused: grok-4.7 isn't answering", "Your plan no longer includes grok-4.3", "Auto now thinks harder on everyday chat". A paused job's idea no longer says "That job is still paused". On the Ideas board, situation and router suggestions keep their own title instead of reading "I can paused: …". Crash, screen-recording and audio-check cards no longer say "Your automation … finished". Settings → Permissions now says calendar and mail are read through Grok Build's Google Calendar and Gmail connectors.

- Linux: `grokhub-linux-v2.12.1.tar.gz` and AUR `pkgver=2.12.1`.
- Windows: `GrokHub-Setup-2.12.1.exe` and `grokhub-windows-v2.12.1.zip`.

## 2.12.0 — 2026-10-09

- When GrokHub isn't sure you'd want something done on its own, it now shows a one-tap suggestion that names the item ("Tidy Downloads: move 14 installers to ~/Downloads/old") with a Do it button, instead of asking "Should I …?". Do it runs it once through the normal checks. Dismiss is quiet: no prompt, the topic stays off your feed for the rest of the day, and GrokHub counts it as a mild no. "Don't do this again" now also means it won't suggest it again. Sending, paying, deleting, credentials and irreversible system steps still wait on the white approval card.
- Computer use asks you before it sends, pays, deletes or ends your session by keyboard, not just by clicking. Pressing Enter in a chat app (Slack, Discord, Teams, Messages…), Ctrl+Enter or Alt+S in a mail window, or Enter on a checkout page shows a hard approval card naming the key and the window, and typed text that ends in a new line counts as Enter. Buttons like Place your order, Check out, Forward, Move to Trash, Show password, Uninstall, Shut down and Log out are hard too, and a click on a control GrokHub can't name parks on a checkout page or a message being written. This now covers GrokHub's own desktop tools in chat as well as the desktop server and the Cua driver.
- A desktop session that had to recover now writes one short lesson when it ends: what failed, what got it past that, and which app it was in ("Gedit: click 'Save' changed nothing 3× → then key 'ctrl+s'"). Lessons are written with no extra model call and stay on your machine under `lessons/`. A run that went smoothly writes none. Each night at dream time GrokHub merges duplicate lessons and drops ones older than 90 days. Loading lessons into the next session comes in a later update.
- A new desktop session now starts with up to three past lessons that fit its goal, at the top of what the model sees as a short "Past lessons" block (about 400 tokens at most). Lessons from the same app and with the same goal words come first. A lesson that came before a session that still didn't finish ranks lower next time. Each lesson a session used is named in its trace ("Used lesson: Gedit: …").
- A failed screenshot heals itself. GrokHub tries the next capture backend in the same step (KDE: KWin ScreenShot2, then Spectacle, then the Screenshot portal; wlroots: grim, then the Screenshot portal) and remembers the one that worked for the rest of the session. One backend failing no longer pauses the session or raises a new approval card. Only when every backend fails twice does it pause once, with one note naming each backend's error ("Can't capture the screen: KWin ScreenShot2: …; portal Screenshot: timed out.").
- A send, delete or other hard step you don't answer within 5 minutes no longer ends a desktop session. That one step is not run and its white card stays up; the session goes on with the parts of the task that don't need it, then waits with a note naming the step ("Waiting on you: “click Send”. Approve its card and I'll run it once and go on."). Approve later and it runs once; Deny and it never runs. The session no longer ends after 10 idle minutes while a step waits, and Halt still denies it.

## 2.11.0 — 2026-10-09

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

- Linux: `grokhub-linux-v2.11.0.tar.gz` and AUR `pkgver=2.11.0`.
- Windows: `GrokHub-Setup-2.11.0.exe` and `grokhub-windows-v2.11.0.zip`.

## 2.10.99 — 2026-10-09

- Long tasks no longer stop at 60 steps, 30 minutes or 50 turns: a desktop session, a chat, `/bg` and night jobs run until they're done, you Stop or Halt. A run that's stuck or repeating itself is told to re-plan and keeps going instead of pausing, and approving a card after the reply ended now picks the task back up instead of waiting for you to type.
- Bug hunt (`docs/audits/bug-hunt-2026-10-08.md`). The hard-action check no longer misses a delete or shutdown after a newline, inside `$(...)` or backticks, or behind `env`, `time`, `nice`, `timeout`, `pkexec` or `busybox`, and it now catches MCP tools named like `reply`, `forward`, `sendMessage`, `send_sms`, `post_tweet`, `create_charge`, `buy`, `transfer` or `drop_table`. A one-time approval no longer covers a longer command that contains it. Step logs no longer keep `Authorization` headers, `x-api-key`, access or refresh tokens or client secrets. Long host output, the nightly review and the step log no longer crash on accented or CJK text at the size cap. A permissions file saved with a BOM keeps its Deny rules, and a file that can't be read is never overwritten. Screenshots work again on GNOME with a non-ASCII Pictures folder. On Windows, the maximize button restores again. Quit from the tray no longer hangs on a busy MCP server, and a clean exit no longer leaves a pid file that can stop the next launch. A failed settings or key save now says so instead of "Saved".
- Dream time is a setting now (Settings → Behavior → Dream time). Dream, the nightly review and the Sunday self-review run at the hour you pick instead of always at 9 PM, and moving it after tonight's run doesn't run them twice.
- Settings → Cabin defaults has an "On-device model for background tasks" switch (off by default). Turned on, background work like dream, digests and summaries can run on your own machine while chat stays on your cloud model, and it takes effect without a restart. Until a local model is installed the row says "No local model installed" and background work keeps using the cloud.
- A short setup wizard greets a fresh install: welcome, two app settings (close to tray, dream time), then an on-device model step that checks your GPU, memory and free disk and says which model fits, how big the download is and where it will go. Skip any step; it won't open on its own again, and Settings → Behavior → Setup (or Set up next to the on-device model switch) brings it back. Downloading the model comes in a later update.
- The setup wizard's on-device model step can now download the suggested model (Qwen 2.5 from Hugging Face) into `~/GrokHub/models`, with a progress bar, Pause, Cancel and Resume (it picks up where it stopped, even after a restart), and a checksum check before it counts as installed. GrokHub can't run the model yet, so background work stays on your cloud model and Settings says so.
- Unattended Grok Build runs now block a delete or shutdown written in capitals (`REMOVE-ITEM`, `DEL`), called by full path (`/bin/rm`, `C:\Windows\System32\shutdown.exe`) or behind `cmd /c`, `powershell -Command` or `bash -c`, and they refuse encoded PowerShell (`-EncodedCommand`, `-enc`) outright; elsewhere encoded PowerShell asks first. Moving files to the Recycle Bin on a network or removable drive now says on the card that they will be deleted for good.
- Cleanup, no behavior change: removed 29 functions and constants nothing called, folded duplicate text-clip, `now_ms` and time-ago helpers into one copy each, dropped the unused `base64` dependency, and fixed two broken links in `docs/REFERENCE.md`.

- Linux: `grokhub-linux-v2.10.99.tar.gz` and AUR `pkgver=2.10.99`.
- Windows: `GrokHub-Setup-2.10.99.exe` and `grokhub-windows-v2.10.99.zip`.

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

Windows gate fixes (Spike-1W, code review only; the live Windows run is still to do). Ctrl+Alt+Delete and the other session-ending keys park in any modifier order or spelling (`Alt+Ctrl+Del`), a PowerShell delete behind `-ExecutionPolicy Bypass` parks, and `Stop-Computer`, `Restart-Computer`, `Format-Volume`, `Clear-Disk`, `format` and `Clear-RecycleBin` park as hard actions on every path, unattended runs included. No UI changes.

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

The bot rules go beta first: every new feature and fix PR targets `beta`, `beta` moves to `main` only with the maintainer's approval as a squash-merged PR, and hotfixes go to `main` with the maintainer's approval and are merged back into `beta`. PRs into `beta` don't bump the version; each promotion to `main` carries one bump and one release. The rules are in `CLAUDE.md`, `.cursor/rules`, and a new `CONTRIBUTING.md`.

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

The bot rules drop done-and-green auto-ship: bots now get a PR green and ready to merge, with proof of work, and stop. Nothing is merged, tagged, or released, Claude's and Cursor's PRs included, without the maintainer's explicit approval, and merging is squash and merge. The latest-`main` rule stays.

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

Repo rules for new Cursor and Claude chats. `CLAUDE.md` and `.cursor/rules/repo-gates.mdc` write down the real gates: branch off `main` and never push to it, no secrets or credential files, the exact CI test and clippy commands, the version-bump file list, the 12,000-byte composer source-scan window, no merges without the maintainer's OK, and no tag or release without a "full ship". `.cursor/rules/merge-prs.mdc` now agrees with them: no merges, tags or releases unless the maintainer says so. `.gitignore` also covers `target/` anywhere, key and certificate files (`*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.crt`, `id_rsa*`), and `auth.json`, `secrets.json` and `credentials.json`, while still allowing `.env.example`. No tracked file is ignored. The working tree and full git history were scanned for committed secrets (gitleaks 8.30.1 plus targeted greps). Nothing is in the current tree. One historical finding was reported separately for a rotation decision, and history was not rewritten. The app itself is unchanged.

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
