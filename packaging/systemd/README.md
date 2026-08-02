# GrokHub agent (systemd user unit)

Keeps GrokHub’s agent core alive after login (tray + job queue), without a visible window until you open it.

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-agent.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub-agent.service
systemctl --user status grokhub-agent.service
```

Pause autonomy anytime from the tray menu or **Settings → Autonomy**.
