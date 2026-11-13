---
name: bag-and-tag
description: Promote beta to main, bump VERSION/CHANGELOG, bag and tag vX.Y.Z, publish GitHub Release artifacts, verify install paths. Explicit user ask required. /bag-and-tag
---

# Bag and Tag

Promote ritual for this repo: **beta → main**, version bump, annotated tag, release artifacts, install verify. Not day-to-day beta PRs (**viper-mode** / **qa-walkthrough**).

**Read `GUIDE.md` in this folder** for preflight, reconcile, bump files, tag, and verify.

Cite **principle-prove-it-works**, **principle-make-operations-idempotent**, **AGENTS.md** branch rules.

## Playbook

1. **Preflight:** user explicitly asked to promote; beta tip green; Unreleased has content or bump is intentional.
2. **Reconcile:** compare `beta` vs `main` trees; never force-push either branch.
3. **Bump on beta:** `VERSION`, root `plugin.json`, `.cursor-plugin/plugin.json`, `.claude-plugin/plugin.json`, marketplace files; move CHANGELOG Unreleased → `## X.Y.Z`; PR → beta; squash-merge on green.
4. **Promote:** PR beta → main (squash-merge on green; same pattern as prior promote PRs).
5. **Bag & tag:** annotated `vX.Y.Z` on main tip matching VERSION (or `workflow_dispatch` Release on main).
6. **Artifacts:** confirm GitHub Release has tarball/zip/SHA256SUMS (`.github/workflows/release.yml`).
7. **Installers:** verify `install.sh` / `install_for.sh` / marketplace paths still match `docs/CONSUMING.md`.
8. **Report:** PR URLs, merge SHAs, release URL, `git diff beta main` empty.
