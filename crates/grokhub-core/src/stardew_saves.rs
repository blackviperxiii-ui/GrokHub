//! Stardew Valley farm saves, read-only. The game writes one folder per farm
//! under its `Saves` folder, each with a small `SaveGameInfo` file (the
//! farmer as XML) beside the big main save. This reads only `SaveGameInfo`:
//! farmer and farm name, money, day, skills and play time. It never writes a
//! save, needs no mod, and a file it can't parse (half-written, too big, a
//! later format) is skipped.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::update_feed::{digest_card, UpdateCard};

/// Stardew Valley on Steam.
pub const APP_ID: u32 = 413150;
pub const NAME: &str = "Stardew Valley";

/// Biggest `SaveGameInfo` read.
const MAX_FILE: u64 = 8 * 1024 * 1024;

const SEASONS: [&str; 4] = ["Spring", "Summer", "Fall", "Winter"];

/// Where the farms can be: the user's own folder, then the Proton prefix in
/// each Steam library under `steam_root`.
pub fn saves_dirs(steam_root: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        if cfg!(windows) {
            dirs.push(home.join("AppData/Roaming/StardewValley/Saves"));
        } else {
            dirs.push(home.join(".config/StardewValley/Saves"));
        }
    }
    if let Some(root) = steam_root {
        for lib in crate::steam::library_folders(root) {
            dirs.push(
                lib.join("steamapps/compatdata")
                    .join(APP_ID.to_string())
                    .join("pfx/drive_c/users/steamuser/AppData/Roaming/StardewValley/Saves"),
            );
        }
    }
    dirs
}

/// The farms in every saves folder on this computer.
pub fn find_farms(steam_root: Option<&Path>) -> Vec<StardewFarm> {
    saves_dirs(steam_root, dirs::home_dir().as_deref())
        .iter()
        .flat_map(|d| read_farms(d))
        .collect()
}

/// One farm's `SaveGameInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StardewFarm {
    /// The farm's folder name, which stays the same across saves.
    pub id: String,
    pub farmer: Option<String>,
    pub farm: Option<String>,
    pub money: Option<u64>,
    pub total_earned: Option<u64>,
    pub day: Option<u32>,
    /// 0 Spring to 3 Winter.
    pub season: Option<u32>,
    pub year: Option<u32>,
    /// Farming, mining, combat, foraging, fishing.
    pub skills: [Option<u32>; 5],
    pub played_ms: Option<u64>,
    /// When the game last wrote it.
    pub saved: Option<SystemTime>,
}

const SKILLS: [(&str, &str); 5] = [
    ("farmingLevel", "Farming"),
    ("miningLevel", "Mining"),
    ("combatLevel", "Combat"),
    ("foragingLevel", "Foraging"),
    ("fishingLevel", "Fishing"),
];

/// Parse a `SaveGameInfo` (the `<Farmer>` element). Only the root's direct
/// children are read, so an item's `<name>` deeper in can't pass for the
/// farmer's. `None` when it isn't one.
pub fn parse_save_info(xml: &str) -> Option<StardewFarm> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth = 0usize;
    let mut field: Option<String> = None;
    let mut text = String::new();
    let mut farm = StardewFarm::default();
    let mut root_ok = false;
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) => {
                depth += 1;
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if depth == 1 {
                    if name != "Farmer" {
                        return None;
                    }
                    root_ok = true;
                } else if depth == 2 {
                    field = Some(name);
                    text.clear();
                }
            }
            Event::Text(t) if depth == 2 => {
                if let Ok(s) = t.decode() {
                    text.push_str(&s);
                }
            }
            Event::GeneralRef(r) if depth == 2 => {
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
                if depth == 2 {
                    if let Some(f) = field.take() {
                        take_field(&mut farm, &f, text.trim());
                    }
                }
                if depth == 1 {
                    break;
                }
                depth = depth.checked_sub(1)?;
            }
            // A half-written file ends before the root closes.
            Event::Eof => return None,
            _ => {}
        }
    }
    (root_ok && farm.farmer.is_some()).then_some(farm)
}

fn take_field(farm: &mut StardewFarm, field: &str, v: &str) {
    let text = || (!v.is_empty()).then(|| v.to_owned());
    match field {
        "name" => farm.farmer = text(),
        "farmName" => farm.farm = text(),
        "money" => farm.money = v.parse().ok(),
        "totalMoneyEarned" => farm.total_earned = v.parse().ok(),
        "dayOfMonthForSaveGame" => farm.day = v.parse().ok(),
        "seasonForSaveGame" => farm.season = v.parse().ok().filter(|s| *s < 4),
        "yearForSaveGame" => farm.year = v.parse().ok(),
        "millisecondsPlayed" => farm.played_ms = v.parse().ok(),
        _ => {
            if let Some(i) = SKILLS.iter().position(|(tag, _)| *tag == field) {
                farm.skills[i] = v.parse().ok();
            }
        }
    }
}

/// Read one farm folder's `SaveGameInfo`, size-capped.
pub fn read_farm(dir: &Path) -> Option<StardewFarm> {
    let path = dir.join("SaveGameInfo");
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let mut farm = parse_save_info(text.trim_start_matches('\u{feff}'))?;
    farm.id = dir.file_name()?.to_string_lossy().into_owned();
    farm.saved = meta.modified().ok();
    Some(farm)
}

/// Every farm in a `Saves` folder, newest save first.
pub fn read_farms(saves: &Path) -> Vec<StardewFarm> {
    let Ok(entries) = std::fs::read_dir(saves) else {
        return Vec::new();
    };
    let mut farms: Vec<StardewFarm> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| read_farm(&e.path()))
        .collect();
    farms.sort_by(|a, b| b.saved.cmp(&a.saved).then_with(|| a.id.cmp(&b.id)));
    farms
}

impl StardewFarm {
    /// `Spring 9, Year 2`.
    pub fn date(&self) -> Option<String> {
        let season = SEASONS.get(self.season? as usize)?;
        Some(format!("{season} {}, Year {}", self.day?, self.year?))
    }

    /// `Testfarmer of Pickle Farm: Spring 9, Year 2. 12,340g (45,600g earned). Farming 4, Mining 2, Combat 1, Foraging 3, Fishing 0. 6h 20m played.`
    pub fn farm_line(&self) -> String {
        let mut who = self.farmer.clone().unwrap_or_default();
        if let Some(farm) = &self.farm {
            who.push_str(&format!(" of {farm} Farm"));
        }
        let mut parts = vec![who];
        if let Some(date) = self.date() {
            parts.push(date);
        }
        match (self.money, self.total_earned) {
            (Some(m), Some(t)) => parts.push(format!("{}g ({}g earned)", gold(m), gold(t))),
            (Some(m), None) => parts.push(format!("{}g", gold(m))),
            _ => {}
        }
        let skills: Vec<String> = SKILLS
            .iter()
            .zip(self.skills)
            .filter_map(|((_, label), lvl)| lvl.map(|l| format!("{label} {l}")))
            .collect();
        if !skills.is_empty() {
            parts.push(skills.join(", "));
        }
        if let Some(ms) = self.played_ms {
            parts.push(format!("{} played", played(ms)));
        }
        format!("{}: {}.", parts[0], parts[1..].join(". "))
    }
}

/// `12,340`.
pub fn gold(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `6h 20m`, `45m`.
pub fn played(ms: u64) -> String {
    let min = ms / 60_000;
    match min / 60 {
        0 => format!("{min}m"),
        h => format!("{h}h {}m", min % 60),
    }
}

/// What a play session did to the farm saved last: `before` is how it was
/// when the game started (`None` for a new farm).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FarmSession {
    pub before: Option<StardewFarm>,
    pub after: StardewFarm,
}

impl FarmSession {
    /// The farm saved during the session, compared with how it was when the
    /// game started. `None` when no farm was saved since then.
    pub fn between(before: &[StardewFarm], after: &[StardewFarm]) -> Option<Self> {
        let changed = after
            .iter()
            .find(|a| match before.iter().find(|b| b.id == a.id) {
                Some(b) => b.saved != a.saved || b.played_ms != a.played_ms,
                None => true,
            })?;
        Some(Self {
            before: before.iter().find(|b| b.id == changed.id).cloned(),
            after: changed.clone(),
        })
    }

    /// Gold earned during the session.
    pub fn earned(&self) -> Option<u64> {
        let b = self.before.as_ref()?.total_earned?;
        Some(self.after.total_earned?.saturating_sub(b))
    }

    /// Skills that went up: `Farming 4 to 5`.
    pub fn skill_ups(&self) -> Vec<String> {
        let Some(before) = &self.before else {
            return Vec::new();
        };
        SKILLS
            .iter()
            .zip(before.skills.iter().zip(self.after.skills))
            .filter_map(|((_, label), (b, a))| match (b, a) {
                (Some(b), Some(a)) if a > *b => Some(format!("{label} {b} to {a}")),
                _ => None,
            })
            .collect()
    }

    /// `Pickle Farm, Spring 3 to Spring 9, Year 2. Earned 4,200g. Farming 4 to 5.`
    pub fn summary(&self) -> String {
        let farm = self
            .after
            .farm
            .as_ref()
            .map(|f| format!("{f} Farm"))
            .unwrap_or_else(|| "The farm".into());
        let mut parts = Vec::new();
        // Within one year the start drops its year: `Spring 3 to Spring 9, Year 2`.
        let from = self.before.as_ref().and_then(|b| {
            if b.year.is_some() && b.year == self.after.year {
                Some(format!("{} {}", SEASONS.get(b.season? as usize)?, b.day?))
            } else {
                b.date()
            }
        });
        match (from, self.after.date()) {
            (Some(a), Some(b))
                if self.before.as_ref().and_then(day_index) != day_index(&self.after) =>
            {
                parts.push(format!("{farm}, {a} to {b}"))
            }
            (_, Some(b)) => parts.push(format!("{farm}, {b}")),
            _ => parts.push(farm),
        }
        if let Some(g) = self.earned().filter(|g| *g > 0) {
            parts.push(format!("Earned {}g", gold(g)));
        }
        let ups = self.skill_ups();
        if !ups.is_empty() {
            parts.push(ups.join(", "));
        }
        parts.join(". ") + "."
    }
}

/// The session as a Pulse card, posted when the game closes. The farmer's
/// name stays out of it.
pub fn session_card(s: &FarmSession, at_ms: u64) -> UpdateCard {
    let days = match (s.before.as_ref(), &s.after) {
        (Some(b), a) => day_index(a)
            .zip(day_index(b))
            .map(|(a, b)| a.saturating_sub(b)),
        _ => None,
    };
    let title = match days {
        Some(1) => format!("{NAME} session: 1 day"),
        Some(n) if n > 1 => format!("{NAME} session: {n} days"),
        _ => format!("{NAME} session"),
    };
    digest_card(
        "source:game-watch:stardew-valley",
        &title,
        &s.summary(),
        at_ms,
    )
}

/// Days since the farm's first day: 28 to a season, 4 seasons a year.
fn day_index(f: &StardewFarm) -> Option<u32> {
    let year = f.year?.checked_sub(1)?;
    let day = f.day?.checked_sub(1)?;
    Some(year * 112 + f.season? * 28 + day)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(label: &str) -> Self {
            let d = std::env::temp_dir()
                .join(format!("grokhub-stardew-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Self(d)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // A trimmed 1.6 SaveGameInfo with a made-up farmer. Items carry their
    // own <name> deeper in.
    fn info(day: u32, season: u32, earned: u64, farming: u32, played: u64) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<Farmer xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <name>Testfarmer</name>
  <items>
    <Item xsi:type="Tool"><name>Hoe</name><stack>1</stack></Item>
  </items>
  <farmName>Pickle &amp; Jam</farmName>
  <money>12340</money>
  <totalMoneyEarned>{earned}</totalMoneyEarned>
  <millisecondsPlayed>{played}</millisecondsPlayed>
  <farmingLevel>{farming}</farmingLevel>
  <miningLevel>2</miningLevel>
  <combatLevel>1</combatLevel>
  <foragingLevel>3</foragingLevel>
  <fishingLevel>0</fishingLevel>
  <luckLevel>0</luckLevel>
  <dayOfMonthForSaveGame>{day}</dayOfMonthForSaveGame>
  <seasonForSaveGame>{season}</seasonForSaveGame>
  <yearForSaveGame>2</yearForSaveGame>
</Farmer>"#
        )
    }

    #[test]
    fn save_game_info_parses_and_items_do_not_pass_for_the_farmer() {
        let f = parse_save_info(&info(9, 0, 45600, 4, 22_800_000)).unwrap();
        assert_eq!(f.farmer.as_deref(), Some("Testfarmer"));
        assert_eq!(f.farm.as_deref(), Some("Pickle & Jam"));
        assert_eq!(f.money, Some(12340));
        assert_eq!(f.skills, [Some(4), Some(2), Some(1), Some(3), Some(0)]);
        assert_eq!(f.date().as_deref(), Some("Spring 9, Year 2"));
        assert_eq!(
            f.farm_line(),
            "Testfarmer of Pickle & Jam Farm: Spring 9, Year 2. 12,340g (45,600g earned). Farming 4, Mining 2, Combat 1, Foraging 3, Fishing 0. 6h 20m played."
        );
    }

    #[test]
    fn broken_or_foreign_files_give_nothing() {
        let whole = info(9, 0, 1, 1, 1);
        assert_eq!(parse_save_info(&whole[..whole.len() / 2]), None);
        assert_eq!(parse_save_info("<SaveGame><name>x</name></SaveGame>"), None);
        assert_eq!(parse_save_info("not xml at all"), None);
        assert_eq!(parse_save_info(""), None);
    }

    #[test]
    fn an_oversize_file_is_skipped() {
        let d = Dir::new("big");
        let farm = d.0.join("Testfarmer_123");
        std::fs::create_dir_all(&farm).unwrap();
        let mut text = info(1, 0, 0, 0, 0);
        text.push_str(&" ".repeat(MAX_FILE as usize + 1));
        std::fs::write(farm.join("SaveGameInfo"), text).unwrap();
        assert_eq!(read_farm(&farm), None);
    }

    #[test]
    fn farms_are_found_in_the_proton_prefix() {
        let d = Dir::new("proton");
        let steam = d.0.join("steam");
        let saves = steam.join(
            "steamapps/compatdata/413150/pfx/drive_c/users/steamuser/AppData/Roaming/StardewValley/Saves",
        );
        let farm = saves.join("Testfarmer_123");
        std::fs::create_dir_all(&farm).unwrap();
        std::fs::write(farm.join("SaveGameInfo"), info(9, 1, 100, 1, 60_000)).unwrap();
        let dirs = saves_dirs(Some(&steam), Some(&d.0.join("home")));
        assert!(dirs.contains(&saves));
        let farms: Vec<StardewFarm> = dirs.iter().flat_map(|p| read_farms(p)).collect();
        assert_eq!(farms.len(), 1);
        assert_eq!(farms[0].id, "Testfarmer_123");
        assert_eq!(farms[0].date().as_deref(), Some("Summer 9, Year 2"));
    }

    #[test]
    fn a_session_is_the_farm_saved_since_the_start() {
        let mut before = parse_save_info(&info(3, 0, 40000, 4, 1_000)).unwrap();
        before.id = "Testfarmer_123".into();
        let mut other = before.clone();
        other.id = "Other_456".into();
        let mut after = parse_save_info(&info(9, 0, 44200, 5, 900_000)).unwrap();
        after.id = "Testfarmer_123".into();

        assert_eq!(
            FarmSession::between(
                &[before.clone(), other.clone()],
                &[before.clone(), other.clone()]
            ),
            None
        );
        let s = FarmSession::between(&[before.clone(), other.clone()], &[after.clone(), other])
            .unwrap();
        assert_eq!(s.earned(), Some(4200));
        assert_eq!(s.skill_ups(), vec!["Farming 4 to 5".to_string()]);
        assert_eq!(
            s.summary(),
            "Pickle & Jam Farm, Spring 3 to Spring 9, Year 2. Earned 4,200g. Farming 4 to 5."
        );
        let card = session_card(&s, 1);
        assert_eq!(card.title, "Stardew Valley session: 6 days");
        assert!(!card.body.as_deref().unwrap_or("").contains("Testfarmer"));

        // A farm started this session has nothing to compare with.
        let new = FarmSession::between(&[], &[after]).unwrap();
        assert_eq!(new.summary(), "Pickle & Jam Farm, Spring 9, Year 2.");
        assert_eq!(session_card(&new, 1).title, "Stardew Valley session");
    }

    #[test]
    fn numbers_read_plainly() {
        assert_eq!(gold(0), "0");
        assert_eq!(gold(999), "999");
        assert_eq!(gold(1000), "1,000");
        assert_eq!(gold(1_234_567), "1,234,567");
        assert_eq!(played(45 * 60_000), "45m");
        assert_eq!(played(380 * 60_000), "6h 20m");
    }
}
