# Loadout addon for The Elder Scrolls Online

Loadout ships this addon inside the app and installs it when you ask: Settings → Game watcher → The Elder Scrolls Online → **Install addon**. It copies the `Loadout` folder into each ESO server folder's `AddOns` (`live` for NA, `liveeu` for EU) and keeps it up to date when Loadout updates. **Remove addon** deletes only that folder.

## What it does

It listens to ZeniMax's public addon events and writes a `LoadoutDB` saved variable with your own characters' level, champion points, class, race, zone, gold, play time, achievement points, deaths and recent level-ups and achievements. Loadout reads `SavedVariables/Loadout.lua` as data and never runs it.

It shows nothing, sends no chat, automates nothing and changes nothing in the game. Every API call is guarded, so a renamed function in a later game update leaves that one field empty instead of raising an error.

ESO writes saved variables only on logout, quit or `/reloadui`, so new data shows up in Loadout after one of those.

This addon is not created by, affiliated with or sponsored by ZeniMax Media Inc. or its affiliates. The Elder Scrolls® and related logos are registered trademarks or trademarks of ZeniMax Media Inc. in the United States and/or other countries.

## Install by hand

Copy the `Loadout` folder into `Documents/Elder Scrolls Online/live/AddOns/` (or `liveeu`). On Linux under Proton that folder is inside the game's prefix, under `drive_c/users/steamuser/Documents`. If the game marks it out of date after a patch, tick **Allow out of date add-ons** in the Add-Ons menu.
