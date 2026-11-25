# Agent teams

An agent team is a set of agent profiles that each run as their own agent:
their own chats, memory, skills, connections and permissions. Agents on a team
can hand each other work over an A2A-style path (the Agent2Agent protocol,
v1.0 shapes). This page describes the data, the isolation model and how the
pieces build on what the app already has.

## Words

| Word | Meaning |
|---|---|
| Profile | One agent: who it is (`about`), what kind it is, what it may do, who it may message. |
| Kind | `role` (a job such as researcher or reviewer), `family`, `staff` or `customer`. The kind sets safe defaults. |
| Team | A named group with one lead. Members and the lead may message each other. |
| Home | The agent's own config folder, `{config}/agents/<id>/`. |
| Task | One piece of work one agent asked another to do, with a state (A2A `TaskState`). |

Profiles and teams live in `{config}/agent-team.json` (`grokhub_core::agent_team::TeamFile`).

## Isolation model

Each agent turn runs with the config folder pinned to the agent's home (the
same `ConfigGuard` the app's helpers already carry across threads). What
moves with the pin is the agent's own; what the core crate keeps under the
app's folder stays shared. Every thread a turn starts for its own work
(batched tool calls, background compaction, MCP connects) takes the pin with
it. Shared state the agent crate keeps is read from the app's folder inside a
turn too: the router's (`perm::app_config_dir`), and egress consent, its log
and its cards, which the egress guard and the consent ledger resolve to the
app's folder for an agent's home. The person's chat words ("use low effort
for debugging") steer their chat and its helpers, not an agent. An agent's
shell sees its own workspace and none of the app's folder around it: the
app's files, the other agents' homes and its own files outside `work/` are
hidden, and the app's sandbox settings apply. The hard floor guards the
skills and memory folders of the app and of every agent.

| Its own (under its home) | Shared with the app | Never crosses a message |
|---|---|---|
| Chat history (`sessions/`) | Model sign-ins and keys | Its chat history |
| Memory (`memory/`: MEMORY.md, USER.md, SOUL.md, index) | Offline only | Its memory |
| Skills (`skills/`) and plugins | Egress consent and the EgressGuard | Files in its workspace |
| MCP connections (`mcp.json`) | The hard floor and the OS sandbox | Tool output, except the reply it chooses to send |
| Permission rules and grants | | |
| Automations | | |
| Workspace (`work/`) | | |

A message carries only text and JSON data parts, capped in size. The agent
that receives it decides what to do with its own permissions: a message can't
lend the sender's permissions, tools or memory.

### What each kind may do

| Kind | Access by default | Desktop | May start a message |
|---|---|---|---|
| `role` | Ask | Off | To the agents in `talks_to`, and its team lead |
| `family` | Ask | Off | Same as `role` |
| `staff` | Ask | Off | Same as `role` |
| `customer` | Read only | Never | Never: it only answers |

Access is `read_only` (no writes, no shell), `ask` (unattended runs refuse
anything that would need a person) or `auto` (the app's judge decides, inside
the hard floor and the sandbox). A customer profile can't be saved with
`auto`, desktop or `talks_to`.

### Who may message whom

- The person (the main chat or the app) may message any agent.
- An agent may message the agents in its `talks_to` list.
- On a team, the lead and each member may message each other.
- A customer agent never starts a message.
- A chain of messages stops at 3 hops, and an agent already in the chain
  can't be asked again (no loops).
- A paired computer may message only an agent marked `share` whose
  `share_with` lists its device id. To that computer, any other agent looks
  missing. Its turn runs read only and unattended, with only `read_file`,
  `list_dir`, `grep` and `glob`, under the automation origin, and the
  message is fenced as outside text (`grokhub_agent::answer_peer`).

## A2A shapes

`grokhub_core::a2a` holds the protocol shapes, serialized as in A2A v1.0:

- `Message` (`messageId`, `contextId`, `taskId`, `role` `ROLE_USER` or
  `ROLE_AGENT`, `parts`, `metadata`), `Part` (`{"text"}` or `{"data",
  "mediaType"}`; file parts are refused).
- `Task` (`id`, `contextId`, `status`, `artifacts`, `history`) and
  `TaskState` (`TASK_STATE_SUBMITTED`, `_WORKING`, `_INPUT_REQUIRED`,
  `_COMPLETED`, `_FAILED`, `_CANCELED`, `_REJECTED`, `_AUTH_REQUIRED`).
- JSON-RPC 2.0 methods `SendMessage`, `GetTask`, `ListTasks` and
  `CancelTask`. The others answer `UnsupportedOperationError` (-32004).
  A2A error codes -32001 to -32009 keep their spec names.
- `AgentCard` built from a profile: one skill per profile, JSON-RPC binding,
  no streaming and no push notifications.

`TaskBook` keeps tasks in memory and enforces the lifecycle: a terminal task
(completed, failed, canceled, rejected) takes no more messages, and only a
task that hasn't ended can be canceled.

## What it builds on

- **Helpers.** A profile turn is a helper run with the config pinned to the
  agent's home, the profile's gate (never wider than the caller's, never
  `Always`) and fresh system text built from the home, with the profile's
  `about` as the persona. The caller's history and system text don't cross.
- **`message_agent`.** The tool is offered only while the sender may message
  at least one agent, and its `agent` argument lists just those, so a chat
  with no team file sees the same tool list as before. A read-only chat
  isn't offered it. Inside an agent's turn the sender is that agent (the
  pinned home names it), and helpers stop at depth 2.
- **Rules.** The agent's turn keeps the caller's deny and ask rules (they
  only tighten) and adds its own rules and grants from its home. The
  caller's allow rules and grants stay the caller's.
- **Tools.** A profile's `tools` list binds the helper's tool set, kept
  inside any job tool set the caller already runs under.
- **Playbooks.** The agent's message is sorted without a model, and the
  usual pick chooses from the app's playbooks the one that fits. A profile
  that lists `playbooks` picks only among those. An unclear message gets
  none, as in a chat. The steps go into the agent's system text, before its
  persona.
- **Models.** The router's state stays the app's: an agent's calls route
  over the app's model list, local models, spend and benches
  (`perm::app_config_dir`), not over its home. A profile can name a
  `model_profiles.toml` tier (for example `workhorse`). Auto then picks
  among that tier's models it could send (rule `agent:tier`); the device
  has no tier and stays in reach. A tier no such model carries picks as
  usual (rule `agent:tier:missing`).

## Build order

1. Core: profiles, teams, the talk policy, A2A shapes and the task book (this
   page).
2. Agent crate: the isolated profile turn and the `message_agent` tool.
3. Agent crate: the playbook and model tier hooks.
4. Desktop app: a Team page to make profiles from the four kinds, and a task
   log.
5. Hub: agent cards and the JSON-RPC endpoint for paired peers, for profiles
   marked `share`.
