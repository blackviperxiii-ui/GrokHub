# GrokHub

**v0.8.37** — Grok-native agent control plane for **Arch Linux / CachyOS**.

Adaptive modes · Imagine · connectors · skills · automations · usage meter · free-Grok fallback · setup sync · unsandboxed desktop host (CLI · files · apps).

**Repository:** [github.com/blackviperxiii-ui/Grok-Hub](https://github.com/blackviperxiii-ui/Grok-Hub)

> Windows builds live in a **separate repository**. This repo is Linux-focused (Arch / CachyOS packaging, AUR, and in-app updates).

---

## Always-on autonomy

Durable **agent job queue**, autonomy levels (0 Assist → 4 Goal mode), tray Pause/Resume, optional headless agent.

| Level | Behavior |
|------|----------|
| 0 Assist | Chat; tools when asked |
| 1 Supervised | Default — tools with confirm |
| 2 Semi-auto | Queue when busy |
| 3 Always-on | Auto-claim workboard; tray approvals |
| 4 Goal mode | Multi-step resume with budgets |

```bash
# Tray / hidden window — agent core stays up
grokhub --agent

# systemd user unit
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-agent.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now grokhub-agent.service
```

UI: **Queue** sidebar · **Settings → Autonomy**. Website connectors stay status-only until native OAuth invoke.

## Install — Arch Linux / CachyOS

### From source (recommended)

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel

git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub

sudo ./scripts/install-arch.sh
grokhub
```

### User install (no root)

Prefer this if you already have a system install under `/usr` and want updates without `pkexec`:

```bash
./scripts/install-arch.sh --user
# ensure ~/.local/bin is on PATH
grokhub
```

| After user install | Path |
|--------------------|------|
| Launcher | `~/.local/bin/grokhub` |
| App files | `~/.local/lib/grokhub` |
| User data | `~/.config/GrokHub` |

### AUR-style local package

```bash
cd packaging/aur
cp PKGBUILD-bin PKGBUILD
makepkg -si
grokhub
```

| After system install | Path |
|----------------------|------|
| Launcher | `/usr/bin/grokhub` |
| App files | `/usr/lib/grokhub` |
| Desktop entry | `/usr/share/applications/grokhub.desktop` |
| User data | `~/.config/GrokHub` |

**Pin to taskbar:** open **GrokHub** from the app menu, then pin that icon — not a generic Electron process.

More detail: [packaging/aur/README.md](packaging/aur/README.md)

### Release tarball (updater / offline)

From [Releases](https://github.com/blackviperxiii-ui/Grok-Hub/releases), download **`grokhub-desktop-v*.tar.gz`** (includes `.output`). The in-app updater uses the same asset.

---

## First-run setup

1. Open **Settings**.
2. Connect access (any one works):
   - **Link Grok website** — free grok.com account works (free-tier fallback)
   - **Connect with Grok OAuth** — SuperGrok / X Premium+
   - **xAI API key** — from [console.x.ai](https://console.x.ai)
3. Optional: **Install app menu entry** (`.desktop` launcher).
4. Optional: GitHub token + **Setup sync** across machines.

Secrets and chat history stay on the device; clean installs do not ship personal data.

---

## What you get

| Surface | Capability |
|---------|------------|
| **Agent** | Streaming chat · stop · Adaptive router · host tools · reply/copy/delete · attach · voice |
| **History** | Search · pin · folders · date groups · rename / delete |
| **Imagine** | Image / video · aspect · quality · reference · lightbox · multi-delete |
| **Connectors** | Website-linked status · tools where available |
| **Skills / Automations** | Heartbeat schedules · multi-time runs |
| **Desktop host** | Unsandboxed shell · files · apps (bash) |
| **Usage** | Plan meter · poll from grok.com every minute |
| **Updates** | Check / install / rollback / self-test from this GitHub repo |
| **Command palette** | `Ctrl+K` — jump to views, modes, recent chats |
| **Appearance** | Dark / Light / System theme |

---

## Repair (broken / blank window)

Preserves chats & secrets; rebuilds UI and reinstalls the shell:

```bash
git pull
./scripts/repair-install.sh
grokhub
```

Build a release tarball **with** `.output` (for GitHub Releases / updater):

```bash
npm run release:bundle
# upload dist-release/grokhub-desktop-v*.tar.gz as a release asset
```

---

## Development

```bash
npm install
npm run dev              # UI on 0.0.0.0:8080
npm run desktop:dev      # Electron → dev UI (run `npm run dev` first)
npm run desktop:build    # production .output for packaging
npm run typecheck
```

| Script | Purpose |
|--------|---------|
| `npm run desktop` | Electron against `GROKHUB_URL` |
| `npm run desktop:arch` | Arch helper launcher |
| `npm run aur:release` | AUR release helper |
| `npm run release:bundle` | Desktop tarball for Releases |

---

## Uninstall

**System install:**

```bash
sudo rm -f /usr/bin/grokhub /usr/share/applications/grokhub.desktop
sudo rm -rf /usr/lib/grokhub
# optional user data:
# rm -rf ~/.config/GrokHub
```

**User install:**

```bash
rm -f ~/.local/bin/grokhub ~/.local/share/applications/grokhub.desktop
rm -rf ~/.local/lib/grokhub
# optional: rm -rf ~/.config/GrokHub
```

---




## Workboard

Kanban-style task board the agent and you share:

| | |
|--|--|
| Agent pins | `WORK_PIN: title \| detail \| priority=high` |
| Agent updates | `WORK_UPDATE: id-or-title \| status=in_progress` |
| You | Approve · Stage · Start · Done · Dismiss |
| Slash | `/board` · `/board add Fix the meter` |

Statuses: **proposed → approved → staged → in progress → done** (or dismiss).

## Project workspace

Settings → Project: bind a folder. Agent context gets a tree + README/package summary; host tools prefer that tree.

## Learning & self-improvement

After **every** agent turn GrokHub:

1. Extracts prefs / paths / topics from the exchange  
2. Appends a line to `daily/YYYY-MM-DD.md`  
3. Writes facts into `MEMORY.md` / prefs into `USER.md`  
4. Updates `STATUS.md` + `LEARNINGS.md`  
5. Every **3** turns: light reflect · every **12**: full reflect (LLM when online)  

Agent can also emit: `MEMORY_NOTE: durable fact`

## Learning & self-improvement

GrokHub learns from turn outcomes, 👍/👎 on replies, and explicit prefs:

| | |
|--|--|
| **Signals** | Success/fail turns, host/tool use, user prefs in chat, thumbs |
| **Insights** | Distilled bullets pinned into context |
| **Adaptive bias** | Soft weight on Fast/Think/Deep/Build from track record |
| **Reflect** | `/learn reflect` or Settings → Learning → writes `LEARNINGS.md` |
| **Manual** | `/learn note …` · rate assistant messages |

## File memory (M1)

Long-term agent memory is plain Markdown under your user data folder (not the install tree):

```text
~/.config/GrokHub/memory/
  USER.md          # profile / prefs
  MEMORY.md        # durable facts & decisions
  daily/YYYY-MM-DD.md
```

| Action | How |
|--------|-----|
| View / edit | Settings → Memory |
| Chat write | `/memory note` · `/memory user …` · `/memory today …` |
| Inspect | `/memory show` |
| Compact flush | Facts written to `MEMORY.md` + daily log |
| Model pin | Budgeted injection every turn via context manager |

## Context management

GrokHub budgets what is sent to the model (not what you see in the UI):

| Piece | Behavior |
|-------|----------|
| **Budget** | ~96k token estimate; headroom for tools + reply |
| **Pins** | Memory notes, OpenClaw import, connector caps (hard-capped) |
| **Window** | Recent messages under budget; older tool dumps trimmed |
| **Compact** | Auto when over ~72% budget, or **`/compact`** / toolbar **Compact** |
| **Summary** | Stored on the thread; full chat history stays visible |
| **Flush** | Heuristic facts appended to persistent memory notes |
| **Inspect** | Toolbar **Context N%** or **`/context`** |

## Stability notes (Linux)

| Topic | Behavior |
|-------|----------|
| **Install pick** | Launcher prefers a complete `~/.local/lib/grokhub` over `/usr` when the system bridge is missing `factoryReinstall` |
| **Updates** | Stop UI via pidfile + cmdline check only — never `fuser -k` on the port |
| **Rollback** | `.prev` kept up to ~7 days, pruned after a healthy self-test |
| **Sandbox** | Default `no-sandbox` for system Electron; set `GROKHUB_SANDBOX=1` to try Chromium sandbox |
| **Logs** | `~/.config/GrokHub/logs/app-*.log` and `ui.log` |
| **Electron** | Arch/CachyOS: use distro `electron` (e.g. 43). DevDependency pin is for tooling; runtime is system electron |

## License

MIT
