# Setup Vstack — host guide

Use this after the short form in `SKILL.md`. Full Works-with matrix and consumer pointer text: `docs/CONSUMING.md` (cite; do not paste the whole table here).

## 1. Detect host

Explicit user target wins (`setup for Claude`, `--host codex`, etc.).

Otherwise self-identify from the running agent, then cues:

| Signal | Host |
|--------|------|
| Cursor IDE / plugin / Cloud Agent | `cursor` |
| Claude Code / `.claude/` plugin context | `claude` |
| Grok Build / `.grok/` | `grok` |
| Codex, Muse, Windsurf, OpenCode, Antigravity (or portable `.agents/skills`) | that name, or `agents` |
| Aider / Continue | documented-generic only — point at CONSUMING; do not invent skill dirs |

Never assume Cursor because this skill lives in a Cursor-shaped repo.

## 2. Install skills

Prefer marketplace/plugin when the host has one. Else release or clone install. From a **vstack checkout**:

| Host | Preferred | Fallback |
|------|-----------|----------|
| `cursor` | Team marketplace **Import from Repo** (CONSUMING Projects checklist) or local plugin | `install_for.sh --host cursor --global` or `--project …` → `.agents/skills` |
| `claude` | `/plugin marketplace add blackviperxiii-ui/vstack` then `/plugin install vstack@vstack` | `install.sh --target claude` or `install_for.sh --host claude` (`--global` or `--project`) |
| `codex` / `muse` / `windsurf` / `opencode` / `antigravity` / `agents` | `install_for.sh --host <name> --project …` (or `--global`) | `install.sh --target agents`; Codex also `npx skills add blackviperxiii-ui/vstack` |
| `grok` | `install_for.sh --host grok --project …` or `--global` | `install.sh --target grok` |

Release one-liner (user-home targets only):

```bash
curl -fsSL https://raw.githubusercontent.com/blackviperxiii-ui/vstack/main/install.sh | bash -s -- --target <cursor|claude|agents|grok>
```

Do not `cp -R` skills by hand into invented paths. Do not write Claude/agents/grok dirs when host is Cursor unless the user asked for a multi-install. Do not write `~/.cursor/**` unless host is `cursor`.

## 3. Always-on pointer (project)

When working in a **consumer** project, ensure a short vstack pointer in `AGENTS.md` (and optional one-liner in `CLAUDE.md`) using the snippet in CONSUMING. Skip inventing full playbook pastes.

## 4. Cursor-only model rule

**Only if host is `cursor`.** If missing, write `~/.cursor/rules/vstack-models.mdc` (or project `.cursor/rules/`):

```
---
description: Viper Stack models — Auto / Composer Standard
alwaysApply: true
---
# budget: medium (high)
feature, refactoring, bug-fix, perf-issue, hillclimb: auto
judgment and prose: auto
hardest tasks: auto
how, why, arena, swarm, architect, interrogate: auto
```

Tell the user: parent Auto or Composer 2.5 with Fast OFF, then a fresh session.

On Claude and other hosts: skip this file. Model defaults are host-native; skills + AGENTS/CLAUDE pointer are enough.

## 5. Verify

1. Confirm `viper-mode/SKILL.md` is readable on the install path for this host (plugin skills, `~/.claude/skills/viper-mode`, `.agents/skills/viper-mode`, `.grok/skills/viper-mode`, etc.).
2. Ask the user to reload / start a new session if the host requires it.
3. Start nontrivial work with `/viper-mode` (or load `viper-mode/SKILL.md`).
4. Optional once: offer `/create-verification-skill` or `/create-formal-verification-skill` if the project has no verify harness / formal verify skill.

### Cursor: skills missing but VERSION looks right

1. Compare local `VERSION` to the skill dirs actually present under the active install / plugin tree.
2. List Cursor plugin cache hashes (e.g. under `~/.cursor/plugins/cache/…/vstack*`); if two trees disagree, delete only the stale hash.
3. Reinstall (`install.sh --version` or marketplace refresh), then **Reload Window**.
4. Do not blame slash-menu truncation until dual-cache is proven.
5. Cloud Agents cannot prove the user's home FS — route a machine-capable worker or give the user the commands above.

### Cursor Projects / Cloud Agents

Personal local plugin and Sync Skills do not reach Projects. Point admins at `docs/CONSUMING.md` → **Cloud Agents and Projects**: Import from Repo (not MCP), Required, Auto Refresh + GitHub App, Refresh after release, **new** agent.

**Dual-ship first:** repo must ship root `plugin.json` (Agent Plugins) **and** `.cursor-plugin/plugin.json`. If Import from Repo works but the Cursor-hosted/marketplace plugin does not, verify dual-ship before calling it docs/dashboard-only.

**Don't close** an Import-vs-Cursor-supplied gap until both channels list skills in a **new** Project agent. Packaging gap → dual-ship root + `.cursor-plugin/`.

## Hard rules

- Host isolation: one host's paths per run unless the user explicitly requests multi-install.
- Aider / Continue: CONSUMING only (`--read AGENTS.md` / rules pointer); no fake skill install.
- Keep rigor; protect token allowance (Cursor model rule uses `auto`).
