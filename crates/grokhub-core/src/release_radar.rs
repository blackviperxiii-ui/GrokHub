//! Release radar: upcoming DLC and sequels for the games the user owns or
//! wishlisted. Opt-in. Once a day the cabin asks Steam's public store API
//! about the most-played games: each one's DLC list, and a store search for
//! the series name. Anything marked "coming soon" that the user does not own
//! becomes one Pulse post with its art and date; "Not interested" and "Save"
//! on those posts move how many the next check may post. A one-tap keeps a
//! reminder that posts again on release day.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::chips::{ChipKind, QuickChip};
use crate::steam_wishlist::{capsule_url, store_url};
use crate::update_feed::UpdateCard;

pub const FILE_NAME: &str = "release-radar.json";
pub const SOURCE_NAME: &str = "Release radar";
pub const CHECK_EVERY_MS: u64 = 24 * 60 * 60 * 1000;
/// Games asked about on one check.
pub const SEEDS_PER_CHECK: usize = 6;
/// DLC and search hits looked up per game.
pub const LOOKUPS_PER_SEED: usize = 4;
pub const POSTS_PER_CHECK: i32 = 3;
pub const POSTS_MAX: i32 = 5;
pub const REMIND_PREFIX: &str = "radar:remind:";
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

pub fn details_url(app_id: u32) -> String {
    format!("https://store.steampowered.com/api/appdetails?appids={app_id}&l=english")
}

pub fn search_url(term: &str) -> String {
    format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
        crate::pulse_sources::query_value(term)
    )
}

/// What the store says about one app.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub name: String,
    pub dlc: Vec<u32>,
    pub coming_soon: bool,
    pub date_text: String,
    /// The store's app type: "game", "dlc", "music", "demo", … ("" if missing).
    pub app_type: String,
    /// For DLC, the game it belongs to.
    pub full_game: Option<u32>,
}

/// One `appdetails` reply.
pub fn parse_details(body: &str, app_id: u32) -> Option<Details> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let data = v.pointer(&format!("/{app_id}/data"))?;
    let name = data.get("name")?.as_str()?.trim().to_string();
    let dlc = data
        .get("dlc")
        .and_then(|d| d.as_array())
        .map(|d| {
            d.iter()
                .filter_map(|x| u32::try_from(x.as_u64()?).ok())
                .collect()
        })
        .unwrap_or_default();
    let release = data.get("release_date");
    let coming_soon = release
        .and_then(|r| r.get("coming_soon"))
        .and_then(|c| c.as_bool())
        .unwrap_or(false);
    let date_text = release
        .and_then(|r| r.get("date"))
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let app_type = data
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let full_game = data
        .pointer("/fullgame/appid")
        .and_then(|id| match id {
            serde_json::Value::String(s) => s.parse::<u64>().ok(),
            other => other.as_u64(),
        })
        .and_then(|id| u32::try_from(id).ok());
    Some(Details {
        name,
        dlc,
        coming_soon,
        date_text,
        app_type,
        full_game,
    })
}

/// App id and name from a `storesearch` reply.
pub fn parse_search(body: &str) -> Vec<(u32, String)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    v.get("items")
        .and_then(|i| i.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    let id = u32::try_from(it.get("id")?.as_u64()?).ok()?;
                    let name = it.get("name")?.as_str()?.trim().to_string();
                    Some((id, name))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The series name: the part before a subtitle, without a trailing number
/// ("Factorio: Space Age" and "Hades II" give "Factorio" and "Hades").
pub fn series_name(name: &str) -> String {
    let head = name.split([':', '–', '—']).next().unwrap_or(name);
    let head = head.split(" - ").next().unwrap_or(head).trim();
    let words: Vec<&str> = head.split_whitespace().collect();
    let numbered = |w: &str| {
        w.chars().all(|c| c.is_ascii_digit()) || w.chars().all(|c| matches!(c, 'I' | 'V' | 'X'))
    };
    let keep = match words.split_last() {
        Some((last, rest)) if !rest.is_empty() && numbered(last) => rest,
        _ => &words[..],
    };
    keep.join(" ")
}

fn month(word: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let w = word.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| w.starts_with(m))
        .map(|i| i as u32 + 1)
}

/// Unix ms of an exact store date ("14 Nov, 2026", "Nov 14, 2026"); `None`
/// for "Coming soon", "Q4 2026" and other loose dates.
pub fn release_day_ms(text: &str) -> Option<u64> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() != 3 {
        return None;
    }
    let num = |w: &str| w.parse::<u32>().ok();
    let (day, mon, year) = match (num(words[0]), month(words[0])) {
        (Some(d), _) => (d, month(words[1])?, num(words[2])?),
        (None, Some(m)) => (num(words[1])?, m, num(words[2])?),
        _ => return None,
    };
    if !(1..=31).contains(&day) || year < 1970 {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y = i64::from(year) - i64::from(mon <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(mon) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days).ok().map(|d| d * DAY_MS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dlc,
    Sequel,
}

/// Something coming for a game the user has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upcoming {
    pub app_id: u32,
    pub name: String,
    /// The owned or wishlisted game it belongs to.
    pub base: String,
    pub kind: Kind,
    pub date_text: String,
    pub date_ms: Option<u64>,
}

impl Upcoming {
    pub fn title(&self) -> String {
        format!("Coming: {}", self.name)
    }

    pub fn body(&self) -> String {
        let what = match self.kind {
            Kind::Dlc => format!("New DLC for {}", self.base),
            Kind::Sequel => format!("New in the {} series", series_name(&self.base)),
        };
        let when =
            if self.date_text.is_empty() || self.date_text.eq_ignore_ascii_case("coming soon") {
                "No date yet.".to_string()
            } else {
                format!("Out {}.", self.date_text)
            };
        format!("{what}. {when}")
    }

    pub fn radar_card(&self, now_ms: u64) -> UpdateCard {
        let source_id = format!("source:release-radar:{}", self.app_id);
        let mut card =
            crate::update_feed::digest_card(&source_id, &self.title(), &self.body(), now_ms);
        card.citations = vec![store_url(self.app_id)];
        card.pulse.source_name = Some(SOURCE_NAME.to_string());
        card.pulse.image_urls = vec![capsule_url(self.app_id)];
        card.pulse.because = Some(format!("You have {}", self.base));
        card
    }

    /// "Remind me on release day", when the store gives an exact day.
    pub fn chip(&self) -> Option<QuickChip> {
        self.date_ms?;
        Some(QuickChip {
            id: format!("{REMIND_PREFIX}{}", self.app_id),
            label: format!("Remind me when {} is out", self.name),
            value: format!("{REMIND_PREFIX}{}", self.app_id),
            kind: ChipKind::Chat,
            score: 1.0,
            hint: format!("Posts again on {}", self.date_text),
            primary: false,
        })
    }
}

/// What a looked-up candidate really is, from the store's own type. A
/// search hit only counts as a sequel when the store calls it a game, and as
/// DLC only when it is DLC for the seed game itself; soundtracks, demos,
/// videos and DLC for other games are dropped.
fn classify(found_as: Kind, d: &Details, seed_id: u32) -> Option<Kind> {
    match (found_as, d.app_type.as_str()) {
        (_, "music" | "demo" | "video" | "series" | "episode" | "advertising" | "mod") => None,
        (Kind::Dlc, _) => Some(Kind::Dlc),
        (Kind::Sequel, "game") => Some(Kind::Sequel),
        (Kind::Sequel, "dlc") if d.full_game == Some(seed_id) => Some(Kind::Dlc),
        (Kind::Sequel, _) => None,
    }
}

/// Look up what is coming for `seeds` (app id, name), skipping anything in
/// `owned`. `get` fetches a URL.
pub fn scan(
    seeds: &[(u32, String)],
    owned: &BTreeSet<u32>,
    get: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Vec<Upcoming>, String> {
    let mut out: Vec<Upcoming> = Vec::new();
    // Store lookups by id, kept for the whole check: the same app can come
    // up for several seeds and be dropped for one but kept for another.
    let mut looked: BTreeMap<u32, Option<Details>> = BTreeMap::new();
    let mut posted: BTreeSet<u32> = BTreeSet::new();
    let mut reached = false;
    let mut last_err = String::new();
    for (seed_id, seed_name) in seeds.iter().take(SEEDS_PER_CHECK) {
        let mut candidates: Vec<(u32, Kind)> = Vec::new();
        match get(&details_url(*seed_id)) {
            Ok(body) => {
                reached = true;
                if let Some(d) = parse_details(&body, *seed_id) {
                    candidates.extend(
                        d.dlc
                            .iter()
                            .filter(|id| !owned.contains(id))
                            .take(LOOKUPS_PER_SEED)
                            .map(|id| (*id, Kind::Dlc)),
                    );
                }
            }
            Err(e) => last_err = e,
        }
        let series = series_name(seed_name);
        if series.chars().count() >= 4 {
            if let Ok(body) = get(&search_url(&series)) {
                reached = true;
                let lower = series.to_ascii_lowercase();
                candidates.extend(
                    parse_search(&body)
                        .into_iter()
                        .filter(|(id, name)| {
                            *id != *seed_id
                                && !owned.contains(id)
                                && name.to_ascii_lowercase().starts_with(&lower)
                        })
                        .take(LOOKUPS_PER_SEED)
                        .map(|(id, _)| (id, Kind::Sequel)),
                );
            }
        }
        for (id, kind) in candidates {
            if posted.contains(&id) {
                continue;
            }
            let looked_up = looked.entry(id).or_insert_with(|| {
                get(&details_url(id))
                    .ok()
                    .and_then(|body| parse_details(&body, id))
            });
            let Some(d) = looked_up.clone() else {
                continue;
            };
            let Some(kind) = classify(kind, &d, *seed_id) else {
                continue;
            };
            if d.coming_soon {
                posted.insert(id);
                out.push(Upcoming {
                    app_id: id,
                    date_ms: release_day_ms(&d.date_text),
                    name: d.name,
                    base: seed_name.clone(),
                    kind,
                    date_text: d.date_text,
                });
            }
        }
    }
    if !reached && !seeds.is_empty() {
        return Err(last_err);
    }
    Ok(out)
}

/// Library (most played first) and wishlist taken in turn, so the wishlist
/// is not stuck behind the whole library. Wishlisted games the user now owns
/// are left out.
pub fn interleave_seeds(
    library: Vec<(u32, String)>,
    wishlist: Vec<(u32, String)>,
) -> Vec<(u32, String)> {
    let owned: BTreeSet<u32> = library.iter().map(|(id, _)| *id).collect();
    let mut wish = wishlist.into_iter().filter(|(id, _)| !owned.contains(id));
    let mut lib = library.into_iter();
    let mut out = Vec::new();
    loop {
        let (a, b) = (lib.next(), wish.next());
        if a.is_none() && b.is_none() {
            return out;
        }
        out.extend(a);
        out.extend(b);
    }
}

/// How many posts one check may make, moved by the feedback score the user
/// gave this source's posts.
pub fn posts_allowed(source_bias: i32) -> usize {
    usize::try_from((POSTS_PER_CHECK + source_bias).clamp(0, POSTS_MAX)).unwrap_or(0)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub name: String,
    pub date_ms: u64,
}

/// What the radar remembers: when it last looked, what it already posted,
/// and the release-day reminders.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Radar {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub last_check_ms: u64,
    #[serde(default)]
    pub posted: BTreeSet<u32>,
    #[serde(default)]
    pub reminders: BTreeMap<u32, Reminder>,
    /// The game the next check starts from, so every game gets a turn. An app
    /// id, not a position: the list is rebuilt and re-sorted every day.
    #[serde(default)]
    pub next_seed: Option<u32>,
}

impl Radar {
    pub fn due(&self, now_ms: u64) -> bool {
        self.on && now_ms.saturating_sub(self.last_check_ms) >= CHECK_EVERY_MS
    }

    /// The items not posted before, at most `allowed`, soonest first; they
    /// count as posted from now on.
    pub fn fresh(&mut self, mut found: Vec<Upcoming>, allowed: usize) -> Vec<Upcoming> {
        found.sort_by_key(|u| (u.date_ms.unwrap_or(u64::MAX), u.app_id));
        let mut out = Vec::new();
        for u in found {
            if out.len() >= allowed {
                break;
            }
            if self.posted.insert(u.app_id) {
                out.push(u);
            }
        }
        out
    }

    /// The next `SEEDS_PER_CHECK` seeds from `next_seed` (the top of the list
    /// when that game is gone), and the game the check after starts from.
    /// Going round one check at a time, games past the first few are asked
    /// about on later days. Set `next_seed` only once the check worked, so a
    /// failed one is tried again.
    pub fn seed_batch(&self, seeds: &[(u32, String)]) -> (Vec<(u32, String)>, Option<u32>) {
        if seeds.is_empty() {
            return (Vec::new(), None);
        }
        let start = self
            .next_seed
            .and_then(|id| seeds.iter().position(|(seed, _)| *seed == id))
            .unwrap_or(0);
        let take = SEEDS_PER_CHECK.min(seeds.len());
        let batch = seeds
            .iter()
            .cycle()
            .skip(start)
            .take(take)
            .cloned()
            .collect();
        (batch, Some(seeds[(start + take) % seeds.len()].0))
    }

    pub fn remind(&mut self, u: &Upcoming) -> bool {
        let Some(date_ms) = u.date_ms else {
            return false;
        };
        self.reminders.insert(
            u.app_id,
            Reminder {
                name: u.name.clone(),
                date_ms,
            },
        );
        true
    }

    /// Reminders whose day has come; they are taken off the list.
    pub fn take_due(&mut self, now_ms: u64) -> Vec<(u32, Reminder)> {
        let due: Vec<u32> = self
            .reminders
            .iter()
            .filter(|(_, r)| r.date_ms <= now_ms)
            .map(|(id, _)| *id)
            .collect();
        due.into_iter()
            .filter_map(|id| self.reminders.remove(&id).map(|r| (id, r)))
            .collect()
    }
}

/// The release-day post for a reminder.
pub fn out_today_card(app_id: u32, r: &Reminder, now_ms: u64) -> UpdateCard {
    let source_id = format!("source:release-radar:out:{app_id}");
    let mut card = crate::update_feed::digest_card(
        &source_id,
        &format!("Out today: {}", r.name),
        "You asked to be reminded on release day.",
        now_ms,
    );
    card.citations = vec![store_url(app_id)];
    card.pulse.source_name = Some(SOURCE_NAME.to_string());
    card.pulse.image_urls = vec![capsule_url(app_id)];
    card
}

pub fn load(config_dir: &Path) -> Radar {
    std::fs::read_to_string(config_dir.join(FILE_NAME))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, radar: &Radar) -> Result<(), String> {
    let text = serde_json::to_string_pretty(radar).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&config_dir.join(FILE_NAME), &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made-up app ids and names.
    fn details(id: u32, name: &str, dlc: &[u32], soon: bool, date: &str) -> String {
        typed(id, name, "game", None, dlc, soon, date)
    }

    fn typed(
        id: u32,
        name: &str,
        kind: &str,
        full_game: Option<u32>,
        dlc: &[u32],
        soon: bool,
        date: &str,
    ) -> String {
        let mut data = serde_json::json!({
            "type": kind, "name": name, "dlc": dlc,
            "release_date": { "coming_soon": soon, "date": date }
        });
        if let Some(game) = full_game {
            // The store sends this id as a string.
            data["fullgame"] = serde_json::json!({ "appid": game.to_string(), "name": "x" });
        }
        serde_json::json!({ id.to_string(): { "success": true, "data": data } }).to_string()
    }

    fn store() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert(
            details_url(100),
            details(100, "Star Forge", &[101, 102, 103], false, "3 Mar, 2021"),
        );
        m.insert(
            details_url(101),
            typed(
                101,
                "Star Forge: Deep Space",
                "dlc",
                Some(100),
                &[],
                true,
                "14 Nov, 2026",
            ),
        );
        m.insert(
            details_url(102),
            typed(
                102,
                "Star Forge: Soundtrack",
                "music",
                Some(100),
                &[],
                true,
                "Coming soon",
            ),
        );
        m.insert(
            details_url(103),
            typed(
                103,
                "Star Forge: Owned Pack",
                "dlc",
                Some(100),
                &[],
                true,
                "Q1 2027",
            ),
        );
        m.insert(
            details_url(110),
            details(110, "Star Forge II", &[], true, "Coming soon"),
        );
        m.insert(
            details_url(120),
            typed(
                120,
                "Star Forge II Demo",
                "demo",
                None,
                &[],
                true,
                "Coming soon",
            ),
        );
        m.insert(
            details_url(121),
            typed(
                121,
                "Star Forge II: Season Pass",
                "dlc",
                Some(110),
                &[],
                true,
                "Coming soon",
            ),
        );
        m.insert(
            details_url(122),
            typed(
                122,
                "Star Forge Original Score",
                "music",
                None,
                &[],
                true,
                "Coming soon",
            ),
        );
        m.insert(
            search_url("Star Forge"),
            serde_json::json!({ "items": [
                { "id": 100, "name": "Star Forge" },
                { "id": 120, "name": "Star Forge II Demo" },
                { "id": 121, "name": "Star Forge II: Season Pass" },
                { "id": 122, "name": "Star Forge Original Score" },
                { "id": 110, "name": "Star Forge II" },
                { "id": 101, "name": "Star Forge: Deep Space" },
                { "id": 999, "name": "Starforged Tales" }
            ]})
            .to_string(),
        );
        m
    }

    #[test]
    fn a_fixture_library_gives_upcoming_dlc_and_sequels() {
        let store = store();
        let mut fetched = Vec::new();
        let mut get = |url: &str| {
            fetched.push(url.to_string());
            store.get(url).cloned().ok_or_else(|| "404".to_string())
        };
        let owned = BTreeSet::from([100, 103]);
        let found = scan(&[(100, "Star Forge".into())], &owned, &mut get).unwrap();
        let names: Vec<(&str, Kind, Option<u64>)> = found
            .iter()
            .map(|u| (u.name.as_str(), u.kind, u.date_ms))
            .collect();
        assert_eq!(
            names,
            vec![
                (
                    "Star Forge: Deep Space",
                    Kind::Dlc,
                    release_day_ms("14 Nov, 2026")
                ),
                ("Star Forge II", Kind::Sequel, None),
            ]
        );
        assert!(
            !fetched.contains(&details_url(103)),
            "owned DLC is not looked up"
        );
        assert_eq!(
            fetched.iter().filter(|u| **u == details_url(101)).count(),
            1,
            "DLC found twice is looked up once"
        );
        assert_eq!(found[0].body(), "New DLC for Star Forge. Out 14 Nov, 2026.");
        assert_eq!(
            found[1].body(),
            "New in the Star Forge series. No date yet."
        );
        let card = found[0].radar_card(5);
        assert_eq!(card.title, "Coming: Star Forge: Deep Space");
        assert_eq!(card.pulse.source_name.as_deref(), Some(SOURCE_NAME));
        assert!(found[1].chip().is_none(), "no exact date, no reminder");
        assert!(
            fetched.contains(&details_url(102)) && fetched.contains(&details_url(122)),
            "the soundtracks were looked up and then dropped as music"
        );
    }

    #[test]
    fn a_search_hit_that_is_dlc_for_the_seed_is_labelled_dlc() {
        let mut store = BTreeMap::new();
        store.insert(
            details_url(200),
            details(200, "Moon Mill", &[], false, "1 Jan, 2020"),
        );
        store.insert(
            search_url("Moon Mill"),
            serde_json::json!({ "items": [{ "id": 201, "name": "Moon Mill: Tides" }] }).to_string(),
        );
        store.insert(
            details_url(201),
            typed(
                201,
                "Moon Mill: Tides",
                "dlc",
                Some(200),
                &[],
                true,
                "Coming soon",
            ),
        );
        let mut get = |url: &str| store.get(url).cloned().ok_or_else(|| "404".to_string());
        let found = scan(&[(200, "Moon Mill".into())], &BTreeSet::new(), &mut get).unwrap();
        assert_eq!(
            found.iter().map(|u| (u.app_id, u.kind)).collect::<Vec<_>>(),
            vec![(201, Kind::Dlc)]
        );
        assert_eq!(found[0].body(), "New DLC for Moon Mill. No date yet.");
    }

    #[test]
    fn wishlist_games_interleave_and_every_seed_gets_a_turn() {
        let named = |ids: &[u32]| {
            ids.iter()
                .map(|id| (*id, format!("G{id}")))
                .collect::<Vec<_>>()
        };
        let seeds = interleave_seeds(named(&[1, 2, 3, 4, 5, 6, 7, 8]), named(&[50, 3, 51]));
        assert_eq!(
            seeds.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![1, 50, 2, 51, 3, 4, 5, 6, 7, 8],
            "owned wishlist entry 3 is dropped"
        );
        let mut radar = Radar::default();
        // Each check that worked saves where the next one starts.
        let check = |r: &mut Radar, seeds: &[(u32, String)]| {
            let (batch, next) = r.seed_batch(seeds);
            r.next_seed = next;
            batch.into_iter().map(|(id, _)| id).collect::<Vec<_>>()
        };
        assert_eq!(check(&mut radar, &seeds), vec![1, 50, 2, 51, 3, 4]);
        assert_eq!(radar.next_seed, Some(5));
        assert_eq!(check(&mut radar, &seeds), vec![5, 6, 7, 8, 1, 50]);
        // A check that failed does not save, so the same games are asked again.
        let (retry, _) = radar.seed_batch(&seeds);
        assert_eq!(
            retry.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![2, 51, 3, 4, 5, 6]
        );
        // Playtime re-sorted the library overnight: the next check still
        // starts at game 2, wherever it moved.
        let resorted = interleave_seeds(named(&[8, 7, 2, 1, 3, 4, 5, 6]), named(&[50, 51]));
        assert_eq!(check(&mut radar, &resorted), vec![2, 1, 3, 4, 5, 6]);
        // The game it would start from left the list: start from the top.
        radar.next_seed = Some(99);
        assert_eq!(check(&mut radar, &seeds), vec![1, 50, 2, 51, 3, 4]);
        let few = named(&[9, 10]);
        assert_eq!(
            check(&mut radar, &few),
            vec![9, 10],
            "a short list is asked once each"
        );
        assert_eq!(radar.seed_batch(&[]), (Vec::new(), None));
    }

    #[test]
    fn a_hit_dropped_for_one_game_still_posts_as_dlc_for_its_own() {
        let mut store = store();
        store.insert(
            search_url("Star Forge"),
            serde_json::json!({ "items": [{ "id": 300, "name": "Star Forge Builder Kit" }] })
                .to_string(),
        );
        store.insert(
            details_url(300),
            typed(
                300,
                "Star Forge Builder Kit",
                "dlc",
                Some(400),
                &[],
                true,
                "Coming soon",
            ),
        );
        store.insert(
            details_url(400),
            details(400, "Kit Builder", &[300], false, ""),
        );
        let mut fetched = Vec::new();
        let mut get = |url: &str| {
            fetched.push(url.to_string());
            store.get(url).cloned().ok_or_else(|| "404".to_string())
        };
        let seeds = [
            (100, "Star Forge".to_string()),
            (400, "Kit Builder".to_string()),
        ];
        let found = scan(&seeds, &BTreeSet::from([100, 103, 400]), &mut get).unwrap();
        let kit: Vec<_> = found.iter().filter(|u| u.app_id == 300).collect();
        assert_eq!(kit.len(), 1);
        assert_eq!(
            (kit[0].kind, kit[0].base.as_str()),
            (Kind::Dlc, "Kit Builder")
        );
        assert_eq!(
            fetched.iter().filter(|u| **u == details_url(300)).count(),
            1,
            "looked up once, judged per game"
        );
    }

    #[test]
    fn no_reply_at_all_is_an_error() {
        let mut get = |_: &str| Err::<String, String>("could not find the host".into());
        assert_eq!(
            scan(&[(100, "Star Forge".into())], &BTreeSet::new(), &mut get),
            Err("could not find the host".into())
        );
    }

    #[test]
    fn series_names_and_store_dates() {
        assert_eq!(series_name("Factorio: Space Age"), "Factorio");
        assert_eq!(series_name("Hades II"), "Hades");
        assert_eq!(series_name("Half-Life 2"), "Half-Life");
        assert_eq!(series_name("Portal"), "Portal");
        assert_eq!(release_day_ms("1 Jan, 1970"), Some(0));
        assert_eq!(release_day_ms("Nov 14, 2023"), Some(1_699_920_000_000));
        assert_eq!(release_day_ms("14 Nov, 2023"), Some(1_699_920_000_000));
        assert_eq!(release_day_ms("Q4 2026"), None);
        assert_eq!(release_day_ms("Coming soon"), None);
    }

    #[test]
    fn posts_dedupe_and_reminders_come_due() {
        let up = |id: u32, date_ms: Option<u64>| Upcoming {
            app_id: id,
            name: format!("Game {id}"),
            base: "Star Forge".into(),
            kind: Kind::Dlc,
            date_text: "14 Nov, 2026".into(),
            date_ms,
        };
        let mut radar = Radar::default();
        let first = radar.fresh(vec![up(2, Some(900)), up(1, None), up(3, Some(500))], 2);
        assert_eq!(
            first.iter().map(|u| u.app_id).collect::<Vec<_>>(),
            vec![3, 2]
        );
        let again = radar.fresh(vec![up(2, Some(900)), up(1, None)], 5);
        assert_eq!(again.iter().map(|u| u.app_id).collect::<Vec<_>>(), vec![1]);
        assert!(radar.remind(&up(3, Some(500))));
        assert!(!radar.remind(&up(1, None)));
        assert_eq!(radar.take_due(499), Vec::new());
        let due = radar.take_due(500);
        assert_eq!(due.len(), 1);
        assert_eq!(
            out_today_card(due[0].0, &due[0].1, 600).title,
            "Out today: Game 3"
        );
        assert!(radar.reminders.is_empty());
        assert_eq!(
            (posts_allowed(-5), posts_allowed(0), posts_allowed(9)),
            (0, 3, 5)
        );
    }
}
