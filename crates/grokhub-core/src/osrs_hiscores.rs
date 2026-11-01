//! Old School RuneScape through Jagex's official hiscores
//! (`secure.runescape.com/m=hiscore_oldschool*/index_lite.json`), read by
//! the player's display name. No key, no sign-in, no client: Jagex allows
//! only approved third-party clients and plugins, so Loadout ships none and
//! reads only these public pages.
//!
//! This module is pure: parsing, the gains between two reads and the
//! snapshots kept on disk. The app sends the request.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Old School RuneScape on Steam.
pub const APP_ID: u32 = 1343370;
pub const NAME: &str = "Old School RuneScape";
pub const HOST: &str = "https://secure.runescape.com";
/// Biggest answer read.
pub const MAX_BODY: u64 = 256 * 1024;
/// At most one read an hour, plus Refresh.
pub const REFRESH_MS: u64 = 60 * 60 * 1000;
/// Snapshots kept for gains: two days of hourly reads.
const KEEP: usize = 48;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Which hiscore table: ironmen are ranked apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountType {
    #[default]
    Normal,
    Ironman,
    Hardcore,
    Ultimate,
}

impl AccountType {
    pub const ALL: [AccountType; 4] = [
        AccountType::Normal,
        AccountType::Ironman,
        AccountType::Hardcore,
        AccountType::Ultimate,
    ];

    fn table(self) -> &'static str {
        match self {
            Self::Normal => "hiscore_oldschool",
            Self::Ironman => "hiscore_oldschool_ironman",
            Self::Hardcore => "hiscore_oldschool_hardcore_ironman",
            Self::Ultimate => "hiscore_oldschool_ultimate",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Ironman => "Ironman",
            Self::Hardcore => "Hardcore ironman",
            Self::Ultimate => "Ultimate ironman",
        }
    }
}

/// A display name Jagex allows: 1 to 12 letters, digits, spaces, `-` or `_`.
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n.chars().count() <= 12
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
}

/// The hiscores URL for a player. The name is percent-encoded; spaces
/// become `%20`.
pub fn url(name: &str, kind: AccountType) -> String {
    let mut q = String::new();
    for b in name.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => q.push(b as char),
            _ => q.push_str(&format!("%{b:02X}")),
        }
    }
    format!("{HOST}/m={}/index_lite.json?player={q}", kind.table())
}

/// One skill. `None` rank, level or XP means unranked on the hiscores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub rank: Option<u64>,
    pub level: Option<u64>,
    pub xp: Option<u64>,
}

/// A boss, clue or minigame count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub name: String,
    pub rank: Option<u64>,
    pub score: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsrsStats {
    pub skills: Vec<Skill>,
    pub activities: Vec<Activity>,
    #[serde(default)]
    pub fetched_ms: u64,
}

#[derive(Deserialize)]
struct RawSkill {
    name: String,
    #[serde(default)]
    rank: i64,
    #[serde(default)]
    level: i64,
    #[serde(default)]
    xp: i64,
}

#[derive(Deserialize)]
struct RawActivity {
    name: String,
    #[serde(default)]
    rank: i64,
    #[serde(default)]
    score: i64,
}

#[derive(Deserialize)]
struct Raw {
    skills: Vec<RawSkill>,
    #[serde(default)]
    activities: Vec<RawActivity>,
}

fn ranked(n: i64) -> Option<u64> {
    (n >= 0).then_some(n as u64)
}

/// Parse `index_lite.json`. `-1` means unranked.
pub fn parse(text: &str, now_ms: u64) -> Option<OsrsStats> {
    let raw: Raw = serde_json::from_str(text).ok()?;
    if raw.skills.is_empty() {
        return None;
    }
    Some(OsrsStats {
        skills: raw
            .skills
            .into_iter()
            .map(|s| Skill {
                name: s.name,
                rank: ranked(s.rank),
                level: ranked(s.level).filter(|_| s.rank >= 0),
                xp: ranked(s.xp).filter(|_| s.rank >= 0),
            })
            .collect(),
        activities: raw
            .activities
            .into_iter()
            .map(|a| Activity {
                name: a.name,
                rank: ranked(a.rank),
                score: ranked(a.score),
            })
            .collect(),
        fetched_ms: now_ms,
    })
}

fn thousands(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl OsrsStats {
    fn overall(&self) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == "Overall")
    }

    /// `Total level 1,532, 48,210,330 XP, rank 152,004. Highest: Fishing 92,
    /// Woodcutting 90, Cooking 88. Top kill counts: Zulrah 412, Vorkath 230.`
    pub fn stats_line(&self) -> String {
        let mut out = Vec::new();
        match self.overall() {
            Some(o) if o.level.is_some() => {
                let mut line = format!("Total level {}", thousands(o.level.unwrap_or(0)));
                if let Some(xp) = o.xp {
                    line.push_str(&format!(", {} XP", thousands(xp)));
                }
                if let Some(r) = o.rank {
                    line.push_str(&format!(", rank {}", thousands(r)));
                }
                out.push(line);
            }
            _ => out.push("Overall unranked".into()),
        }
        let mut skills: Vec<&Skill> = self
            .skills
            .iter()
            .filter(|s| s.name != "Overall" && s.level.is_some())
            .collect();
        skills.sort_by(|a, b| b.xp.cmp(&a.xp).then_with(|| a.name.cmp(&b.name)));
        if !skills.is_empty() {
            let top: Vec<String> = skills
                .iter()
                .take(3)
                .map(|s| format!("{} {}", s.name, s.level.unwrap_or(0)))
                .collect();
            out.push(format!("Highest: {}", top.join(", ")));
        }
        let mut kc: Vec<&Activity> = self
            .activities
            .iter()
            .filter(|a| a.score.is_some_and(|s| s > 0) && !is_meta_activity(&a.name))
            .collect();
        kc.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
        if !kc.is_empty() {
            let top: Vec<String> = kc
                .iter()
                .take(3)
                .map(|a| format!("{} {}", a.name, thousands(a.score.unwrap_or(0))))
                .collect();
            out.push(format!("Top counts: {}", top.join(", ")));
        }
        out.join(". ") + "."
    }
}

/// Rows that are scores, not kills or clears.
fn is_meta_activity(name: &str) -> bool {
    matches!(
        name,
        "League Points"
            | "Deadman Points"
            | "Bounty Hunter - Hunter"
            | "Bounty Hunter - Rogue"
            | "Bounty Hunter (Legacy) - Hunter"
            | "Bounty Hunter (Legacy) - Rogue"
            | "LMS - Rank"
            | "PvP Arena - Rank"
            | "Soul Wars Zeal"
            | "Rifts closed"
            | "Colosseum Glory"
            | "Collections Logged"
    )
}

/// XP gained per skill between two reads, biggest first. Unranked on
/// either side counts as nothing, never as a loss.
pub fn gains(before: &OsrsStats, after: &OsrsStats) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = after
        .skills
        .iter()
        .filter(|s| s.name != "Overall")
        .filter_map(|s| {
            let was = before.skills.iter().find(|b| b.name == s.name)?.xp?;
            let now = s.xp?;
            (now > was).then(|| (s.name.clone(), now - was))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// `Last 24 hours: 412,000 XP, mostly Slayer (210,000) and Attack (90,500).`
pub fn gains_line(snapshots: &[OsrsStats]) -> Option<String> {
    let latest = snapshots.last()?;
    let base = snapshots
        .iter()
        .find(|s| latest.fetched_ms.saturating_sub(s.fetched_ms) <= DAY_MS)?;
    if std::ptr::eq(base, latest) {
        return None;
    }
    let g = gains(base, latest);
    let total: u64 = g.iter().map(|(_, x)| x).sum();
    if total == 0 {
        return Some("No XP gained since the read a while ago.".into());
    }
    let hours = latest.fetched_ms.saturating_sub(base.fetched_ms) / (60 * 60 * 1000);
    let span = match hours {
        0 => "Since the last read".to_string(),
        1 => "Last hour".to_string(),
        h => format!("Last {h} hours"),
    };
    let top: Vec<String> = g
        .iter()
        .take(2)
        .map(|(n, x)| format!("{n} ({})", thousands(*x)))
        .collect();
    Some(format!(
        "{span}: {} XP, mostly {}.",
        thousands(total),
        top.join(" and ")
    ))
}

/// The player and their recent reads, kept on disk.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OsrsSaved {
    pub name: String,
    #[serde(default)]
    pub kind: AccountType,
    #[serde(default)]
    pub snapshots: Vec<OsrsStats>,
}

impl OsrsSaved {
    /// Add a read; the oldest go once there are more than two days' worth.
    pub fn push(&mut self, stats: OsrsStats) {
        self.snapshots.push(stats);
        if self.snapshots.len() > KEEP {
            let extra = self.snapshots.len() - KEEP;
            self.snapshots.drain(..extra);
        }
    }

    pub fn latest(&self) -> Option<&OsrsStats> {
        self.snapshots.last()
    }
}

pub fn saved_path(config_dir: &Path) -> PathBuf {
    config_dir.join("games").join("osrs-hiscores.json")
}

pub fn load(config_dir: &Path) -> Option<OsrsSaved> {
    let path = saved_path(config_dir);
    if std::fs::metadata(&path).ok()?.len() > 4 * 1024 * 1024 {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn save(config_dir: &Path, saved: &OsrsSaved) -> std::io::Result<()> {
    let path = saved_path(config_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string(saved).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape of index_lite.json, trimmed, made-up player.
    const LITE: &str = r#"{"skills":[{"id":0,"name":"Overall","rank":152004,"level":1532,"xp":48210330},{"id":1,"name":"Attack","rank":200000,"level":85,"xp":3300000},{"id":2,"name":"Defence","rank":-1,"level":1,"xp":-1},{"id":11,"name":"Fishing","rank":90000,"level":92,"xp":6600000},{"id":9,"name":"Woodcutting","rank":95000,"level":90,"xp":5400000},{"id":8,"name":"Cooking","rank":100000,"level":88,"xp":4500000},{"id":19,"name":"Slayer","rank":150000,"level":87,"xp":4000000}],"activities":[{"id":0,"name":"League Points","rank":5000,"score":9000},{"id":1,"name":"Clue Scrolls (all)","rank":80000,"score":120},{"id":2,"name":"Zulrah","rank":40000,"score":412},{"id":3,"name":"Vorkath","rank":60000,"score":230},{"id":4,"name":"Zalcano","rank":-1,"score":-1}]}"#;

    #[test]
    fn names_and_urls_follow_jagex_rules() {
        assert!(valid_name("Zezima"));
        assert!(valid_name("Iron Man_2"));
        assert!(!valid_name("thirteen chars"));
        assert!(!valid_name("bad/name"));
        assert_eq!(
            url("Iron Man", AccountType::Ironman),
            "https://secure.runescape.com/m=hiscore_oldschool_ironman/index_lite.json?player=Iron%20Man"
        );
        assert!(url("Zezima", AccountType::Normal).contains("/m=hiscore_oldschool/"));
    }

    #[test]
    fn the_lite_feed_reads_plainly_and_unranked_stays_unranked() {
        let s = parse(LITE, 1).unwrap();
        let def = s.skills.iter().find(|k| k.name == "Defence").unwrap();
        assert_eq!((def.rank, def.level, def.xp), (None, None, None));
        assert_eq!(
            s.stats_line(),
            "Total level 1,532, 48,210,330 XP, rank 152,004. Highest: Fishing 92, Woodcutting 90, Cooking 88. Top counts: Zulrah 412, Vorkath 230, Clue Scrolls (all) 120."
        );
        assert_eq!(parse("not json", 0), None);
        assert_eq!(parse(r#"{"skills":[]}"#, 0), None);
    }

    #[test]
    fn gains_cover_the_last_day_and_never_go_negative() {
        let a = parse(LITE, 0).unwrap();
        let mut b = parse(
            &LITE
                .replace("4000000}", "4210000}")
                .replace("3300000", "3390500"),
            3 * 3_600_000,
        )
        .unwrap();
        assert_eq!(
            gains(&a, &b),
            vec![
                ("Slayer".to_string(), 210_000),
                ("Attack".to_string(), 90_500)
            ]
        );
        let mut saved = OsrsSaved::default();
        saved.push(a.clone());
        assert_eq!(gains_line(&saved.snapshots), None);
        saved.push(b.clone());
        assert_eq!(
            gains_line(&saved.snapshots).as_deref(),
            Some("Last 3 hours: 300,500 XP, mostly Slayer (210,000) and Attack (90,500).")
        );
        // A read more than a day old drops out of the window.
        b.fetched_ms = DAY_MS + 10;
        saved.snapshots = vec![a.clone(), b.clone()];
        assert_eq!(gains_line(&saved.snapshots), None);
        assert!(gains(&b, &a).is_empty());
    }

    #[test]
    fn only_two_days_of_reads_are_kept_and_they_round_trip() {
        let s = parse(LITE, 0).unwrap();
        let mut saved = OsrsSaved {
            name: "Zezima".into(),
            ..Default::default()
        };
        for i in 0..60 {
            let mut x = s.clone();
            x.fetched_ms = i;
            saved.push(x);
        }
        assert_eq!(saved.snapshots.len(), KEEP);
        assert_eq!(saved.snapshots[0].fetched_ms, 12);
        let dir = std::env::temp_dir().join(format!("loadout-osrs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        save(&dir, &saved).unwrap();
        assert_eq!(load(&dir), Some(saved));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
