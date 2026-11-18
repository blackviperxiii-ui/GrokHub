---
name: setup-vstack
description: Install and configure Viper Stack for the current coding agent (Cursor, Claude Code, Codex, Antigravity, Muse, Windsurf, OpenCode, Grok Build).
---

# Setup Vstack

Install skills and always-on config for **this** host. Never write another host's paths.

**Read `GUIDE.md` in this folder** for detect → install → pointer → verify. Host matrix: `docs/CONSUMING.md`.

Short form:

1. Detect host (explicit user target wins).
2. Install: prefer marketplace/plugin; else `install.sh` / `install_for.sh` for that host.
3. In a project: ensure consumer `AGENTS.md` / `CLAUDE.md` pointer (CONSUMING snippet).
4. Cursor-only: write `~/.cursor/rules/vstack-models.mdc` if missing (model budget). Never write `~/.cursor/**` on other hosts.
5. Verify `viper-mode/SKILL.md` on the chosen path; reload / new session; start with `/viper-mode`.
