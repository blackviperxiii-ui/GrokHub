# GrokHub is the Grok Build GUI

**Version:** 2.9.4

GrokHub is the native egui cabin. Grok Build (`grok` CLI) is the agent, the host shell, and computer-use (eyes and hands). One repo, one version — Linux tarball/AUR and Windows `GrokHub-Setup-<version>.exe` ship from the same tag.

## Split

Cabin owns: window, tray, project sidebar as cwd, LAN hub / Android, Hey Grok voice, Imagine toolbox.

Grok Build owns: coding tools, bash, sandbox, permissions, plan mode, skills/plugins/MCP, sessions, `/imagine` when ACP supports it, and desktop computer-use.

Transport: chat is headless `grok -p --output-format streaming-json` with `--sandbox off`, a desktop `--rules` line, and `--leader-socket` on the cabin socket (do not share `~/.grok/leader.sock`). New chats use the user `~/.grok` so tools and `grok sessions` match the TUI. ACP `grok agent stdio` is Ask (Allow / Deny). Night and phone `/v1/task` stay on `grok -p`. Do not vendor grok-build crates. Cabin overlay (`install.sh`) runs the official installer from `https://x.ai/cli` so `grok` is on PATH with `grokhub`.

## Chat

`send_chat` runs `grok -p` whose cwd is the bound project, or `~/GrokHub-Work` when unbound — never the cabin process cwd. Stream user and assistant text into bubbles that use the full chat pane. Thinking is faded thought process, not a bubble. Stop / Halt / tray Halt SIGTERMs the `grok -p` child (`session/cancel` on ACP). A dead stored session id retries without `--resume`. Disk-full / permission-denied handshake errors land in the chat with the cwd named. Grok.com-style “I don’t have access to your computer” thoughts are stripped from the pane.

Composer pills: Chat / Plan / Ask, Ask / Auto / Always-approve, and Effort (low / medium / high / xhigh → `grok agent --reasoning-effort`). Hover a pill for what it does. Segment pills, catalog triggers, settings switches, and sidebar chrome use Plasma-style click feel (hover wash, press shrink, ~120ms selection blend). Tool cards, diffs, and computer-use frames sit in a collapsed Work tree. Permission prompts Allow / Deny / Always.

## Desktop

Grok Build owns computer-use. There is no Desk / Take over menu. The cabin keeps tool cards and the last ACP frame in a collapsed Work tree. Halt cancels the ACP turn.

## History and extensions

History search types across SOUL/USER/MEMORY and every chat. A new query drops the previous needle's hits; a finished walk only installs when it still matches the box. A hit opens that memory file or thread. Re-opening the file already in the Memory editor keeps unsaved typing.

Below the search, History is `grok sessions list` (no disk walk of subagents). Delete is `grok sessions delete` against `~/.grok`, then a refresh from that list. Session transcripts load via `grok export`. The Connectors tab runs `grok inspect` / `grok mcp` / skills / plugins JSON.

## Settings

Quiet-hour clocks and the daily/host caps type into buffers. Save parses them. A half-typed clock or an hour with no `:` (`7`) keeps the last-good value instead of the factory window or turning the guard off.

## Auth

First-time installers ship Grok Build CLI **alpha** (`GROK_CHANNEL=alpha` / Windows `x.ai/cli/alpha`). First run is Get Started: cabin device-code Super Grok OAuth writes `secrets.json` and, when `~/.grok/auth.json` is empty, the same tokens so `grok` is signed in. Settings → Connect does that CLI write only if the CLI is not already connected. Agent auth is that session, an existing `grok login`, or `XAI_API_KEY`. Imagine uses the same token (console key optional). Voice still prefers cabin `secrets.json` / console key.

## Overlay vs agent updates

Cabin overlay (`/update`) updates the GUI, then runs `grok update --alpha` on Linux and Windows. The app stays on the alpha track. If `grok` is on stable (including after a stable upgrade), detect that from `~/.grok/config.toml` `[cli] channel` or `grok update --check --json` and switch with `grok update --alpha`. Do not pass `--stable`. Do not yank a working alpha install. Windows Setup Update downloads the latest `grokhub-windows` zip from GitHub (no clone required), then `grok update --alpha`. A Windows source clone still overlays with `install-windows.ps1`. Settings → **Install Grok Build CLI** (Account or Update) and first-run install Grok Build CLI alpha when `grok` is missing or broken. `/learn` (alias `/learn reflect`) is a cabin slash: palette click and typed send both run reflect.
