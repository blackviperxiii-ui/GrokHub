---
name: land-it
description: Whole-app feel dive. Find where UX, interactivity, QoL, and code miss the mark; ranked low-effort fixes. /land-it
---

# Land It

Product-feel diagnosis when the app "almost" works but never quite lands. Not a ship gate (**qa-walkthrough**). Cite **viper-mode**, **principle-experience-first**; polish via **sharpen**/**unslop**. No sibling skill growth.

## 1. Whole-app dive

Boot real UI when possible (Playwright/browser/desktop runner). Else systematic code+flow read of every major surface: empty, error, loading, first-run, narrow/wide, light/dark. Name each flow.

## 2. Miss-the-mark

Where unfinished, confusing, inert, or almost-but-not-delightful. State intended experience vs what ships. Evidence: screenshot path, `file:line`, flow name.

## 3. Interactivity and QoL

Feedback, affordances, progressive disclosure, keyboard/shortcuts, undo, defaults, recovery, purposeful motion, density, friction in core loops.

## 4. Code that hurts feel

Dead-end state machines, missing optimistic UI, blocking waits, silent failures, half-wired features, copy/layout fighting code.

## 5. Output

One scannable report ranked P0/P1/P2. Each: where, why it fails to land, smallest fix, optional verify. Safe auto-fixes → **beta**; else **Needs decision**. Hand ship/security/tests to **qa-walkthrough**.
