# Loadout addon for World of Warcraft

Loadout ships this addon inside the app and installs it for you: Library → World of Warcraft → **Install addon**, or **Install addon in all versions** when you have more than one. It copies this folder into each WoW flavor's `Interface/AddOns/Loadout` and keeps it up to date when Loadout updates. **Remove addon** deletes only that folder.

## Which versions

One copy loads on Retail and every Classic flavor: Mists of Pandaria Classic, TBC Classic Anniversary, Classic Era, and their PTR and beta clients. The `## Interface:` line lists each live build number, so no flavor marks it out of date.

Retail gets the most data. Classic has no Mythic+, Great Vault, renown, specializations or mount journal on most flavors, so the addon reads what Classic does have: professions from the skill list, your talent trees in place of a spec (for example `Arcane (40/21/0)`), reputations, currencies, quests and everything else below that the flavor offers. Anything a flavor lacks is left out.

## What it does

It listens to Blizzard's public addon events and writes a `LoadoutDB` saved variable with your own characters' level, class, spec, zone, gold, item level (equipped and overall) and gear, professions, currencies, renown and reputations, Mythic+ rating and keystone, Great Vault progress, raid and dungeon lockouts, achievement points, mounts and pets, quests, played time, boss kills and wipes, deaths and achievements. Loadout reads `WTF/Account/<account>/SavedVariables/Loadout.lua` and never runs it.

It reads nothing in combat, when Midnight hides many values, and nothing at logout, when the game has already cleared values such as gold. It sends no chat or addon messages, makes no protected calls and automates nothing.

In game it says hello in chat when you log in, and on Retail shows up in the minimap's addon menu. Click it there, or type `/loadout`, for a one-line summary.

WoW finds new addons only when it starts, so quit and restart WoW after installing. WoW writes saved variables only on logout, exit or `/reload`, so new data shows up in Loadout after one of those.

## Install by hand

Copy the `Loadout` folder into `World of Warcraft/_retail_/Interface/AddOns/` (or `_classic_`, `_anniversary_`, `_classic_era_`, and the PTR or beta folders). On Linux under Wine that folder is inside the prefix's `drive_c`.

Loadout writes the `## Interface:` line from the game's own `.build.info` when it installs, so the addon isn't marked out of date. When copying by hand, tick **Load out of date AddOns** if WoW flags it.
