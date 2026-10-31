//! Dota 2 adapter for the in-game watcher (design: `docs/design/game-watcher/`).
//! Dota 2 sends the same Valve Game State Integration snapshots as CS2, to
//! the same loopback listener (`game_watch_gsi`). This reads the fields the
//! watcher needs and diffs two snapshots into the shared `EventKind`s: kills,
//! deaths and the match end. Dota has no rounds, so a match end counts as a
//! one-round score (1-0 for a win, 0-1 for a loss).

use serde_json::Value;

use crate::game_watch::EventKind;

/// The game state Dota sends once the match is decided.
pub const POST_GAME: &str = "DOTA_GAMERULES_STATE_POST_GAME";

/// The fields of a Dota 2 GSI payload the watcher reads. Everything is
/// optional in the feed (menus, hero pick, spectating), so missing fields
/// stay `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DotaState {
    /// `map.name`, e.g. `start` for a normal match.
    pub map: Option<String>,
    /// `map.game_state`, e.g. `DOTA_GAMERULES_STATE_GAME_IN_PROGRESS`.
    pub game_state: Option<String>,
    /// `map.win_team`: `radiant`, `dire` or `none`.
    pub win_team: Option<String>,
    /// `player.team_name`: the player's side, `radiant` or `dire`.
    pub team: Option<String>,
    pub kills: Option<u32>,
    pub deaths: Option<u32>,
    /// `hero.name`, e.g. `npc_dota_hero_axe`.
    pub hero: Option<String>,
}

fn text(v: &Value, path: &[&str]) -> Option<String> {
    path.iter()
        .try_fold(v, |v, k| v.get(k))?
        .as_str()
        .map(str::to_owned)
}

fn num(v: &Value, path: &[&str]) -> Option<u32> {
    path.iter()
        .try_fold(v, |v, k| v.get(k))?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
}

/// Parse one GSI POST body. `None` when it isn't JSON.
pub fn parse_dota(body: &str) -> Option<DotaState> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(DotaState {
        map: text(&v, &["map", "name"]),
        game_state: text(&v, &["map", "game_state"]),
        win_team: text(&v, &["map", "win_team"]),
        team: text(&v, &["player", "team_name"]),
        kills: num(&v, &["player", "kills"]),
        deaths: num(&v, &["player", "deaths"]),
        hero: text(&v, &["hero", "name"]),
    })
}

/// The events between two payloads, the same way `cs2_events` reads CS2:
/// a counter going up is a kill or a death, and the game state turning
/// post-game with a winner is the match end. The first payload is only a
/// baseline. A match end needs a real game state before it and the player's
/// side; spectating gets none.
pub fn dota_events(prev: Option<&DotaState>, next: &DotaState) -> Vec<EventKind> {
    let Some(prev) = prev else {
        return Vec::new();
    };
    let up = |a: Option<u32>, b: Option<u32>| match (a, b) {
        (Some(a), Some(b)) if b > a => b - a,
        _ => 0,
    };
    let mut out = Vec::new();
    out.extend((0..up(prev.kills, next.kills)).map(|_| EventKind::Kill));
    out.extend((0..up(prev.deaths, next.deaths)).map(|_| EventKind::Death));
    let was_playing = prev.game_state.as_deref().is_some_and(|s| s != POST_GAME);
    if next.game_state.as_deref() == Some(POST_GAME) && was_playing {
        let side = |s: &str| matches!(s, "radiant" | "dire");
        if let (Some(win), Some(team)) = (
            next.win_team.as_deref().filter(|w| side(w)),
            next.team.as_deref().filter(|t| side(t)),
        ) {
            let won = u32::from(win == team);
            out.push(EventKind::MatchEnd { won, lost: 1 - won });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_watch::{recap, Recap, WatchEvent};

    fn payload(state: &str, win: &str, kills: u32, deaths: u32) -> String {
        format!(
            r#"{{"provider":{{"name":"Dota 2","appid":570,"version":47,"timestamp":1700000000}},
               "map":{{"name":"start","matchid":"7400000000","game_time":1200,"game_state":"{state}","win_team":"{win}","radiant_score":30,"dire_score":21}},
               "player":{{"steamid":"76561190000000000","name":"player","activity":"playing","kills":{kills},"deaths":{deaths},"assists":7,"team_name":"radiant"}},
               "hero":{{"id":2,"name":"npc_dota_hero_axe","level":18,"alive":true}}}}"#
        )
    }

    const LIVE: &str = "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS";

    #[test]
    fn a_dota_payload_parses_the_fields_the_watcher_reads() {
        let s = parse_dota(&payload(LIVE, "none", 5, 2)).unwrap();
        assert_eq!(
            s,
            DotaState {
                map: Some("start".into()),
                game_state: Some(LIVE.into()),
                win_team: Some("none".into()),
                team: Some("radiant".into()),
                kills: Some(5),
                deaths: Some(2),
                hero: Some("npc_dota_hero_axe".into()),
            }
        );
        assert_eq!(parse_dota("not json"), None);
        assert_eq!(parse_dota("{}"), Some(DotaState::default()));
    }

    #[test]
    fn dota_changes_between_payloads_become_events() {
        let a = parse_dota(&payload(LIVE, "none", 5, 2)).unwrap();
        let b = parse_dota(&payload(LIVE, "none", 7, 3)).unwrap();
        assert_eq!(
            dota_events(Some(&a), &b),
            vec![EventKind::Kill, EventKind::Kill, EventKind::Death]
        );
        // A repeat snapshot adds nothing, and the first one is a baseline.
        assert_eq!(dota_events(Some(&b), &b), vec![]);
        assert_eq!(dota_events(None, &b), vec![]);
        let won = parse_dota(&payload(POST_GAME, "radiant", 7, 3)).unwrap();
        assert_eq!(
            dota_events(Some(&b), &won),
            vec![EventKind::MatchEnd { won: 1, lost: 0 }]
        );
        let lost = parse_dota(&payload(POST_GAME, "dire", 7, 3)).unwrap();
        assert_eq!(
            dota_events(Some(&b), &lost),
            vec![EventKind::MatchEnd { won: 0, lost: 1 }]
        );
        // Post game to post game is not a second match end.
        assert_eq!(dota_events(Some(&won), &won), vec![]);
    }

    #[test]
    fn a_dota_match_end_needs_a_winner_and_the_players_side() {
        let live = parse_dota(&payload(LIVE, "none", 1, 1)).unwrap();
        let mut end = parse_dota(&payload(POST_GAME, "none", 1, 1)).unwrap();
        assert_eq!(dota_events(Some(&live), &end), vec![]);
        end.win_team = Some("dire".into());
        end.team = None;
        assert_eq!(dota_events(Some(&live), &end), vec![]);
        end.team = Some("spectator".into());
        assert_eq!(dota_events(Some(&live), &end), vec![]);
        // No game state before it (menus): not a match the player was seen in.
        end.team = Some("dire".into());
        assert_eq!(dota_events(Some(&DotaState::default()), &end), vec![]);
        assert_eq!(
            dota_events(Some(&live), &end),
            vec![EventKind::MatchEnd { won: 1, lost: 0 }]
        );
    }

    #[test]
    fn a_dota_timeline_feeds_the_shared_recap() {
        let snaps = [
            payload(LIVE, "none", 0, 0),
            payload(LIVE, "none", 2, 0),
            payload(LIVE, "none", 2, 1),
            payload(POST_GAME, "radiant", 3, 1),
        ];
        let mut prev: Option<DotaState> = None;
        let mut timeline = Vec::new();
        for (i, body) in snaps.iter().enumerate() {
            let next = parse_dota(body).unwrap();
            for kind in dota_events(prev.as_ref(), &next) {
                timeline.push(WatchEvent {
                    at_ms: i as u64 * 1_000,
                    kind,
                });
            }
            prev = Some(next);
        }
        assert_eq!(
            recap(&timeline),
            Recap {
                won: 1,
                lost: 0,
                kills: 3,
                deaths: 1,
                longest_win_streak: 0
            }
        );
    }
}
