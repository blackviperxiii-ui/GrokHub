# GrokHub is the Grok Build GUI

**Version:** 2.9.11

GrokHub is the native egui cabin. Grok Build (`grok` CLI) is the agent, the host shell, and computer-use (eyes and hands). One repo, one version — Linux tarball/AUR and Windows `GrokHub-Setup-<version>.exe` ship from the same tag.

## Split

Cabin owns: window, tray, project sidebar as cwd, LAN hub / Android, Hey Grok voice (Ara, PTT STT then TTS of the reply body not thinking, live Voice · Listening + Stop, line stays open until Stop / live mic / Ctrl+G), Imagine toolbox.

Grok Build owns: coding tools, bash, sandbox, permissions, plan mode, skills/plugins/MCP, sessions, `/imagine` when ACP supports it, and desktop computer-use.

Transport: chat is headless `grok -p --output-format streaming-json` with `--sandbox off`, a desktop `--rules` line, and `--leader-socket` on the cabin socket (do not share `~/.grok/leader.sock`). New chats use the user `~/.grok` so tools and `grok sessions` match the TUI. ACP `grok agent stdio` is Ask (Allow / Deny). Night and phone `/v1/task` stay on `grok -p`. Do not vendor grok-build crates. Cabin overlay (`install.sh`) runs the official installer from `https://x.ai/cli` so `grok` is on PATH with `grokhub`.

## Chat

Left-rail **Chat** is the new-chat control. There is no separate New chat button. Click Chat on an empty draft (no dialogue, no Grok session) to pull that same draft up. After dialogue has started, Chat opens a new draft. Keep one empty draft at a time. Old convos are sidebar History (`grok sessions`).

`send_chat` runs `grok -p` whose cwd is the bound project, or `~/GrokHub-Work` when unbound — never the cabin process cwd. Stream user and assistant text into bubbles that use the full chat pane. Thinking is faded thought process, not a bubble. A green live dot plus Thinking / Running / Waiting sits on the turn (and above the composer); hover shows the current action. Stop / Halt / tray Halt SIGTERMs the `grok -p` child (`session/cancel` on ACP). A dead stored session id retries without `--resume`. Disk-full / permission-denied handshake errors land in the chat with the cwd named. Grok.com-style “I don’t have access to your computer” thoughts are stripped from the pane.

Composer pills: Chat / Plan / Ask, Ask / Auto / Always-approve, and Effort (None / Minimal / Low / Medium / High / Extra High → `grok -p --reasoning-effort`; a saved Max loads as Extra High). Chat passes `--model grok-4.7` unless `/model` pins another id. Hover a pill for what it does. Shared buttons (segment pills, catalog triggers, settings switches, sidebar chrome, Copy, Reply, slash-pick rows, and the settings close control) hover-scale to 1.035 over 120ms, shrink on press, and scale plus fill on keyboard focus. The same paint runs on Linux and Windows. Off-screen chat rows skip paint. Row height is cached by thread and pane width, and each row has a stable id so a skip does not move selection or hover. Tool cards, diffs, and computer-use frames sit in a collapsed Work tree. Permission prompts Allow / Deny / Always.

## Desktop

Grok Build owns computer-use. There is no Desk / Take over menu. The cabin keeps tool cards and the last ACP frame in a collapsed Work tree. Halt cancels the ACP turn.

## History and extensions

History search types across SOUL/USER/MEMORY and every chat. A new query drops the previous needle's hits; a finished walk only installs when it still matches the box. A hit opens that memory file or thread. Re-opening the file already in the Memory editor keeps unsaved typing.

Below the search, History is `grok sessions list` (no disk walk of subagents). Delete is `grok sessions delete` against `~/.grok`, then a refresh from that list. Session transcripts load via `grok export`. The Connectors tab runs `grok inspect` / `grok mcp` / skills / plugins JSON.

## Settings

Quiet-hour clocks and the daily/host caps type into buffers. Save parses them. A half-typed clock or an hour with no `:` (`7`) keeps the last-good value instead of the factory window or turning the guard off.

## Auth

First-time installers ship Grok Build CLI **alpha** (`GROK_CHANNEL=alpha` / Windows `x.ai/cli/alpha`). First run is Get Started: cabin device-code Super Grok OAuth writes `secrets.json` and, when `~/.grok/auth.json` is empty, the same tokens so `grok` is signed in. Get Started shows live device-code / OAuth failures; leftover wall or install status is not an OAuth error. Settings → Connect does that CLI write only if the CLI is not already connected. Agent auth is that session, an existing `grok login`, or `XAI_API_KEY`. Imagine uses the same token (console key optional). Voice is Ara. Hey Grok is push-to-talk STT into chat, then TTS of the reply body (not the thought process) on Linux and Windows. While live, Voice · Listening sits above the composer with Stop. After a listen/speak turn the line stays open and listens again until Stop, the live mic, or Ctrl+G / Super+G.

## Overlay vs agent updates

Cabin overlay (`/update`) updates the GUI, then runs `grok update --alpha` on Linux and Windows (current alpha **1.0.38**; headless spawn flags match 1.0.36; `clone --cone` is unused CLI-only). Linux prepends `$HOME/.grok/bin:$HOME/.local/bin` for that CLI step (same idea as Windows `%USERPROFILE%\.grok\bin`). The app stays on the alpha track. If `grok` is on stable (including after a stable upgrade), detect that from `~/.grok/config.toml` `[cli] channel` or `grok update --check --json` and switch with `grok update --alpha`. Do not pass `--stable`. Do not yank a working alpha install. When GitHub Latest is newer than the running cabin, notify in-app (titlebar **Update available** → Settings → Update overlay, Update + Restart). Not a web page. Not on Account. Settings → Update → **Update Grok Build CLI** runs `grok update --alpha` when grok is already installed. Windows Setup Update downloads the latest `grokhub-windows` zip from GitHub when there is no clone, or when a leftover clone is not a usable `main` checkout, then `grok update --alpha`. A Windows source clone on `main` still overlays with `install-windows.ps1`. First cabin launch and reinstall automatically install Grok Build CLI alpha when `grok` is missing or unusable (Windows Setup also runs the official installer; it does not assume grok is already on PATH). The Install control is hidden when grok is present or an alpha install is already running. `/learn` (alias `/learn reflect`) is a cabin slash: palette click and typed send both run reflect.
