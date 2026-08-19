# GrokHub systemd user units

## Cabin in the tray

Close on the window hides the cabin. The process keeps working (chat jobs, hub, idle reflect). Tray: **Show cabin**, **Halt**, **Quit**.

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub.service
```

`grokhub --agent` is the same: window starts hidden, tray is up. `GROKHUB_TRAY=0` quits on close (no tray).

## LAN hub only

Optional. The cabin already embeds the hub when you click **Start share**.
Use this when a box should keep `/v1` up without the GUI.

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-hub.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub-hub.service
```

Requires `grokhub` / `grokhub-hub` on `PATH` (`~/.local/bin` or `/usr/bin`).
Desktop control is Grok Build computer-use. Halt on the tray cancels the ACP turn.
