# GrokHub

Native Rust cabin. No Electron. No Tauri. One repo, one `main`, one version — two ship artifacts.

**v2.10.25** — Accept on an idea files a workboard Todo from the feed and from Ideas. Digest and idea cards share `updates.json` and do not use the event cap of 4. The user dismisses an idea; Housekeep expires it at about two weeks. One brief box steers the digest. Quiet hours hold digest, idea, and finished-loop visibility without blocking `poll_grok_loop`. Minimize, close-to-tray, and the next launch open a fresh empty chat; the previous transcript and pin stay in History. The feed stays hidden when nothing is undismissed. Cabin History is the cabin's own chats through headless `grok -p`, not `grok sessions list`. Creating a project does not wipe those chats. Background jobs such as workboard summarize stay off History and do not paint on the open chat. Leaving a chat does not stop them, or a live reply. Leave a project chat and come back: the same chat opens. Pins and transcripts stay. A project click does not open the Workboard. A second prompt stays on the same History row. A new row is only a new chat. Copy and Reply under a short user bubble stay inside the window. Clicking Plan switches the session to Plan and leaves the thread title and History row alone. Quick-chip × stays in its reserved slot: hovering it no longer flashes the pointer, and a click dismisses that chip. Compact, Copy session, and Export sit in the titlebar menu beside minimize. The context usage bar stays. Signed-in empty home drops the Coding / Life chip and the under-greeting workboard summary. That slot is an update feed (`updates.json`): newest first, hidden when nothing is undismissed. A finished `/loop` posts `automation_done` from `poll_grok_loop`. Saving a schedule posts `schedule_created`. Cards stay until opened or dismissed. Right-click a History chat to Pin, Unpin, or Rename. Pinned chats stay in a block at the top, last pinned first, and a blank rename keeps the previous title. Pin and title live on the chat in `threads.json` and survive a project filter. Workboards is a rail row under Skills and Connectors: Todo / Doing / Blocked / Done cards in `workboard.json`. A run start files a Doing card for that chat; the run finishing moves it to Done. Open chat on a card switches to the linked thread and leaves Workboards on the rail. A project is a folder of persistent chats. Click it to filter History and file new chats there; click it again for every chat. Delete puts those chats back in History and does not wipe transcripts. A project click does not open the Workboard. btw is the side-ask session pill (saved as ask) and does not stop a live run. View plan reopens a Plan-mode plan. Fork shows only on a long thread or when context is half full, with a one-time explainer. Quick chips are one fixed-height line with an ellipsis, a fluid count that drops overflow, and they stay up mid-conversation. Settings → **Cabin defaults** pins the chat model (Auto saves empty), reasoning effort, Ask or Auto, and Chat / Plan / btw in `app.json` for headless `grok -p`. Always stays on the composer. **Always collapse** starts thoughts folded everywhere; collapsing one thought folds that session and expand opens one at a time. Thought process is one quiet collapse control (chevron, lower contrast). Hide is gone. Collapse still toggles the thought body. Skills and Connectors cards in a row share one height, clamp the description to three lines with a word-boundary ellipsis, and pin Use in chat to the card bottom. Accepting a Suggested automation always drops that tile from store + UI. Quiet daily Suggested Automations and Skills come from prior session history (night/review path, persist). Automations drops Follow along / Teach this once. Loop and scheduled titles wrap. New chats index into History immediately and Windows re-lists on a tighter watch so rows do not lag forever. Selecting a cabin skill follows into grok -p / ACP (`Follow skill {name}` matches; the kick prepends the steps). Skills Suggested tiles from the nightly review Add via `save_skill`. Connectors owns the GitHub PAT plus read-only Who am I / List repos (`run_connector` only — no writes, no other websites). `/workflow` `/compact` `/rewind` honor the PermissionMode pill (Ask fail-closed if ACP is down). Chat bubbles keep inner pad (no left/top clip); transcript sits flush beside the rail; usage and suggestion chips are one fixed-height line with an ellipsis. Session pill **btw** (saved as ask; a live run keeps going). Shared confirm sheet for Ask Always, session Always, and destructive host (title + consequence + Confirm/Run + Cancel). Titlebar Quiet until X while Behavior quiet hours are on. History Last you / fork map. Device glance only when hub share or a last frame is bound. Signed-in empty home update feed under the greeting when a card is undismissed; the slot hides when the feed is empty. Ask-card Always confirms session skip and that night / loop / phone inherit `--always-approve` until quit. btw (saved as ask) is a side ask: a live run keeps going, then the question sends look-safe (`grok -p --permission-mode default`, no desktop-do-the-work rules). Idle btw sends that same look-safe ask. One down-arrow jumps to latest (Last you is right-click or hold). Quick chips pad inside one fixed-height line and stay up mid-chat. The btw pill keeps id `ask` with 8px inset. Night / inbox / anticipate inherit `scheduled_args` like loops — Ask is fail-closed, no ACP, and the night slot marks ran after a live kick. Always idle matches Ask/Auto. Selected Always is a 2px amber stroke only (no yellow wash or text). Cabin hi-fi: three surface layers, 1px borders, Fluent 16/20 icons, Inter 16/13/12, session row quieter than permission. Settings → **Update** stays visible for a manual check. The titlebar chip still hides until something is newer. A click when the probe found nothing still overlays CLI then cabin. Cabin UI pass: primary empty-home chip, session pill **btw**, voice Listening / Speaking / Ready. Empty ranking shows one muted **Nothing queued**. Grok Build **1.0.38** alpha. Interactive Ask starts ACP so Allow / Deny can show; if the agent is down the turn is denied (no `grok -p` fallthrough). Auto/Always stay on `grok -p`. Skills, Automations, and the Imagine wall keep hover inside the slot. A long user bubble wraps inside the row and keeps its leading gap. A thought starts expanded; collapse leaves one short row that opens again; that fold survives the live-to-stored handoff; the reply stays. Account sets a display name and a local profile picture; the avatar menu, the rail, and the connected hint hide the email. Default chat model is **grok-4.7**. Effort is None / Minimal / Low / Medium / High / Extra High; a saved Max loads as Extra High. Composer Stop is a disc with a small rounded mark. Idle Stop and the idle mic sit still; they ease while hovered, pressed, listening, speaking, or a reply is running. The transcript Running row has no Stop. The changing status text above the composer is gone. The context usage bar stays. Shared buttons hover-scale to 1.035 over 120ms, shrink on press, and scale plus fill on keyboard focus (same paint on Linux and Windows). Off-screen chat rows skip paint; row height follows the pane width and each row keeps a stable id. Voice is Ara; Hey Grok is PTT STT then TTS of the reply body (not thinking). A live Listening / Speaking / Ready strip with Stop sits above the composer. The strip stays up while Listening or Speaking and auto-hides about a second after Ready. Composer Stop / grok -p errors / Consult re-arm listen; failed STT shows Ready, not Listening. Chat shows a green live dot plus Thinking / Running / Waiting on the turn (hover is the current action). That line does not sit above the composer. The Ask card names the command, path, or site; live secrets stay redacted. Naming a schedule teaches that routine on Automations and leaves the rewind snapshot out. History is the cabin's own chats on Linux and Windows. First launch and reinstall install CLI alpha when grok is missing; wait / Get Started paint as the only pane and show live OAuth / device-code failures (not leftover wall or install status). Titlebar chip says **Update CLI**, **Update cabin**, or **Update CLI and cabin** when something is newer (checked every 2 hours, in-app). Settings → **Update** stays visible. One Update runs only what is pending, or both when that click finds nothing: `grok update --alpha` first when a newer alpha exists, then the cabin. Linux PATH prepends `~/.grok/bin` and `~/.local/bin`. Windows leftover clones (not `main`) use the GitHub zip, same as Setup with no clone. Imagine videos play in-pane. Avatar menu is Settings / Help / Connect. Account sets a display name and a local profile picture kept in cabin config. The avatar menu, the rail, and the connected hint do not show the email. Empty-home greeting and chips rank from the situation. Windows taskbar/tray icon and tray Quit. Settings is OAuth Account, quiet-hours dropdown, overlay Update. Chat rail reuses one empty draft (no New chat button). `/update` runs `grok update --alpha` on Linux and Windows. History is the cabin chat list.

| Platform | Artifact | Latest |
|----------|----------|--------|
| **Linux** (Arch / CachyOS) | `grokhub-linux-v2.10.25.tar.gz`, AUR | **v2.10.25** |
| **Windows** (x86_64) | `GrokHub-Setup-2.10.25.exe`, `grokhub-windows-v2.10.25.zip` | **v2.10.25** |
| **Android** | [Grok-Hub-Android](https://github.com/blackviperxiii-ui/Grok-Hub-Android) | key-fob — pair, task, JPEG |

Windows vs Linux in the cabin is `cfg(windows)` / `cfg(unix)`. The older [GrokHub-Windows](https://github.com/blackviperxiii-ui/GrokHub-Windows) fork is an archive source — new cabin work lands here.

## Run

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 ffmpeg alsa-utils
rustup default stable
git clone https://github.com/blackviperxiii-ui/GrokHub.git
cd GrokHub
cargo test --workspace
./scripts/install.sh --user
grokhub
grok --version
```

Or without installing:

```bash
cargo run -p grokhub-app
cargo run -p grokhub-app -- --agent
cargo run -p grokhub-app -- --hub
cargo run -p grokhub-app -- --doctor
GROKHUB_HUB_PORT=18766 cargo run -p grokhub-hub
```

The tray icon is there from launch. Close / titlebar × hides the cabin — the window unmaps and stays unmapped until it loses focus (then a pinned taskbar click, tray **Show cabin**, or a second `grokhub` raises it). It does not minimize to the taskbar. Drag the titlebar body to move the undecorated window. Size and position come back on the next launch. Jobs, hub, and idle reflect keep running. Tray: **Show cabin**, **Halt**, **Quit**. The first hide shows one “Still running in the tray” toast and writes `closeToTrayTipSeen` in `app.json`. Later hides, including after relaunch, do not toast. A first hide during quiet hours does not set that flag, so the next hide outside quiet hours can still tip once. The cabin status line still says it is in the tray. `grokhub --agent` starts already hidden. `GROKHUB_TRAY=0` quits on close.

`./scripts/install.sh --user` installs [Grok Build](https://x.ai/cli) **alpha** (`GROK_CHANNEL=alpha`, `grok`) next to the cabin. First launch Get Started connects Super Grok and signs in `grok` too. Chat is headless **`grok -p --output-format streaming-json`** (Grok Build 1.0.21+) with `--sandbox off` and a desktop rule so Grok uses this machine. Ask/Allow/Deny is ACP (`grok agent stdio`) when the permission pill is Ask. MCP elicitation (form or URL) uses the same ACP session. Night, loops, inbox, and phone `/v1/task` stay on `grok -p` and inherit that same PermissionMode pill via `scheduled_args` (Ask is fail-closed, no ACP; Auto is `--permission-mode auto`; Always is `--always-approve`). btw (saved as ask) is a side ask: a live run keeps going, then the question sends look-safe on `grok -p` (`--permission-mode default`, no desktop-do-the-work rules). Idle btw sends that same look-safe ask. Bound project is `--cwd`; unbound uses `~/GrokHub-Work` (never the cabin process cwd). Overlay `/update` runs only what is newer: `grok update --alpha` when a newer alpha exists, then the cabin overlay when the cabin is newer. If `grok` is on stable after an upgrade, the cabin switches it back to alpha and does not yank a working alpha install. `/context` and `/usage` show Grok Build server tokens (including reasoning); `/usage` also totals today's cabin spend and, when a session is attached, `grok usage <id>` (1.0.14 per-turn cost). `/compact` and `/rewind` talk to Grok. Halt kills the `grok -p` child (`session/cancel` on ACP). Truncated replies and transient 5xx retries stay on the same turn. Credit-limit errors offer Try Again.

Slash: `/help` · `/new` · `/scratch` · `/clear` · `/undo` · `/retry` · `/stop` · `/sh` · `/host` · `/plan` · `/always-approve` · `/sessions` · `/inspect` · `/project` · `/memory` · `/recall` · `/forget` · `/board` · `/imagine` · `/skill` · `/compact` · `/learn` · `/update` · `/send` · `/sync` · `/hub` · `/inhabit` · `/rewind` · `/room` · `/export` · `/rename` · `/pin` · `/delete` · `/effort` · `/dream` · `/palette`. Type `/help` in the cabin for the rest. `/skill <name>` runs that skill. Skills and Connectors lists **Cabin skills** (`~/.config/GrokHub/skills`) next to the Grok Build catalog, with the run count and Use in chat, plus nightly **Suggested** tiles that Add via `save_skill`. Connectors holds the GitHub PAT and read-only Who am I / List repos tiles. `/compact` keeps the last 8 visible turns and, with `/workflow` and `/rewind`, honors the PermissionMode pill. `/context` counts visible turns. `/scratch` blocks `/forget` and Memory Save. `/rewind` restores the bound project root (or Grok conversation rewind when mapped). `/sync` merges chats and memory with paired computers. `/project` also takes `bind`, `new`, `folder`, `rename`, `move`, `delete`, `clear`. Right-click a sidebar project to rename or remove it — Delete drops the row, not the files.

Composer session pills: **Chat** / **Plan** / **btw**. Permission: **Ask** / **Auto** / **Always**. Hover a pill for a short tip. Both pills are remembered across launches — Always-approve is the exception and resets to Ask, same as the leftover `yolo` reset. On a permission card, **Enter** allows and **Esc** denies while the composer is empty; a half-typed follow-up still sends on Enter. Interactive Ask starts ACP (`grok agent stdio`) so Allow / Deny can show; if ACP cannot start the turn is denied (no `grok -p --sandbox off` fallthrough). btw stays look-safe on Auto/Always (`--permission-mode default`, no desktop-do-the-work) and does not stop a live run. Auto/Always map to `--permission-mode auto` / `--always-approve`. Effort dropdown: **None** / **Minimal** / **Low** / **Medium** / **High** / **Extra High** (`grok -p --reasoning-effort`). `/effort` sets the same. A saved Max effort loads as Extra High. Default model is **Grok 4.7** (`grok -p --model grok-4.7`). Greeting and chips use `grok-4.7` through `grok login`.

The **Automations** page holds both schedulers. **Scheduled** is the cabin's own clock list (`automations.json`) — pause, **Run** now, or **Remove** a job, and each row shows its schedule, next run and run count. **Loops** is the Grok Build `/loop` interval list. **New job** takes either shape: `/loop 30m check deploy` or `every weekday at 9, summarize the board`.

The left-rail **Chat** button is the new-chat control — there is no separate New chat row. Click Chat on an empty draft (no dialogue) to pull that same chat up. After a reply has started, Chat opens a new draft. One empty draft at a time. Old convos stay in sidebar History (the cabin's own chats).

Projects sit in the left rail. `+` makes a project (`~/GrokHub-Work/<slug>`) or a one-level folder. Double-click or right-click to rename (display name only — the path stays). Right-click a project to add it to a folder or remove it. Folders are sidebar only; they do not move files. Click a project to bind it and filter History to that folder, and to reopen the project chat you left. Creating a project does not wipe History. New chats filed there stay on disk. Click the project again, while you are already in its chat, for every chat. Delete puts those chats back in History and does not wipe transcripts. A project click does not open the Workboard. Bound tree is the world.

History search runs as you type across SOUL/USER/MEMORY and every chat; a new query drops the previous needle's hits so a late walk cannot open the wrong thread. A hit is a door — click a memory line to open that file in the editor, a chat line to open that thread. Re-opening the memory file already in the editor keeps unsaved typing. Below the search, History is the cabin's own chats (newest used, pins on top). Headless `grok -p` keeps the session on that chat. Background runs such as “summarize the workboard” are not rows. Right-click **Delete** or the History-page Delete button removes the cabin chat and its Grok Build session. Transcripts stay on the chat until you delete it. The cabin stays on Grok Build CLI **alpha** (`grok update --alpha`). It does not switch off alpha.

Imagine stills use dedicated **`grok-imagine-image-2.0`** (falls back to `grok-imagine-image` on timeout). Video kind calls **`grok-imagine-video-1.5`**. Auth is `grok login` first, then a console key / cabin OAuth. Hey Grok: push-to-talk STT into chat, then TTS of the reply body (not thinking). Same on Linux and Windows. Desktop control is **Grok Build computer-use** — the cabin keeps tool cards, diffs, and computer-use frames in a collapsed Work tree in chat. No Desk / Take over menu. Halt / Stop / tray Halt / Ctrl+Shift+Esc SIGTERM the `grok -p` child. Stream buffers clip at `IMAGE_FILE_CAP` / `TEXT_FILE_CAP`. Desk frames drop above `FRAME_CAP`. Titlebar × unmaps to tray. Plus-button stills ride `--prompt-json` image blocks.

Settings → **Account** is Super Grok device-code OAuth only — **Connect** / **Sign out** (or `grokhub --oauth`). That also writes `~/.grok/auth.json` when the Grok Build CLI is not already connected. Tokens live in `~/.config/GrokHub/secrets.json` (mode 0600; Windows user-only DACL), never in markdown. Settings → Appearance is **Dark**, **Light**, or **System**. Settings → Behavior holds close-to-tray, the living wall, and one **quiet hours** dropdown (Off / common windows). Picking a window saves it. Settings → **Cabin defaults** pins the chat model, reasoning effort, Ask or Auto, and Chat / Plan / btw in `app.json`, plus **Always collapse**. Auto model saves an empty pin. Always is not written from that page. Close-to-tray still hides the cabin; the desktop tip is once, and only when quiet hours allow it. GitHub PAT is not a Settings page — the connector owns that.

Windows Setup and first cabin launch (also Linux tarball / AUR first launch) **automatically** install Grok Build CLI **alpha** (`GROK_CHANNEL=alpha` / `https://x.ai/cli/alpha`) when `grok` is missing or unusable. Reinstall does the same. A working alpha install is left alone. The Install control is hidden when grok is already present or an alpha install is already running. A leftover `grok.exe` that cannot start (missing DLL) shows one cabin error, not a looping Windows dialog.

Settings → **Update** is **Install Grok Build CLI** only when grok is still missing after the automatic install failed, then one **Update** control (always visible), progress, and **Restart** after a clean cabin overlay. The cabin checks every 2 hours (and at launch) for a newer Grok Build CLI alpha (`https://x.ai/cli/alpha` vs `grok --version`) and a newer GitHub Latest cabin. The titlebar chip is the notice (in-app — not a web page) and says **Update CLI**, **Update cabin**, or **Update CLI and cabin**. The Settings button stays up when the probe found nothing so you can still overlay. Acting on it runs only what is pending: `grok update --alpha` first, then the cabin. A click when nothing was detected still overlays both. A working alpha install is updated only when a newer alpha exists. It does not switch the CLI to stable. `grokhub --update` / `/update` use that same rule. When the cabin is newer it retargets a leftover Origin or `GrokHub-Windows` clone to GitHub (`https://github.com/blackviperxiii-ui/GrokHub.git`), then `git pull --ff-only origin main`. Linux overlays with `./scripts/install.sh --user`. If a leftover `grok` is on stable, overlay / first launch switches it to alpha. Windows Setup users do not need a clone: when the cabin is newer, Update downloads the latest `grokhub-windows-v*.zip` from GitHub into `%LOCALAPPDATA%\Programs\GrokHub` (PATH includes `%USERPROFILE%\.grok\bin` for `grok update --alpha`). A leftover Windows clone (wrong branch, detached HEAD) uses that same zip. A Windows source clone on `main` still overlays with `scripts/install-windows.ps1`. Linux overlay / Update CLI prepends `$HOME/.grok/bin:$HOME/.local/bin` so `grok update --alpha` finds the official install. The clone path is not a Settings field — empty uses `GROKHUB_SRC` or the install receipt. After a clean overlay, **Restart** reloads hub, drops the cabin pid lock, starts a new overlay `grokhub`, and exits this process.

Settings → **About** is the app name, version, Grok Build line, doctor, and a redacted diagnostics copy. It does not list today's usage buckets or the model catalog (`/usage` and `/models` still do).

Chat is headless `grok -p` on this desktop (full filesystem and shell; Grok is told not to claim it lacks computer access). Night, loops, and phone `/v1/task` enqueue the same on the bound project and inherit the composer PermissionMode pill. Halt / Stop / Ctrl+Shift+Esc kill the child. Chat only saves a night job when you asked to schedule one — a reply that mentions “every day at” or “heartbeat every” as advice does not. A clock ask (“every weekday at 9pm, summarize the board”) is saved as a cabin automation in `automations.json` with its hour intact; an interval ask (`/loop 30m`, “every 2 hours”) is saved as a Grok Build loop. The pulse fires both. Anticipate only fires a `Follow skill` on a real `need to` / `remind me` insight that matches a skill, not polite “if you need” chit-chat. A 15s heartbeat runs housekeep, inbox, night, review, wall, mid-thought, reflect, and anticipate. Hidden idle cabins wait for that pulse. Phone dispatch completes on halt / error. `/rewind` restores only the bound project root.

Android / Windows: link `libgrokhub_ffi` and include `crates/grokhub-ffi/include/grokhub.h`.

| Binary | Crate | Job |
|--------|-------|-----|
| `grokhub` | `crates/grokhub-app` | Cabin GUI around Grok Build `grok -p` |
| `grokhub-hub` | `crates/grokhub-hub` | Standalone LAN `/v1` hub (port **18766**) |
| `grok` | xAI Grok Build CLI | Official coding-agent CLI (`https://x.ai/cli`) — installed with the cabin |
| `libgrokhub_ffi` | `crates/grokhub-ffi` | C ABI for Android / Windows (pair/port/models; no HOST_CMD) |

Config and memory: `~/.config/GrokHub` (`app.json`, `projects.json`, `updates.json`, `suggestions.json`, `secrets.json` mode 0600 / Windows user-only DACL, `memory/SOUL.md`, `USER.md`, `MEMORY.md`).

## First run

1. The installer already put **Grok Build CLI alpha** on PATH (`GROK_CHANNEL=alpha` / Windows `x.ai/cli/alpha`).
2. Launch `grokhub`. **Get Started** asks to connect Super Grok (existing device-code OAuth). That also writes `~/.grok/auth.json` when the CLI has no session.
3. Settings → Connect Grok OAuth repeats the CLI write if `grok` is still logged out.
4. Optional: Devices → **Start share** for the Android key-fob. Auto/Always chat is `grok -p`. Halt stops the child. Ask shows Allow / Deny when ACP is up; if ACP is down, Ask denies the turn.

Tokens stay in `secrets.json`. Never in markdown.

Composer is a pill: **Ask anything**. Five quick chips sit centered under the bar and rank the next likely move (last slash, last mode, Imagine vs chat, unfinished work, skills). The empty-home greeting is time + name + last project, not a memory dump. Plus opens Upload / Paste. Session pills are Chat / Plan / btw; permission is Ask / Auto / Always. Hover a pill for what it does. Mic is Hey Grok (Ara). It eases while listening or speaking and sits still when idle. While voice is live, Listening / Speaking / Ready sits above the composer with Stop. The strip stays up while Listening or Speaking and auto-hides about a second after Ready. Stop, the live mic, or Ctrl+G / Super+G leave. Failed STT shows Ready, not Listening. Enter sends; Ctrl+Enter is a newline. Composer Stop is a disc with a small rounded mark; idle Stop sits still, and it eases while hovered, pressed, or a reply is running. The transcript Running row has no Stop. The changing status text above the composer is gone. The context usage bar stays. A green live dot plus Thinking / Running / Waiting sits on the turn; hover shows the current action. That line does not sit above the composer. Shared buttons hover-scale to 1.035 over 120ms, shrink on press, and scale plus fill on keyboard focus. Off-screen chat rows skip paint; height follows the pane width and each row keeps its id. Chat streams Grok Build tokens onto the thread that started the job. Follow-ups queue instead of killing the turn. Tool cards, diffs, permission prompts, and desk frames render in the pane. Leftover pages (Devices, Memory, History, Automations, Workboard, Command) use the same catalog chrome. Command is a user `/sh` field, not the agent. `/v1/frame.jpg` serves the last ACP computer-use image when one exists.

## Always-on hub

The cabin embeds the hub when you start share. For a headless box:

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-hub.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub-hub.service
```

## Devices (phone / other PC)

Pair code `ABC-234`. Devices paints a real LAN IPv4 (`http://192.168.x.x:18766`), not a `<lan>` placeholder. Expired pair codes hide and rotate; New / rotated codes persist. The pair tile hides when the hub is not sharing. Android talks HTTP. Do not inhabit onto the phone. Hub `complete` is owner-only.

Contract: [`docs/superpowers/plans/2026-08-14-dispatch-android-notes.md`](docs/superpowers/plans/2026-08-14-dispatch-android-notes.md).

| Method | Path | Auth |
|--------|------|------|
| `GET` | `/v1/health` | none |
| `POST` | `/v1/pair` | pairing code |
| `POST` | `/v1/task` | Bearer |
| `GET` | `/v1/task/:id` | Bearer |
| `GET` | `/v1/results` | Bearer |
| `GET` | `/v1/frame.jpg` | Bearer (`?since=` → 304) |
| `POST` | `/v1/voice/client-secret` | Bearer — mints a 5-minute xAI realtime secret from the cabin console key. Android/browser use `wsProtocol` (`xai-client-secret.<token>`). OAuth cannot mint this. |

## Packaging

| Path | Role |
|------|------|
| `~/.local/bin/grokhub` | User install (`./scripts/install.sh --user`) |
| `~/.local/bin/grok` | Grok Build CLI (official xAI installer; also `~/.grok/bin/grok`) |
| `/usr/bin/grokhub` | System / makepkg |
| `/usr/bin/grok` | System Grok Build CLI (AUR `post_install`) |
| `~/.config/GrokHub` | Linux user data (`app.json`, `projects.json`, `secrets.json`, memory) |
| `%LOCALAPPDATA%\Programs\GrokHub` | Windows cabin (`grokhub.exe`) |
| `%APPDATA%\GrokHub` | Windows user data |

Linux tarball: `grokhub-linux-v*.tar.gz` from `./scripts/make-release-bundle.sh`.  
Windows installer: `GrokHub-Setup-<version>.exe` from `./scripts/make-windows-release.ps1` (Inno Setup). A tag publishes both.

Arch notes: [`packaging/README-ARCH.md`](packaging/README-ARCH.md).

## Uninstall

```bash
rm -f ~/.local/bin/grokhub ~/.local/bin/grokhub-hub
rm -rf ~/.local/lib/grokhub
rm -f ~/.local/share/applications/grokhub.desktop
# optional: rm -rf ~/.config/GrokHub
# optional Grok Build CLI: rm -f ~/.local/bin/grok ~/.local/bin/agent; rm -rf ~/.grok
sudo rm -f /usr/bin/grokhub /usr/bin/grokhub-hub
sudo rm -f /usr/share/applications/grokhub.desktop
```

## Development

```bash
cargo test --workspace
cargo run -p grokhub-app
cargo run -p grokhub-app -- --agent
cargo run -p grokhub-hub
cargo run -p grokhub-app -- --update
```

Spec: [`docs/superpowers/specs/2026-08-14-rust-parity-design.md`](docs/superpowers/specs/2026-08-14-rust-parity-design.md).

## License

MIT
