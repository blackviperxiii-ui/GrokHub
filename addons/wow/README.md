# Loadout addon for World of Warcraft

Loadout ships this addon inside the app and installs it for you: Library → World of Warcraft → **Install addon**. It copies this folder into each WoW flavor's `Interface/AddOns/Loadout` and keeps it up to date when Loadout updates. **Remove addon** deletes only that folder.

## What it does

It listens to Blizzard's public addon events and writes a `LoadoutDB` saved variable with your own characters' level, class, zone, gold, item level, played time (after you type `/played`), boss kills and wipes, deaths and achievements. Loadout reads `WTF/Account/<account>/SavedVariables/Loadout.lua` and never runs it.

It shows no frames, sends no chat or addon messages, makes no protected calls and automates nothing. `/loadout` prints its version.

WoW writes saved variables only on logout, exit or `/reload`, so new data shows up in Loadout after one of those.

## Install by hand

Copy the `Loadout` folder into `World of Warcraft/_retail_/Interface/AddOns/` (or `_classic_`, `_classic_era_`). On Linux under Wine that folder is inside the prefix's `drive_c`.

Loadout writes the `## Interface:` line from the game's own `.build.info` when it installs, so the addon isn't marked out of date. When copying by hand, tick **Load out of date AddOns** if WoW flags it.
