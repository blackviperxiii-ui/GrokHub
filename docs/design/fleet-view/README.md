# Fleet view: spike note

A Fleet page in the desktop app (Tauri only) shows every machine running the
app and the usage of each coding subscription, in one place. This note lists
where each number comes from, what is and isn't possible today, and the build
order. It ships before any code.

## Machines

**What exists.** The hub already knows its paired peers: `Peer { id, name,
token, last_seen }` (`crates/grokhub-core/src/state.rs`). `/v1/status` returns
each peer as `{id, name, role}`, and every authed call moves `last_seen`. No
peer reports its CPU, memory or version. The local machine's CPU comes from
`/proc/stat` (`src-tauri/src/pc_health.rs`, only while a game runs), and its
memory from `grokhub_agent::device::{ram_mb, free_ram_mb}`. The app's version
is `shell_update::version()`, and `check_shell_update` knows the newest
release.

**Plan.** Each machine reports a small card about itself, and paired
machines push it to the hub they're paired with:

| Field | Source |
| --- | --- |
| Name | The hub's device name (`HubState.device_name`) |
| Online | Last report under 3 minutes old; older ones show dimmed, never hidden |
| CPU % | The `/proc/stat` change since the last reading on Linux; Windows and macOS show a dash for now |
| Memory | `ram_mb` and `free_ram_mb` |
| App version | `shell_update::version()`, flagged when `check_shell_update` has a newer one |
| Last seen | The report's time |

- A new authed `POST /v1/fleet` takes a machine's card every 60 seconds, and
  `GET /v1/fleet` lists the cards the hub holds. Both use the existing peer
  token, so nothing new is shared and nothing goes off the LAN.
- The hub keeps the last card per peer while it runs (`HubState.fleet`), so
  a computer that went to sleep still shows its last values, dimmed. A
  restart of the hub clears them until each computer reports again.
- The app can share its hub (Devices → Start share) but can't yet pair with
  another computer's hub as a client, so a second computer needs a join
  step (address and code, the token kept in the OS keychain) before it can
  push its card.
- Another app's version (any agent tool the person runs) is a configured
  command whose first line of output is the version. Nothing is guessed.

## Subscriptions

One row per account: the rolling-window bar, the weekly or monthly bar, the
time until each reset, the plan tier, and whether it's in use now.

| Subscription | Data source | Status |
| --- | --- | --- |
| Grok (SuperGrok) | The `x-ratelimit-*` headers the app already reads (`plan_room::latest()`: requests and tokens left, the reset) and the plan tier (`Entitlement.tier`) | Live: the data is already in the app |
| Claude | Claude Code's local transcripts (`~/.claude/projects/**/*.jsonl`). Each assistant line carries `message.usage` (input, output and cache tokens) and a `timestamp`, so the page can sum the current 5-hour window and the last 7 days | Live, local and read-only. Anthropic doesn't publish plan limits as token numbers, so the bars show tokens used and the window's reset. They show a percent only when the person enters their limit. Claude Code's sign-in is never read or reused |
| Cursor | No official per-user usage API. The Admin and Analytics APIs are for Enterprise teams only (`cursor.com/docs/api`). Personal usage shows only in the editor and the web dashboard | A row the person fills in: percent used and the reset date, kept locally, with a link to the dashboard. A team with an Admin API key (kept in the OS keychain) gets the live pooled usage instead |
| Antigravity | No documented read-only API. The CLI's `/usage` (alias `/quota`) opens an interactive panel only, and the IDE shows quotas on its Settings page (`antigravity.google/docs/cli/commands/usage`, `/docs/plans`). The CLI says it keeps the quota status on disk, but the format isn't documented | A row the person fills in, for now: the 5-hour and weekly use per model group, with the resets. Next, the on-disk quota file is checked on a machine where the CLI is installed. If it's a plain local file, the page reads it read-only. No cookies, no Google sign-in, no unofficial auth |

Rules for every row:

- Read-only, from local files or an official API. Credentials live in the OS
  keychain and never show on the page or in a log.
- A row that fails shows its last value and when it was read, never a blank.
- Refresh every 5 minutes, and when the page opens. Claude's files are read
  from the end, newest first, so a big history stays cheap.

## Look

Black background, red `#D7263D` for outlines and for bars over 90 %, and amber
only on the page's one action (Refresh). Muted text uses the theme's text
tokens, with no gray fills.

## Build order

Each step is its own PR into `beta`.

1. **This note.**
2. **Machine cards.** The card type and its staleness (`grokhub-core`
   `fleet`), `POST` and `GET /v1/fleet` on the hub, and this computer's card
   (CPU on Linux, memory, version) through the `fleet_machines` command.
3. **Join a hub.** Pair this computer with another computer's hub as a
   client and push its card every 60 seconds.
4. **Usage parsers.** The Claude Code transcript window, the Grok plan room as
   a row, and the rows the person fills in for Cursor and Antigravity, with
   tests on fixture files.
5. **Fleet page.** On the rail next to Devices: machines on top, then
   subscription rows, dimmed rows for machines that are asleep.
6. **Antigravity live row**, if step 4's check finds a readable local file.

## Skipped in this note

- Agent task lists beside each row need a task board connector. They aren't
  part of this feature.
