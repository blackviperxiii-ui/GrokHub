# GrokHub

Native Rust cabin. No Electron. No Tauri. One repo, one `main`, one version — two ship artifacts.

**v2.12.1** — Always Allow stops only for money, sending, deleting, credentials and irreversible system changes; a no-model pause keeps retrying and resumes on its own; and every feed card names its item.

| Platform | Artifact | Latest |
|----------|----------|--------|
| **Linux** (Arch / CachyOS) | `grokhub-linux-v2.12.1.tar.gz`, AUR | **v2.12.1** |
| **Windows** (x86_64) | `GrokHub-Setup-2.12.1.exe`, `grokhub-windows-v2.12.1.zip` | **v2.12.1** |

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

`./scripts/install.sh --user` installs [Grok Build](https://x.ai/cli) **alpha** (`GROK_CHANNEL=alpha`, `grok`) next to the cabin. First launch Get Started connects Super Grok and signs in `grok` too. Chat is headless **`grok -p --output-format streaming-json`** (Grok Build 1.0.21+) with `--sandbox off` and a desktop rule so Grok uses this machine. Ask/Allow/Deny is ACP (`grok agent stdio`) when the permission pill is Ask. MCP elicitation (form or URL) uses the same ACP session. Night, loops, and `/send` tasks stay on `grok -p` and inherit that same PermissionMode pill via `scheduled_args` (Ask is fail-closed, no ACP; Auto is `--permission-mode auto`; Always is `--always-approve`). btw (saved as ask) is a side ask: a live run keeps going, then the question sends look-safe on `grok -p` (`--permission-mode default`, no desktop-do-the-work rules). Idle btw sends that same look-safe ask. Bound project is `--cwd`; unbound uses `~/GrokHub-Work` (never the cabin process cwd). Overlay `/update` runs only what is newer: `grok update --alpha` when a newer alpha exists, then the cabin overlay when the cabin is newer. If `grok` is on stable after an upgrade, the cabin switches it back to alpha and does not yank a working alpha install. `/context` and `/usage` show Grok Build server tokens (including reasoning); `/usage` also totals today's cabin spend and, when a session is attached, `grok usage <id>` (1.0.14 per-turn cost). `/compact` and `/rewind` talk to Grok. Halt kills the `grok -p` child (`session/cancel` on ACP). Truncated replies and transient 5xx retries stay on the same turn. Credit-limit errors offer Try Again.

Slash: `/help` · `/new` · `/scratch` · `/clear` · `/undo` · `/retry` · `/stop` · `/sh` · `/host` · `/plan` · `/always-approve` · `/sessions` · `/inspect` · `/project` · `/memory` · `/recall` · `/forget` · `/board` · `/imagine` · `/skill` · `/compact` · `/learn` · `/update` · `/send` · `/sync` · `/hub` · `/inhabit` · `/rewind` · `/room` · `/export` · `/rename` · `/pin` · `/delete` · `/effort` · `/dream` · `/palette` · `/bg` · `/queue`. Type `/help` in the cabin for the rest. `/skill <name>` runs that skill. Skills and Connectors lists **Cabin skills** (`~/.config/GrokHub/skills`) next to the Grok Build catalog, with the run count and Use in chat, plus nightly **Suggested** tiles that Add via `save_skill`. Connectors holds the GitHub PAT and read-only Who am I / List repos tiles. `/compact` keeps the last 8 visible turns and, with `/workflow` and `/rewind`, honors the PermissionMode pill. `/workflow pause`, `/workflow resume`, and `/workflow stop` use that pill. Empty `/workflow` and `/workflows` open Skills on Workflows. `/context` counts visible turns. `/scratch` blocks `/forget` and Memory Save. `/rewind` restores the bound project root (or Grok conversation rewind when mapped). `/sync` merges chats and memory with paired computers. `/project` also takes `bind`, `new`, `folder`, `rename`, `move`, `delete`, `clear`. Right-click a sidebar project to rename or remove it — Delete drops the row, not the files.

Composer session pills: **Chat** / **Plan** / **btw**. Permission: **Ask** / **Auto** / **Always**. Hover a pill for a short tip. Both pills are remembered across launches — Always-approve is the exception and resets to Ask, same as the leftover `yolo` reset. On a permission card, **Enter** allows and **Esc** denies while the composer is empty; a half-typed follow-up still sends on Enter. Interactive Ask starts ACP (`grok agent stdio`) so Allow / Deny can show; if ACP cannot start the turn is denied (no `grok -p --sandbox off` fallthrough). btw stays look-safe on Auto/Always (`--permission-mode default`, no desktop-do-the-work) and does not stop a live run. Auto/Always map to `--permission-mode auto` / `--always-approve`. Effort dropdown: **None** / **Low** / **Medium** / **High** / **Extra High** (`grok -p --reasoning-effort`). `/effort` sets the same. A saved Max effort loads as Extra High and a saved Minimal as Low. Default model is **Grok 4.7** (`grok -p --model grok-4.7`). Greeting and chips use `grok-4.7` through `grok login`.

The **Automations** page holds both schedulers. **Scheduled** is the cabin's own clock list (`automations.json`) — pause, **Run** now, or **Remove** a job, and each row shows its schedule, next run and run count. **Loops** is the Grok Build `/loop` interval list. **New job** takes either shape: `/loop 30m check deploy` or `every weekday at 9, summarize the board`.

The left-rail **Chat** button is the new-chat control — there is no separate New chat row. Click Chat on an empty draft (no dialogue) to pull that same chat up. After a reply has started, Chat opens a new draft. One empty draft at a time. Old convos stay in sidebar History (the cabin's own chats).

Projects sit in the left rail. `+` makes a one-level folder. It does not create a project. Double-click or right-click to rename (display name only — the path stays). Right-click a project to add it to a folder or remove it. Folders are sidebar only; they do not move files. A folder looks like a folder. Click it to show the chats underneath. That click does not open a chat, and it does not hide or replace History. A chat under the folder opens that chat only. It does not filter, hide, or remove the other chats. A project is a chat row under its folder. Clicking it opens that chat. Every chat that belongs to the folder or to a project inside it is listed under the open folder, and those chats stay out of History. Collapsing the folder hides them in the project tree and does not change History. Creating a project does not wipe History. Clicking a chat in History opens it and leaves that row where it is. A new message moves the row. Selecting, focusing, or opening a row is not activity. Pins stay last-pinned-first. Right-click a folder or project and choose New chat to file another chat there; the projects stay in the folder. Delete puts those chats back in History and does not wipe transcripts. A project click does not open the Workboard. Bound tree is the world.

History search runs as you type across SOUL/USER/MEMORY and every chat; a new query drops the previous needle's hits so a late walk cannot open the wrong thread. A hit is a door — click a memory line to open that file in the editor, a chat line to open that thread. Re-opening the memory file already in the editor keeps unsaved typing. Below the search, History is the cabin's own chats (newest used, pins on top). Headless `grok -p` keeps the session on that chat. Background runs such as “summarize the workboard” are not rows. Right-click **Delete** or the History-page Delete button removes the cabin chat and its Grok Build session. Transcripts stay on the chat until you delete it. The cabin stays on Grok Build CLI **alpha** (`grok update --alpha`). It does not switch off alpha.

Imagine stills use dedicated **`grok-imagine-image-2.0`** (falls back to `grok-imagine-image` on timeout). Video kind calls **`grok-imagine-video-1.5`**. Auth is `grok login` first, then a console key / cabin OAuth. Hey Grok: push-to-talk STT into chat, then TTS of the reply body (not thinking). Same on Linux and Windows. Desktop control is **Grok Build computer-use** — the cabin keeps tool cards, diffs, and computer-use frames in a collapsed Work tree in chat. No Desk / Take over menu. The shipped `grokhub-desktop` MCP is the desktop tool path on Linux and Windows. In front of it and every Grok Build tool call, the cabin runs a stricter pre-check that never loosens the permission pill: a hard floor deny, a white hard-class card even under Always (money, send, delete, credentials, irreversible OS: partitions, the bootloader and writes into `/boot` or the ESP), and Settings → *Let Grok control the desktop* as Access. Full is an inline Grant full card in the Work tree. With Always on, nothing else asks: `sudo`, package admin (`pacman -S`, `systemctl restart`), commands the engine can't split, and your own ask rules run without a card; deny rules and the floor still refuse. Halt / Stop / tray Halt / Ctrl+Alt+H SIGTERM the `grok -p` child. Stream buffers clip at `IMAGE_FILE_CAP` / `TEXT_FILE_CAP`. Desk frames drop above `FRAME_CAP`. Titlebar × unmaps to tray. Plus-button stills ride `--prompt-json` image blocks.

Settings → **Account** is Super Grok device-code OAuth only — **Connect** / **Sign out** (or `grokhub --oauth`). That also writes `~/.grok/auth.json` when the Grok Build CLI is not already connected. Tokens live in `~/.config/GrokHub/secrets.json` (mode 0600; Windows user-only DACL), never in markdown. Settings → Appearance is **Dark**, **Light**, or **System**. Settings → Behavior holds close-to-tray, the living wall, and one **quiet hours** dropdown (Off / common windows). Picking a window saves it. Settings → **Cabin defaults** pins the chat model, reasoning effort, Ask or Auto, and Chat / Plan / btw in `app.json`, plus **Always collapse**. Auto model saves an empty pin. Always is not written from that page. Close-to-tray still hides the cabin; the desktop tip is once, and only when quiet hours allow it. GitHub PAT is not a Settings page — the connector owns that.

Windows Setup and first cabin launch (also Linux tarball / AUR first launch) **automatically** install Grok Build CLI **alpha** (`GROK_CHANNEL=alpha` / `https://x.ai/cli/alpha`) when `grok` is missing or unusable. Reinstall does the same. A working alpha install is left alone. The Install control is hidden when grok is already present or an alpha install is already running. A leftover `grok.exe` that cannot start (missing DLL) shows one cabin error, not a looping Windows dialog.

Settings → **Update** is **Install Grok Build CLI** only when grok is still missing after the automatic install failed, then one **Update** control (always visible), progress, and **Restart** after a clean cabin overlay. The cabin checks every 2 hours (and at launch) for a newer Grok Build CLI alpha (`https://x.ai/cli/alpha` vs `grok --version`) and a newer GitHub Latest cabin. The titlebar chip is the notice (in-app — not a web page) and says **Update CLI**, **Update cabin**, or **Update CLI and cabin**. The Settings button stays up when the probe found nothing so you can still overlay. Acting on it runs only what is pending: `grok update --alpha` first, then the cabin. A click when nothing was detected still overlays both. A working alpha install is updated only when a newer alpha exists. It does not switch the CLI to stable. `grokhub --update` / `/update` use that same rule. When the cabin is newer it retargets a leftover Origin or `GrokHub-Windows` clone to GitHub (`https://github.com/blackviperxiii-ui/GrokHub.git`), then `git pull --ff-only origin main` (`origin beta` on a beta install; see Channels). Linux overlays with `./scripts/install.sh --user`. If a leftover `grok` is on stable, overlay / first launch switches it to alpha. Windows Setup users do not need a clone: when the cabin is newer, Update downloads the latest `grokhub-windows-v*.zip` from GitHub into `%LOCALAPPDATA%\Programs\GrokHub` (PATH includes `%USERPROFILE%\.grok\bin` for `grok update --alpha`). A leftover Windows clone (wrong branch, detached HEAD) uses that same zip. A Windows source clone on `main` still overlays with `scripts/install-windows.ps1`. Linux overlay / Update CLI prepends `$HOME/.grok/bin:$HOME/.local/bin` so `grok update --alpha` finds the official install. The clone path is not a Settings field — empty uses `GROKHUB_SRC` or the install receipt. After a clean overlay, **Restart** reloads hub, drops the cabin pid lock, starts a new overlay `grokhub`, and exits this process.

Settings → **About** is the app name, version, Grok Build line, doctor, and a redacted diagnostics copy. It does not list today's usage buckets or the model catalog (`/usage` and `/models` still do).

Chat is headless `grok -p` on this desktop (full filesystem and shell; Grok is told not to claim it lacks computer access). Night, loops, and `/send` tasks enqueue the same on the bound project and inherit the composer PermissionMode pill. Halt / Stop / Ctrl+Shift+Esc kill the child. Chat only saves a night job when you asked to schedule one — a reply that mentions “every day at” or “heartbeat every” as advice does not. A clock ask (“every weekday at 9pm, summarize the board”) is saved as a cabin automation in `automations.json` with its hour intact; an interval ask (`/loop 30m`, “every 2 hours”) is saved as a Grok Build loop. The pulse fires both. Anticipate only fires a `Follow skill` on a real `need to` / `remind me` insight that matches a skill, not polite “if you need” chit-chat. A 15s heartbeat runs housekeep, inbox, night, review, wall, mid-thought, reflect, and anticipate. Hidden idle cabins wait for that pulse. A queued `/send` task completes on halt / error. `/rewind` restores only the bound project root.

| Binary | Crate | Job |
|--------|-------|-----|
| `grokhub` | `crates/grokhub-app` | Cabin GUI around Grok Build `grok -p` |
| `grokhub-hub` | `crates/grokhub-hub` | Standalone LAN `/v1` hub (port **18766**) |
| `grok` | xAI Grok Build CLI | Official coding-agent CLI (`https://x.ai/cli`) — installed with the cabin |

Config and memory: `~/.config/GrokHub` (`app.json`, `projects.json`, `updates.json`, `suggestions.json`, `secrets.json` mode 0600 / Windows user-only DACL, `memory/SOUL.md`, `USER.md`, `MEMORY.md`). `/recall` reads those memory files unless `app.json` sets `"memory_backend": "amr"`, which reads local `amr/nodes` instead.

## Channels: stable and beta

There are two channels. **Stable** builds `main`, where releases are cut. **Beta** builds the `beta` branch, where new features and fixes land first. Switch on Linux with one command from your clone:

```bash
./scripts/install.sh --user --channel beta     # fetch origin, check out beta, build, install
./scripts/install.sh --user --channel stable   # back to main
grokhub --version                              # GrokHub 2.12.1-beta (beta @ abc1234) or GrokHub 2.12.1 (main @ f7dcf9a)
```

`--channel` fetches `origin`, checks out the branch (creating a local tracking branch the first time, and fast-forwarding it after that), then builds and installs. Stable follows `main`, not the latest release tag, because the in-app Update already pulls `main`. The switch refuses to run over uncommitted changes, and it stops instead of resetting when your local branch has commits that aren't on `origin`.

The channel is saved to `~/.config/GrokHub/channel` (or `$GROKHUB_CONFIG/channel`), next to the `source` receipt. A plain `./scripts/install.sh --user` keeps the saved channel and builds whatever is checked out; it doesn't move git. Settings → **Update**, `/update`, and `grokhub --update` read the receipt: a beta install pulls `origin beta` and never `origin main`. If the clone isn't on the channel's branch, Update stops and says which command to run. `./scripts/install.sh --dry-run` prints the channel, branch, and receipt it would use, and exits.

`--version` comes from `crates/grokhub-app/build.rs`, which reads the branch and short SHA from git at build time. A beta build adds `-beta` to the version; the Cargo version itself never changes. A build made without git shows just `GrokHub 2.12.1`.

Limits: channels are Linux-only for now. `scripts/install-windows.ps1` has no `-Channel`, and Windows Setup always installs stable releases (a beta receipt on Windows never falls back to the stable release zip). The Update chip still only notices new releases, so on beta, use Settings → **Update** to pull new beta commits.

## First run

1. The installer already put **Grok Build CLI alpha** on PATH (`GROK_CHANNEL=alpha` / Windows `x.ai/cli/alpha`).
2. Launch `grokhub`. **Get Started** asks to connect Super Grok (existing device-code OAuth). That also writes `~/.grok/auth.json` when the CLI has no session.
3. Settings → Connect Grok OAuth repeats the CLI write if `grok` is still logged out.
4. Optional: Devices → **Start share** to pair another computer. Auto/Always chat is `grok -p`. Halt stops the child. Ask shows Allow / Deny when ACP is up; if ACP is down, Ask denies the turn.

Tokens stay in `secrets.json`. Never in markdown.

Composer is a pill. **Ask anything** is a faint placeholder on the same center as the paperclip, mic, and send. Five quick chips sit centered under the bar and rank the next likely move (last slash, last mode, Imagine vs chat, unfinished work, skills). The empty-home greeting is time + name + last project, not a memory dump. The paperclip opens Upload / Paste. Session pills are Chat / Plan / btw; permission is Ask / Auto / Always. Hover a pill for what it does. Mic is Hey Grok (Ara). It eases while listening or speaking and sits still when idle. While voice is live, Listening / Speaking / Ready sits above the composer with Stop. The strip stays up while Listening or Speaking and auto-hides about a second after Ready. Stop, the live mic, or Ctrl+G / Super+G leave. Failed STT shows Ready, not Listening. Enter sends; Ctrl+Enter is a newline. Composer Stop is a disc with a small rounded mark; idle Stop sits still, and it eases while hovered, pressed, or a reply is running. The changing status text above the composer is gone. The context usage bar stays. No live dot or Thinking label sits on the turn; the composer glow shows a running reply, and Stop's hover names the current action. Shared buttons hover-scale to 1.035, rise 1px over 120ms, shrink on press, and scale plus fill on keyboard focus. Chips, permission segments, and the rail share one highlight that glides. Off-screen chat rows skip paint; height follows the pane width and each row keeps its id. Chat streams Grok Build tokens onto the thread that started the job. A message typed while a reply runs steers it: Enter stops the turn where it is, keeps what it said and did in the transcript, and carries on with your message plus a note of that progress. Alt+Enter (or `/queue <message>`) queues it for after the reply instead, and a cabin-wide Grok command such as `/compact`, or Grok's own background tasks on the turn, still queue. With text typed during a run, a small row above the composer offers Steer and Queue, and lists queued messages with Steer now and Remove. Tool cards, diffs, permission prompts, and desk frames render in the pane. Leftover pages (Devices, Memory, History, Automations, Workboard, Command) use the same catalog chrome. Command is a user `/sh` field, not the agent. `/v1/frame.jpg` serves the last ACP computer-use image when one exists.

## Pulse

**Pulse** (it was Ideas) is one page with two tabs, Feed first. It opens on **Feed**, and the switch at the top reads Feed | Ideas. **Feed** is a column of posts: the source, the headline, two or three sentences, the source's own image when it has one, and Like / Discuss. **Ideas**, the second tab, lists what the cabin could do next, grouped by category (up to four of the most useful first, then Financial Management, Productivity, Relationships, Health & Fitness, Shopping, and More ideas). Each row has a category icon, a bold "I can …" line, a short reason, and a ··· menu. Hover or arrow onto a row to see Run · Snooze · Dismiss inline; R, S, Enter, N, and D act on the focused row. Titles are the model's own "I can …" line, up to 70 characters. Images come only from the post's thumbnail or the linked page's `og:image`, are cached in `pulse-images/` in the config folder, and show a placeholder until they arrive.

Every card is one of five kinds: **Do** (one tap), **Watch** (news and results), **Learn** ("I can learn this and do it your way"), **Automate** (a repeat that could run on a schedule), and **Quiet** (kept, but not shown). Plain rules rank them, not the model: a skill ask, something due within a day, unfinished work on the same topic, and a repeat move a card up; a card you accepted gets a bump; Not this and Dismiss push that topic down. The model only writes the sentence. The ··· menu has **Run in the background** (sends the idea as `/bg`, so it runs invisibly like any background task; Ask mode refuses it, as it refuses every `/bg`), **Snooze until 9 AM**, **Always do this** (or **That's right** / **That's wrong** on a Learn card), **Open**, **Not this**, and **Dismiss**. Likes, Not this, Dismiss and right/wrong each add one line to `MEMORY.md`, for example `- [2026-10-04] pulse: dismissed "Sort the photos" reason=not-this`, and the next ranking reads them.

**Feed instructions** on the Feed tab is a plain-text prompt that shapes every future post. Edit it any time. After three likes or skips the cabin rewrites it itself, keeping your words and adding what it learned under "Show more of" and "Show less of". Cards that arrive during quiet hours wait and come back as one "While you were in quiet hours" post. The Home and Ideas data from older builds moves into Pulse once on first launch; nothing is deleted. The card deck on the empty chat is off by default now; Settings → Behavior → **Cards on the empty chat** turns it back on.

## Background tasks

Work can run beside the chat while you keep talking. `/bg <task>` starts a headless `grok -p` that forks this chat's Grok session, so it knows the conversation without writing into it. A bare `/bg` moves the live reply off the composer; it keeps going and its answer posts on that chat when it ends. Sending in another chat does the same to a reply that is still running there instead of cutting it off. Grok can start one itself with a `BACKGROUND_TASK: <instructions>` line in its reply. Up to 3 run at once. Each shows above the composer with its time, last tool, and Stop; `/bg stop` stops them all, and tray Halt or Ctrl+Alt+H stops them with the live turn (composer Stop and `/stop` leave them running). A result waits while its chat is mid-reply, and the next turn on that chat is told what came back. With Ask on, `/bg` and Grok's `BACKGROUND_TASK:` lines are refused, because a background run can't ask for approval (switch to Auto). Unwatched runs under Ask (background, automations, night, `/send`) pass Grok Build `--permission-mode dontAsk` and `--deny` rules for shell, edit, and write. They end when the cabin quits.

## Always-on hub

The cabin embeds the hub when you start share. For a headless box:

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-hub.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub-hub.service
```

## Devices (other computers)

Pair code `ABC-234`. Devices paints a real LAN IPv4 (`http://192.168.x.x:18766`), not a `<lan>` placeholder. Expired pair codes hide and rotate; New / rotated codes persist. The pair tile hides when the hub is not sharing. Paired computers use the hub for `/sync` (chats and memory) and `/inhabit`. Phone pairing was scrapped on 2026-10-07.

| Method | Path | Auth |
|--------|------|------|
| `GET` | `/v1/health` | none |
| `POST` | `/v1/pair` | pairing code |
| `GET` | `/v1/status` | Bearer |
| `GET` / `PUT` | `/v1/snapshot` | Bearer (`/sync`) |
| `GET` / `POST` | `/v1/inhabit` | Bearer |
| `GET` / `POST` | `/v1/frame` | Bearer |

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

Arch notes: [`packaging/README-ARCH.md`](../packaging/README-ARCH.md).

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

Spec: [`docs/superpowers/specs/2026-08-14-rust-parity-design.md`](superpowers/specs/2026-08-14-rust-parity-design.md).

## License

MIT
