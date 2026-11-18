# Bag and Tag — detail

Use after the short playbook in `SKILL.md`. Encodes the proven promote → tag → release ritual for **this** repo. Never promote without an explicit user ask. Never force-push `main` or `beta`.

Do **not** treat this skill as a generic “ship” or “deploy” playbook for other products.

## Preflight

- Explicit promote/release ask from the user (standing rule: never invent a main promote).
- `origin/beta` tip CI green on required checks (`lint`, `token-check`, `pii-guard`).
- Decide open Dependabot / noise PRs: merge, close, or defer — do not leave them ambiguous mid-cut.
- `CHANGELOG.md` `## Unreleased` has content, or the bump is intentional with an empty Unreleased after move.

## Reconcile beta vs main

- `git fetch origin beta main`.
- Compare trees: after a successful promote, trees should match even if commit graphs diverge (squash promotes).
- If histories conflict on a promote branch, prefer keeping the **beta tree** (same pattern as prior promote PRs); never rewrite protected branches with force-push.

## Version bump (on beta first)

Bump these to the same `X.Y.Z`:

- `VERSION`
- root `plugin.json` (Agent Plugins), `.cursor-plugin/plugin.json`, and `.claude-plugin/plugin.json`
- marketplace manifests that embed the version (if present)

Move `## Unreleased` notes into `## X.Y.Z` (keep an empty Unreleased stub). One CHANGELOG line per change historically; do not invent prose.

Open a PR → **beta**, squash-merge on green. Strip `Co-authored-by` gmail trailers (pii-guard).

## Promote beta → main

- PR from a promote branch based on beta tip, targeting **main**.
- Squash-merge on green only.
- Confirm `git diff origin/beta origin/main` is empty after merge.

## Bag & tag + artifacts

- Annotated tag `vX.Y.Z` on the main tip whose `VERSION` equals `X.Y.Z`, **or** run Actions → Release → `workflow_dispatch` on **main** (see `.github/workflows/release.yml`).
- Workflow publishes `vstack-X.Y.Z.tar.gz`, `.zip`, and `SHA256SUMS`; refuses wrong branch, mismatched tag, or an existing release.
- Confirm the GitHub Release page and assets.

## Installer verify

Against `docs/CONSUMING.md`:

- `install.sh` / `install_for.sh` targets still document Cursor, Claude, agents, grok paths.
- Marketplace / plugin install one-liners in the release notes still match CONSUMING.
- Spot-check that `viper-mode/SKILL.md` is reachable on the documented install path for at least one host.

**Cursor skill visibility (VERSION alone is insufficient):** when the cut adds skills, prove the new skill dirs exist under the active install (prefer a local machine). If dual cache trees disagree, purge only the stale hash. A cloud-only run must not claim Cursor visibility is proven.

## Report

PR URLs + merge SHAs (bump and promote), release URL, tag → SHA, tree match `beta` ≡ `main`, any deferred Dependabot items.
