---
name: statechart
description: "Check a TLA+ model of a system that moves between states, such as a product flow, a protocol, or a service, and draw it as a Mermaid statechart. The report gives the shortest path to each state that breaks a rule, as numbered steps. Use for /statechart, or to show a bug as a path through states."
---

# Statechart

The chart shows how a system moves between states, in the words of the person who reads it. That person, the reader, decides whether the behavior is right. The reader can be a product manager, an engineer, or a reviewer. State labels say what the reader observes, such as "Paying". Step labels say the event that moves the system, such as "tap Pay" or "lease expires". Code identifiers stay out of both.

This skill reads a model and draws it. To build a lasting verify skill around a model, use `/create-formal-verification-skill`. To bring one back in line with the code, use `/maintain-formal-verification-skill`.

## Direction

Pick one, and say which:

- **From a model.** A TLA+ model, another formal model, or concrete code exists. Derive the chart from it. Follow Run through Hand off below.
- **From a chart.** The owners must agree on the behavior first. Draw the Mermaid chart, get it agreed, then write a skeleton model from it. Follow "From a chart to a model" below, then Run.

If the code already has a spec, copy it to the scratch folder and chart the copy. Keep its invariants. Tie each variable and each action to code that a reviewer can open, and keep one indivisible step in the code as one action in the model. As an option, check your copy against the config that the spec's authors pass with.

## Run

Work in a scratch folder outside the repo. Write a TLA+ spec and its config. Then run:

```sh
<skill dir>/scripts/statechart SPEC.tla [--config FILE] [--view FILE] [--out DIR]
```

`scripts/statechart.ts` is TypeScript, so run it with a runtime that can execute TypeScript. `scripts/statechart` is a POSIX shell script that picks one. It needs Node 22.18 to 22.x, Node 23.6 or later, Bun 1.1.5 or later, or Deno 2.0 or later. On native Windows, run `node --disable-warning=ExperimentalWarning <skill dir>/scripts/statechart.ts` with Node 22.18 to 22.x, or Node 23.6 or later.

The first run installs TLC and Java with mise and needs the network. Without mise, set `TLA2TOOLS` to the path of `tla2tools.jar`, and `JAVA` to a java binary (Java 11 or later).

## Write the spec

- Keep the model to few states. Split a state only to keep two bugs apart, or to show a step that would stay inside one label.
- Write each rule that the reader agrees to as an INVARIANT. Put the reader's sentence for the rule in a comment above it. A rule can be about a screen, a result, a stored value, or a message. If the model has no variable for a rule, add the variable. Never weaken a rule to make it hold, and never drop one.
- A rule that must hold only when the system has settled holds in the states where no other action is enabled. Write it with that condition.
- For a goal that the system must be able to reach, assert that the system never reaches it. The counterexample that TLC gives is the path to the goal. Name that path in the hand-off.
- Write `Next` as a disjunction of the actions, such as `YouPressPlay \/ YouPressStop`. If you do not, TLC names every step `Next`. Give an action that the reader drives its own disjunct when the reader must see it apart from what the system does.
- A state with no step out breaks `NoDeadlock`, also when it is a correct end state. When an INVARIANT or a PROPERTY already has that name, the rule is `CHECK_DEADLOCK`. Add to `Next` an action that is true only in the end states and changes nothing, such as `Done == phase \in {"paid", "failed"} /\ UNCHANGED vars`, and give it a label in `step()`.

## Pick the chart

You do the work that needs judgment: the words on each state and arrow, the phases and their colors, the title, and the report to the reader. Put your work in a view module, and give it with `--view`.

To show the reader's words, the view exports `state(vars)`. The value that `state()` returns picks the form:

- A string gives a flat chart: `export const state = (v) => (v.paid ? "Paid" : "Paying");`
- An array gives parent states: `export const state = (v) => v.main.split(".");`
- An object gives one parallel region for each key: `export const state = (v) => ({ Playback: v.playback, Volume: v.volume });`

Label each state with what the reader can see or cares about, such as "Paying" or "Lease held, write pending". Keep variable names out of labels. In `vars`, a TLA+ sequence is an object with keys from 1, so `v.pc[1]` is `pc[1]`. A set is an array.

The view can also export `step(action, from, to)`. It gets the TLC action name and the values before and after the step, and it returns the label of the arrow. Write each step label as the user sees it happen, such as "you press play" or "the connect fails". Do not name the code or the TLA+ action. Two actions that end the same in the chart get the same label, so that they share one arrow.

When a step can occur only in some conditions, end its label with the condition in brackets, in the reader's words, such as `you press play [while the file loads]`. In a regions chart, the command writes the condition on the other regions on the arrow, in the words of their labels, such as `[when Offline]`. Your brackets replace it, and two arrows from one state can then look the same. So in a regions chart, do not write brackets for a condition on another region. When a value that no label shows decides where a step goes, name the result in the label, such as "the roster read fails". When two steps end in the same state for different causes, name the cause in each label.

The view can also export these:

- `group(vars)` returns the name of a phase. Each chart state gets the color of its group. When the model states behind one chart state are in different groups, that state gets no color.
- `colors` gives each group name a color class.
- `title` is a string that the chart shows at the top.

Groups are optional. A state that breaks a rule keeps its own mark whatever its group.

The command calls `state()` and `group()` at most once for each model state, and `step()` at most once for each step. The chart and the report both use those answers. So make each function depend only on its arguments.

The chart does not draw a step that stays in one label. Split a label when the reader must see such a step.

Aim for about 20 arrows or fewer, because a busier chart is hard to read. First return coarser labels from `state()`. Give one label to values that act the same, and leave out a detail that the reader does not need, such as a call in flight whose return changes nothing. Keep each state of a bug path, and keep each mark. When a merge changes a `breaks` state to `mayBreak`, it hides where a rule breaks. Do not merge two states that break different rules, because the reader then cannot see which rule breaks where. For a large overview, keep the shortest paths to the settled states and each bug path. If the chart is still too busy, return `null` from `step()` for the steps that the reader does not need, such as a reset that every state has, or a step whose arrow repeats what other arrows show. Obey these rules when you hide steps:

- Keep each chart state reachable by arrows from a start state.
- Keep at least one arrow out of each chart state that has a step out in the model.
- Hide all the steps of an arrow, or none of them. When you hide only some, its condition gets narrower.
- In the hand-off, say which steps the chart hides.

## Read the report

- In the chart, a state that breaks a rule has the class `breaks`. When only some model states behind it break the rule, it has the class `mayBreak`. The Legend names the rules of each mark. When two rules break, it names both on one mark, so say in the hand-off which state breaks which rule. In a regions chart, a state gets a mark when each combination of region states that includes it can break a rule. When a combination can break a rule and none of its states has a mark, each state in that combination gets a mark.
- TLC goes past a state that breaks a rule, so the chart shows the states after a bug.
- Because TLC goes past bugs, a model with no limit on its states can run without end. Keep the constants small, or add a CONSTRAINT.
- With a VIEW or a SYMMETRY, TLC counts states that it treats as the same as one state, and the chart draws that one state. An arrow can lead to it rather than to the state that the step reaches, and the report says so.
- TLC checks the rules in each state past a CONSTRAINT or an ACTION_CONSTRAINT, but it does not explore that state or check it for deadlock. When TLC's shortest path for a rule ends in such a state, the chart draws that state, and the report gives that path. The chart does not draw the other states past the bound. A deadlock in such a state does not show. TLC also checks the rules after each step that an ACTION_CONSTRAINT excludes, also when it reaches the next state another way. When TLC's shortest path for a rule ends with such a step, the chart draws that step.
- The command checks the rules with TLC on the config as written, with one worker. When a rule breaks, the command removes that rule and runs TLC again, until no rule breaks. After the first break, each run stops at 100,000 distinct states, so that a large model does not run without end. When a run reaches that bound or stops on an error, the report lists the rules that it did not check under "Rules not checked", and the chart does not mark them. A rule that is not checked can hold or break. To check it, remove the rules that break from the config, or make the model smaller. The bound adds a module that changes TLC's order of the variables, so for a property TLC can find a different counterexample than a run without the bound.
- `tlc.out` is what TLC printed on the config as written. When the chart does not show the bug path of a rule that breaks, `tlc-RULE.out` is what TLC printed for the run that found it. When TLC repeats its first warning, plain TLC prints it twice, but `tlc.out` shows it once, because TLC leaves the second copy out of the output that this command reads. That happens only when Java runs with `-XX:+UseParallelGC`, since otherwise TLC's first warning is its advice to pass that flag, which it prints once.

Read each chart and each report yourself before you show them. Follow each numbered path in order. Fix a label that reads like code.

When you propose a fix, write it as a second model, run the command on it with a new `--out` folder, and show both charts.

## Show the chart

Show `chart.mmd` to the user as a `mermaid` code block. The chart has a budget of 500 states, counted by TLC on the config as written. Above that, the chart draws only the paths to the rules that break, and when no rule breaks, the command writes no `chart.mmd`. It still checks each rule, and the report gives the state count and each bug path. To see the whole graph, make the model smaller.

Render charts with Pele. When the user asks for an image, or a chart is too large to read as text, follow `references/render.md`. Do not use a different renderer unless the user asks for it.

## From a chart to a model

Use this direction when the owners must agree on the behavior before a model exists.

1. Draw the chart by hand as Mermaid `stateDiagram-v2`. Give each independent part of the system its own region. Write one `%% never` comment line for each rule. Draw a restart or a stop as an arrow onward, not as a dead end.
2. Render the chart as in "Show the chart", and agree the image with the owners. When a rule breaks on the chart that the owners agreed, that is a bug in the design. Escalate it to the owners. Do not change the chart to hide it.
3. Write the spec from the agreed chart. Add the real guards and variables from the code. Run the command, and compare its chart with the agreed one. Where they differ, ask the owners whether the code or the chart is wrong.

## Hand off

Give each chart and its Mermaid text to the person who must decide, with the rendered image when you made one. Add one line for each bug: the rule, the reader's sentence for that rule, and the path as the report numbers it. For each bug, also say whether you reproduced it against the real code, and whether the proposed fix holds in the second model. When the chart shows no path for a bug, give the counterexample from the `tlc-RULE.out` file. Quote the state count and the lists of rules from the report as it prints them. Say what the model leaves out. Give the command that makes the files again, with a new `--out` folder. Point the people who will maintain the model at `/create-formal-verification-skill` and `/maintain-formal-verification-skill`.

When the model describes code, replay each bug path against the real code, as a test or a live repro, one step for each action. Drive the code only through its real entry points, and judge it only by what a user or a caller can observe. If a bug does not occur, think that the model is wrong until you show that it is not. Then fix the model, not the rule. When the model describes a design that has no code yet, the checker's trace is the evidence. Say so.
