# GrokHub is the Grok Build GUI

**Version:** 2.8.2

GrokHub is the native egui cabin. Grok Build (`grok` CLI) is the agent, the host shell, and computer-use (eyes and hands).

## Split

Cabin owns: window, tray, project sidebar as cwd, LAN hub / Android, Hey Grok voice, Imagine toolbox.

Grok Build owns: coding tools, bash, sandbox, permissions, plan mode, skills/plugins/MCP, sessions, `/imagine` when ACP supports it, and desktop computer-use.

Transport: chat is headless `grok -p --output-format streaming-json` with `--sandbox off`, a desktop `--rules` line, and `--leader-socket` on the cabin socket (do not share `~/.grok/leader.sock`). New chats use the user `~/.grok` so tools and `grok sessions` match the TUI. ACP `grok agent stdio` is Ask (Allow / Deny). Night and phone `/v1/task` stay on `grok -p`. Do not vendor grok-build crates. Cabin overlay (`install.sh`) runs the official installer from `https://x.ai/cli` so `grok` is on PATH with `grokhub`.

## Chat

`send_chat` runs `grok -p` whose cwd is the bound project, or `~/GrokHub-Work` when unbound — never the cabin process cwd. Stream thought and text into existing bubbles. Stop / Halt / tray Halt SIGTERMs the `grok -p` child (`session/cancel` on ACP). A dead stored session id retries without `--resume`. Disk-full / permission-denied handshake errors land in the chat with the cwd named. Grok.com-style “I don’t have access to your computer” thoughts are stripped from the pane.

Composer pills: Chat / Plan / Ask, Ask / Auto / Always-approve, and Effort (low / medium / high / xhigh → `grok agent --reasoning-effort`). Segment pills, catalog triggers, settings switches, and sidebar chrome use Plasma-style click feel (hover wash, press shrink, ~120ms selection blend). Tool cards, diffs, and computer-use frames render in the chat pane. Permission prompts Allow / Deny / Always.

## Desktop

Grok Build owns computer-use. There is no Desk / Take over menu. The cabin renders tool cards and the last ACP frame in chat. Halt cancels the ACP turn.

## History and extensions

History is `grok sessions list` only (no disk walk of subagents). Delete is `grok sessions delete` against `~/.grok`, then a refresh from that list. Session transcripts load via `grok export`. The Connectors tab runs `grok inspect` / `grok mcp` / skills / plugins JSON.

## Auth

Agent: `grok login` cached token, `grok.com` ACP auth, or `XAI_API_KEY`. Imagine uses the same token (console key optional). Voice still prefers cabin `secrets.json` / console key.

## Overlay vs agent updates

Cabin overlay (`/update`) updates the GUI, then runs `grok update` on the current channel. It does not pass `--alpha` or `--stable`.
