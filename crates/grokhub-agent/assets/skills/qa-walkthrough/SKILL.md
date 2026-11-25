---
name: qa-walkthrough
description: Ultimate ship check. Walk the real app, review all code, hunt security holes, vulnerabilities, slop, missing tests and optimizations; fix safe items; one ranked ship/no-ship report. /qa-walkthrough
---

# QA Walkthrough

The last gate before ship. Use it before promoting beta to main, after a big feature, or when the user asks for a QA walkthrough. It works in any repo. Audio or video output goes to **av-qa-walkthrough**. Cite **viper-mode**.

The goal is a product that is tested, secure and clean, with evidence behind every claim. A finding without evidence is a guess, and a pass you did not run is not a pass. Say "not checked" and why for anything you skipped.

## 0. Scope and setup

1. State what is shipping in one or two sentences: the diff range (`git log <base>..<head>`), the features it touches, and who uses them.
2. List the critical flows before you open the app. A critical flow is one whose failure loses data, money, access or trust, or blocks first use. These get walked, reviewed and mapped to tests no matter how small the diff looks.
3. Find how the repo boots, builds and tests (README, `AGENTS.md`, package scripts, CI workflow). Use the runner the repo documents. Do not invent commands.
4. Keep a findings list from the first minute. Record each finding when you see it, with its evidence, and rank at the end.

## 1. App walk

Boot the real app with the host's browser, computer-use or terminal tool (Playwright, a desktop dev build, the repo's runner). A walk through code is not a walk through the app. If the app cannot boot here, say so, say what you tried, and mark the walk "not run" in the report.

Walk every screen, flow and setting the change touches, then the critical flows end to end. For each one, cover these states:

- Empty, loading, error and success.
- Offline or a failed request, and recovery after it.
- First run with no data, and a returning user with a lot of data.
- Light and dark theme, narrow and wide viewport, and 200% text size.
- Keyboard only: every action reachable, focus visible, focus order sane, no trap.

Hunt for bugs, layout defects (overflow, clipping, overlap, misalignment, contrast, broken states, drift between similar screens), accessibility gaps and confusing UX. Try the inputs a user gets wrong: empty, very long, pasted, duplicate submit, back button mid-flow, two tabs.

Screenshot every finding and note the exact steps. Check the browser console and network panel on each screen; an error or a failed request there is a finding even when the screen looks fine.

## 2. Code review

Read every changed area in full, then its neighbors: the callers, the callees, and the code that shares its data. Check:

- Logic and edge cases: empty, null, zero, one, many, boundaries, time zones, encodings.
- Errors: every failure path either recovers or reports. Nothing is swallowed.
- Concurrency: races, double submits, reentrancy, stale closures, shared mutable state.
- Leaks: listeners, timers, handles, subscriptions, caches with no bound.
- Docs, comments and names that no longer match the behavior.
- Web frontend against **modern-web-guidance**.

Root-cause each bug with **principle-fix-root-causes**: find the first wrong decision, then grep for the same pattern elsewhere. What a change could break beyond its diff goes to **blast-radius**.

## 3. Security and vulnerabilities

- Secrets, tokens or personal info in code, history, logs, config or fixtures. Run the repo's pii-guard and secret scan.
- Injection (SQL, shell, path traversal, XSS, template, prompt), unsafe deserialization, SSRF and open redirects.
- Authentication and authorization gaps, missing permission or approval checks, insecure defaults, and overly broad CORS, CSP or token scopes.
- Input validation and rate limits on every boundary (IPC, HTTP, CLI, files, websockets).
- Dependencies: run the ecosystem audit (`cargo audit`, `npm audit`, `pip-audit`, Semgrep) and flag known CVEs and stale pins. Deeper update work goes to **updater**.
- CI and workflows: least-privilege permissions, pinned actions, and no untrusted input in `run:`.

For each line above, name the command you ran and its result, or say it was not run and why. A security finding is never fixed silently. It goes in the report even when you fix it.

## 4. Tests

Run the full suite and the real build. Record the command and the counts (passed, failed, skipped).

Map each critical flow and each finding to a test that would catch it. Where none exists, write one for anything risky or untested, following **principle-test-behavior-not-implementation**. A flaky test is a finding. A skipped or disabled test is a finding unless it carries a reason that still holds. Never skip, disable or loosen a test to get green.

## 5. Slop, improvements, optimizations

Remove slop: dead code, redundant comments (**no-comments**), needless abstractions, naming drift, leftover debug output and duplicate logic. Prose and UI copy go through **unslop**.

Note improvements with **sharpen**: startup, bundle size, memory, queries and clearer UX. Measure before you call something slow. Real performance work goes to **optimize**, and a number you intend to report goes through **benchmark-checklist**.

## 6. Output

One report, ranked by severity:

- **Critical.** Data loss, security hole, crash or a blocked critical flow. Blocks ship.
- **High.** A broken feature or a wrong result with no reasonable workaround. Blocks ship unless the user accepts it.
- **Medium.** A defect with a workaround, or a risk that needs a test.
- **Low.** Polish, slop and nice-to-have.

Tag each finding UI, code, security, tests, slop or perf. Give each one where it is (`file:line` or screen), the steps to reproduce, expected against actual, and the evidence (screenshot, log line, command output). List what you did not check.

End with the verdict: **ship** or **no-ship**, plus the blockers. A verdict of ship means every critical flow was walked and no critical or unaccepted high finding is open.

What to do with each finding:

- A clear, safe fix goes in a small PR into **beta** with its test, squash-merged on green CI. Where the repo runs no CI, run its checks locally and say so.
- A finding that needs a human decision goes under **Needs decision** with the options and your recommendation. Never guess.
- Redesigns and big suggestions go in the report only. Never apply them silently.

Track the fixes you open with **follow-through** until each is merged or handed off.
