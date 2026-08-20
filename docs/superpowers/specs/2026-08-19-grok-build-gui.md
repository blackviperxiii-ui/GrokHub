# GrokHub is the Grok Build GUI

**Version:** 2.6.42

GrokHub is the native egui cabin. Grok Build (`grok` CLI over ACP) is the agent, the host shell, and computer-use (eyes and hands).

## Split

Cabin owns: window, tray, project sidebar as cwd, LAN hub / Android, Hey Grok voice, Imagine toolbox.

Grok Build owns: coding tools, bash, sandbox, permissions, plan mode, skills/plugins/MCP, sessions, `/imagine` when ACP supports it, and desktop computer-use.

Transport: spawn installed `grok --no-auto-update agent stdio`. Do not vendor grok-build crates. Headless `grok -p` is only a fallback. Cabin overlay (`install.sh`) runs the official installer from `https://x.ai/cli` so `grok` is on PATH with `grokhub`.

## Chat

`send_chat` opens or reuses an ACP session whose cwd is the bound project, or `~/GrokHub-Work` when unbound — never the cabin process cwd. Stream thought and text into existing bubbles. Stop / Halt / tray Halt is `session/cancel`. A dead stored session id retries `session/new` without resume. Disk-full / permission-denied handshake errors land in the chat with the cwd named.

Composer pills: Chat / Plan / Ask and Ask / Auto / Always-approve. Tool cards, diffs, and computer-use frames render in the chat pane. Permission prompts Allow / Deny / Always.

## Desktop

Grok Build owns computer-use. There is no Desk / Take over menu. The cabin renders tool cards and the last ACP frame in chat. Halt cancels the ACP turn.

## History and extensions

History lists `grok sessions` plus cabin chats. The Connectors tab runs `grok inspect` / `grok mcp` / skills / plugins JSON.

## Auth

Agent: `grok login` cached token or `XAI_API_KEY`. Imagine uses the same token (console key optional). Voice still prefers cabin `secrets.json` / console key.

## Overlay vs agent updates

Cabin overlay (`/update`) updates the GUI only. `grok update` updates the agent.
