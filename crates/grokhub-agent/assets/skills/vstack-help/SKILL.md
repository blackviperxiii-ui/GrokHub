---
name: vstack-help
description: Guides users through vstack setup, /viper-mode, and picking the skill, playbook, or principle for a task. Type /vstack-help with a question.
disable-model-invocation: true
---

# Vstack help

Answer the user's question about vstack, hand them a prompt they can send, and link the file the answer came from. For a help question, don't start the work. The user asked how, and a vstack run spends real tokens, so let them send the prompt.

A message that asks for work, such as "use vstack to fix this bug", is not a help question. Read [`viper-mode`](../viper-mode/SKILL.md), do the work under it, and mention once that in Cursor a Custom Mode keeps it on.

This file maps questions to the skills and docs that hold the answers. Those files own the details. Read the file you route to before you quote it, and trust it when it disagrees with this map. The links here point into the installed skills, which the user may not be able to open, so give the user the file's public copy: `https://github.com/blackviperxiii-ui/vstack/blob/main/` followed by its path from the repo root, such as `skills/how/SKILL.md`.

## Find out what they need

Infer the need from the message and the conversation. A named situation, such as "which skill reviews a PR?", goes straight to its section. If the need is still unclear, ask one multiple-choice question with these options, then answer only the section they pick:

- Get set up
- Start a task with `/viper-mode`
- Pick a skill for a situation
- Fix a run that went wrong
- Make vstack my own

Check the state that changes the answer, and mention it only when it does:

- Find the host first. vstack runs on Cursor, Claude Code, Codex, Antigravity, Muse, Windsurf, OpenCode and Grok Build, and install paths, command names and some features differ between them.
- No vstack pointer in the project's `AGENTS.md` or `CLAUDE.md`, and no always-applied vstack rule from the Cursor plugin, means agents there don't load vstack unless the user types a skill. `/setup-vstack` adds the pointer.
- In Cursor, no `~/.cursor/rules/vstack-models.mdc` means `/setup-vstack` hasn't written the model rule for this user, so every role runs on the chat's model. That is also the vstack default. On other hosts that file never exists, and the host's own model settings apply.
- No `verify-*` skill or other app harness in the project means agents have no scripted way to drive the app. Mention `/create-verification-skill` when the question is about proving a change works.
- No formal verification skill in the project means no property is checked past what tests reach. Mention `/create-formal-verification-skill` when the question is about a race, an invariant, or another case tests rarely hit. When one exists and the code it covers changed after the commit it records as last in sync, mention `/maintain-formal-verification-skill` if the question touches that code.

When setup hasn't run and it matters, ask whether the user wants to run it now. It matters when the user is new, the question is about setup or cost, or the answer depends on which models run. Ask at most once per chat. If the need is also unclear, ask both questions together. Offer two choices:

- Now: give them `/setup-vstack` to type, and answer their question too.
- Later: answer their question, and add one line saying every role keeps running on the chat's model until they run `/setup-vstack`.

## Get set up

1. Install for the host. Prefer a plugin or marketplace where the host has one. Never copy `skills/` by hand.
   - Cursor: install vstack as a plugin, then reload. The release installer does it with `curl -fsSL https://raw.githubusercontent.com/blackviperxiii-ui/vstack/main/install.sh | bash`. Cursor Projects and Cloud Agents need the team marketplace install instead.
   - Claude Code: `/plugin marketplace add blackviperxiii-ui/vstack`, then `/plugin install vstack@vstack`.
   - Codex, Antigravity, Muse, Windsurf and OpenCode: from a vstack clone, `./scripts/install_for.sh --host <name> --project <path>`. It installs into `.agents/skills/`.
   - Grok Build: `./scripts/install_for.sh --host grok --project <path>`. It installs into `.grok/skills/`.
2. Run [`/setup-vstack`](../setup-vstack/SKILL.md). It detects the host, installs for that host only, and makes sure the project's `AGENTS.md` or `CLAUDE.md` carries the short vstack pointer. In Cursor only, it also writes the model rule `~/.cursor/rules/vstack-models.mdc`, which applies to new chats. Then it offers once each to generate a verification skill and a formal verification skill for the project, unless the project already has one.
3. Start a real task with `/viper-mode`, a goal, and a check that can pass or fail.

What loads on its own depends on the install. The Cursor plugin ships one always-applied rule, and a consumer `AGENTS.md` pointer does the same job on other hosts. Both tell the agent to load vstack, start with `viper-mode`, and Research, Plan, then Execute. A skill whose frontmatter has `disable-model-invocation: true` loads only when the user types it or when `/viper-mode` runs it. The other skills can also load when the user's words match their description. The [README](../../README.md), [`docs/USAGE.md`](../../docs/USAGE.md) and [`docs/CONSUMING.md`](../../docs/CONSUMING.md) have the details, and `docs/CONSUMING.md` holds the full host matrix. Offer to word their first prompt with them, per [`references/prompting.md`](references/prompting.md).

If cost is the worry, say where the tokens go and how to spend fewer. vstack spends extra tokens on subagents and review panels, and it keeps both cheap by default. Subagents inherit the chat's model. The chat runs on Auto or Composer 2.5 Standard with Fast off. A review or design panel is several fresh subagents on that same model, and it uses different vendors' models only when the user asks for a high-stakes multi-model pass. The reasoning budget is medium or small. To spend less, keep the chat on Auto or a cheaper model, since every role follows it. Ask for a smaller panel, which runs fewer subagents. In Cursor, a line in the model rule overrides one role, and rerunning `/setup-vstack` rewrites the rule. Save `/viper-mode` for work that needs rigor.

Host differences worth saying:

- In Claude Code, each command from the plugin takes a `vstack:` prefix, such as `/vstack:viper-mode`.
- In Cursor, pick `/viper-mode` from the `/` menu so the skill attaches. Plain typed text may not attach it.
- Custom Modes, cloud agents, Automations and review bots (Bugbot and similar) are host features. On a host without one, a step that needs it is skipped with a one-line note.
- Most workflow skills, including `/viper-mode`, `/how`, and `/teach`, spawn subagents. On a host with no subagent tool, the parent does that work itself.
- Grok Bot, GrokHub, Aider and Continue get the `AGENTS.md` pointer only. `docs/CONSUMING.md` says what works there.

## Start a task with `/viper-mode`

`/viper-mode` matches the task to a playbook, copies the playbook's steps into the todo list, and runs the other skills as the steps need them. It always goes Research, Plan, then Execute, with no edits before a written plan. A good prompt states the goal and how to tell it's done. It doesn't list skills, because a hand-written sequence tends to drop or reorder steps the playbook would keep. Read [`references/prompting.md`](references/prompting.md) before you help word one. [`docs/USAGE.md`](../../docs/USAGE.md) has examples.

Whether `/viper-mode` stays on depends on how the user starts it:

- Where the project carries the vstack pointer or the Cursor plugin's rule, the agent loads `viper-mode` for every task without being asked.
- In Cursor, Enter on `/viper-mode` attaches the skill to one message. It fades as the chat moves on.
- In Cursor, Option+Enter on Mac or Alt+Enter on Windows, or Use as Mode from the skill entry, makes it a Custom Mode. It stays in context every turn until the user exits the mode, and it stays out of casual turns.
- Cursor's docs list Custom Modes in the Agents Window and the CLI. Elsewhere, and on other hosts, start each new task with `/viper-mode`.

Link [Cursor's skills docs](https://cursor.com/docs/skills) when this comes up in Cursor. Mid-chat, "new task" makes the mode match a fresh playbook. `/viper-mode` already starts each subagent its playbook steps need as a fresh subagent with a kickoff whose first line is `Read <path>/worker.md before anything else.`, where `<path>` is the absolute path of the `viper-mode` skill folder. To get the same rules from a subagent of your own, open its kickoff with that line.

## Pick a skill

The default answer is `/viper-mode`, which runs most of the others when its steps need them. Name a skill directly when the user wants more or less of something than the playbook gives. Read the skill before you recommend it, and give one example prompt.

| The user wants to | Skill |
|---|---|
| Do any non-trivial task with rigor | [`/viper-mode`](../viper-mode/SKILL.md) |
| Check that the agent understood their goal and the problem before it starts | [`/in-your-own-words`](../in-your-own-words/SKILL.md) |
| Get a prompt rewritten deeper, not bigger, before sending it: what a top professional would add, their raw material and methods, and an outside check. It returns the prompt and doesn't do the task | [`/sharpen`](../sharpen/SKILL.md) |
| Learn the expert words for what they're building, including words borrowed from neighboring fields, with every cited source checked, and get their description rewritten with them | [`/reframe`](../reframe/SKILL.md) |
| Know how code works now, or where new code should live | [`/how`](../how/SKILL.md) |
| Know why code is shaped this way, or where a number came from | [`/why`](../why/SKILL.md) |
| Understand a change or subsystem, explained plainly | [`/teach`](../teach/SKILL.md) |
| Catch up on their own recent work on a topic | [`/recall`](../recall/SKILL.md) |
| Catch up on what changed in this chat since their last message, in plain words and small visuals, with every item waiting on them listed and decidable from the reply alone | [`/ketchup`](../ketchup/SKILL.md) |
| Catch up like `/ketchup`, in a short subtitled video of slides, with the waiting calls as text under it | [`/ketchup-video`](../ketchup-video/SKILL.md) |
| Know what a small diff could break outside itself | [`/blast-radius`](../blast-radius/SKILL.md) |
| Settle types and module shape before code that crosses a function boundary | [`/architect`](../architect/SKILL.md) |
| Design tables, write or review a migration, isolate tenants, or choose a database | [`/database-architect`](../database-architect/SKILL.md) |
| Get several attempts at one brief, merged into the best one | [`/arena`](../arena/SKILL.md) |
| Run parallel checks over slices, or race workers, and get one report | [`/swarm`](../swarm/SKILL.md) |
| Have several fresh reviewers go at a diff and try to break it, keeping only findings a run proves | [`/interrogate`](../interrogate/SKILL.md) |
| Fix a bug test-first when a cheap local test exists | [`/tdd`](../tdd/SKILL.md) |
| Apply TypeScript rules to `.ts` or `.tsx` work | [`/typescript-best-practices`](../typescript-best-practices/SKILL.md) |
| Follow Google Chrome's modern web platform guides while writing or changing web frontend code | [`/modern-web-guidance`](../modern-web-guidance/SKILL.md) |
| Build, debug, or publish a Manifest V3 Chrome extension | [`/chrome-extensions`](../chrome-extensions/SKILL.md) |
| Strip comments before review, using a reviewer that didn't write them | [`/no-comments`](../no-comments/SKILL.md) |
| Clean AI tells out of prose | [`/unslop`](../unslop/SKILL.md) |
| Write docs, an RFC, a README, a PR description, or a commit message to a standard | [`/technical-writing`](../technical-writing/SKILL.md) |
| Hear the last reply again in plain words | [`/bro`](../bro/SKILL.md) |
| Give agents a scripted way to drive the app and prove behavior | [`/create-verification-skill`](../create-verification-skill/SKILL.md) |
| Bring a verification skill and its feature map back in line with the app | [`/maintain-verification-skill`](../maintain-verification-skill/SKILL.md) |
| Formally verify a race, invariant, or other property that tests rarely reach | [`/create-formal-verification-skill`](../create-formal-verification-skill/SKILL.md) |
| Bring a formal verification back in line after the code it covers changed, and learn whether a failure is drift or a bug | [`/maintain-formal-verification-skill`](../maintain-formal-verification-skill/SKILL.md) |
| Show a PM or designer how a system moves between states as a Mermaid statechart, with each bug as a numbered path | [`/statechart`](../statechart/SKILL.md) |
| Vet a performance number before reporting or acting on it | [`/benchmark-checklist`](../benchmark-checklist/SKILL.md) |
| Make a slow or heavy app faster: measure the real artifact, fix the root cause, measure again | [`/optimize`](../optimize/SKILL.md) |
| Find out why an app that works still doesn't feel right, across the whole app, with ranked low-effort fixes | [`/land-it`](../land-it/SKILL.md) |
| Raise a real UI toward a premium, modern, fluid feel: hierarchy, type, color, space, motion, states | [`/elevate`](../elevate/SKILL.md) |
| Build faux product UI for a marketing page, feature card, or docs hero, as a still or an animated cursor demo | [`/stageshow`](../stageshow/SKILL.md) |
| Run the last check before shipping: walk the real app, review the code, hunt security holes, missing tests and slop, and get one ranked ship or no-ship report | [`/qa-walkthrough`](../qa-walkthrough/SKILL.md) |
| Run that ship check for audio and video: playback, sync, codecs and devices, with `ffprobe` and `ffmpeg` evidence | [`/av-qa-walkthrough`](../av-qa-walkthrough/SKILL.md) |
| Find everything in an app that needs updating, get the safe bumps as PRs and the rest as cards | [`/updater`](../updater/SKILL.md) |
| Promote `beta` to `main`, bump the version, tag, publish release artifacts, and verify the install paths. It runs only on an explicit ask | [`/bag-and-tag`](../bag-and-tag/SKILL.md) |
| Run a large or cross-cutting change, or one to review after stepping away | [`/figure-it-out`](../figure-it-out/SKILL.md) |
| Hold an agent to its assigned tasks: a ledger, check-ins, stall detection, and done only with evidence | [`/follow-through`](../follow-through/SKILL.md) |
| Keep a decision log during a run, and review it afterward | [`/show-me-your-work`](../show-me-your-work/SKILL.md) |
| Install and configure vstack for the current coding agent | [`/setup-vstack`](../setup-vstack/SKILL.md) |
| Turn their own working habits into a personal mode skill | [`/automate-me`](../automate-me/SKILL.md) |
| Turn what a finished task taught into skill edits | [`/reflect`](../reflect/SKILL.md) |
| Stop agents from repeating the same mistakes in this repo | [`/correct`](../correct/SKILL.md) |
| Build a page whose buttons wake a Grok Bot over a webhook | [`/make-bot-ui`](../make-bot-ui/SKILL.md) |
| Find their way around vstack | `/vstack-help` |

If a skill directory next to this one is missing from the table, read its frontmatter and route by its description. The `principle-*` directories are covered under principles below.

Close calls:

- `/how` explains what the code does. `/why` explains the reasons. `/teach` runs one or both and explains the result plainly.
- `/arena` gives every worker the same brief and merges the best parts. `/swarm` splits work into slices or a race and returns one report.
- `/interrogate` reviews the diff. `/blast-radius` looks for breakage outside the diff and proves the one fact that makes the change safe.
- `/recall` rebuilds context across recent chats. `/ketchup` covers only this chat since the user's last message. Resuming one specific chat or branch is the Session pickup playbook.
- `/figure-it-out` designs one rigorous run. The Autonomous run playbook drives one task to a finish condition.
- `/architect` shapes code. `/database-architect` shapes the schema and its migrations. `/arena` takes a design that is large or contested.
- `/land-it` diagnoses where an app misses. `/elevate` raises the craft of a real UI. `/stageshow` builds fake UI for marketing. None of the three is a ship gate.
- `/qa-walkthrough` is the ship gate for an app. `/av-qa-walkthrough` is the ship gate for audio and video output. `/bag-and-tag` is the release itself, after the gate.
- `/optimize` makes the app faster. `/benchmark-checklist` vets one number before anyone trusts it.
- `/follow-through` tracks who owns each task and whether it moved. `/show-me-your-work` records each decision and its evidence.
- `/modern-web-guidance` covers the web platform. `/chrome-extensions` covers only extension work, and sends general web work inside the extension back to `/modern-web-guidance`.

Not in vstack:

- `/deslop`, `control-cli`, and `control-ui` ship in a separate Cursor plugin that vstack does not bundle. To remove code slop without it, apply `/no-comments` and the Laziness Protocol to the diff. To drive the real UI or CLI without it, use the host's browser, computer-use or terminal tool, as [`/qa-walkthrough`](../qa-walkthrough/SKILL.md) does.
- `/loop` and `/create-skill` are host built-ins, not vstack skills. A host without one has no stand-in from vstack.
- vstack has no `/orchestrate` skill. If the slash menu shows `/orchestrate`, another plugin provides it.

## Playbooks and principles

Playbooks are step lists inside `/viper-mode`, not skills, so they have no slash command. Inside `/viper-mode`, describing the task picks one, and these phrases name one directly:

- "babysit this pr" or "check on pr 123" runs Babysit. It drives the PR to merge-ready and stops there. It doesn't merge unless the user asks to merge, land, or ship.
- "land the stack" runs Shipping.
- "take over this branch" runs Session pickup.
- "pause safely" runs Pause safely.
- "full autopilot on this queue" runs Autopilot-full. "autopilot, don't merge" runs Autopilot-stack.
- "run the eval playbook" runs Eval.

Two jobs that sound like playbooks are skills. A performance problem in an app goes to `/optimize`. A release goes to `/bag-and-tag`, and only when the user asks for it in so many words.

Without `/viper-mode`, a phrase such as "babysit this pr" can start the host's own skill for the same job instead. The Playbooks section of [`viper-mode`](../viper-mode/SKILL.md) lists every playbook. Its PR playbooks cover opening, babysitting, and landing a PR.

vstack has no planning skill. The host's plan mode works alongside it, and `/viper-mode` writes a plan before any edit either way. For work that spans phases or stacked PRs, asking `/viper-mode` for a plan runs the Multi-phase plan playbook, which writes the plan and doesn't implement it. For a design question, the Prototype playbook or `/architect` settles it in code first.

Principles are one-rule skills that `/viper-mode` reads and cites in its replies. The user rarely invokes one. They steer with the names instead, as in "apply prove it works. show me the real output." Typing `/principle-<name>` still loads one on demand. vstack has 24 of them, one folder each next to this one, and the Principles section of [`viper-mode`](../viper-mode/SKILL.md) groups them. Read a principle's own file before you explain it.

## Fix a run that went wrong

| Symptom | Fix |
|---|---|
| The mode stopped applying after a few turns | In Cursor it was started with Enter. Start it as a Custom Mode, or start each task with `/viper-mode`. On any host, the `AGENTS.md` pointer from `/setup-vstack` keeps it loaded. |
| Typing `/viper-mode` did nothing in Cursor | Pick it from the `/` menu so the skill attaches. Plain typed text may not attach it. |
| The skills are missing from the menu | Run `/setup-vstack`. In Cursor it checks for a stale plugin cache and has the user reload the window. Cursor Projects and Cloud Agents need the team marketplace install in [`docs/CONSUMING.md`](../../docs/CONSUMING.md), then a new agent. |
| A question got treated as the next step of the last task | Say "new task", or say the turn doesn't need the mode. |
| A new model choice had no effect | In Cursor, the rule from `/setup-vstack` applies to new chats. Start one. On other hosts the host's own model setting decides. |
| Runs cost more than expected | See the cost paragraph under Get set up. |
| A skill didn't load on its own | A skill with `disable-model-invocation: true` loads only when the user types it or when `/viper-mode` runs it, and `/viper-mode` doesn't run every skill. Type the skill. |
| Parallel agents overwrote each other | Give each agent its own worktree, or run them as cloud agents where the host has them, which each get their own machine. |
| An overnight run moved but finished nothing | The run needs a check that can pass or fail, not a duration. Where the host has `/loop`, give it that check. See Before stepping away in [`references/prompting.md`](references/prompting.md). |
| The reply claims success from a green build | Ask for the real command, flow, stored value, or profile. That's the prove-it-works principle. |
| The agent said done and the work wasn't | Ask for `/follow-through`. It keeps a ledger and marks a task done only with evidence. |

For a run that drifts, [`references/prompting.md`](references/prompting.md) has one-line steers. [`references/recipes.md`](references/recipes.md) has the prompts worth copying.

## Make vstack my own

- [`/automate-me`](../automate-me/SKILL.md) drafts a personal mode skill from the user's own history, to use alongside `/viper-mode`.
- [`/reflect`](../reflect/SKILL.md) after a session turns its lessons into skill edits the user approves.
- [`/correct`](../correct/SKILL.md) turns a mistake agents keep making in the repo into a check that stops it.
- `/viper-mode write a skill for <workflow>` runs the authoring playbook. The eval playbook tests a skill change blind.
- Fix a misbehaving skill in its own PR, not inside the feature work where it went wrong.

## Reply

Lead with the answer. Give at most one example prompt in a code block, adapted from [`references/recipes.md`](references/recipes.md) when one fits, then the link to that file. Keep it short unless the user asked for the whole map.
