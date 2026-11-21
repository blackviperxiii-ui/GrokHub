# Script-first runtime: design note

Loadout should run well on any computer, including a weak one with no GPU.
To get there, scripts do every step they can, and the model is only the
brain: it decides how a task is done and fills in what the scripts need. No
model runs in the background deciding everything. Once a task is solved, its
steps are saved, so the next run needs fewer model calls, or none.

This note is the first deliverable. It changes no code. It covers:

- what exists today;
- the target;
- the saved-step format;
- when the model wakes;
- the build order.

Each phase is one small PR, and no code starts until this note is approved.

## What happens today

Several script-first pieces already ship. Each covers one path.

- **Script-first automations.** In `grokhub_core::automation_check`, a
  job's check command runs first, and its `CheckGate` picks the next step:
  - wait for the next slot;
  - wait with no model call when `on_change` sees the same output;
  - finish with the output alone (`no_agent`);
  - or wake the model with only what the check printed.

  `grokhub_agent::check_decide` adds one cheap judge call that is
  schema-constrained. A reply that doesn't parse falls back to the full
  turn. `blueprint_catalog.rs` seeds common jobs.
- **Triggers are timers, webhooks and chat.**
  - `src-tauri/src/scheduler.rs` fires `automations.json` jobs on a clock.
  - Signed webhooks and paired Telegram or Discord messages are queued in
    `channels/inbox/`. `channel_inbox.rs` gives each one a full read-only
    run (#2106).
  - Nothing watches files or folders.
- **Actions are tools.** Every tool has a JSON input schema
  (`tools/registry.rs`, `tools/defs.rs`). `tools/ptc.rs` runs a short Python
  script in the OS sandbox, with a bridge to read-only tools only.
  `tools/batch.rs` runs a reply's read-only calls in parallel.
- **Saved know-how is written for the model.**
  - A skill (`{config}/skills/*/SKILL.md`) is instructions that the model
    reads and then plans from.
  - Save as skill (`screen_skill.rs`) turns a screen recording into one.
  - `grokhub_core::recipe` already keeps a *desktop* replay: a screen size
    plus a list of `ComputerOp`s that replays without the model.
  - Skill patches pass the dry-run replay gate in `harness/self_improve.rs`.
  - Every write goes through the ChangeLedger, so it can be undone.
- **Model tiers exist.**
  - The router's class table (`route/policy.rs`) sets effort per class.
  - A local model can serve only `background:` classify, triage, redact and
    summarize (`route/local.rs` `LOCAL_CLASSES`).
  - Judges use `background:judge`.
- **One gate.** Every step, whether it comes from the model, a desktop
  replay or a proactive act, goes through `harness::approval::decide`. That
  covers the hard floor, `HardClass` parks (which park even under Always),
  Approvals, and the egress guard. `unattended::run` closes permits that
  nobody is there to answer.

**What's missing**

- Outside a check gate, every run is a full agent turn that plans from
  scratch, even the hundredth time it does the same thing.
- Steps that worked are not kept in a form that runs without the model.
- A failed step ends the run. There is no small "what now?" question to the
  model.
- There are no file triggers.

## Target

### Routines

A **routine** is a saved list of steps that runs without the model. The
name *recipe* is already taken by the desktop replay, which becomes one step
kind below.

Routines live in `{config}/routines/<name>/routine.json`. Scripts sit next
to the file. Each write goes through the ChangeLedger, so `/routines undo`
works the way `/skills undo` does.

```json
{
  "version": 1,
  "name": "disk-report",
  "about": "Tell me when the system disk passes 90%.",
  "triggers": [{ "timer": "daily 09:00" }],
  "inputs": {
    "mount": { "type": "string", "default": "/" }
  },
  "steps": [
    { "id": "df", "tool": "run_terminal_command", "args": { "command": "df -P {{inputs.mount}}" } },
    { "id": "use", "expect": { "from": "df.stdout", "regex": "(\\d+)%", "group": 1, "at_least": 90 },
      "else": "done" }
  ],
  "result": "{{inputs.mount}} is {{use.value}}% full."
}
```

**Step kinds** (one key picks the kind):

| Kind | Runs | Model |
|---|---|---|
| `tool` | One registered tool, with arguments from the template. | none |
| `script` | A file in the routine's folder, in the PTC sandbox. | none |
| `desktop` | A saved desktop recipe (`grokhub_core::recipe`). | none |
| `expect` | A fixed check on an earlier output: exit code, regex, JSON path, number compare. Picks the next step or `done`. | none |
| `ask` | A decision point: one schema-constrained call that returns a named branch or input values. | small |

**Templates** only substitute values: `{{inputs.x}}`,
`{{<step>.stdout}}`, `{{<step>.json.<path>}}` and `{{<step>.value}}`. There
are no expressions and no loops, so a routine can be read top to bottom.
In a command, the runner quotes each substituted value as one argument;
it is never spliced in raw. The rest of a command's text is fixed when the
routine is saved.

`result` is the run's answer. It shows where automation results show today,
and it goes back to the chat or webhook that triggered the run. A run that
ends at `done` before the last step has no result, so nothing is shown.

**Triggers:**

- `timer`, through the scheduler;
- `webhook`, by the hook's name, from `channels/inbox/`;
- `file`, a folder and glob (phase 4 picks how files are watched);
- `chat`.

A chat trigger is a `run_routine` tool. The model sees each routine's name,
`about` text and input schema, and fills in the inputs instead of planning
the steps.

### When the model wakes

A routine that goes as planned makes no model call. The model wakes only at
these four points. Each one asks a small, structured question.

1. **Fill.** A trigger fires but a required input has no value, or a chat
   ask names a routine loosely. The call returns the inputs, using the
   routine's own input schema as the reply format.
2. **Branch.** At an `ask` step, the call returns one of that step's named
   branches.
3. **Repair.** A step fails, or an `expect` gets an output it can't read.
   The call gets:
   - the routine;
   - the failed step;
   - the cut output.

   It returns `{"do": "retry" | "skip" | "stop" | "replan", "args": {…}, "why": "…"}`.
   - `retry` may change only that step's arguments.
   - There is at most one repair per step and two per run.
   - After that, the run stops and tells the user what failed.
4. **Replan.** The routine can't do this task, or no routine fits. This is
   today's path: a full turn (`unattended::run`, or the chat itself).

**Tiers:**

| Call | Class | Model |
|---|---|---|
| fill, branch | `routine:fill`, aliased to `background:classify` | local when local is on |
| repair | `routine:repair`, aliased to `background:judge` | the small cloud tier, or local when eligible |
| replan | `chat:default` | the full model |

The classes are aliases in `route/policy.rs`, as `episode:step` →
`desktop:soft` is, so no new effort rows are added. Every call fails safe:
a reply that doesn't parse becomes a stop with a plain message, never a
guess.

### Safety stays where it is

- Each `tool`, `script` and `desktop` step goes through `decide` exactly as a
  model tool call does. Routines can't skip the hard floor, park a hard
  class, or skip the egress guard.
- **Saving asks for approval of the routine's changes.** Steps that only
  read run freely. When a routine is saved, the user approves its *change*
  steps once, with the exact tool and argument template shown.
  - That consent is bound to the routine's content hash, so any edit asks
    again.
  - A `HardClass` step is never pre-approved: it parks on every run.
  - A routine with no consent runs like an Ask-mode automation, so its
    change steps are refused.
- Webhook and chat triggers keep their trust rules (signature, pairing).
  Webhook bodies are only ever input values, never steps.

### Model use shrinks over time

- **Save as routine.** After a turn that ran several tool steps, the turn's
  span already lists each call and its arguments. A deterministic pass turns
  that list into a draft routine. One `routine:fill` call names the inputs,
  that is, which literal values should become `{{inputs.…}}`. The draft
  shows as a question. Nothing is saved until the user says Yes, as with
  Save as skill.
- **Run counts.** Each run records its model calls (fill, branch, repair).
  The Routines page shows runs and model calls per routine, so the drop is
  visible.
- **Repeated repairs become a fix.** When the same repair keeps happening,
  a patched routine is proposed. It passes the existing replay gate in
  `self_improve.rs` before it is offered.

### Weak machines

In steady state there is no resident model.

- Timers come from the scheduler that already runs, and webhooks from the
  hub. File triggers come from the OS.
- With no local model, fill and branch go to the small cloud tier. With no
  provider at all, a routine still runs every step that needs no model, and
  a step that needs the model stops with "Set up AI to finish this step."

## How it fits what exists

| Piece today | Role in this design |
|---|---|
| `automation_check` / `CheckGate` | Same idea; an automation with a check is a two-step routine (`script`, then `expect`). Automations keep working unchanged. |
| `check_decide` | The call shape for `ask`, fill and repair: schema-constrained, fails safe. |
| `unattended::run` | The replan path, unchanged. |
| `harness::approval::decide`, `HardClass`, egress guard | Every step, unchanged. |
| `route/policy.rs`, `route/local.rs` | Two class aliases; local serves fill and branch only. |
| `tools/ptc.rs` sandbox | Runs `script` steps. |
| `grokhub_core::recipe` | Runs `desktop` steps. |
| ChangeLedger, Save as skill | How routines are saved and undone. |
| `self_improve.rs` replay gate | Checks proposed routine fixes. |
| `channels/inbox/` (#2106) | Webhook and chat triggers. |

**Non-goals:**

- a second agent loop, or a model polling in the background;
- any change to `hard.rs`, `approval.rs` or the egress guard;
- replacing skills (a skill teaches the model; a routine runs without it);
- egui (Tauri shell only).

## Open decisions for the maintainer

1. **File format.** JSON (proposed: strict, easy to validate and diff) or
   Markdown with fenced steps (easier to read and hand-edit).
2. **Approval for change steps on replay.** Consent once per routine, bound
   to its content hash (proposed), or Ask on every run.
3. **File triggers.** The `notify` crate (a new dependency, instant) or
   folder mtimes checked on the scheduler's tick (no dependency, up to a
   minute late).
4. **Automations.** Leave them separate (proposed for now), or show them on
   the Routines page as one-step routines in phase 6.

## Phase 1 notes

Phase 1 (`grokhub_core::routine`) took the proposed JSON format (decision 1).
It also settles these details of the format:

- A `desktop` step holds its replay inline, as `{ "screen": …, "ops": […] }`,
  because desktop recipes have no saved names today.
- An `ask` step is `{ "id": "pick", "ask": "<question>", "branches": { "<name>": "<step or done>" } }`.
  It has at least two branches and never falls through.
- An `expect` reads one `from` (`df.stdout`, `df.exit_code`,
  `probe.json.free_mb`, `inputs.mount`). An optional `regex`/`group` picks
  the value out of it, which then checks against `equals`, or against
  `at_least`/`at_most`. With no check, the value must not be empty.
  - A miss jumps to `else`.
  - With no `else`, the step fails, which is the repair point in phase 3.
- Jumps only go forward. The validator lists every problem in step order:
  - unknown tools;
  - bad references;
  - steps that can never run;
  - a step reading an output that may not have run on every path to it;
  - quotes written around a value in a command (the runner quotes values
    itself).

## Phase 2 notes

Phase 2 (`grokhub_agent::routine_run`) runs steps with no model.

- **Gate.** Each `tool`, `script` and `desktop` call goes through
  `gate::decide_with`, as a model's call does:
  - the hard floor and hard classes;
  - the permission policy;
  - Ask, with the person's answer when someone is there. Unattended, a
    change step is refused, as in an Ask-mode automation. This holds until
    phase 5 adds consent.
- **Scripts** run as the sandboxed `run_tool_script` call, with their filled
  `args` in `ARGS`.
- **Desktop steps** map each saved op to the desktop tool, one call at a
  time. They need the app's desktop control.
- **Exit codes.** A command's `exit N` line becomes its `exit_code`. A
  non-zero exit fails the step, unless the next step is an `expect` on that
  exit code.
- **Stops.** Missing inputs, `ask` steps and failures stop the run with a
  plain message. The model wakes at those points in phase 3.
- **Command.** `grokhub-hub routine run <name> [--dry-run] [--set key=value]`
  runs in the current folder and asks on the terminal before a change step.
  A dry run fills and gates every step and runs none.

## Phase 3 notes

Phase 3 (`grokhub_agent::routine_wake`) wakes the model at three points.
Each call has a fixed schema (`routine_fill`, `routine_branch`,
`routine_repair`) and uses low effort with no tools. A failed call, or a
reply that doesn't parse, stops the run with a plain message and never
guesses.

- **Fill.** It runs only when the run has words to read: the chat ask or
  webhook text that started it. The reply is `{name, value}` text pairs,
  typed by the input's kind. With no words, the run stops: "it needs a value
  for inputs.x".
- **Branch.** The reply must name one of the step's branches. It sees the
  earlier outputs (clipped) as data.
- **Repair.** It runs only for a step that *broke*: a tool error, an output
  the check can't read, or a miss with no `else`. It never runs for a step
  the gate, the person or the setup refused.
  - `retry` carries the step's new arguments (`args_json`), which go through
    the gate again.
  - There is at most one repair per step and two per run. Then the run stops
    and says what failed.
  - `replan` ends the run as `Replan`, and the caller starts the full turn
    (phase 4).
- **Classes.** `routine:fill` is aliased to `background:classify`, which is
  local-eligible, and `routine:repair` to `background:judge`.
- **Command.** `grokhub-hub routine run` has no model yet: the client is
  built in the app, so these points stop there. Phase 4's app triggers pass
  one in.

## Phase 4 notes

Phase 4 starts routines without a person typing a command. Part one
(`grokhub_agent::routine_triggers` and the `run_routine` tool) holds the
logic; part two wires it into the app's scheduler and webhook inbox.

- **State.** `routines/state.json` keeps each routine's next timer slot and
  the files its file triggers have seen. A trigger seen for the first time
  only primes that state, so turning a routine on never fires it for a past
  slot or for files that were already there. A routine that is gone loses
  its state.
- **Timer.** `daily` and `weekdays` reuse the automation clock
  (`compute_next_run`); `every` adds its minutes. A missed slot fires once.
  Several timers on one routine share the soonest slot.
- **File.** The folder's direct files whose names match the glob (`*` and
  `?`; dot files skipped), by modified time. `~/` and relative folders are
  under the home folder. A new or changed file starts a run, with its path
  in the `file` input when the routine has one. At most 5 runs per routine
  per tick; the rest wait for the next tick.
- **Webhook.** The hook's name picks the routine. Top-level body fields
  named like an input, of that input's kind, fill it; the body text is the
  words a fill reads.
- **Chat.** `run_routine` lists the routines with the `chat` trigger and
  shows only when there is one, so other tool lists don't change. Starting
  one asks nothing, since each step meets the gate inside the run; plan mode
  refuses it. The chat can't answer a step's ask yet (phase 5), so a step
  that changes something is refused. A routine can't name `run_routine` as
  a step.
- **App.** The scheduler's tick runs due routines after automations, under
  the same lease, so an open cabin or app keeps them from running twice.
  The inbox gives a webhook to the routine that listens for its hook before
  any model run. Each run is unattended and reports on the bell as
  `routine:<name>`. The model is woken only to fill, branch or repair; with
  no route, commands run in the home folder and those points stop the run.
  A `replan` hands the job to one read-only assistant turn, with the
  routine's report marked as data.

## Phase 5 notes

Part one (`grokhub_agent::routine_consent`) is decision 2, as proposed.

- **Change steps.** A tool step whose tool isn't read-only, and every
  desktop step. Script steps run in the read-only sandbox, so they need no
  consent.
- **Consent.** `routines/consent.json` keeps, per routine, the SHA-256 of
  its canonical JSON and the change step ids allowed. Any edit changes the
  hash and the consent stops counting (`Edited`) until it is given again.
- **Runs.** App runs and the `run_routine` tool pass the consent as the
  run's approver. It allows only the listed steps, and never a step that
  `harness::decide` parks as a hard class. With no consent, change steps
  are refused as before.
- **Granting.** `grokhub-hub routine allow <name>` lists each change step
  with its argument template and asks on the terminal; `routine revoke`
  takes it back. The Routines page (phase 6) adds the same as a click. No
  model reply or routine step reaches `grant`.

Part two (`grokhub_agent::routine_draft` and the `save_routine` tool) is
Save as routine.

- **Source.** The chat session's last recorded turn: the calls after its
  last user message whose output shows they ran and worked (no refusal, no
  cut-off or garbled call, a command's `exit 0`). Only file, command, edit
  and web-read tools are kept; planning, memory and chat tools are not. At
  most 30 steps.
- **Naming.** The chat model is already in the turn, so it names the
  routine, its about line and its inputs in the tool call; no separate
  `routine:fill` call is made. Each input names a literal value; every
  string argument that is that value, or (for values of 3+ characters)
  holds it, gets `{{inputs.name}}`, and the value becomes the default. A
  value found in no step is an error.
- **Yes.** `save_routine` is a control tool, so the approval card is the
  person's yes, and plan mode and unattended runs refuse it. It shows only
  in attended chats.
- **Ledger.** Saves go through the change ledger as kind `routine`
  (`changes/routines.jsonl`); `grokhub-hub routine undo <name>` walks back
  one save at a time, removing the routine after its first. Saving never
  grants consent, and a re-save changes the hash, so earlier consent stops
  counting.

## Phase 6 notes

Decision 4 as proposed: automations stay separate. The Routines list is its
own section on the Automations page, under the automations, not a new view.

- **Runs.** `grokhub_agent::routine_log` keeps one line per run in
  `routines/runs.jsonl` (the newest 2,000): when, what started it (`timer`,
  a file's path, `webhook <hook>`, `chat`, `run now`, `terminal`), whether
  it finished, its model calls, and its last line, clipped and with secrets
  redacted. A dry run is not a run. A stop or a hand-over to an assistant
  turn counts as not finished.
- **On and off.** Off is kept in the trigger state, so no timer, file,
  webhook or chat start reaches it. Back on, its triggers prime again, so it
  doesn't fire for what happened while it was off.
- **Rows.** Each routine shows its triggers, step count, change steps and
  where their consent stands, on or off, runs and model calls, and the
  newest run's line. A routine that doesn't load still shows, with why.
- **Clicks.** Allow and Undo open only on a pointer click and ask first, so
  a key press on a focused button can't grant consent. Run now runs the
  routine off the main thread as a trigger would, with its defaults, through
  the same consent. A routine never runs twice at once: a Run now and a
  trigger that meet get "already running".
- **Desktop only.** The six commands aren't web commands, so a paired
  browser doesn't get the section.

## Build order

Each phase is one PR into `beta`, with tests that assert literal results.

1. **Format.** `grokhub_core::routine`: types, parser, validator (unknown
   tools, bad templates, missing inputs, steps that can't be reached) and
   template fill. Pure, with no runner.
2. **Runner.** `tool`, `script`, `desktop` and `expect` steps, each through
   `decide`, with no model. A `--routine run <name> [--dry-run]` flag. The
   proof is a dry run and a real run of `disk-report` in the cloud box.
3. **Wake points.** Fill, branch and repair calls, the two class aliases,
   the repair limits, and stop-and-tell. Tests use a stand-in client.
4. **Triggers.** Timer through the scheduler, webhook by name, the
   `run_routine` chat tool, and file triggers (per decision 3).
5. **Save as routine.** A draft from a turn's span, a Yes to save, and the
   ChangeLedger with undo. Consent for change steps (per decision 2).
6. **Routines page.** List, on/off, runs and model calls, consent, and
   automations (per decision 4).
7. **Fixes from repeated repairs.** Proposed through the replay gate.
