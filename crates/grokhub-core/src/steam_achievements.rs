//! Achievement nudges for Pulse. Opt-in. Once a week the cabin reads the
//! user's achievements for the games they play most (the public Steam
//! community stats page, `?xml=1`, no key) and the global unlock rates
//! (`GetGlobalAchievementPercentagesForApp`, no key), then posts a Pulse card
//! when a game is a few achievements from 100% or the next one is rare. Each
//! card links Steam's free community guides. The SteamID is sealed on disk by
//! the caller and never kept here or put in a card; this file holds only the
//! prefs and what each game was last nudged at.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "pulse-steam-achievements.json";
pub const SOURCE_NAME: &str = "Steam achievements";
/// One check a week.
pub const CHECK_EVERY_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Games read per check, most recently played first.
pub const GAMES_PER_CHECK: usize = 8;
/// Nudges one check posts before the feedback score moves it.
pub const NUDGES_PER_CHECK: i32 = 2;
/// Most nudges one check can post however much the user likes them.
pub const NUDGES_MAX: i32 = 4;
/// "Close to complete": at most this many left.
pub const CLOSE_LEFT: usize = 3;
/// Games with fewer achievements than this are never "close"; one of two
/// left isn't a nudge.
pub const MIN_TOTAL: usize = 5;
/// An achievement fewer than this share of players have is rare.
pub const RARE_PERCENT: f32 = 10.0;
/// A rare-next nudge needs the user this far through the list.
pub const RARE_DONE_PERCENT: usize = 75;

/// One achievement on the user's stats page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Achievement {
    /// Steam's internal name; the global rates are keyed by it.
    pub api: String,
    pub name: String,
    pub unlocked: bool,
}

/// What the stats page said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Player {
    /// The profile or its game details are private.
    Private,
    /// The game has no achievements.
    NoStats,
    List(Vec<Achievement>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A few left to 100%.
    Close,
    /// Well through the list and the next one is rare.
    Rare,
}

/// A locked achievement and the share of players who have it, when known.
#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    pub name: String,
    pub percent: Option<f32>,
}

/// A game worth a Pulse card.
#[derive(Clone, Debug, PartialEq)]
pub struct Nudge {
    pub app_id: u32,
    pub game: String,
    pub kind: Kind,
    pub done: usize,
    pub total: usize,
    /// For [`Kind::Close`] every locked one, rarest last; for [`Kind::Rare`]
    /// the rarest one alone.
    pub left: Vec<Left>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tracker {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub last_check_ms: u64,
    /// The last check found the profile's game details private.
    #[serde(default)]
    pub private: bool,
    /// Unlocked count each game was last nudged at, so a card posts once
    /// until the user unlocks another.
    #[serde(default)]
    pub nudged: BTreeMap<u32, usize>,
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

pub fn load(config_dir: &Path) -> Tracker {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, tracker: &Tracker) -> Result<(), String> {
    let text = serde_json::to_string_pretty(tracker).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path(config_dir), &text)
}

pub fn player_url(steam_id: u64, app_id: u32) -> String {
    format!("https://steamcommunity.com/profiles/{steam_id}/stats/{app_id}/achievements/?xml=1")
}

pub fn global_url(app_id: u32) -> String {
    format!("https://api.steampowered.com/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/?gameid={app_id}")
}

/// Steam's free community guides for the game.
pub fn guides_url(app_id: u32) -> String {
    format!("https://steamcommunity.com/app/{app_id}/guides/")
}

pub fn capsule_url(app_id: u32) -> String {
    format!("https://cdn.akamai.steamstatic.com/steam/apps/{app_id}/header.jpg")
}

/// How many nudges one check may post: the base, moved by the feedback score
/// the user gave this source's posts.
pub fn nudges_allowed(source_bias: i32) -> usize {
    usize::try_from((NUDGES_PER_CHECK + source_bias).clamp(0, NUDGES_MAX)).unwrap_or(0)
}

/// The stats page (`?xml=1`). `None` for a body that isn't one.
pub fn parse_player(body: &str) -> Option<Player> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(body);
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut list = Vec::new();
    let mut cur: Option<Achievement> = None;
    let mut root = None;
    let mut error = String::new();
    let mut privacy = String::new();
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if root.is_none() {
                    root = Some(name.clone());
                }
                if name == "achievement" {
                    let closed = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == b"closed")
                        .is_some_and(|a| a.value.as_ref() == b"1");
                    cur = Some(Achievement {
                        api: String::new(),
                        name: String::new(),
                        unlocked: closed,
                    });
                }
                stack.push(name);
                text.clear();
            }
            Event::Text(t) => {
                if let Ok(s) = t.decode() {
                    text.push_str(&s);
                }
            }
            Event::CData(c) => {
                if let Ok(s) = c.decode() {
                    text.push_str(&s);
                }
            }
            Event::GeneralRef(r) => {
                if let Ok(name) = r.decode() {
                    text.push_str(match name.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(_) => {
                let name = stack.pop().unwrap_or_default();
                let value = text.trim().to_string();
                text.clear();
                match (name.as_str(), cur.as_mut()) {
                    ("achievement", Some(_)) => {
                        if let Some(a) = cur.take().filter(|a| !a.api.is_empty()) {
                            list.push(a);
                        }
                    }
                    ("apiname", Some(a)) => a.api = value,
                    ("name", Some(a)) => a.name = value,
                    ("error", None) => error = value,
                    ("privacyState", None) => privacy = value,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    match root.as_deref() {
        Some("playerstats") if privacy.eq_ignore_ascii_case("private") => Some(Player::Private),
        Some("playerstats") if list.is_empty() => Some(Player::NoStats),
        Some("playerstats") => Some(Player::List(list)),
        Some("response") if error.to_ascii_lowercase().contains("private") => Some(Player::Private),
        Some("response") if !error.is_empty() => Some(Player::NoStats),
        _ => None,
    }
}

/// Share of players with each achievement, by api name.
pub fn parse_global(body: &str) -> BTreeMap<String, f32> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return BTreeMap::new();
    };
    v.pointer("/achievementpercentages/achievements")
        .and_then(|a| a.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    let name = it.get("name")?.as_str()?.to_string();
                    // Steam sends the rate as a number or, lately, a string.
                    let pct = match it.get("percent")? {
                        serde_json::Value::String(s) => s.trim().parse().ok()?,
                        n => n.as_f64()? as f32,
                    };
                    Some((name, pct))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a game's list is worth reading the global rates for.
pub fn worth_rates(list: &[Achievement]) -> bool {
    let done = list.iter().filter(|a| a.unlocked).count();
    list.len() >= MIN_TOTAL && done * 100 >= list.len() * 50
}

/// The nudge for one game, if any: a few left to 100% first, else a rare
/// next one once the user is well through the list.
pub fn pick(
    app_id: u32,
    game: &str,
    list: &[Achievement],
    rates: &BTreeMap<String, f32>,
) -> Option<Nudge> {
    let total = list.len();
    let done = list.iter().filter(|a| a.unlocked).count();
    if total < MIN_TOTAL || done == total {
        return None;
    }
    let mut left: Vec<Left> = list
        .iter()
        .filter(|a| !a.unlocked)
        .map(|a| Left {
            name: if a.name.is_empty() {
                a.api.clone()
            } else {
                a.name.clone()
            },
            percent: rates.get(&a.api).copied(),
        })
        .collect();
    // Commonest first, unknown rates last: the easy wins lead.
    left.sort_by(|a, b| {
        let key = |l: &Left| l.percent.map_or(-1.0, |p| p);
        key(b).total_cmp(&key(a))
    });
    let nudge = |kind, left| Nudge {
        app_id,
        game: game.to_string(),
        kind,
        done,
        total,
        left,
    };
    if left.len() <= CLOSE_LEFT {
        return Some(nudge(Kind::Close, left));
    }
    if done * 100 < total * RARE_DONE_PERCENT {
        return None;
    }
    let rare = left
        .into_iter()
        .filter(|l| l.percent.is_some_and(|p| p < RARE_PERCENT))
        .min_by(|a, b| {
            a.percent
                .unwrap_or(0.0)
                .total_cmp(&b.percent.unwrap_or(0.0))
        })?;
    Some(nudge(Kind::Rare, vec![rare]))
}

fn percent_text(p: f32) -> String {
    if p < 1.0 {
        format!("{p:.1}%")
    } else {
        format!("{}%", p.round() as u32)
    }
}

impl Nudge {
    pub fn title(&self) -> String {
        match self.kind {
            Kind::Close => format!("{} away from 100% in {}", self.left.len(), self.game),
            Kind::Rare => {
                let rare = &self.left[0];
                match rare.percent {
                    Some(p) => format!(
                        "{} in {}: only {} of players have it",
                        rare.name,
                        self.game,
                        percent_text(p)
                    ),
                    None => format!("{} is next in {}", rare.name, self.game),
                }
            }
        }
    }

    pub fn body(&self) -> String {
        let names: Vec<String> = self
            .left
            .iter()
            .map(|l| match l.percent {
                Some(p) => format!("{} ({} of players)", l.name, percent_text(p)),
                None => l.name.clone(),
            })
            .collect();
        match self.kind {
            Kind::Close => format!(
                "Left: {}. Steam's community guides are free.",
                names.join(", ")
            ),
            Kind::Rare => format!(
                "You have {} of {}. Steam's community guides are free.",
                self.done, self.total
            ),
        }
    }

    /// The Feed card: Save / Not interested act on it like any source post.
    /// Links the guides, never the user's profile.
    pub fn card(&self, now_ms: u64) -> crate::UpdateCard {
        let source_id = format!("source:steam-achievements:{}:{}", self.app_id, self.done);
        let mut card =
            crate::update_feed::digest_card(&source_id, &self.title(), &self.body(), now_ms);
        card.citations = vec![guides_url(self.app_id)];
        card.pulse.source_name = Some(SOURCE_NAME.to_string());
        card.pulse.image_urls = vec![capsule_url(self.app_id)];
        card.pulse.because = Some(format!("{} of {} achievements", self.done, self.total));
        card
    }
}

impl Tracker {
    pub fn due(&self, now_ms: u64) -> bool {
        self.on && now_ms.saturating_sub(self.last_check_ms) >= CHECK_EVERY_MS
    }

    /// Fold one check's picks in and return the ones to post, closest to
    /// 100% first, at most `allowed`. A game posts again only after the user
    /// unlocks another achievement in it.
    pub fn check(
        &mut self,
        picks: Vec<Nudge>,
        private: bool,
        allowed: usize,
        now_ms: u64,
    ) -> Vec<Nudge> {
        self.last_check_ms = now_ms;
        self.private = private;
        if private {
            return Vec::new();
        }
        let mut out: Vec<Nudge> = picks
            .into_iter()
            .filter(|n| self.nudged.get(&n.app_id) != Some(&n.done))
            .collect();
        out.sort_by(|a, b| {
            let rank = |n: &Nudge| (n.kind != Kind::Close, n.total - n.done);
            rank(a).cmp(&rank(b)).then(a.app_id.cmp(&b.app_id))
        });
        out.truncate(allowed);
        for n in &out {
            self.nudged.insert(n.app_id, n.done);
        }
        out
    }

    /// Off: forget everything.
    pub fn turn_off(&mut self) {
        *self = Tracker::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(rows: &[(&str, &str, bool)]) -> String {
        let mut body = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<playerstats><privacyState>public</privacyState><game><gameName>Fake Quest</gameName></game><achievements>",
        );
        for (api, name, done) in rows {
            body.push_str(&format!(
                "<achievement closed=\"{}\"><iconClosed>x</iconClosed><name><![CDATA[{name}]]></name><apiname><![CDATA[{api}]]></apiname><description><![CDATA[d]]></description></achievement>",
                u8::from(*done)
            ));
        }
        body.push_str("</achievements></playerstats>");
        body
    }

    fn list(done: usize, total: usize) -> Vec<Achievement> {
        (0..total)
            .map(|i| Achievement {
                api: format!("ACH_{i}"),
                name: format!("Feat {i}"),
                unlocked: i < done,
            })
            .collect()
    }

    fn rates(pairs: &[(&str, f32)]) -> BTreeMap<String, f32> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn the_stats_page_reads_names_and_unlocks_or_says_private() {
        let body = stats(&[("WIN", "Win & Grin", true), ("LOSE", "Lose", false)]);
        assert_eq!(
            parse_player(&body),
            Some(Player::List(vec![
                Achievement {
                    api: "WIN".into(),
                    name: "Win & Grin".into(),
                    unlocked: true
                },
                Achievement {
                    api: "LOSE".into(),
                    name: "Lose".into(),
                    unlocked: false
                },
            ]))
        );
        assert_eq!(
            parse_player(
                "<response><error><![CDATA[This profile is private.]]></error></response>"
            ),
            Some(Player::Private)
        );
        assert_eq!(
            parse_player("<playerstats><privacyState>private</privacyState></playerstats>"),
            Some(Player::Private)
        );
        assert_eq!(
            parse_player("<response><error>The specified game has no stats.</error></response>"),
            Some(Player::NoStats)
        );
        assert_eq!(parse_player("<html><body>Sign in</body></html>"), None);
        assert_eq!(parse_player("not xml at all"), None);
    }

    #[test]
    fn global_rates_read_numbers_and_strings() {
        let got = parse_global(
            r#"{"achievementpercentages":{"achievements":[{"name":"A","percent":"12.5"},{"name":"B","percent":3.25},{"name":"C"}]}}"#,
        );
        assert_eq!(got, rates(&[("A", 12.5), ("B", 3.25)]));
        assert!(parse_global("oops").is_empty());
    }

    #[test]
    fn two_left_is_a_close_nudge_with_the_easy_one_first() {
        let r = rates(&[("ACH_8", 2.0), ("ACH_9", 40.0)]);
        let n = pick(1145360, "Hades", &list(8, 10), &r).unwrap();
        assert_eq!(n.kind, Kind::Close);
        assert_eq!(n.title(), "2 away from 100% in Hades");
        assert_eq!(
            n.body(),
            "Left: Feat 9 (40% of players), Feat 8 (2% of players). Steam's community guides are free."
        );
        let card = n.card(7);
        assert_eq!(
            card.citations,
            vec!["https://steamcommunity.com/app/1145360/guides/"]
        );
        assert_eq!(card.pulse.source_name.as_deref(), Some(SOURCE_NAME));
        assert_eq!(card.pulse.because.as_deref(), Some("8 of 10 achievements"));
    }

    #[test]
    fn a_rare_next_one_needs_the_user_well_through_the_list() {
        let r = rates(&[
            ("ACH_16", 30.0),
            ("ACH_17", 0.4),
            ("ACH_18", 8.0),
            ("ACH_19", 20.0),
        ]);
        let n = pick(10, "Fake Quest", &list(16, 20), &r).unwrap();
        assert_eq!(n.kind, Kind::Rare);
        assert_eq!(
            n.title(),
            "Feat 17 in Fake Quest: only 0.4% of players have it"
        );
        assert_eq!(
            n.body(),
            "You have 16 of 20. Steam's community guides are free."
        );
        // Half done with the same rare one left: no nudge.
        assert_eq!(pick(10, "Fake Quest", &list(10, 20), &r), None);
        // Well through, but nothing left is rare: no nudge.
        let common = rates(&[("ACH_16", 30.0), ("ACH_17", 40.0)]);
        assert_eq!(pick(10, "Fake Quest", &list(16, 20), &common), None);
    }

    #[test]
    fn small_lists_and_finished_games_never_nudge() {
        assert_eq!(pick(1, "Tiny", &list(2, 4), &BTreeMap::new()), None);
        assert_eq!(pick(1, "Done", &list(10, 10), &BTreeMap::new()), None);
        assert!(!worth_rates(&list(2, 10)));
        assert!(worth_rates(&list(5, 10)));
    }

    #[test]
    fn a_check_posts_once_per_unlock_and_nothing_when_private() {
        let close = |id, done| pick(id, "G", &list(done, 10), &BTreeMap::new()).unwrap();
        let mut t = Tracker {
            on: true,
            ..Default::default()
        };
        let first = t.check(vec![close(1, 8), close(2, 9), close(3, 7)], false, 2, 100);
        let ids: Vec<u32> = first.iter().map(|n| n.app_id).collect();
        assert_eq!(ids, vec![2, 1], "fewest left first, capped at two");
        assert_eq!(t.last_check_ms, 100);
        let again = t.check(vec![close(1, 8), close(2, 9)], false, 2, 200);
        assert!(again.is_empty(), "same unlock count posts once");
        let moved = t.check(vec![close(1, 9)], false, 2, 300);
        assert_eq!(moved.len(), 1);
        let hidden = t.check(vec![close(3, 7)], true, 2, 400);
        assert!(hidden.is_empty());
        assert!(t.private);
        assert!(!t.due(400));
        assert!(t.due(400 + CHECK_EVERY_MS));
        t.turn_off();
        assert_eq!(t, Tracker::default());
    }

    #[test]
    fn the_feedback_score_moves_how_many_post() {
        assert_eq!(nudges_allowed(0), 2);
        assert_eq!(nudges_allowed(-5), 0);
        assert_eq!(nudges_allowed(9), 4);
    }
}
