import type { ReactNode } from "react";
import type { BattlenetGame, LibraryGame } from "../api";
import { Destiny } from "./Destiny";
import { Elite } from "./Elite";
import { Eso } from "./Eso";
import { Factorio } from "./Factorio";
import { Gw2 } from "./Gw2";
import { Minecraft } from "./Minecraft";
import { Osrs } from "./Osrs";
import { Poe } from "./Poe";
import { RocketLeague } from "./RocketLeague";
import { Stardew } from "./Stardew";
import { Wow } from "./Wow";

export const GW2_APP_ID = 1284210;
export const ESO_APP_ID = 306130;
export const OSRS_APP_ID = 1343370;
export const DESTINY_APP_ID = 1085660;
export const POE_APP_ID = 238960;
export const POE2_APP_ID = 2694490;
export const ELITE_APP_ID = 359320;
export const FACTORIO_APP_ID = 427520;
export const STARDEW_APP_ID = 413150;
export const ROCKET_LEAGUE_APP_ID = 252950;
export const MINECRAFT_LAUNCHER_APP_ID = 1672970;

export type CompanionTile =
  | { kind: "steam"; game: LibraryGame }
  | { kind: "battlenet"; game: BattlenetGame };

function isMinecraftTile(game: LibraryGame): boolean {
  if (game.appId === MINECRAFT_LAUNCHER_APP_ID) return true;
  return /^Minecraft/i.test(game.name);
}

/** Pick the per-game companion screen for a Library tile, if any. */
export function companionScreen(tile: CompanionTile): ReactNode {
  if (tile.kind === "battlenet" && tile.game.product === "wow") {
    return <Wow />;
  }
  if (tile.kind === "steam") {
    if (isMinecraftTile(tile.game)) {
      return <Minecraft />;
    }
    switch (tile.game.appId) {
      case GW2_APP_ID:
        return <Gw2 />;
      case ESO_APP_ID:
        return <Eso />;
      case OSRS_APP_ID:
        return <Osrs />;
      case DESTINY_APP_ID:
        return <Destiny />;
      case POE_APP_ID:
      case POE2_APP_ID:
        return <Poe appId={tile.game.appId} />;
      case ELITE_APP_ID:
        return <Elite />;
      case FACTORIO_APP_ID:
        return <Factorio />;
      case STARDEW_APP_ID:
        return <Stardew />;
      case ROCKET_LEAGUE_APP_ID:
        return <RocketLeague />;
      default:
        return null;
    }
  }
  return null;
}
