# Command Centre board guide (Notion access)

Board: "Grok Bot Command Centre" database, data source "Command Center"
(collection://f66443ba-e39d-4eeb-b8fd-14c2d87c3da1). Check it at the start of every session.

## Fields
- Name: card title. Body holds Evidence / Fix / Acceptance.
- Status: Idea, Ready, Doing, Waiting, Needs you, Done, Notes.
- Assigned: who owns the card. Coders: Claude, Copilot, Jules.
- Order: pickup order (number). Lowest Ready Order goes first. Blank = not queued.
- Priority: High, Medium, Low, Eh... Whenever.
- Project: Loadout work is Project = GrokHub.
- Result: short outcome or current state with links and any assumptions.
- Picked up: date you started. Done at: date the work landed.
- Archive now / Archive section: archive controls. Leave them alone.

## Finding your work
1. Filter: Status = Ready AND Assigned = your name (Claude, Copilot or Jules).
2. Sort: Priority High first, then lowest Order.
3. Take the top card. Read the whole body, and the linked GitHub issue if there is one.
4. Don't start a new card while you already have 2 open PRs.

## Working a card
- Start: set Status = Doing and Picked up = now. Add a line to Result saying you started.
- PR opened: put the PR link and a one-line summary in Result.
- Merged to beta (merges are done by the Repo bot): set Status = Done,
  Done at = merge time, and update Result with the PR link and summary.
- Never set Done before the PR has merged to beta.
- Blocked: set Status back to Ready and put a clear note in Result saying
  what blocked you and what's needed. Don't use Needs you.

## Never
- Edit, reassign or reorder cards assigned to anyone else.
- Delete or archive cards, or change the board's schema or options.
- Change Order or Priority on any card.

## Repo rules (blackviperxiii-ui/GrokHub, app "Loadout")
- PRs target beta only. Never main or genesis.
- Squash-merge style: one clean subject line, no Co-authored-by lines.
- No force-pushes.
- Run fmt, lint and tests locally before pushing, then push once.
- At most 2 open PRs per coder.
- The Repo bot does all merges. Never merge your own PR.
- No personal info in the repo: write 'user' for names and /home/user for paths.
