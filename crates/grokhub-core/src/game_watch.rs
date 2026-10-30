//! In-game watcher proof of concept (design: `docs/design/game-watcher/`).
//! Reads Counter-Strike 2 Game State Integration payloads, turns the change
//! between two payloads into match events, and runs plain rules over the
//! timeline for pointers and an end-of-match recap. No model is called and
//! nothing leaves the machine; the local listener and UI are follow-ups.

use serde_json::Value;

/// One thing that happened in a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    RoundStart { round: u32 },
    RoundEnd { round: u32, won: bool },
    Kill,
    Death,
    MatchEnd { won: u32, lost: u32 },
}

/// An event and when it was seen, in ms since the epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// a stats counter going up, the map phase turning `gameover`.
pub fn cs2_events(prev: Option<&Cs2State>, next: &Cs2State) -> Vec<EventKind> {
    let blank = Cs2State::default();
    let prev = prev.unwrap_or(&blank);
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
    if next.map_phase.as_deref() == Some("gameover")
        && prev.map_phase.as_deref() != Some("gameover")
    {
        let (ct, t) = (next.score_ct.unwrap_or(0), next.score_t.unwrap_or(0));
        let (won, lost) = if next.team.as_deref() == Some("CT") {
            (ct, t)
        } else {
            (t, ct)
        };
        out.push(EventKind::MatchEnd { won, lost });
    }
    out
}

/// A death this soon after the round goes live counts as an early death.
pub const EARLY_DEATH_MS: u64 = 20_000;

/// Rule-based pointers for the timeline so far, newest rule state only.
pub fn pointers(timeline: &[WatchEvent]) -> Vec<String> {
    let mut out = Vec::new();
    let results: Vec<bool> = timeline
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::RoundEnd { won, .. } => Some(won),
            _ => None,
        })
        .collect();
    if results.len() >= 3 && results[results.len() - 3..].iter().all(|w| !w) {
        out.push(
            "Three rounds lost in a row. A save round or a timeout might reset things.".into(),
        );
    }
    // Early deaths across the last five rounds that went live.
    let mut early = Vec::new();
    let mut live_at = None;
    for e in timeline {
        match e.kind {
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
    }
    let recent = &early[early.len().saturating_sub(5)..];
    let n = recent.iter().filter(|e| **e).count();
    if n >= 3 {
        out.push(format!(
            "You died in the first 20 seconds in {n} of the last {} rounds. Try a slower opening.",
            recent.len()
        ));
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
            EventKind::RoundStart { .. } => {}
        }
    }
    if let Some((won, lost)) = final_score {
        r.won = won;
        r.lost = lost;
    }
    r
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

    fn ev(at_ms: u64, kind: EventKind) -> WatchEvent {
        WatchEvent { at_ms, kind }
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
            ev(
                2,
                EventKind::RoundEnd {
                    round: 2,
                    won: false,
                },
            ),
            ev(
                3,
                EventKind::RoundEnd {
                    round: 3,
                    won: false,
                },
            ),
        ];
        assert_eq!(pointers(&t), Vec::<String>::new());
        t.push(ev(
            4,
            EventKind::RoundEnd {
                round: 4,
                won: false,
            },
        ));
        assert_eq!(
            pointers(&t),
            vec![
                "Three rounds lost in a row. A save round or a timeout might reset things."
                    .to_string()
            ]
        );
        t.push(ev(
            5,
            EventKind::RoundEnd {
                round: 5,
                won: true,
            },
        ));
        assert_eq!(pointers(&t), Vec::<String>::new());
    }

    #[test]
    fn early_deaths_in_three_of_five_rounds_fire_one_pointer() {
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
        assert_eq!(
            pointers(&t),
            vec![
                "You died in the first 20 seconds in 3 of the last 5 rounds. Try a slower opening."
                    .to_string()
            ]
        );
        // Two early deaths out of five stays quiet.
        t[9].at_ms = 4 * 100_000 + 60_000;
        assert_eq!(pointers(&t), Vec::<String>::new());
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
}
