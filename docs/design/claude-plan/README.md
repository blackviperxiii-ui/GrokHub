# Claude plan provider: design note

The desktop app (Tauri only) can run chats on the user's own Claude plan by
starting the official Claude Code CLI on this computer. The user signs in
through Claude Code's own `claude auth login`. The app never reads, copies,
stores or passes on a Claude credential or token, never shows its own Claude
sign-in screen, and never changes the `claude` binary. Requests count against
the user's plan, the same as when they run Claude Code themselves.

This note ships with the first step, detection. It lists what the CLI
offers, how it plugs into the app, the build order, and what stays open.

## Why the CLI, and not the plan's token

Source: `code.claude.com/docs/en/legal-and-compliance`.

- **What the page doesn't allow.** Third-party developers may not "offer
  Claude.ai login into their own applications, or ... route requests through
  Free, Pro, Max, or Ultra plan credentials on behalf of their users", and
  may not "collect, store, or intermediate Claude.ai credentials or session
  tokens".
- **What it does allow.** It doesn't "prevent an end user from signing in to
  the unmodified Claude Code binary with their own Claude subscription".
- **The conditions for running Claude Code inside a product.** The binary
  is run as published, with none of its sign-in methods removed. Nobody pays
  for, resells or intermediates the user's Claude usage. Each user signs in
  with their own credentials.

So the app starts the user's own `claude` and reads its output. It never
sends a request to Anthropic itself, never sees a token, and never stands
between the user and their plan.

Naming follows the same page: Claude Code's and Anthropic's names aren't
used in the app's or a feature's name. The provider is **Claude (your
plan)**, and its plain-text line says it "runs Claude Code on this computer".
The existing API-key provider stays **Claude (Anthropic)**.

## What the CLI offers

Sources: `code.claude.com/docs/en/cli-reference`, `/headless`,
`/agent-sdk/typescript`.

| Need | CLI |
| --- | --- |
| Installed | `claude --version` exits 0 |
| Signed in | `claude auth status` prints JSON and exits 0 when signed in, 1 when not. Only `authMethod` is read: `claude.ai` and `oauth_token` (a `claude setup-token` token, which needs a plan) mean the plan; `api_key` and the rest mean other billing, and the page says so. Nothing else from the output is kept or logged |
| One turn | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages`, the prompt as one JSON line on stdin |
| Model | `--model` with an alias (`opus`, `sonnet`, `haiku`) |
| Same conversation | `--session-id <uuid>` on the first turn, `--resume <uuid>` after. `--continue` takes the newest conversation in the folder, which may belong to another chat or to the user's own terminal, so it isn't used |
| Tools | `--allowedTools` and `--disallowedTools` rules, `--permission-mode default` (never `bypassPermissions`) |
| Approvals | `--permission-prompt-tool mcp__<server>__<tool>` with `--mcp-config`: Claude Code asks that MCP tool before a tool it would otherwise ask about |
| Usage | Each `result` line has `modelUsage` and `total_cost_usd` (an estimate). `rate_limit_event` lines carry `status`, `utilization` and `resetsAt` |
| Stop | SIGINT ends the turn and records its result; SIGTERM records nothing |

## How a turn runs

The chat worker already runs turns through the `Engine` trait
(`crates/grokhub-agent/src/events.rs`). Today the worker holds only a
`NativeEngine` (`src-tauri/src/agent.rs`). A second engine,
`ClaudePlanEngine`, starts `claude` for each turn and turns its stream into
the same `AcpEvent`s the chat already shows:

| stream-json line | Chat event |
| --- | --- |
| `system` / `init` | Keeps `session_id`; nothing shown |
| `stream_event` text delta | Text |
| `stream_event` thinking delta | Thought |
| `assistant` `tool_use` block | Tool row, in progress, with a short summary of its input |
| `user` `tool_result` block | Same row, done or failed, output clipped as for native tools |
| `permission_denied` | Same row, failed, "Not allowed" |
| `assistant.error` | Error. `authentication_failed` reads "Sign in to Claude Code: run `claude auth login`" |
| `result` | Usage, then the turn ends |
| Exit with no `result` | Error, then the turn ends |

The parser is a pure function from a line to events, tested on literal
fixture lines. The router path (`providers::stream_model`) only carries text,
so it isn't used.

## Tools and approvals

- **First:** read-only tools only (`--allowedTools Read Grep Glob`,
  `--permission-prompts none`). A tool Claude wanted but wasn't allowed shows
  as a failed row and, after the turn, a card: "Claude wanted to run X. Allow
  it for this chat?" Allowing it adds the rule for the next turns.
- **Next:** live approvals. A small stdio MCP server inside the app's binary
  answers `--permission-prompt-tool`. It runs the app's own allow, ask and
  deny rules and, on ask, shows the chat's normal approval card. The answer
  goes back as `{behavior: "allow"}` or `{behavior: "deny", message}`.
  "Always allow" stays an app rule passed as `--allowedTools`. The app never
  writes Claude Code's settings.

## Choosing it

There's no per-chat model picker today. Settings → AI models gets a
**Claude (your plan)** row showing installed, signed in and the billing kind.
While Claude Code is signed in with a plan, **Allow for chats** asks first
(what runs, where, and that it counts against the plan). Once allowed, a
chat's tab menu has **Run on Claude (your plan)**, and the choice is kept per
chat in `{config}/claude-plan-chats.json`. **Stop using for chats** switches
every chat back. Auto routing never picks it on its own.

Each turn checks `claude auth status` again (about a quarter of a second). A
chat on the plan sends nothing while Claude Code is signed out or signed in
another way, such as an API key, so it never bills somewhere the user didn't
allow.

## Usage

The per-turn tokens and estimated cost come from `result.modelUsage`. Plan
headroom comes from `rate_limit_event` (`utilization`, `resetsAt`), and
`credits_required` means the plan's included usage is used up. The Fleet page
already reads Claude Code's local transcripts for the 5-hour window and the
week, so it needs no change.

## Platforms

- **Finding `claude`:** `PATH`, then the installer's usual folders
  (`~/.local/bin`, `/opt/homebrew/bin`, `%USERPROFILE%\.local\bin`), since an
  app started from the desktop may not see the shell's `PATH`. On Windows,
  `claude.exe` first, then `claude.cmd`.
- **Spawning:** no console window on Windows, stdin carries the prompt, and
  the working folder is the chat's workspace.
- **Stopping:** SIGINT on Linux and macOS. On Windows the process is ended,
  and that turn's result is lost; the chat says so.

## Build order

Each step is its own PR into `beta`.

1. **This note.**
2. **Detection** (ships with this note). `claude_plan::detect` (installed, version, signed in,
   billing kind) and the Settings row, with a fake `claude` script in tests.
3. **Stream parser.** Lines to chat events, with fixtures for every kind
   above.
4. **Engine.** `ClaudePlanEngine` behind an engine choice in the chat worker,
   read-only tools, the consent card and the per-chat switch. Proof: a real
   turn and a read-only tool task on Linux. The session flags from step 5
   ship here too, since a second message would otherwise start over.
5. **History.** Turns saved to the chat's history like native turns, so a
   chat on the plan survives a restart and shows in History.
6. **Usage.** The per-turn usage line and the plan headroom.
7. **Live approvals.** The MCP approval server.

## Open

- **Terms.** The page says running Claude Code "in your products or
  services (e.g. in hosted sandboxes or other agent infrastructure)" needs
  Anthropic's Commercial Terms. Whether that covers an open-source desktop
  app the user installs and runs themselves, against their own sign-in, isn't
  spelled out; the page points such questions to Anthropic. The design meets
  every listed condition either way. Showing plan limits may also read as
  "offering rate limits", which the Agent SDK page doesn't allow without
  approval, so step 6 shows tokens and the reset only, never as the plan's
  limit.
- **Subagents.** Tool calls inside a Claude subagent carry
  `parent_tool_use_id`. Whether the permission tool is asked for those isn't
  documented; it's checked in step 7.
- **Approval wait.** How long Claude Code waits for the permission tool's
  answer isn't documented; step 7 measures it.
- **Windows stop.** Whether `--resume` still works after a turn was ended on
  Windows is checked in step 5.
