# Claude plan provider: design note

The desktop app (Tauri only) can run chats on the user's own Claude plan by
starting the official Claude Code CLI on this computer. The user signs in
through Claude Code's own `claude auth login`. The app never reads, copies,
stores or passes on a Claude credential or token, never shows its own Claude
sign-in screen, and never changes the `claude` binary. Requests count against
the user's plan, the same as when they run Claude Code themselves.

This note ships before any code. It lists what the CLI offers, how it plugs
into the app, the build order, and what stays open.

## Why the CLI, and not the plan's token

Anthropic's terms (code.claude.com/docs/en/legal-and-compliance) allow "an
end user signing in to the unmodified Claude Code binary with their own
Claude subscription". They don't allow an app to collect, store or relay
claude.ai credentials, or to send requests through plan credentials itself.
So the app starts `claude` and reads its output. It never sends a request to
Anthropic itself, and it never impersonates Claude Code.

Naming follows the same page: the provider is **Claude (your plan)**, with
the line "runs Claude Code on this computer". "Claude Code" is never used as
the provider's or the app's name. The existing API-key provider stays
**Claude (Anthropic)**.

## What the CLI offers

Sources: `code.claude.com/docs/en/cli-reference`, `/headless`,
`/agent-sdk/typescript`.

| Need | CLI |
| --- | --- |
| Installed | `claude --version` exits 0 |
| Signed in | `claude auth status` prints JSON and exits 0 when signed in, 1 when not. Only `authMethod` is read: `claude.ai` means the plan; `api_key` and the rest mean API billing, and the page says so. Nothing else from the output is kept or logged |
| One turn | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages`, the prompt as one JSON line on stdin |
| Model | `--model` with an alias (`opus`, `sonnet`, `haiku`) |
| Same conversation | `--session-id <uuid>` on the first turn, `--resume <uuid>` after. `--continue` skips `-p` sessions, so it isn't used |
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
**Claude (your plan)** row showing installed, signed in and the billing kind,
with Allow for chats (the same consent card the other providers use). Once
allowed, a chat can be switched to it from the chat's menu, and the choice is
kept per chat. Auto routing never picks it on its own.

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
2. **Detection.** `claude_plan::detect` (installed, version, signed in,
   billing kind) and the Settings row, with a fake `claude` script in tests.
3. **Stream parser.** Lines to chat events, with fixtures for every kind
   above.
4. **Engine.** `ClaudePlanEngine` behind an engine choice in the chat worker,
   read-only tools, the consent card and the per-chat switch. Proof: a real
   turn and a read-only tool task on Linux.
5. **Resume and history.** `--session-id` and `--resume`, and turns saved to
   the chat's history like native turns.
6. **Usage.** The per-turn usage line and the plan headroom.
7. **Live approvals.** The MCP approval server.

## Open

- **Terms.** "Ordinary use" isn't defined further, and showing plan limits
  may read as offering rate limits. The design stays on the side of the
  wording that allows it: the user's own sign-in, the unmodified binary,
  nothing intermediated.
- **Subagents.** Tool calls inside a Claude subagent carry
  `parent_tool_use_id`. Whether the permission tool is asked for those isn't
  documented; it's checked in step 7.
- **Approval wait.** How long Claude Code waits for the permission tool's
  answer isn't documented; step 7 measures it.
- **Windows stop.** Whether `--resume` still works after a turn was ended on
  Windows is checked in step 5.
