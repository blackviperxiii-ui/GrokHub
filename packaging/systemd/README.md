# GrokHub agent (systemd user unit)

Optional. Keeps GrokHub’s agent core alive after login (tray + background agent), without a visible window until you open it.

**This is not required** for normal chat. Only enable if you want always-on agent mode.

```bash
# From a full install tree (or repo checkout):
./scripts/sync-user-integration.sh --agent --now

# Or manually:
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-agent.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub-agent.service
systemctl --user status grokhub-agent.service
```

Pause autonomy anytime from the tray menu or **Settings → Agent → Autonomy**.

## Not the same as `~/.grok`

The standalone xAI **`grok` CLI** under `~/.grok/downloads/` is a **different product**.
GrokHub’s command is always **`grokhub`**. A runaway high-CPU `grok` process is almost never GrokHub.
