# AGENTS.md — GrokHub cabin notes

## Base branch
Feature/fix PRs → `beta`; every agent squash-merges its own PR into `beta` as soon as CI is green (no review hold). `.github/`, clippy, and release-script changes ship the same way. Only `beta` → `main` promotion, tags, and releases need the maintainer's approval. Hotfixes → `main` only with the maintainer's approval, then merge-back PR into beta (merge commit). No version bump on beta PRs — add `changelog.d/<slug>.md`; promotions run `cargo xtask bump`.

## Bot
Prefer Grok Build on the box (roles: explore, plan, implementer). Cursor Cloud paused until Oct 11. One bot per job.

## Compass files
Read the compass file before editing a module: `docs/compass/README.md` lists one short map per crate and hot module (owns, change recipe, what breaks, gotchas). Fix it in the same PR when you move code; `compass_paths` fails on stale paths.

## Cross-platform recode
Tauri 2 + React + TS shell approved (2026-10-23); egui cabin ships until parity. Plan and decisions: `docs/cross-platform/README.md`.

## Quality gates
Semgrep before Ready. dyl-review quick for draft asks — never dyl-ready-pr merge/babysit. Continual Learning keeps this file current.

## Skill cues
Reach for these coding skills at the matching moment (same practice by hand if a skill is not installed). Cues only — do not vendor plugin or skill files into the repo.

- **check-compiler-errors** after Rust or TypeScript edits, before pushing
- **loop-on-ci** or **fix-ci** once a PR is open, until it is green
- **fix-merge-conflicts** when a PR is behind `beta`
- **verify-this** or **run-smoke-tests** before marking work Done
- **blast-radius** before changing shared code (router, agent loop, memory, local model launcher)
- **tdd** for logic with clear behavior
- **statechart** for multi-state flows (online/offline/degraded, intentions)
- **make-pr-easy-to-review** when opening a PR

## Require vstack
Coding agents working on this repo use the [vstack](https://github.com/blackviperxiii-ui/vstack) skills; start with `viper-mode`. Do not copy vstack skills or rules into this repository.
- Claude Code: `.claude/settings.json` adds the vstack marketplace and turns on `vstack@vstack`, so it loads once you trust the folder. If `/plugin` doesn't list it, run `/plugin marketplace add blackviperxiii-ui/vstack` and `/plugin install vstack@vstack`.
- Cursor: import `blackviperxiii-ui/vstack` as a team marketplace; set it to Required and enable Auto Refresh.
- Other agents (Codex, Copilot, Jules): run `npx skills add blackviperxiii-ui/vstack -g -y` before you start, so the skills install for your user and nothing lands in the repo.
- The `blast-radius`, `tdd` and `statechart` cues above are vstack skills (`/vstack:blast-radius` and so on once the plugin loads).

How to use vstack here:
- Start nontrivial work with `viper-mode`. It routes to `how` and `why` before edits, `architect` for new modules, `blast-radius` before shared code, `tdd` for bugs and `interrogate` for reviews.
- Issues and board cards: run `follow-through` from pickup to PR (start comment, blocked note, done with proof).
- PR bodies and docs: `technical-writing`.
- `ui/` TypeScript: `typescript-best-practices`. Multi-state flows: `statechart`. Before a beta to main promotion: `qa-walkthrough`.
- No install? Read a skill straight from `https://github.com/blackviperxiii-ui/vstack/blob/main/skills/<name>/SKILL.md`.

## Branch cleanup
Delete own head after merge/close; scratch ASAP; never main/beta. GitHub auto-delete head branches is on.

## UI theme (Wave 1 + Critiquito motion 4–5)
- Composer glow: white `#e7e9ea` (`composer_glow_rgb` / FG). Breath via `icons::composer_breath`. Idle α≈0.25; streaming 0.25↔0.55; settle to idle in `GLOW_SETTLE_SECS` (200ms). Skip when `motion_ok` is false.
- Always permission: white ring on dark; settle fill α 0→1 and scale 0.98→1.0 over `ALWAYS_SETTLE_SECS` (140ms), then stop — no perpetual pulse.


## No personal info (hard rule)

This is open-source work meant for anyone to use. Keep every file, commit, PR and issue generic.

- Never include a real name, nickname used as a person, personal email, phone, address, employer, city, machine name, or real home-folder path. Where a name is needed, write "user". Paths use `/home/user`.
- Never mention the maintainer's other projects, personal bots or assistants, task boards, or personal workflow (Notion cards, Slack pings, approval habits). Examples use neutral names like `my-app`.
- Commits use the GitHub no-reply email. Squash merges use a clean subject and body with no `Co-authored-by` lines.
- `scripts/pii-guard/pii_guard.py` runs in CI as the required `pii-guard` check; run it locally before pushing.

## Command Centre board guide (GitHub access only)

Work is planned on the Notion board "Grok Bot Command Centre". Cards with
Status = Ready sync to GitHub issues on blackviperxiii-ui/GrokHub (app "Loadout").
You don't need Notion access: the issue is your copy of the card.

### Finding your work
1. Open issues with your label: claude, copilot or jules.
2. Do High priority first, then lowest Order (both are shown on the card, linked from the issue).
3. Each issue links its Notion card and copies its Evidence / Fix / Acceptance.
4. Don't start a new issue while you already have 2 open PRs.

### Board terms used in issues
- Status: Idea, Ready, Doing, Waiting, Needs you, Done, Notes.
- Assigned: Claude, Copilot or Jules (matches your label).
- Priority: High, Medium, Low, Eh... Whenever.
- Order: pickup sequence. Lower goes first.
- Result: the card's outcome line. Picked up / Done at: start and finish dates.

### Working an issue
- Start: comment on the issue saying you've started (the card moves to Doing).
- PR body must include:
  - `Closes #N` (the issue number)
  - the Notion card URL from the issue
  - a one-line summary of the change
- On merge to beta, the cards-sync workflow sets the card to Done, fills in
  Done at, puts the PR link in Result and closes the issue. Don't do this yourself.
- Blocked: comment on the issue with what blocked you and what's needed,
  then stop. The card goes back to Ready with your note.

### Never
- Work issues labelled for another coder, or change their labels.
- Close or delete issues, or relabel your own issue to someone else.

### Repo rules
- PRs target beta only. Never main or genesis.
- Squash-merge style: one clean subject line, no Co-authored-by lines.
- No force-pushes.
- Run fmt, lint and tests locally before pushing, then push once.
- At most 2 open PRs per coder.
- Merges: auto-merge (squash) turns on when a PR into beta opens, and GitHub
  merges it once the required checks pass. If auto-merge is off and the PR is
  green and not held, squash-merge it into beta yourself. Never merge into main.
- No personal info in the repo: write 'user' for names and /home/user for paths.

Coders with Notion access: see `docs/agents/board-guide-notion.md`.
