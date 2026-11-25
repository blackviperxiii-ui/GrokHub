---
name: land-it
description: Whole-app feel dive. Find where UX, interactivity, QoL, and code miss the mark; ranked low-effort fixes. /land-it
---

# Land It

A product-feel diagnosis for an app that almost works but never quite lands. The output is a ranked list of the smallest fixes that would change how the app feels. It is not a ship gate (**qa-walkthrough**) and not a visual polish pass (**elevate**). Cite **viper-mode** and **principle-experience-first**. Polish wording with **sharpen** and **unslop**.

Judge the app as its user would, not as its author. The question for every screen is what the user came to do, and what stood between them and doing it.

## 1. Whole-app dive

Boot the real UI with the host's browser or computer-use tool (Playwright, a desktop dev build). Reading code tells you what the app does. Only using it tells you how it feels. If it cannot boot here, read the code and flows of every major surface instead, and say in the report that the dive was read-only.

First write down, in one line each, who the user is and the three to five core loops they repeat (the things they open the app to do). Walk those first, then every other major surface.

For each surface, cover empty, error, loading, first run, narrow and wide, light and dark. Name each flow so findings can point at it. Time the core loops by hand: count the clicks, keystrokes and waits from intent to result.

## 2. Miss-the-mark

Find where the app is unfinished, confusing, inert or one step short of delightful. For each spot, state the experience the screen seems to intend and what actually ships. Common misses:

- The primary action is not the most obvious thing on the screen.
- The user has to remember something the app already knows.
- A step exists only because of how the code is organised.
- Something happened and the app did not say so, or said so too late.
- A dead end: an empty state, error or finished state with no next step.
- A feature that is present but half-wired, so it invites a click and does nothing useful.

Give each finding its evidence: a screenshot, a `file:line`, and the flow name.

## 3. Interactivity and QoL

Check each of these on the core loops:

- **Feedback.** Every action answers within about 100 ms, even if the answer is only a pressed state or a spinner.
- **Affordances.** What can be clicked, dragged or edited looks like it can.
- **Progressive disclosure.** Common things are in reach. Rare things are one level down, not in the way.
- **Keyboard and shortcuts.** The core loops work without the mouse. Enter and Escape do what the user expects.
- **Undo and recovery.** A mistake costs seconds, not work. Destructive actions can be undone or are confirmed.
- **Defaults.** The most likely choice is already made. The app remembers the last choice.
- **Motion.** It shows where something came from or went. It never makes the user wait.
- **Density and friction.** No needless confirmation, modal, page load or re-entry of data in a core loop.

## 4. Code that hurts feel

Trace each feel problem to the code that causes it:

- Dead-end states with no handler for empty or error.
- No optimistic UI, so every action waits on the network.
- Blocking waits on the main thread or behind a full-page loader.
- Silent failures: a caught error that tells no one.
- Half-wired features, stub handlers and flags left off.
- Layout that fights the code: fixed sizes, magic numbers, content that shifts after load.

Check web UI against **modern-web-guidance**. A measured slowness goes to **optimize**. A design question bigger than one screen goes to **architect**.

## 5. Output

One report, ranked:

- **P0.** A core loop is blocked, confusing or loses work.
- **P1.** A core loop has friction a user hits every time.
- **P2.** A rough edge off the main path.

For each finding give where it is, why it misses, the smallest fix, and how to verify the fix. Lead with the fixes that cost little and change the feel most. Keep it to what you saw. Do not pad the list with generic advice.

Then act:

- A safe, small fix goes in a PR into **beta**, verified in the real UI.
- Anything that changes product behavior or scope goes under **Needs decision** with options and your recommendation.
- Ship, security and test findings go to **qa-walkthrough**. Visual craft goes to **elevate**.
