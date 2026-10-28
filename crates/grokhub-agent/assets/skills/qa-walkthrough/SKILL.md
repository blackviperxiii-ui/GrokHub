---
name: qa-walkthrough
description: Ultimate ship check. Walk the real app, review all code, hunt security holes, vulnerabilities, slop, missing tests and optimizations; fix safe items; one ranked ship/no-ship report. /qa-walkthrough
disable-model-invocation: true
---

# QA Walkthrough

Last gate before ship: beta→main, big features, or "run a QA walkthrough". Any repo. AV output → **av-qa-walkthrough**. Cite **viper-mode**; no sibling skill growth. Goal: tested, secure, clean, evidence for every claim.

## 1. App walk

Boot the real app (Playwright/browser, desktop dev, or the repo's runner). Walk screens, flows, settings: empty, error, loading, offline, first-run, light/dark, narrow/wide. Hunt bugs, UI defects (layout, overflow, contrast, alignment, broken states, style drift), a11y and confusing UX. Screenshot evidence.

## 2. Code review

Read every changed area, then its neighbors. Check logic, edge cases, error handling, races, resource leaks, and that docs match behavior. Root-cause each bug via **principle-fix-root-causes**.

## 3. Security and vulnerabilities

- Secrets, tokens or personal info in code, history, logs, config or fixtures; run the repo's pii-guard and secret scan.
- Injection (SQL, shell, path traversal, XSS, template, prompt), unsafe deserialization, SSRF, open redirects.
- Authn/authz gaps, missing permission or approval checks, insecure defaults, overly broad CORS, CSP or tokens.
- Input validation and rate limits on every boundary (IPC, HTTP, CLI, files, websockets).
- Dependencies: run the ecosystem audit (`cargo audit`, `npm audit`, `pip-audit`, Semgrep) and flag known CVEs and stale pins.
- CI and workflows: least-privilege permissions, pinned actions, no untrusted input in `run:`.

## 4. Tests

Run the full suite and the real build. Map each critical flow and each finding to a test; write missing tests for anything risky or untested. Flaky tests are findings.

## 5. Slop, improvements, optimizations

Deslop: dead code, redundant comments, needless abstractions, naming drift, leftover debug, duplicate logic. **sharpen**: performance, startup, bundle size, memory, queries, clearer UX.

## 6. Output

One report ranked by severity (critical, high, medium, low), tagged UI, code, security, tests, slop or perf, each with where, repro, expected vs actual and evidence. End with a verdict: **ship** or **no-ship** plus the blockers.

- Clear, safe fixes → small PRs into **beta** with tests; squash-merge on green CI.
- Needs a human decision → **Needs decision** with options; never guess.
- Redesigns and big suggestions → report only; never apply silently.
