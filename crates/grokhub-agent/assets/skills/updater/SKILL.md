---
name: updater
description: Find everything in an app that needs updating (deps, advisories, toolchains, CI, game addons, mod sources, model ids, APIs); safe bumps as PRs, the rest as cards. /updater
---

# Updater

Keeps an app's support surfaces current. Scripts read; you read only `report.md`. Cite **viper-mode**.

## 1. Run

```bash
python3 <this skill>/scripts/updater.py run --repo .
```

It inventories surfaces from the repo itself (manifests, lockfiles, workflows, `.toc` files, model ids, API hosts, launchers, mod sources, anti-cheat guards, locales), checks each against its official upstream, and writes a ranked report outside the repo. "Unchecked sources" lists URLs the shell can't reach: fetch each with a tool that writes the raw bytes straight to disk (curl, or a scrape returning rawHtml), save as `<out>/fetched/<name>`, rerun. Never save a summary, an excerpt or a hand-rebuilt table; if only part of a body is reachable, leave it unchecked. Try each source once: blocked or cut off means unchecked, not a hunt for device access or new tools.

## 2. Act by `action`

- **auto**: one branch and PR per area: `updater.py bump --area cargo|npm|pypi|wow`, run the repo's fast checks (build, lint, clippy, tests for changed crates or packages, with the runner the repo documents), open the PR into the integration branch (often `beta`) and let CI run the full suite. Review the new crates and duplicate majors `bump --area cargo` prints; hold the crate that pulls one in (`cargo update -p X --precise`) and card it. Leave draft with the last code push, not after green. Merge on green and watch until MERGED.
- **card**: majors, unsound or unmaintained crates, fixes needing a major, toolchain and action majors. Try each major as its own small PR first when the repo's fast checks cover it; card only what fails them, needs a product decision, or is out of scope. One card or issue each, with current → latest and the note, on the repo's board or tracker. Skip deps used only by a surface being retired (open PRs, plans); list them under watch.
- **manual**: verify (unlisted model ids, mod rules) before changing code; then card it.
- **watch**: no action now.
- **guarded**: anti-cheat and other safety lists. Never edit for an update; a change may only add protection.

## 3. Report

Counts per severity; each medium-or-higher finding by name (surface, current → latest or advisory id, action), ignored advisories with their recorded reason; each PR as merged (commit) or pending (what blocks it), cards made, unchecked sources. Don't close the run while a PR it opened is open. Nothing changed without a check that ran.
