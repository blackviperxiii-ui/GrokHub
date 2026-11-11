import type { ReactNode } from "react";
import type { BattlenetGame, LibraryGame } from "../api";
import { Eso } from "./Eso";
import { Gw2 } from "./Gw2";
import { Osrs } from "./Osrs";
import { Wow } from "./Wow";

export const GW2_APP_ID = 1284210;
export const ESO_APP_ID = 306130;
export const OSRS_APP_ID = 1343370;

export type CompanionTile =
  | { kind: "steam"; game: LibraryGame }
  | { kind: "battlenet"; game: BattlenetGame };

/** Pick the per-game companion screen for a Library tile, if any. */
export function companionScreen(tile: CompanionTile): ReactNode {
  if (tile.kind === "battlenet" && tile.game.product === "wow") {
    return <Wow />;
  }
  if (tile.kind === "steam") {
    switch (tile.game.appId) {
      case GW2_APP_ID:
        return <Gw2 />;
      case ESO_APP_ID:
        return <Eso />;
      case OSRS_APP_ID:
        return <Osrs />;
      default:
        return null;
    }
  }
  return null;
}
