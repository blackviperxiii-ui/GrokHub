# Planning board guide (GitHub access only)

Work is planned on the planning board (a Notion database). Cards with
Status = Ready sync to GitHub issues on blackviperxiii-ui/GrokHub (app "Loadout").
You don't need Notion access: the issue is your copy of the card.

## Finding your work
1. Open issues with your label: claude, copilot or jules.
2. Do High priority first, then lowest Order (both are shown on the card, linked from the issue).
3. Each issue links its Notion card and copies its Evidence / Fix / Acceptance.
4. Don't start a new issue while you already have 2 open PRs.

## Board terms used in issues
- Status: Idea, Ready, Doing, Waiting, Needs you, Done, Notes.
- Assigned: Claude, Copilot or Jules (matches your label).
- Priority: High, Medium, Low, Eh... Whenever.
- Order: pickup sequence. Lower goes first.
- Result: the card's outcome line. Picked up / Done at: start and finish dates.

## Working an issue
- Start: comment on the issue saying you've started (the card moves to Doing).
- PR body must include:
  - `Closes #N` (the issue number)
  - the Notion card URL from the issue
  - a one-line summary of the change
- On merge to beta, the cards-sync workflow sets the card to Done, fills in
  Done at, puts the PR link in Result and closes the issue. Don't do this yourself.
- Blocked: comment on the issue with what blocked you and what's needed,
  then stop. The card goes back to Ready with your note.

## Never
- Work issues labelled for another coder, or change their labels.
- Close or delete issues, or relabel your own issue to someone else.

## Repo rules
- PRs target beta only. Never main or genesis.
- Squash-merge style: one clean subject line, no Co-authored-by lines.
- No force-pushes.
- Run fmt, lint and tests locally before pushing, then push once.
- At most 2 open PRs per coder.
- Merges: auto-merge (squash) turns on when a PR into beta opens, and GitHub
  merges it once the required checks pass. If auto-merge is off and the PR is
  green and not held, squash-merge it into beta yourself. Never merge into main.
- No personal info in the repo: write 'user' for names and /home/user for paths.

## vstack skills
Load the vstack skills before you start (`npx skills add blackviperxiii-ui/vstack -g -y`, or read them from `https://github.com/blackviperxiii-ui/vstack/blob/main/skills/<name>/SKILL.md`). Do not copy them into this repo.
- Every issue: `viper-mode` to plan and verify, `follow-through` from your start comment to the PR.
- Before touching shared code: `blast-radius`. Bugs: `tdd`. PR body: `technical-writing`.
