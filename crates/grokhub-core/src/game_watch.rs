//! In-game watcher proof of concept (design: `docs/design/game-watcher/`).
//! Reads Counter-Strike 2 Game State Integration payloads, turns the change
//! between two payloads into match events, and runs plain rules over the
//! timeline for pointers and an end-of-match recap. No model is called and
//! nothing leaves the machine. `MatchTracker` and `ToastGate` are what the
//! cabin runs per event, for any game whose adapter yields `WatchEvent`s.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::update_feed::{digest_card, UpdateCard};

/// One thing that happened in a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    RoundStart { round: u32 },
    RoundEnd { round: u32, won: bool },
    Kill,
    Death,
    MatchEnd { won: u32, lost: u32 },
    // League: the match began (`GameStart`), and an objective fell, with
    // `ally` true when the player's team took it (`None`: side unknown).
    MatchStart,
    Objective { what: Objective, ally: Option<bool> },
    // WoW: a boss pull began, and ended in a kill or a wipe.
    PullStart(Pull),
    PullEnd { pull: Pull, kill: bool },
}

/// The League objectives the watcher records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Objective {
    Dragon,
    Baron,
    Turret,
}

/// A WoW boss encounter, as its `ENCOUNTER_START` / `ENCOUNTER_END` lines
/// name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pull {
    pub encounter_id: u32,
    pub name: String,
    pub difficulty: u32,
    pub group_size: u32,
}

/// An event and when it was seen, in ms since the epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEvent {
    pub at_ms: u64,
    pub kind: EventKind,
}

/// The fields of a CS2 GSI payload the watcher reads. Everything is optional
/// in the feed (spectating, menus, warmup), so missing fields stay `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cs2State {
    pub map_phase: Option<String>,
    pub round: Option<u32>,
    pub round_phase: Option<String>,
    pub win_team: Option<String>,
    pub team: Option<String>,
    pub score_ct: Option<u32>,
    pub score_t: Option<u32>,
    pub kills: Option<u32>,
    pub deaths: Option<u32>,
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
pub fn parse_cs2(body: &str) -> Option<Cs2State> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(Cs2State {
        map_phase: text(&v, &["map", "phase"]),
        round: num(&v, &["map", "round"]),
        round_phase: text(&v, &["round", "phase"]),
        win_team: text(&v, &["round", "win_team"]),
        team: text(&v, &["player", "team"]),
        score_ct: num(&v, &["map", "team_ct", "score"]),
        score_t: num(&v, &["map", "team_t", "score"]),
        kills: num(&v, &["player", "match_stats", "kills"]),
        deaths: num(&v, &["player", "match_stats", "deaths"]),
    })
}

/// The events between two payloads. GSI sends whole snapshots, so events
/// come from what changed: a new round number, a round phase turning `over`,
/// a stats counter going up, the map phase turning `gameover`. The first
/// payload (`prev` is `None`) is only a baseline: the watcher may start mid
/// round or on the post-match screen, so nothing it shows was seen happen.
pub fn cs2_events(prev: Option<&Cs2State>, next: &Cs2State) -> Vec<EventKind> {
    let Some(prev) = prev else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if next.round_phase.as_deref() == Some("live") && prev.round_phase.as_deref() != Some("live") {
        if let Some(round) = next.round {
            out.push(EventKind::RoundStart { round });
        }
    }
    let up = |a: Option<u32>, b: Option<u32>| match (a, b) {
        (Some(a), Some(b)) if b > a => b - a,
        _ => 0,
    };
    out.extend((0..up(prev.kills, next.kills)).map(|_| EventKind::Kill));
    out.extend((0..up(prev.deaths, next.deaths)).map(|_| EventKind::Death));
    if next.round_phase.as_deref() == Some("over") && prev.round_phase.as_deref() != Some("over") {
        if let (Some(round), Some(win), Some(team)) = (next.round, &next.win_team, &next.team) {
            out.push(EventKind::RoundEnd {
                round,
                won: win == team,
            });
        }
    }
    // A match end needs a real phase before it, and the player's side to
    // know which score is theirs. Without the side (spectating) there is no
    // `MatchEnd`, and the recap falls back to the round results.
    let was_playing = prev.map_phase.as_deref().is_some_and(|p| p != "gameover");
    if next.map_phase.as_deref() == Some("gameover") && was_playing {
        let (ct, t) = (next.score_ct.unwrap_or(0), next.score_t.unwrap_or(0));
        match next.team.as_deref() {
            Some("CT") => out.push(EventKind::MatchEnd { won: ct, lost: t }),
            Some("T") => out.push(EventKind::MatchEnd { won: t, lost: ct }),
            _ => {}
        }
    }
    out
}

/// A death this soon after the round goes live counts as an early death.
pub const EARLY_DEATH_MS: u64 = 20_000;

/// A pointer and the event time it fired at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    pub at_ms: u64,
    pub text: String,
}

const LOSS_STREAK: &str =
    "Three rounds lost in a row. A save round or a timeout might reset things.";

/// Every pointer the timeline has fired so far, oldest first. A rule fires
/// once when its condition turns true and re-arms only after it turns false
/// again, so a watcher that reruns this per event shows only pointers newer
/// than the last one it showed, and the recap lists each one once.
pub fn pointers(timeline: &[WatchEvent]) -> Vec<Pointer> {
    let mut out = Vec::new();
    let mut losses = 0;
    // Per round that went live: did the player die early in it.
    let mut early: Vec<bool> = Vec::new();
    let mut live_at = None;
    let mut early_on = false;
    for e in timeline {
        match e.kind {
            EventKind::RoundEnd { won, .. } => {
                losses = if won { 0 } else { losses + 1 };
                if losses == 3 {
                    out.push(Pointer {
                        at_ms: e.at_ms,
                        text: LOSS_STREAK.into(),
                    });
                }
            }
            EventKind::RoundStart { .. } => {
                live_at = Some(e.at_ms);
                early.push(false);
            }
            EventKind::Death => {
                if let (Some(start), Some(last)) = (live_at, early.last_mut()) {
                    *last |= e.at_ms.saturating_sub(start) <= EARLY_DEATH_MS;
                }
            }
            _ => {}
        }
        // Early deaths across the last five rounds that went live.
        let recent = &early[early.len().saturating_sub(5)..];
        let n = recent.iter().filter(|e| **e).count();
        if n >= 3 && !early_on {
            out.push(Pointer {
                at_ms: e.at_ms,
                text: format!(
                    "You died in the first 20 seconds in {n} of the last {} rounds. Try a slower opening.",
                    recent.len()
                ),
            });
        }
        early_on = n >= 3;
    }
    out
}

/// What the post-match recap card shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recap {
    pub won: u32,
    pub lost: u32,
    pub kills: u32,
    pub deaths: u32,
    pub longest_win_streak: u32,
}

/// Sum a finished timeline into the recap. The score comes from `MatchEnd`
/// when present, else from the round results seen.
pub fn recap(timeline: &[WatchEvent]) -> Recap {
    let mut r = Recap::default();
    let mut streak = 0;
    let mut final_score = None;
    for e in timeline {
        match e.kind {
            EventKind::Kill => r.kills += 1,
            EventKind::Death => r.deaths += 1,
            EventKind::RoundEnd { won, .. } => {
                if won {
                    r.won += 1;
                    streak += 1;
                    r.longest_win_streak = r.longest_win_streak.max(streak);
                } else {
                    r.lost += 1;
                    streak = 0;
                }
            }
            EventKind::MatchEnd { won, lost } => final_score = Some((won, lost)),
            _ => {}
        }
    }
    if let Some((won, lost)) = final_score {
        r.won = won;
        r.lost = lost;
    }
    r
}

/// What one event brings the cabin: a new pointer to show, or the match's
/// end with its recap and every pointer it fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchNews {
    Pointer(String),
    End { recap: Recap, pointers: Vec<String> },
}

/// One match as the cabin follows it, game-agnostic: the timeline so far,
/// how many of its pointers went out, and whether its recap did. A
/// `MatchEnd` closes the match once; a repeat of it adds nothing, and the
/// next other event starts a new match.
#[derive(Debug, Default)]
pub struct MatchTracker {
    timeline: Vec<WatchEvent>,
    shown: usize,
    ended: bool,
}

impl MatchTracker {
    pub fn take(&mut self, event: WatchEvent) -> Vec<MatchNews> {
        let end = matches!(event.kind, EventKind::MatchEnd { .. });
        if self.ended {
            if end {
                return Vec::new();
            }
            *self = MatchTracker::default();
        }
        self.timeline.push(event);
        let fired = pointers(&self.timeline);
        let mut out: Vec<MatchNews> = fired[self.shown.min(fired.len())..]
            .iter()
            .map(|p| MatchNews::Pointer(p.text.clone()))
            .collect();
        self.shown = fired.len();
        if end {
            self.ended = true;
            let mut texts: Vec<String> = Vec::new();
            for p in fired {
                if !texts.contains(&p.text) {
                    texts.push(p.text);
                }
            }
            out.push(MatchNews::End {
                recap: recap(&self.timeline),
                pointers: texts,
            });
        }
        out
    }
}

/// Pointer toasts come at most once per this long. One that comes sooner is
/// not queued; the recap card still lists it.
pub const POINTER_TOAST_GAP_MS: u64 = 60_000;

/// Rate limit for pointer toasts.
#[derive(Debug, Default)]
pub struct ToastGate {
    last_ms: Option<u64>,
}

impl ToastGate {
    /// True when a toast may show at `now_ms`, and counts it as shown.
    pub fn allow(&mut self, now_ms: u64) -> bool {
        if self
            .last_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < POINTER_TOAST_GAP_MS)
        {
            return false;
        }
        self.last_ms = Some(now_ms);
        true
    }
}

/// The recap as a Pulse card: the result in the title, the numbers and the
/// match's pointers in the body. A score shows only for games played in
/// rounds; a one-game result (Dota 2's 1-0) is just won or lost.
pub fn recap_card(game: &str, recap: &Recap, pointers: &[String], at_ms: u64) -> UpdateCard {
    let result = match recap.won.cmp(&recap.lost) {
        std::cmp::Ordering::Greater => "won",
        std::cmp::Ordering::Less => "lost",
        std::cmp::Ordering::Equal => "tied",
    };
    let title = if recap.won + recap.lost > 1 {
        format!("{game} match: {result} {}-{}", recap.won, recap.lost)
    } else {
        format!("{game} match: {result}")
    };
    let mut body = format!("{} kills, {} deaths.", recap.kills, recap.deaths);
    if recap.longest_win_streak > 1 {
        body.push_str(&format!(
            " Longest win streak: {} rounds.",
            recap.longest_win_streak
        ));
    }
    if !pointers.is_empty() {
        body.push_str(" Pointers this match: ");
        body.push_str(&pointers.join(" "));
    }
    let slug: String = game
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    digest_card(&format!("source:game-watch:{slug}"), &title, &body, at_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(round: u32, phase: &str, win: &str, kills: u32, deaths: u32) -> String {
        format!(
            r#"{{"map":{{"phase":"live","round":{round},"team_ct":{{"score":3}},"team_t":{{"score":5}}}},
               "round":{{"phase":"{phase}","win_team":"{win}"}},
               "player":{{"team":"CT","match_stats":{{"kills":{kills},"deaths":{deaths}}}}}}}"#
        )
    }

    #[test]
    fn a_gsi_payload_parses_the_fields_the_watcher_reads() {
        let s = parse_cs2(&payload(7, "live", "", 4, 2)).unwrap();
        assert_eq!(s.round, Some(7));
        assert_eq!(s.round_phase.as_deref(), Some("live"));
        assert_eq!(s.team.as_deref(), Some("CT"));
        assert_eq!((s.score_ct, s.score_t), (Some(3), Some(5)));
        assert_eq!((s.kills, s.deaths), (Some(4), Some(2)));
        assert_eq!(parse_cs2("not json"), None);
        assert_eq!(parse_cs2("{}"), Some(Cs2State::default()));
    }

    #[test]
    fn changes_between_payloads_become_events() {
        let freeze = parse_cs2(&payload(7, "freezetime", "", 4, 2)).unwrap();
        let live = parse_cs2(&payload(7, "live", "", 4, 2)).unwrap();
        let fight = parse_cs2(&payload(7, "live", "", 6, 3)).unwrap();
        let over = parse_cs2(&payload(7, "over", "T", 6, 3)).unwrap();
        assert_eq!(
            cs2_events(Some(&freeze), &live),
            vec![EventKind::RoundStart { round: 7 }]
        );
        assert_eq!(
            cs2_events(Some(&live), &fight),
            vec![EventKind::Kill, EventKind::Kill, EventKind::Death]
        );
        assert_eq!(
            cs2_events(Some(&fight), &over),
            vec![EventKind::RoundEnd {
                round: 7,
                won: false
            }]
        );
        // A repeat snapshot adds nothing.
        assert_eq!(cs2_events(Some(&over), &over), vec![]);
        let mut end = over.clone();
        end.map_phase = Some("gameover".into());
        assert_eq!(
            cs2_events(Some(&over), &end),
            vec![EventKind::MatchEnd { won: 3, lost: 5 }]
        );
    }

    #[test]
    fn the_first_payload_is_a_baseline_and_fires_nothing() {
        let mut end = parse_cs2(&payload(30, "over", "CT", 20, 15)).unwrap();
        end.map_phase = Some("gameover".into());
        assert_eq!(cs2_events(None, &end), vec![]);
        // A payload with no map phase before it (menu, loading) is not a
        // match the player was seen in, so game over there is no match end.
        let no_map = Cs2State {
            round_phase: Some("over".into()),
            ..Cs2State::default()
        };
        assert_eq!(cs2_events(Some(&no_map), &end), vec![]);
    }

    #[test]
    fn a_match_end_needs_the_players_side() {
        let over = parse_cs2(&payload(30, "over", "T", 20, 15)).unwrap();
        let mut end = over.clone();
        end.map_phase = Some("gameover".into());
        end.team = Some("T".into());
        assert_eq!(
            cs2_events(Some(&over), &end),
            vec![EventKind::MatchEnd { won: 5, lost: 3 }]
        );
        end.team = None;
        assert_eq!(cs2_events(Some(&over), &end), vec![]);
        end.team = Some("SPECTATOR".into());
        assert_eq!(cs2_events(Some(&over), &end), vec![]);
    }

    fn ev(at_ms: u64, kind: EventKind) -> WatchEvent {
        WatchEvent { at_ms, kind }
    }

    fn lost(at_ms: u64, round: u32) -> WatchEvent {
        ev(at_ms, EventKind::RoundEnd { round, won: false })
    }

    fn texts(p: &[Pointer]) -> Vec<(u64, &str)> {
        p.iter().map(|p| (p.at_ms, p.text.as_str())).collect()
    }

    #[test]
    fn three_lost_rounds_in_a_row_fire_one_pointer() {
        let mut t = vec![
            ev(
                1,
                EventKind::RoundEnd {
                    round: 1,
                    won: true,
                },
            ),
            lost(2, 2),
            lost(3, 3),
        ];
        assert_eq!(pointers(&t), vec![]);
        t.push(lost(4, 4));
        assert_eq!(texts(&pointers(&t)), vec![(4, LOSS_STREAK)]);
        // A fourth loss keeps the condition true but fires nothing new.
        t.push(lost(5, 5));
        assert_eq!(texts(&pointers(&t)), vec![(4, LOSS_STREAK)]);
        // A win re-arms it, and three more losses fire it again.
        t.push(ev(
            6,
            EventKind::RoundEnd {
                round: 6,
                won: true,
            },
        ));
        t.extend([lost(7, 7), lost(8, 8), lost(9, 9)]);
        assert_eq!(
            texts(&pointers(&t)),
            vec![(4, LOSS_STREAK), (9, LOSS_STREAK)]
        );
    }

    #[test]
    fn early_deaths_in_three_of_five_rounds_fire_one_pointer() {
        let early =
            "You died in the first 20 seconds in 3 of the last 5 rounds. Try a slower opening.";
        let mut t = Vec::new();
        // Rounds start every 100 s; deaths at +10 s (early) or +60 s (late).
        for (i, after) in [10_000u64, 60_000, 10_000, 60_000, 10_000]
            .iter()
            .enumerate()
        {
            let start = i as u64 * 100_000;
            t.push(ev(
                start,
                EventKind::RoundStart {
                    round: i as u32 + 1,
                },
            ));
            t.push(ev(start + after, EventKind::Death));
        }
        assert_eq!(texts(&pointers(&t)), vec![(410_000, early)]);
        // A sixth round still holds three early deaths in the last five
        // (rounds 2-6) and fires nothing new.
        t.push(ev(500_000, EventKind::RoundStart { round: 6 }));
        t.push(ev(560_000, EventKind::Death));
        assert_eq!(texts(&pointers(&t)), vec![(410_000, early)]);
        // Two early deaths out of five stays quiet.
        t.truncate(10);
        t[9].at_ms = 4 * 100_000 + 60_000;
        assert_eq!(pointers(&t), vec![]);
    }

    #[test]
    fn the_recap_sums_the_timeline_and_prefers_the_final_score() {
        let t = vec![
            ev(1, EventKind::RoundStart { round: 1 }),
            ev(2, EventKind::Kill),
            ev(
                3,
                EventKind::RoundEnd {
                    round: 1,
                    won: true,
                },
            ),
            ev(4, EventKind::RoundStart { round: 2 }),
            ev(5, EventKind::Kill),
            ev(
                6,
                EventKind::RoundEnd {
                    round: 2,
                    won: true,
                },
            ),
            ev(7, EventKind::RoundStart { round: 3 }),
            ev(8, EventKind::Death),
            ev(
                9,
                EventKind::RoundEnd {
                    round: 3,
                    won: false,
                },
            ),
        ];
        assert_eq!(
            recap(&t),
            Recap {
                won: 2,
                lost: 1,
                kills: 2,
                deaths: 1,
                longest_win_streak: 2
            }
        );
        let mut ended = t.clone();
        ended.push(ev(10, EventKind::MatchEnd { won: 13, lost: 9 }));
        let r = recap(&ended);
        assert_eq!((r.won, r.lost), (13, 9));
    }

    fn won(at_ms: u64, round: u32) -> WatchEvent {
        ev(at_ms, EventKind::RoundEnd { round, won: true })
    }

    #[test]
    fn the_tracker_hands_out_each_pointer_once_and_one_end_per_match() {
        let mut m = MatchTracker::default();
        assert_eq!(m.take(won(1, 1)), vec![]);
        assert_eq!(m.take(lost(2, 2)), vec![]);
        assert_eq!(m.take(lost(3, 3)), vec![]);
        assert_eq!(
            m.take(lost(4, 4)),
            vec![MatchNews::Pointer(LOSS_STREAK.into())]
        );
        assert_eq!(m.take(lost(5, 5)), vec![]);
        assert_eq!(m.take(ev(6, EventKind::Kill)), vec![]);
        assert_eq!(
            m.take(ev(7, EventKind::MatchEnd { won: 1, lost: 4 })),
            vec![MatchNews::End {
                recap: Recap {
                    won: 1,
                    lost: 4,
                    kills: 1,
                    deaths: 0,
                    longest_win_streak: 1
                },
                pointers: vec![LOSS_STREAK.into()],
            }]
        );
        // The game says game over again: no second end.
        assert_eq!(
            m.take(ev(8, EventKind::MatchEnd { won: 1, lost: 4 })),
            vec![]
        );
        // The next match starts clean.
        assert_eq!(m.take(ev(9, EventKind::Death)), vec![]);
        assert_eq!(
            m.take(ev(10, EventKind::MatchEnd { won: 0, lost: 1 })),
            vec![MatchNews::End {
                recap: Recap {
                    won: 0,
                    lost: 1,
                    kills: 0,
                    deaths: 1,
                    longest_win_streak: 0
                },
                pointers: vec![],
            }]
        );
    }

    #[test]
    fn a_pointer_that_fires_twice_is_listed_once_at_the_end() {
        let mut m = MatchTracker::default();
        let mut news = Vec::new();
        for (i, e) in [
            lost(1, 1),
            lost(2, 2),
            lost(3, 3),
            won(4, 4),
            lost(5, 5),
            lost(6, 6),
            lost(7, 7),
            ev(8, EventKind::MatchEnd { won: 1, lost: 6 }),
        ]
        .into_iter()
        .enumerate()
        {
            news.extend(m.take(e).into_iter().map(|n| (i, n)));
        }
        assert_eq!(
            news,
            vec![
                (2, MatchNews::Pointer(LOSS_STREAK.into())),
                (6, MatchNews::Pointer(LOSS_STREAK.into())),
                (
                    7,
                    MatchNews::End {
                        recap: Recap {
                            won: 1,
                            lost: 6,
                            kills: 0,
                            deaths: 0,
                            longest_win_streak: 1
                        },
                        pointers: vec![LOSS_STREAK.into()],
                    }
                ),
            ]
        );
    }

    #[test]
    fn pointer_toasts_wait_a_minute_between_each() {
        let mut gate = ToastGate::default();
        assert!(gate.allow(1_000));
        assert!(!gate.allow(1_000));
        assert!(!gate.allow(60_999));
        assert!(gate.allow(61_000));
        assert!(!gate.allow(100_000));
        assert!(gate.allow(200_000));
    }

    #[test]
    fn the_recap_card_names_the_game_and_the_result() {
        let r = Recap {
            won: 13,
            lost: 9,
            kills: 21,
            deaths: 15,
            longest_win_streak: 4,
        };
        let card = recap_card("Counter-Strike 2", &r, &[LOSS_STREAK.into()], 5_000);
        assert_eq!(card.title, "Counter-Strike 2 match: won 13-9");
        assert_eq!(
            card.body.as_deref(),
            Some("21 kills, 15 deaths. Longest win streak: 4 rounds. Pointers this match: Three rounds lost in a row. A save round or a timeout might reset things.")
        );
        assert_eq!(card.source_id, "source:game-watch:counter-strike-2");
        assert_eq!(card.created_at, 5_000);
        assert!(crate::update_feed::names_its_item(&card));

        let dota = Recap {
            won: 0,
            lost: 1,
            kills: 3,
            deaths: 7,
            longest_win_streak: 0,
        };
        let card = recap_card("Dota 2", &dota, &[], 9_000);
        assert_eq!(card.title, "Dota 2 match: lost");
        assert_eq!(card.body.as_deref(), Some("3 kills, 7 deaths."));
        assert_eq!(card.source_id, "source:game-watch:dota-2");
        let tie = Recap {
            won: 12,
            lost: 12,
            ..Recap::default()
        };
        assert_eq!(
            recap_card("Counter-Strike 2", &tie, &[], 1).title,
            "Counter-Strike 2 match: tied 12-12"
        );
    }
}
