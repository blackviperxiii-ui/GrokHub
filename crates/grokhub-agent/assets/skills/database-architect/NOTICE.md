# Notice

This folder contains the Database Architect agent from claude-code-templates
by Daniel (San) Ávila, https://github.com/davila7/claude-code-templates
(`cli-tool/components/agents/database/database-architect.md`), licensed under
the MIT License (`LICENSE` in this folder). The pinned commit and a SHA-256 of
every vendored file are in `VENDORED.json`.

## Changes made by vstack

The upstream file is copied unmodified, apart from these:

- Renamed: upstream `database-architect.md` → `AGENT.md`, so agent hosts load
  the vstack wrapper instead of the upstream trigger. Its text is unchanged.
- Added: `SKILL.md` (vstack wrapper with its own trigger, the safety rules it
  puts first, hand-offs mapped to vstack skills, and SQLite notes), this
  `NOTICE.md`, and the generated `VENDORED.json`.

vstack never runs the upstream `claude-code-templates` installer.
