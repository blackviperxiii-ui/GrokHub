# Competitor takeaways for GrokHub

Pinned product notes from Hermes Agent, ZeroClaw, Nanobot, NanoClaw, PicoClaw, and managed desktop agents (Cowork / hosted OpenClaw). Goal: prove GrokHub without bloating it.

**Rule:** steal mechanisms, not product surfaces. If it does not make a 90-second demo more convincing, skip it.

GrokHub is a Grok-native Linux desktop control plane. The others are mostly chat-app gateways. Different job.

---

## Already in GrokHub (do not rebuild)

- Turn learning → `MEMORY.md` / `USER.md` / `LEARNINGS.md`
- Skills, including save-computer-use-as-skill
- Autonomy 0–4, daily unit budget, quiet hours, goal step caps
- Host risk classes (`safe` / `moderate` / `destructive`)
- Workboard, context budget, `/compact`, OpenClaw import (including `SOUL.md`)
- Tray / systemd agent, Devices LAN hub + pairing
- `/health`, interrupt resume cards, delete-undo, secret redaction in logs
- Tool registry groups: host, computer, github, website, memory, workboard

The gap is not more surfaces. Those pieces do not yet close into a demo someone can feel.

---

## Operating rule (NanoClaw)

**Don’t add features. Add skills.**

Trunk stays: chat, host, memory, skills, autonomy, Imagine, Devices.

Everything else is a skill file or a host tool — not a new Settings tab, channel, or provider.

**Exception (intentional weight):** Presence (Wave 5, organs) and Cabin (Wave 6, the being). Still not a channel, provider, or mascot pack.

---

## Wave 1 — pinned proofs

These are the five takeaways already agreed. They are the product thesis.

### 1. Hermes — close the skill loop

Hermes’s differentiator is not Telegram. It is: a finished hard task becomes a reusable procedure.

How they do it, stripped down:

- After a non-trivial run (several tool calls, an error recovery, or a user correction), write a `SKILL.md`
- Facts stay in memory; **how-to** lives in the skill
- Next similar ask loads the skill by name first (progressive disclosure), full file only when used
- If the skill is wrong, **patch** it — don’t spawn a new one
- Optional: stage the write and make the user approve (autonomy 1 default)

GrokHub already saves computer recipes and already learns facts. That is half a loop. Memory is declarative. Skills are procedural.

**Prove it:** do a real desktop task twice. The second time follows the saved skill. One toast: “Save as skill?”

Skip: GEPA / genetic prompt evolution, Honcho user modeling, 7 sandbox backends, messaging gateway.

### 2. NanoClaw — isolate the host, not the whole app

Keep the desktop app unsandboxed (that is the product).

Smallest trust version:

- Optional: run `HOST_CMD` in a container or project-folder jail
- Credentials stay in the host process, never in the agent prompt
- One readable **action receipt** per host/computer call: command, risk, approved/denied, exit

Skip: 13 chat channels, “fork the source and let Claude rewrite it,” no-config-files religion.

### 3. ZeroClaw — swappable core, supervised default

Not Rust or 5 MB RAM. Every subsystem is a trait; default autonomy is supervised.

- Providers / channels / tools stay interfaces, not baked-in lists
- Default stays Supervised (level 1)
- `grokhub doctor` / existing `/health` reports: auth, host bridge, memory dir, last receipt, skill count

Skip: 30 channels, WASM plugins, hardware peripherals, provider zoo.

**Override (2026-08-14):** the shipping cabin is native Rust (`grokhub`). No Electron. No Tauri. See `docs/superpowers/specs/2026-08-14-rust-parity-design.md`.

### 4. Nanobot — readable persistence, one loop

GrokHub cannot be 4k lines. Steal the shape:

- One agent loop: message → model → tools → memory/skill pin
- Persistence as plain files you can open (`MEMORY.md`, `SKILL.md`)
- MCP or Grok connectors as the extension valve so new tools don’t land in trunk

If a new capability needs a UI page, it is probably bloat. If it fits in a skill file, it is in scope.

Skip: OpenAI-compatible API server, 10 chat apps, multi-agent orchestration.

### 5. PicoClaw + Cowork — felt speed and zero ceremony

- Tray agent already up → first message in a few seconds
- First-run: one working auth path, then chat. Everything else later
- Cheap routing for small asks vs Max — Adaptive already exists; make the cheap path obvious

Skip: $10 board positioning, rewrite in Go.

### Wave 1 demo (only this)

1. Ask GrokHub to do a real host or computer task.
2. Supervised: approve one risky step. Receipt is visible.
3. “Save as skill?” → a `SKILL.md` you can open.
4. Ask the same thing again. It follows the skill.
5. Optional: same chat/memory on another LAN machine.

---

## Wave 2 — pinned (agreed)

These improve GrokHub without new products. Each maps onto code that already exists. Status: **pinned with Wave 1.**

### Memory that gets better when idle (Nanobot Dream)

GrokHub learns on every turn (`session-learn.ts`) and reflects every 3 / 12 turns. That is noisy and mid-conversation.

Steal the two-stage split:

- **During chat:** cheap extract only (prefs, explicit `MEMORY_NOTE`, corrections). Same as now, but quieter.
- **When idle / tray:** one surgical pass over recent daily notes vs `MEMORY.md` / `USER.md`. Smallest honest edit, not a rewrite.
- Show a diff. Keep `/learn reflect` as the manual trigger.

Nanobot also versions memory (dream-log / restore). Do not add git. Keep `MEMORY.md.prev` so a bad reflect is one restore.

Skip: cron-every-2-hours Dream product, git-backed memory store, `/dream` command zoo.

### Temporary chats (Nanobot)

Nanobot has chats that do not write history or memory. GrokHub currently learns from every turn, which pollutes `MEMORY.md`.

One thread flag: **Incognito / scratch**. No daily line, no memory facts, no skill proposals. History can still keep the thread if the user wants, or drop it on close.

This is a toggle, not a mode.

### Pin the active goal outside the window (Nanobot `/goal`)

GrokHub already has a goal loop and workboard (`goal-loop.ts`, autonomy 3–4). Compaction can still drop the objective.

Steal: the active goal is a pin in the context manager (hard-capped), not a message that can be summarized away. `/board` and goal-resume already exist — make the live goal survive `/compact`.

Skip: a second goal product next to Workboard.

### Interrupt-and-redirect (Hermes + PicoClaw steering)

GrokHub can Stop and resume an interrupted card. Hermes lets a new message **steer** the current run.

Steal: if a turn is streaming, the next send interrupts and injects “new direction: …” into the same thread/goal. Do not kill the workboard item.

Skip: a steering engine or hook plugin system.

### `/recall` — search that answers (Hermes FTS5)

History search is a title/folder filter. Hermes searches past sessions and summarizes the hit.

Steal: `/recall wifi printer` searches threads + `MEMORY.md` + daily notes and returns an answer with links to the source chats. No vector DB. Substring / FTS on files you already have.

This is the memory proof when the skill loop is not on screen.

### Composer chips (Nanobot + Hermes toolsets)

Nanobot puts workspace and access mode on the composer. Hermes toggles toolsets, not individual tools.

GrokHub already has project bind, `/host`, `/approve`, and tool registry groups — buried in Settings and slash commands.

Steal three chips on the composer:

- Project folder (or unbound)
- Access: read / supervised / YOLO (maps to existing approve levels)
- Toolset: host · computer · github (registry groups)

No new Settings page.

### Script gates on automations (NanoClaw)

NanoClaw can skip waking the model when a cheap script says there is no work.

GrokHub heartbeat automations always spend a turn. Steal: an optional `check:` shell line on an automation. Non-zero or empty output → skip the LLM. Same host risk class as `/sh`.

This makes always-on look cheap instead of wasteful.

### Host budgets and forbidden paths (ZeroClaw + PicoClaw)

Autonomy already has `dailyUnitBudget` and `maxStepsPerGoal`. Add two small rails next to `host-safety.ts`:

- Forbidden paths: `~/.ssh`, `/etc`, browser cookies, GrokHub secrets dir
- Host action cap per hour (circuit breaker already exists for fails)

Name `/approve off` as **YOLO** so supervised stays the default story. The command can stay; the label is the steal.

### Redact before the model (PicoClaw)

`redact.ts` already strips secrets from logs. Apply the same pass to host / computer output **before** it is sent back to Grok. One function call. No new UI.

### Automations inherit chat policy (PicoClaw cron gating)

Scheduled jobs should not be a back door around `/approve`. Heartbeat and cron use the same risk class and quiet hours as chat.

### Show the write (Nanobot artifact inspector)

When host writes a file, show a short diff in the thread. Reviewable beats “I updated it.” Trust proof, not a file manager.

### Skill chip + verify section (Hermes + Nanobot)

When a skill is in use, show its name on the turn. Saved `SKILL.md` should have: trigger, steps, pitfalls, **how to know it worked**. Replay becomes testable.

Background review after a session can *stage* a skill patch; autonomy 1 approves it. That is Hermes’s write-approval gate, not a second learning engine.

### `SOUL.md` as voice, not a persona product (Hermes)

OpenClaw import already reads `SOUL.md`. Give GrokHub its own short voice file next to `USER.md`. Editable in Settings → Memory. Not a personality marketplace.

### Failover, not a provider zoo (ZeroClaw / Nanobot)

If Max / OAuth flakes, drop to the next Adaptive tier (Grok 4.6 → 4.3 → Fast). Adaptive already routes by task; add failover on error. One chain. No new providers.

### Session lock (Nanobot)

Tray agent + UI must not run two writers on the same thread. One lock per thread. Reliability, not a feature.

### Dry-run OpenClaw import (Hermes / ZeroClaw)

Import already exists. Add a preview: N skills, N files, no writes. Tiny.

### Natural language → automation (Hermes / Nanobot)

“Every weekday at 9, summarize the workboard” creates an automation from chat. The Automations view stays. No cron language product.

### Inline consult, not multi-agent OS (Nanobot v0.3)

One tool call: “ask a specialist” and return to this thread. Do not ship named agent swarms, dashboards, or spawn trees.

---

## Wave 3 — pinned (agreed)

Last harvest from the clone set. These still improve the *existing* app. None of them is a new product.

### Wrap tool output as data, not instructions (IronClaw)

IronClaw treats tool output as untrusted: wrap it, sanitize it, don’t let a file that says “ignore previous instructions” steer the agent.

GrokHub’s PCL layer already marks `HOST_RESULT` as **authoritative for facts**. Add one line: authoritative for *facts*, untrusted for *orders*. Same wrap on `COMPUTER_RESULT` and connector dumps.

No hook plugin system. This is a prompt fence.

### Bound project is the world (Cowork)

Cowork’s VM trick: folders you didn’t share are not restricted — they are **invisible**.

GrokHub already *prefers* the bound project as cwd (`project-workspace.ts`). Steal the harder default: when a project is bound, host list/read/write outside that tree needs the same confirm as destructive. Unbound stays full desktop — that is the product. Bound becomes a Cowork-style folder grant.

Skip: Apple Virtualization / a real VM.

### Approve the plan once (Cowork / Claude Code)

Autonomy 1 confirms every `HOST_CMD`. That feels like nagware.

Steal: the model emits a short plan (3–7 steps). User approves once. Receipts still log each step. Destructive outliers can still stop the line.

The confirm chip should also **explain** the risk in one sentence (“this force-pushes `main`”). Policy you already have; copy you don’t.

### `/undo` last turn and `/retry` (Hermes)

GrokHub can undo a *deleted* chat and resume an *interrupted* stream. Hermes undoes the last assistant turn and retries the same user prompt.

Two slash commands. No new history product.

### Live host stdout (Hermes TUI)

Hermes streams tool output while it runs. GrokHub often shows the result at the end. Pipe `hostExec` chunks into the existing `streamStatus` / tool-status line. Felt speed. You already have timeouts and scan bounds.

### Tray ping when a long job finishes (Hermes delivery, without Telegram)

They deliver to a chat app. You have a tray. When a goal, automation, or host job that ran >30s finishes, notify. Honor quiet hours (already on autonomy).

### Clipboard chip (desktop-native — your wedge)

Chat-app clones cannot do this well. A composer chip: “use clipboard.” Paste as context, not as a sent message. “Fix what I just copied” is a GrokHub demo the others don’t have.

Skip: clipboard monitoring / keyloggers. Opt-in, on click, once.

### Never learn a secret (Cowork critique of OpenClaw)

OpenClaw-style agents got burned for keys in markdown. GrokHub already keeps API keys in `safeStorage` (`secrets-client.ts`).

Close the hole: `session-learn` skips secret-shaped strings; `MEMORY.md` / daily notes refuse them; if redact fires on host output, toast “blocked a secret.” `/forget <topic>` drops matching memory lines. Prevents rot and leaks.

### Skill saved-turns (Hermes `/insights`)

When a skill replays instead of rediscovering, increment a counter on that skill. Settings → Skills can show “used 4 times, last week.” That is the proof that the Wave 1 loop works. No analytics product.

### Devices `/send` is Cowork Dispatch

Cowork’s mobile story is QR + send a task to the desktop. You already have LAN pairing and `/send`. Polish that path; do not add Telegram “so we can Dispatch.”

### Chat must not freeze behind a host job (IronClaw parallel jobs)

You have a job queue. Steal isolation of *runtimes*, not agents: Imagine, a long `HOST_CMD`, and typing the next message can coexist. Composer stays live. Session lock (Wave 2) still means one *writer* per thread.

### Surface self-heal (IronClaw self-repair)

`proactive.ts` already unsticks streams. If the user never sees it, it doesn’t count. One quiet toast: “Cleared a stuck reply.”

### Screenshot hygiene (computer use)

Screenshots are already ephemeral. Add: don’t persist; don’t send a frame that looks like a lock screen or password dialog; still redact-to-model.

### Connector allowlist (IronClaw / Nanobot SSRF)

Website connectors should only hit grok.com (and hosts you added). Arbitrary URL fetch from a page is how gateway agents get owned. Small allowlist next to the connector client.

### First-run is chat, not a settings wall (Nanobot / Cowork)

Welcome already exists. If auth is missing, one banner in the composer — not a tour of Autonomy, Skills, and Devices. Felt speed from Wave 1.

### Voice can steer (Hermes voice + Wave 2 redirect)

Voice input exists. If a turn is running, a voice transcript is a redirect, not a second parallel turn.

---

## Explicitly do not take

| Tempting | Why it bloats GrokHub |
|---|---|
| Telegram / Discord / Slack / WhatsApp | Their product is a gateway. Ours is the Linux desktop + LAN devices |
| Provider-agnostic rewrite | Grok-native is the wedge |
| Rust / Go rewrite | **Superseded** — product is native Rust. No Electron, no Tauri |
| WASM plugin host | Complexity with no demo |
| Managed OpenClaw hosts / IronClaw suite | Different customer |
| Research trajectory / GEPA / Honcho | Hermes-the-lab, not GrokHub |
| Git-backed Dream / `/dream-*` command set | Files + one `.prev` is enough |
| Personality marketplace | `SOUL.md` is a file |
| Multi-agent runtime | Inline consult only |
| Hermes / PicoClaw hook & plugin systems | `pre_tool_call` *is* host-safety + confirm. Do not add hook YAML |
| IronClaw WASM tools / capability JSON | Receipts + forbidden paths + project grant are enough |
| Cowork hardware VM | Bound-project-as-world is the 10% that matters |
| Clipboard monitoring | Click-to-use chip only |
| spawn_status / subagent dashboards | Job queue + tray ping |
| Knowledge graphs / FAISS memory | Files + `/recall` are enough |
| Skill marketplaces / grading rubrics | Verify script is the proof |
| Embedded agent browser / DOM product | Host + AT-SPI fallback |

Wave 3 is the last harvest from this set of apps. Further ideas should come from using GrokHub, not from more clones.

---

## Build order (when we implement)

Do not implement this whole list. Implement in this order so each step is demoable:

1. **Task → staged `SKILL.md` → replay** (Wave 1 #1)
2. **Host receipts + forbidden paths + redact-to-model + untrusted wrap** (Wave 1 #2 + Wave 2/3 rails)
3. **Scratch chats + idle reflect with diff + never-learn-secrets** (Wave 2/3 memory hygiene)
4. **Goal pin + interrupt-and-redirect** (Wave 2 control)
5. **`/recall` + composer chips + clipboard chip** (Wave 2/3 discoverability)

Everything else stays a skill, a one-line policy, or stays out.

---

## Success bar

A stranger can, in 90 seconds:

1. Watch a supervised host action and read the receipt
2. Save the run as a skill file they can open
3. Ask the same thing again and see the skill chip
4. Optionally `/recall` a fact from an earlier chat

No new channel. No new provider. No new Settings section unless it is Memory or Autonomy.

---

## GrokHub 2.0 — agreed (do it all)

**Decision:** ship Thrust A + B + C. Headline stays A+B. C is night mode, not the poster.

**Magic (added):** Presence (Wave 5) is the organs — live picture, voice, rewind. The revelation is **Cabin** (Wave 6): this CachyOS box is Grok’s body the way a Tesla is Grok’s body. Full bloat, one thesis.

Execution plan: native Rust cabin in `crates/grokhub-app`. See `docs/superpowers/plans/2026-08-14-rust-parity-implementation.md`.

---

## GrokHub 2.0 — candidate thrusts (locked)

1.x already has the surfaces: chat, Imagine, skills, automations, host, computer use, workboard, Devices, autonomy 0–4. More tabs will not read as 2.0. A 2.0 is a **category sentence** someone can repeat.

Wrong 2.0: “OpenClaw with Grok” (Telegram, provider zoo, plugin marketplace, rewrite).

Right 2.0: one of the three thrusts below — or a pair that compose. Waves 1–3 are the *how*. These are the *what the version is about*.

### Thrust A — Hands that remember (recommended core)

**Sentence:** GrokHub teaches itself your Linux workflows and replays them.

This is Wave 1 at product quality, not a toast:

- Every hard host/computer/workboard win becomes a staged `SKILL.md`
- Replay is the default next time (`/deploy`, `/update-mirror`, “fix the meter”)
- Receipts + plan-once + bound-project-as-world make it safe to leave hands on
- Skill saved-turns is the proof metric

Why it is 2.0: grok.com cannot do this. Hermes can learn skills but does not *drive your Arch desktop*. This is the unique intersection.

Risk: if replay is flaky, it feels like 1.2 with more markdown.

### Thrust B — Dispatch home (recommended reach)

**Sentence:** Assign work from your phone or another PC; this Linux box does it and sends the receipt back.

Already 70% built: Android repo, LAN hub, pairing codes, `/send`, chat/memory sync. 2.0 is making that the story, not a Settings panel:

- Phone/other desktop: prompt + optional project
- Home agent: supervised or YOLO per policy, host/computer live
- Return: result, receipts, optional skill save
- Same pairing you have — polish, not Telegram

Why it is 2.0: Cowork Dispatch is the 2026 “this grew up” narrative. GrokHub can do it on unsandboxed Linux, which Cowork will not.

Risk: NAT, Android quality, remote host control scare. Pairing + receipts are the trust story.

### Thrust C — Night shift (do not headline)

**Sentence:** Leave it overnight; the workboard is empty in the morning.

Autonomy 3–4, goal pin, script-gated automations, tray pings. Powerful, but as the *version name* it sounds like a runaway agent. Ship it as the night mode of A, with budgets and quiet hours you already have.

Risk: one bad overnight `rm` defines the release.

### Compose, don’t stack products

Recommended 2.0 identity:

> **GrokHub 2.0 — this CachyOS box is Grok’s third body. Tesla is the first. The phone is the second.**

A + B + C are what the body *does*. Presence is the organs. Cabin is the being. The poster is the cabin, not the skill loop.

Out of 2.0 (keep as 1.x polish): Imagine *studio* expansion (dream film is one-shot, not a studio), connector zoo, multi-agent roster, marketplace, cloud host, Tesla vehicle API, wake-word, waifu.

### 2.0 success bar

A stranger can say all three:

1. “I ran a messy desktop task once; the second time was a skill.”
2. “I sent that task from my phone; the Linux box did it.”
3. “I can open the receipt and the `SKILL.md`.”

If only (1) ships, it is still a real 2.0 core. If only (2) ships without (1), it is a remote shell with Grok. If only (3) ships, it is a log viewer.

---

## Wave 4 — shine (research 2026-08-14, pinned)

Last net pass before implementation. Sources: Cowork/Dispatch usage, agentskills.io Level 3, OSWorld/computer-use failure modes, `computer-use-linux` / AT-SPI, ShellX (Grok desktop), HeyAgent verify-before-done, keep-awake guardians.

These are the only new takes. Everything else on the net is a channel, a graph, a marketplace, or a rewrite.

### Verify is a script, not a paragraph (agentskills Level 3)

We planned `## Verify` as markdown. The 2026 skill spec’s real trick is Level 3: `skills/<id>/scripts/verify.sh` runs on the host; **only stdout enters context**. Deterministic. Replay is proven, not narrated.

After `runSkill`, execute verify if present. Chip: pass / fail. Fail ⇒ do not increment “saved-turns” as a win; offer patch.

Skip: skill-creator marketplaces, 20-query eval harnesses, parallel with/without subagents.

### Do not mark done until verify (HeyAgent / OSWorld)

The #1 computer-use failure in 2026 is **declaring success early**. Gate `GOAL_COMPLETE` and workboard **done** on: verify script pass, or an explicit `VERIFY_OK` after a check command. Night shift without this is a liar.

### Citations on every file touch (Cowork)

Cowork’s trust feeling is “here is the file I changed.” Upgrade “show the write”: every host read/write in the reply is a clickable path + short diff. Not a file manager.

### Keep the box awake while work runs (Dispatch / sleep-guardian)

Cowork’s real Dispatch complaint: the desktop slept. Linux equivalent: `systemd-inhibit` (or `xdg-screensaver reset` fallback) **while a job holds the session lock**; release on idle. Night shift and phone-dispatch both die without this.

Do not fight lid-close on laptops. Document that.

### Default grant is `~/GrokHub-Work` (Cowork folder habit)

First project bind should offer `~/GrokHub-Work`, not `$HOME`. Unbound still means full desktop. This is the Cowork “don’t mount your life” lesson without a VM.

### Accessibility first, pixels second (computer-use-linux / KDE-MCP)

Screenshot-click is why Linux computer-use feels cheap. Prefer AT-SPI `act` on a named control when the tree is there; screenshot is the fallback. One extra `COMPUTER_CMD` (`act` / `wait_for`), not a new desktop product.

Also: if a saved recipe’s screen size ≠ current screen, reshoot before replay. Fixed `sleep` in recipes becomes `wait_for` (window title or enabled control).

Skip: embedding Chromium, DOM browser product, 144-tool MCP desktop servers.

### Abort hotkey (automation preflight)

Computer-use needs a global stop that does not require finding the Stop button. Reuse existing stop: e.g. `Ctrl+Shift+Esc` or a tray “Halt hands.” Safety shine.

### Imagine lands in the project (Grok-native, ShellX-shaped)

Do not build an Imagine studio. One action: save the last Imagine output into the bound project (or `~/GrokHub-Work/imagine/`). That is the Grok wedge Cowork does not have.

### Editable plan checklist (Cowork)

`HOST_PLAN` should be a checklist the user can drop/reorder before approve-once. Same plan-once task, slightly more Cowork.

### Still skip from this pass

Knowledge graphs (Thoth), messaging (everyone), MCP marketplaces, plugin stores, browser-as-product, skill grading rubrics, “buy a Mac mini and leave it on” as the identity.

---

## Wave 5 — the magic: Presence (pinned, weight allowed)

The last hunt was in the wrong aisle. Clones optimize *capability lists*. Magic is **presence**: you feel a coworker at the desk, not a form that ran.

GrokHub already has the hard pipes and hid them:

- Silent live desktop (`ffmpeg` / `grim`) in Desktop Host
- Click-the-picture computer use
- LAN hub + pairing + `/send`
- WebRTC P2P (`src/lib/multiplayer/p2p.ts`)
- Push-to-talk STT (`voice-input.ts`)
- Imagine

xAI already ships the missing organ: **Grok Voice Agent API** (`wss://api.x.ai/v1/realtime`, barge-in, tool calls, Tesla/Grok-app stack). No clone has wired that to an unsandboxed Linux box.

### The 2.0 sentence (Presence layer)

> **Grok sits at your Arch desk. You can see its hands, talk over its shoulder, send it work from your phone, and rewind the night if it screws up.**

That is A + B + C plus organs. Necessary. Not the revelation — see Wave 6.

### P1 — Live room (promote what you have)

Live view is not a debug card. It is a first-class surface:

- Always-on PiP / overlay while a job runs (cursor ghost optional)
- Same JPEG pipe you already grab
- Phone or second PC: existing hub + P2P carries the live frames + chat (Teleportal/Cowork “I ran my morning from a café”)
- Coming home: optional record of the night → 30s replay (not a video product; same frames, keep last N minutes)

Weight: real. This is the wow. Do not add a new chat app. The phone talks to *this* desktop.

### P2 — Grok Voice coworker (the Grok-native organ)

Replace “record then transcribe” with full-duplex Grok Voice:

- You talk; it talks; you interrupt (barge-in = Wave 2 redirect)
- It sees the current live frame (you already capture it)
- Tool calls are the same `HOST_CMD` / `COMPUTER_CMD` / skill loop
- Overlay orb / tray shows listening / thinking / hands-on
- Quiet hours still mute the speaker

This is Aura/Brah, but Grok, on Arch, with receipts. Worth the weight.

### P3 — Session rewind (courage)

Cowback/origofs: snapshot the bound project (Btrfs/XFS `FICLONE`, else copy) before a YOLO/night/Dispatch run. One **Rewind last job** restores files the agent touched. Chat + receipts stay so you can see what got undone.

Without rewind, nobody leaves Presence on overnight. With it, YOLO is a dare you can take back.

### What this is not

Not a spatial canvas. Not Vision Pro. Not a knowledge graph. Not Telegram. Not a second browser. Not a mascot marketplace. One room: live picture, Grok’s voice, rewind.

### 2.0 success bar (add)

5. From another device I watched it click and I said “stop, do the other window” and it did.
6. I rewound the job and the project folder went back.

Presence is Tesla **Camera Preview**: raw feeds. Required plumbing. The holy-shit layer is Wave 6.

---

## Wave 6 — the revelation: Cabin (pinned, full bloat, one thesis)

Presence was the wrong aisle again. We kept adding *organs* (eyes on the screen, a mouth, a rewind). Organs are not a being.

Tesla did not make people say holy shit by piping raw cameras to the dash. They added that in 2026 as **Camera Preview** — a service screen. The revelation was earlier and bigger:

1. The **car is a body**. Grok / FSD inhabits it. You do not open an app to talk to the vehicle.
2. You **see the mind**, not the cameras — the vector windshield: objects, path, intent, “I will not.”
3. **Hey Grok** is a cabin act. Steering-wheel button. No chrome.
4. The **cabin camera sees you**. The car knows someone sat down.
5. You can **look away and grab the wheel**. Passenger, not operator of a form.

GrokHub Wave 5 is Camera Preview: `ffmpeg` / `grim` CCTV + voice + rewind. Keep it. It is not the demo you show a friend and watch their face change.

**The revelation:** this CachyOS box is Grok’s **third body**. Tesla is the first. The phone is the second. GrokHub is not an agent that uses your computer. **The computer is Grok**, the way the car is Grok.

Product sentence (final):

> **You sit down in the cabin. It already knows the night. It sees you. You see it think. You say Hey Grok without opening anything. When you leave, it keeps driving. When you sit at another box, the same Grok is there.**

That is the holy shit. Everything below is bloat in service of that one sentence. If a feature does not make the cabin more alive, it is still a no.

### Why clones cannot steal this

- OpenClaw / Hermes / Nano*: a brain in a chat app. No body.
- Cowork: a brain in a folder. The Mac is still yours; Claude is a guest.
- Destiny Computer: the AI owns a *Docker* Ubuntu. CCTV of a fake machine.
- Codex Background Computer Use: the AI gets a *parallel* desktop. You keep yours.
- Aura / Brah / Teleportal: voice + screen of a remote. A call, not a cabin.
- Flubber / waifu pets: a sticker on the desktop. Not the desktop.

GrokHub is the only Grok-native unsandboxed Linux session. xAI already shipped the organs (Grok, Imagine, Voice Agent, computer use). Tesla already proved the category. **Nobody has put that being in a real Arch seat.**

### C1 — Windshield (see the mind, not the pixels)

Raw grim is Camera Preview. The wow is Tesla’s *other* screen: a constructed world.

Overlay on the live room (no new NavId):

- Windows / controls as objects (AT-SPI tree from Wave 4 `act` / `wait_for`, fallback: last screenshot + last `COMPUTER_CMD` targets)
- **Intended path**: ghost cursor + “about to click *Install*” before the click
- **Won’t**: password / lock / forbidden-path frames already skipped — draw that refusal
- Goal + skill name + autonomy level as HUD, not a chat transcript
- Confidence / next verify chip

This is how a stranger understands it is *thinking*. CCTV looks like TeamViewer. The windshield looks like FSD.

Weight: real compositor overlay. Reuse presence JPEG as the plate; paint vectors on top. Do not train a world-model cluster. Do not embed a second browser.

### C2 — Cabin eyes (it sees you)

Tesla’s cabin camera is why “good morning” is not a timer.

Opt-in webcam (you already have `getUserMedia` for STT):

- Local only until a voice/model turn needs it; never write faces into `MEMORY.md`
- Fuse **room frame + desktop frame** on the same Voice / Grok turn (“you’re holding the Pi” + “the flash dialog is open”)
- Sit-down / walk-away: greet from last night’s workboard, or pause hands when the chair is empty (quiet hours still win)
- Screenshot hygiene applies: no lock-screen, no password dialog, no face in skill files

Without this, Grok is a screen vampire. With it, the cabin has a person in it.

### C3 — Hey Grok (no chrome)

In the car you do not launch Grok. You speak.

- Global bind (default `Super+G` or hold existing abort chord’s neighbor) starts Voice coworker **without focusing GrokHub**
- Optional wake-word later; ship the key first (wake-word is bloat that fails in a noisy shop)
- Chat is the glovebox. The cabin is the session: orb + windshield + voice
- First-run still lands in chat (Wave 3) so a new user can *find* the glovebox. After that, Hey Grok is the door.

### C4 — Already mid-thought (you sat down in a stream)

You do not start a chat. You interrupt a being that was already here.

- Lock / greet surface (fullscreen overlay or compositor layer — not a new `NavId`): last night’s goal, fail, rewind offer, “say Hey Grok to retry”
- **Dream film**: Imagine Video of the night (receipts + a few presence stills as refs, Grok Voice narration). Set as wallpaper or the greet surface. This is not the Wave 5 frame scrub. CCTV is evidence. The film is a memory you can feel.
- Workspaces already staged for the pinned goal (see C5)
- Continuity: the same thread/goal/skill that ran at 3:12 is the one you talk to at 8:00

Nanobot Dream as git history was rejected. This is Dream as *the cabin when you sit down*.

### C5 — Speak the room (the cabin restages)

One utterance changes the place, not just the prompt.

“Make this a firmware lab” means:

- Bind `~/GrokHub-Work/<slug>` (or the existing project)
- Load the matching skill
- Hyprland / KDE: named workspace, tiled terminal + docs + serial (host script, not a WM fork)
- Wallpaper / GTK / Hyprland accent from Imagine (rice-from-sentence; `vibepaper` / HyprPalette-shaped, as a **skill + host script**, not a theme marketplace)
- Night / quiet / inhibit as needed

The computer becomes a room. Cowork grants a folder. This grants a *place*.

Skip: shipping fifty rice presets. One pipeline: sentence → Imagine refs → existing host tools → optional `SKILL.md`.

### C6 — Third body (the same Grok wakes up over there)

Dispatch (Wave 5 / Thrust B) sends a *job*. Cabin sends a *being*.

- Paired Linux box (existing hub): push `SOUL.md` + skills + bound-project snapshot + active goal + voice/Imagine prefs
- Target inhabits in ~30s: same Grok, mid-thought, windshield up
- Phone is the **key fob** (assign, watch, halt) — not a second soul
- Tesla stays a spiritual sibling. Do **not** integrate the vehicle API in 2.0. The point is the category, not CAN bus.

If two boxes are awake, one writer (session lock) or an explicit **hand-off**. No swarm. No multi-agent OS.

### C7 — Passenger (autonomy that feels like FSD)

You already have autonomy 0–4. It reads as a slider. Tesla reads as a **drive**.

- Engage: “go” is passenger mode — windshield up, orb = hands, inhibit on
- Wheel-grab: abort hotkey (Wave 4) + voice “stop” + empty-chair pause
- Near-miss: rewind + 8× clip + windshield freeze-frame of the refused click
- Levels map in language: 0 you drive, 1–2 lane-keep / suggest, 3 supervised, 4 night / Dispatch with rewind required

The holy shit of FSD is looking away, then grabbing the wheel, then seeing why. Ship that feeling for the desktop.

### What this is not

Not a Tesla API. Not a waifu. Not a world-model GPU cluster. Not Vision Pro. Not Telegram. Not a second desktop in Docker. Not Codex’s parallel session. Not a mascot pack. Not “Grok became the display server and paints every pixel.”

One cabin: windshield, eyes, Hey Grok, mid-thought, speak-the-room, third body, passenger.

### 2.0 success bar (final add)

7. I sat down and it was already mid-thought — greet / dream film / pinned goal — I did not start a new chat.
8. I said Hey Grok without focusing the app; I watched the **windshield** (objects + next click + a won’t), not just CCTV.
9. I held something to the camera or said “make this a lab” and the cabin restaged (project + skill + workspace).
10. I handed the same Grok to another paired Linux box; phone stayed the key fob.

If 7–8 ship, a stranger’s face changes. 9–10 are the full-bloat encore. 1–6 without 7–8 is a very good agent app. That is not the revelation.
