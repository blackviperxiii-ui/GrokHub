# In-game watcher (design spike)

Status: design plus a proof of concept for Counter-Strike 2. Nothing is wired into the cabin yet.

## What it is

A small local watcher that reads a game's own event feed while the user plays. Plain rules turn those events into pointers and stats, which costs zero model tokens. It keeps a match timeline and shows a recap card after the match. The model runs only when the user asks a question about the match, or once for an optional written recap, on the local model when one is set up.

It builds on what already exists:

- `grokhub_core::game_companion` knows which Steam game is running (Linux launch wrapper or `registry.vdf`, Windows `RunningAppID`).
- `grokhub_core::steam::SteamGame` carries `playtime_min`, the input for picking which game to support first.
- `pc_health` already samples during a game session and posts one Pulse card after it, which is the same session shape.

## Event sources

| Game | Feed | How it arrives | Notes |
| --- | --- | --- | --- |
| Counter-Strike 2, Dota 2 | Valve Game State Integration (GSI) | The game POSTs JSON snapshots to a local URL named in a `gamestate_integration_*.cfg` file in the game's `cfg/` folder | Official and documented, with no anti-cheat concerns. Snapshots, not deltas: events come from diffing. |
| League of Legends | Riot Live Client Data API | `https://127.0.0.1:2999/liveclientdata/*`, polled during a match | Self-signed certificate; it has an event list (`eventdata`). |
| Valorant | No public live API | None | Only a post-match API with a key. Treat as process detection plus recap only. |
| World of Warcraft | `WoWCombatLog.txt` after `/combatlog` | File tail | High volume; an optional small addon could mark boss pulls. |
| Anything else | Steam now-playing, process detection, hotkeys | `game_companion`, `local_governor::game_in_processes` | Session start and end only, plus user-marked moments. |
| Overwolf | Its own SDK | Overwolf apps only | Noted, not used: it would tie Loadout to their runtime. |

## Pick for the first game

The rule is to ship first the supported game with the most `playtime_min` in the user's Steam library, read at runtime and never committed. The proof of concept uses CS2. GSI is the cleanest feed on the list (official, push-based, local, documented), and the same code shape covers Dota 2 later. If the user's top game is League, the League adapter goes first instead; the rules and timeline below don't change.

## Architecture

```
game feed ──> adapter ──> events ──> timeline (in memory, then disk at match end)
 (GSI POST,    (per game,             │
  file tail,    pure parse            ├─> rules ──> pointers (toast / overlay chip)
  poll)         + diff)               └─> recap ──> one Pulse card after the match
                                                    └─> "Ask about this match" (model, on request)
```

- **Service:** a background thread in the cabin, started only while `game_companion` sees a supported game running and the watcher is turned on in Settings. For GSI it's a `127.0.0.1`-only HTTP listener on one fixed port. The `.cfg` file Loadout writes includes an `auth` token, and the listener drops posts without it.
- **Budget:** idle when no game runs. During a match, under 1% of one core and under 10 MB RAM. GSI posts arrive about every 0.1–1 s, and each one is a small JSON parse and diff. Rules run per event, not per frame, and never block the game.
- **Event schema:** `WatchEvent { at_ms, kind }`, with `kind` from a small closed set per family (the PoC has `RoundStart`, `RoundEnd { won }`, `Kill`, `Death`, `MatchEnd { won, lost }`). New games map onto the same kinds where they fit, plus game-specific kinds where they don't.
- **Rules engine:** pure functions over the timeline (`fn(&[WatchEvent]) -> Vec<Pointer>`), one per rule, each with fixture tests. No scripting language. Each pointer has a cooldown so it doesn't repeat every round.
- **Timeline storage:** in memory during the match, then one JSON file per match under the config dir (`game-watch/<game>/<match-start>.json`). Matches older than 30 days or beyond 200 per game get pruned.
- **Recap card:** one Pulse card with the score, K/D, longest win streak and the pointers that fired, plus an "Ask about this match" chip. The chip sends the timeline summary (not the raw feed) to the model the user picked, the local model first.
- **Privacy:** everything stays on the machine. The listener binds to loopback only. Nothing is uploaded, and the model sees a match only when the user asks. Turning the watcher off stops the listener and removes the `.cfg` file Loadout wrote.

## Proof of concept

`crates/grokhub-core/src/game_watch.rs`:

- `parse_cs2(body)` reads the GSI fields the watcher needs: map phase, round, round phase, winning team, the player's team, team scores, and kills and deaths.
- `cs2_events(prev, next)` diffs two snapshots into events. A repeat snapshot adds nothing.
- `pointers(timeline)` has two rules: three rounds lost in a row, and dying in the first 20 seconds in at least 3 of the last 5 rounds.
- `recap(timeline)` gives the score (from `MatchEnd` when present), K/D and longest win streak.

Run: `cargo test -p grokhub-core --lib game_watch`.

Not in the PoC: the listener, the `.cfg` writer, Settings, the overlay and the card. Those are the follow-ups below.

## Follow-up cards (not Ready)

1. GSI listener on loopback, with the auth token, plus writing and removing `gamestate_integration_loadout.cfg` in CS2's `cfg/` folder, behind a Settings toggle.
2. Wire `game_watch` into the cabin: start and stop with `game_companion`, show pointers as a quiet toast, and post the recap card when the match ends (Windows shots).
3. Store and prune per-match timelines on disk, and add "Ask about this match" with the local model first.
4. Dota 2 adapter on the same GSI listener.
5. League adapter over the Live Client Data API (self-signed loopback certificate).
6. WoW combat log tail with boss-pull detection.
7. Pick the first shipped game from the user's top `playtime_min` at runtime and say so in Settings.
