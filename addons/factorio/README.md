# Loadout mod for Factorio

Loadout ships this mod inside the app and installs it when you ask: Settings → Game watcher → Factorio → **Install mod**. It copies the `loadout` folder into Factorio's `mods` folder and keeps it up to date when Loadout updates. **Remove mod** deletes only that folder.

## Before you install

- Any enabled mod puts a save on Factorio's separate modded achievement track, so Steam achievements stop for saves played with it.
- In multiplayer every player needs the same mods, so the mod is best kept for single-player.
- Factorio asks you to enable new mods; Loadout doesn't edit `mod-list.json`.

## What it does

It listens to Factorio's official mod API events (research finished, rocket launched, player died, a player joining) and every five minutes writes `script-output/loadout/state.json` with your research count and current research, rockets launched, your deaths, play time and the per-minute output of a few plates and circuits. It writes the file only on your own computer (`helpers.write_file` with your player index).

It adds no entities, recipes, settings or GUI, sends nothing anywhere and changes nothing in the game. The only state it keeps in the save is a death count per player. Factorio 2.0 or newer.

## Install by hand

Copy the `loadout` folder into Factorio's `mods` folder (`%AppData%\Factorio\mods` on Windows, `~/.factorio/mods` on Linux, `~/Library/Application Support/factorio/mods` on macOS) and enable it in the game's Mods menu.
