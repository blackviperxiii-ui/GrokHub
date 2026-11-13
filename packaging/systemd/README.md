# GrokHub systemd user units

## Start at login

The app starts at login through its own switch: **Settings → Start at login**
(on Linux this writes an autostart entry that runs `grokhub-shell --autostart`,
which opens in the tray). It needs no systemd unit.

`scripts/install.sh --user` moves an old cabin install over: when the cabin's
`grokhub.service` was enabled, it is turned off and the shell's autostart entry
is written in its place, so the switch shows as on.

## LAN hub only

Optional. Use this when a box should keep `/v1` up without the app open.

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-hub.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub-hub.service
```

Requires `grokhub-hub` on `PATH` (`~/.local/bin` or `/usr/bin`).
Halt on the tray stops the running turn.
