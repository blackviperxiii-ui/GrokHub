//! What the Loadout WoW addon saved (`LoadoutDB`, see `wow_addon`), read
//! back from `WTF/Account/<account>/SavedVariables/Loadout.lua`.
//!
//! WoW writes saved variables as a Lua table literal. This reads that
//! literal with a small parser (strings, numbers, booleans, nil, nested
//! tables, `--` comments) and never runs it: it is data, not code. Files and
//! nesting are capped so a broken or hostile file costs nothing.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Biggest saved-variables file read.
const MAX_FILE: u64 = 4 * 1024 * 1024;
/// Deepest table nesting read.
const MAX_DEPTH: usize = 32;

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn skip(&mut self) {
        loop {
            while self.s.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
            if self.s[self.at..].starts_with(b"--") {
                if self.s[self.at + 2..].starts_with(b"[[") {
                    match find(&self.s[self.at + 4..], b"]]") {
                        Some(end) => self.at += 4 + end + 2,
                        None => self.at = self.s.len(),
                    }
                } else {
                    while self.s.get(self.at).is_some_and(|b| *b != b'\n') {
                        self.at += 1;
                    }
                }
                continue;
            }
            return;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        self.skip();
        if self.s.get(self.at) == Some(&b) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Option<&'a str> {
        self.skip();
        let start = self.at;
        while self
            .s
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.at += 1;
        }
        (self.at > start && !self.s[start].is_ascii_digit())
            .then(|| std::str::from_utf8(&self.s[start..self.at]).ok())
            .flatten()
    }

    fn string(&mut self) -> Option<String> {
        self.skip();
        let quote = *self.s.get(self.at)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let b = *self.s.get(self.at)?;
            self.at += 1;
            match b {
                b'\\' => {
                    let e = *self.s.get(self.at)?;
                    self.at += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'\n' => out.push(b'\n'),
                        b'0'..=b'9' => {
                            // \ddd: up to three decimal digits.
                            let mut n = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.s.get(self.at) {
                                    Some(d) if d.is_ascii_digit() => {
                                        n = n * 10 + u32::from(d - b'0');
                                        self.at += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(u8::try_from(n).ok()?);
                        }
                        other => out.push(other),
                    }
                }
                b if b == quote => break,
                b => out.push(b),
            }
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }

    fn number(&mut self) -> Option<Value> {
        self.skip();
        let start = self.at;
        while self
            .s
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
        {
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.at]).ok()?;
        if let Ok(i) = text.parse::<i64>() {
            return Some(Value::from(i));
        }
        let f = text.parse::<f64>().ok().filter(|f| f.is_finite())?;
        serde_json::Number::from_f64(f).map(Value::Number)
    }

    fn value(&mut self) -> Option<Value> {
        self.skip();
        match *self.s.get(self.at)? {
            b'{' => self.table(),
            b'"' | b'\'' => self.string().map(Value::String),
            b'-' | b'0'..=b'9' | b'.' => self.number(),
            _ => match self.ident()? {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                "nil" => Some(Value::Null),
                _ => None,
            },
        }
    }

    /// A table: a JSON object when any key is given, else an array.
    fn table(&mut self) -> Option<Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH || !self.eat(b'{') {
            return None;
        }
        let mut map = Map::new();
        let mut list = Vec::new();
        loop {
            if self.eat(b'}') {
                break;
            }
            if self.eat(b'[') {
                let key = match self.value()? {
                    Value::String(s) => s,
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => return None,
                };
                if !self.eat(b']') || !self.eat(b'=') {
                    return None;
                }
                map.insert(key, self.value()?);
            } else {
                let save = self.at;
                match self.ident() {
                    Some(name) if !matches!(name, "true" | "false" | "nil") && self.eat(b'=') => {
                        let name = name.to_owned();
                        map.insert(name, self.value()?);
                    }
                    _ => {
                        self.at = save;
                        list.push(self.value()?);
                    }
                }
            }
            if !self.eat(b',') && !self.eat(b';') {
                if self.eat(b'}') {
                    break;
                }
                return None;
            }
        }
        self.depth -= 1;
        Some(if map.is_empty() {
            Value::Array(list)
        } else {
            for (i, v) in list.into_iter().enumerate() {
                map.insert((i + 1).to_string(), v);
            }
            Value::Object(map)
        })
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The global assignments in a saved-variables file (`Name = value`), as
/// JSON. A file that doesn't parse gives what parsed before the error.
pub fn parse_saved(text: &str) -> Map<String, Value> {
    let mut p = Parser {
        s: text.as_bytes(),
        at: 0,
        depth: 0,
    };
    let mut out = Map::new();
    while let Some(name) = p.ident() {
        if !p.eat(b'=') {
            break;
        }
        p.depth = 0;
        let Some(v) = p.value() else {
            break;
        };
        out.insert(name.to_owned(), v);
    }
    out
}

/// One character the addon recorded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WowCharacter {
    pub name: String,
    pub realm: String,
    /// Blizzard's class token (`PALADIN`), for colors.
    pub class: String,
    /// The class's display name in the game's language.
    pub class_name: String,
    pub spec: String,
    /// Points per talent tree on Classic clients before Mists (`31/20/0`).
    pub talents: String,
    pub level: u32,
    pub zone: String,
    pub guild: String,
    pub money_copper: u64,
    /// Equipped item level.
    pub item_level: Option<f64>,
    /// Item level counting the best gear in the bags too.
    pub item_level_overall: Option<f64>,
    pub played_secs: Option<u64>,
    pub deaths: u64,
    pub last_seen: u64,
    pub gear: Vec<WowItem>,
    pub professions: Vec<WowSkill>,
    pub currencies: Vec<WowCount>,
    pub reputations: Vec<WowRep>,
    pub mplus_score: Option<f64>,
    pub keystone: Option<WowKeystone>,
    pub vault: Vec<WowVaultSlot>,
    pub lockouts: Vec<WowLockout>,
    pub achievement_points: Option<u64>,
    pub mounts: Option<u64>,
    pub pets: Option<u64>,
    pub quests_in_log: Option<u64>,
    pub quests_completed: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WowItem {
    pub slot: String,
    pub name: String,
    pub level: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WowSkill {
    pub name: String,
    pub skill: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WowCount {
    pub name: String,
    pub quantity: u64,
}

/// A reputation: renown for a major faction, else its standing (1 hated
/// to 8 exalted) and progress inside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WowRep {
    pub name: String,
    pub renown: Option<u32>,
    pub standing: Option<u32>,
    pub progress: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WowKeystone {
    pub name: String,
    pub level: u32,
}

/// One Great Vault slot: `kind` is Blizzard's activity type (1 dungeons,
/// 3 raids, 6 world).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WowVaultSlot {
    pub kind: u32,
    pub progress: u64,
    pub threshold: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WowLockout {
    pub name: String,
    pub difficulty: String,
    pub raid: bool,
    pub done: u32,
    pub total: u32,
    /// Unix seconds when it resets.
    pub resets_at: u64,
}

/// One thing the addon logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WowEvent {
    /// `kill`, `wipe`, `death`, `level` or `achievement`.
    pub kind: String,
    pub at: u64,
    pub character: String,
    /// The boss or achievement name.
    pub name: String,
    pub level: Option<u32>,
}

/// Everything saved in one flavor, across its accounts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WowSaved {
    /// Most recently seen first.
    pub characters: Vec<WowCharacter>,
    /// Oldest first.
    pub events: Vec<WowEvent>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_owned()
}
fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| {
        x.as_u64()
            .or_else(|| x.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
    })
}

fn u32_of(v: &Value, k: &str) -> Option<u32> {
    u(v, k).and_then(|n| u32::try_from(n).ok())
}

/// A saved Lua list: an array, or a table keyed `1`, `2`, ... in order.
fn list(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(Value::Object(m)) => {
            let mut v: Vec<(u64, &Value)> = m
                .iter()
                .filter_map(|(k, v)| Some((k.parse().ok()?, v)))
                .collect();
            v.sort_by_key(|(i, _)| *i);
            v.into_iter().map(|(_, v)| v).collect()
        }
        _ => Vec::new(),
    }
}

fn character(c: &Value) -> WowCharacter {
    let field = |k: &str| list(c.get(k));
    WowCharacter {
        name: s(c, "name"),
        realm: s(c, "realm"),
        class: s(c, "class"),
        class_name: s(c, "className"),
        spec: s(c, "spec"),
        talents: s(c, "talents"),
        level: u32_of(c, "level").unwrap_or(0),
        zone: s(c, "zone"),
        guild: s(c, "guild"),
        money_copper: u(c, "money").unwrap_or(0),
        // Copies before 1.1.0 could save 0 at logout; that isn't a level.
        item_level: c
            .get("itemLevel")
            .and_then(Value::as_f64)
            .filter(|l| *l > 0.0),
        item_level_overall: c
            .get("itemLevelOverall")
            .and_then(Value::as_f64)
            .filter(|l| *l > 0.0),
        played_secs: u(c, "played"),
        deaths: u(c, "deaths").unwrap_or(0),
        last_seen: u(c, "lastSeen").unwrap_or(0),
        gear: field("gear")
            .into_iter()
            .map(|g| WowItem {
                slot: s(g, "slot"),
                name: s(g, "name"),
                level: u32_of(g, "level"),
            })
            .collect(),
        professions: field("professions")
            .into_iter()
            .map(|p| WowSkill {
                name: s(p, "name"),
                skill: u32_of(p, "skill").unwrap_or(0),
                max: u32_of(p, "max").unwrap_or(0),
            })
            .filter(|p| !p.name.is_empty())
            .collect(),
        currencies: field("currencies")
            .into_iter()
            .map(|x| WowCount {
                name: s(x, "name"),
                quantity: u(x, "quantity").unwrap_or(0),
            })
            .filter(|x| !x.name.is_empty())
            .collect(),
        reputations: field("reputations")
            .into_iter()
            .map(|r| WowRep {
                name: s(r, "name"),
                renown: u32_of(r, "renown"),
                standing: u32_of(r, "standing"),
                progress: match (u(r, "value"), u(r, "low"), u(r, "high")) {
                    (Some(v), Some(lo), Some(hi)) if hi > lo => {
                        Some((v.saturating_sub(lo), hi - lo))
                    }
                    _ => None,
                },
            })
            .filter(|r| !r.name.is_empty())
            .collect(),
        mplus_score: c
            .get("mplusScore")
            .and_then(Value::as_f64)
            .filter(|x| *x > 0.0),
        keystone: c.get("keystone").and_then(|k| {
            Some(WowKeystone {
                name: s(k, "name"),
                level: u32_of(k, "level").filter(|l| *l > 0)?,
            })
        }),
        vault: field("vault")
            .into_iter()
            .map(|v| WowVaultSlot {
                kind: u32_of(v, "kind").unwrap_or(0),
                progress: u(v, "progress").unwrap_or(0),
                threshold: u(v, "threshold").unwrap_or(0),
            })
            .collect(),
        lockouts: field("lockouts")
            .into_iter()
            .map(|l| WowLockout {
                name: s(l, "name"),
                difficulty: s(l, "difficulty"),
                raid: l.get("raid").and_then(Value::as_bool).unwrap_or(false),
                done: u32_of(l, "done").unwrap_or(0),
                total: u32_of(l, "total").unwrap_or(0),
                resets_at: u(l, "resetAt").unwrap_or(0),
            })
            .filter(|l| !l.name.is_empty())
            .collect(),
        achievement_points: u(c, "achievementPoints"),
        mounts: u(c, "mounts"),
        pets: u(c, "pets"),
        quests_in_log: u(c, "questsInLog"),
        quests_completed: u(c, "questsCompleted"),
    }
}

/// The characters and events in one `LoadoutDB` value.
pub fn from_db(db: &Value) -> WowSaved {
    let mut out = WowSaved::default();
    if let Some(chars) = db.get("characters").and_then(Value::as_object) {
        out.characters.extend(chars.values().map(character));
    }
    for e in list(db.get("events")) {
        out.events.push(WowEvent {
            kind: s(e, "kind"),
            at: u(e, "at").unwrap_or(0),
            character: s(e, "character"),
            name: s(e, "name"),
            level: u(e, "level").and_then(|l| u32::try_from(l).ok()),
        });
    }
    out
}

/// The addon's saved-variables files in one flavor, one per account.
pub fn saved_files(flavor_dir: &Path) -> Vec<PathBuf> {
    use crate::wow_addon::child_any_case as child;
    let Ok(rd) = std::fs::read_dir(child(&child(flavor_dir, "WTF"), "Account")) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| child(&child(&e.path(), "SavedVariables"), "Loadout.lua"))
        .filter(|p| p.is_file())
        .collect();
    out.sort();
    out
}

/// Everything the addon saved in one flavor, newest character first.
pub fn read_flavor(flavor_dir: &Path) -> WowSaved {
    let mut out = WowSaved::default();
    for path in saved_files(flavor_dir) {
        if std::fs::metadata(&path).map_or(true, |m| m.len() > MAX_FILE) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(db) = parse_saved(&text).get("LoadoutDB") {
            let one = from_db(db);
            out.characters.extend(one.characters);
            out.events.extend(one.events);
        }
    }
    out.characters
        .sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then(a.name.cmp(&b.name)));
    out.events.sort_by_key(|e| e.at);
    out
}

/// `123456789` copper → `12,345g`.
pub fn gold_label(copper: u64) -> String {
    let gold = copper / 10_000;
    let digits = gold.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out + "g"
}

/// One line for a character: `Testchar · 80 Paladin · Dornogal · 12,345g · ilvl 608`.
pub fn character_line(c: &WowCharacter) -> String {
    let mut parts = vec![c.name.clone()];
    let class = if c.class_name.is_empty() {
        &c.class
    } else {
        &c.class_name
    };
    let spec = if c.talents.is_empty() || c.spec.is_empty() {
        c.spec.clone()
    } else {
        format!("{} ({})", c.spec, c.talents)
    };
    let who = [c.level.to_string(), spec, class.clone()];
    parts.push(
        who.iter()
            .filter(|w| !w.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" "),
    );
    if !c.zone.is_empty() {
        parts.push(c.zone.clone());
    }
    parts.push(gold_label(c.money_copper));
    if let Some(ilvl) = c.item_level {
        parts.push(format!("ilvl {}", ilvl.round() as u64));
    }
    parts.join(" · ")
}

fn count_label(n: u64) -> String {
    gold_label(n * 10_000).trim_end_matches('g').to_owned()
}

/// `3h 20m` from seconds.
fn played_label(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else {
        format!("{h}h {m}m")
    }
}

/// Everything else the addon saved about a character, one short line per
/// topic, for its detail view. Lockouts past their reset by `now` (unix
/// seconds) are left out.
pub fn detail_lines(c: &WowCharacter, now: u64) -> Vec<String> {
    let mut out = Vec::new();
    match (c.item_level, c.item_level_overall) {
        (Some(eq), Some(all)) if all.round() > eq.round() => out.push(format!(
            "Item level {} equipped, {} overall",
            eq.round() as u64,
            all.round() as u64
        )),
        (Some(eq), _) => out.push(format!("Item level {}", eq.round() as u64)),
        _ => {}
    }
    let mut mplus = Vec::new();
    if let Some(score) = c.mplus_score {
        mplus.push(format!("Mythic+ rating {}", score.round() as u64));
    }
    if let Some(k) = &c.keystone {
        let name = if k.name.is_empty() {
            "keystone"
        } else {
            &k.name
        };
        mplus.push(format!("+{} {name}", k.level));
    }
    if !mplus.is_empty() {
        out.push(mplus.join(" · "));
    }
    let vault: Vec<String> = [(1, "Dungeons"), (3, "Raids"), (6, "World")]
        .iter()
        .filter_map(|(kind, label)| {
            let slots: Vec<&WowVaultSlot> = c.vault.iter().filter(|v| v.kind == *kind).collect();
            (!slots.is_empty()).then(|| {
                let open = slots
                    .iter()
                    .filter(|v| v.threshold > 0 && v.progress >= v.threshold)
                    .count();
                format!("{label} {open}/{}", slots.len())
            })
        })
        .collect();
    if !vault.is_empty() {
        out.push(format!("Great Vault: {}", vault.join(", ")));
    }
    let locks: Vec<String> = c
        .lockouts
        .iter()
        .filter(|l| l.resets_at == 0 || l.resets_at > now)
        .map(|l| {
            let diff = if l.difficulty.is_empty() {
                String::new()
            } else {
                format!(" {}", l.difficulty)
            };
            if l.total > 0 {
                format!("{}{diff} {}/{}", l.name, l.done, l.total)
            } else {
                format!("{}{diff}", l.name)
            }
        })
        .collect();
    if !locks.is_empty() {
        out.push(format!("Locked: {}", locks.join(", ")));
    }
    if !c.professions.is_empty() {
        let p: Vec<String> = c
            .professions
            .iter()
            .map(|p| format!("{} {}/{}", p.name, p.skill, p.max))
            .collect();
        out.push(p.join(" · "));
    }
    let renown: Vec<String> = c
        .reputations
        .iter()
        .filter_map(|r| Some(format!("{} {}", r.name, r.renown?)))
        .collect();
    if !renown.is_empty() {
        out.push(format!("Renown: {}", renown.join(", ")));
    }
    let reps = c.reputations.iter().filter(|r| r.renown.is_none()).count();
    let mut misc = Vec::new();
    if let Some(p) = c.achievement_points {
        misc.push(format!("{} achievement points", count_label(p)));
    }
    if let Some(m) = c.mounts {
        misc.push(format!("{} mounts", count_label(m)));
    }
    if let Some(p) = c.pets {
        misc.push(format!("{} pets", count_label(p)));
    }
    if reps > 0 {
        misc.push(format!("{reps} reputations"));
    }
    if !misc.is_empty() {
        out.push(misc.join(" · "));
    }
    let mut quests = Vec::new();
    if let Some(q) = c.quests_in_log {
        quests.push(format!("{q} quests in the log"));
    }
    if let Some(q) = c.quests_completed {
        quests.push(format!("{} completed", count_label(q)));
    }
    if let Some(p) = c.played_secs {
        quests.push(format!("played {}", played_label(p)));
    }
    if !quests.is_empty() {
        out.push(quests.join(" · "));
    }
    let top: Vec<String> = c
        .currencies
        .iter()
        .take(4)
        .map(|x| format!("{} {}", x.name, count_label(x.quantity)))
        .collect();
    if !top.is_empty() {
        out.push(top.join(" · "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape WoW writes for the addon's table.
    const SAVED: &str = r#"
LoadoutDB = {
["schema"] = 1,
["characters"] = {
["Testchar-Area 52"] = {
["className"] = "Paladin",
["money"] = 123456789,
["deaths"] = 1,
["name"] = "Testchar",
["itemLevel"] = 608.3,
["level"] = 80,
["class"] = "PALADIN",
["lastSeen"] = 1761955210,
["realm"] = "Area 52",
["played"] = 360000,
["zone"] = "Dornogal",
},
["Alt-Area 52"] = {
["name"] = "Alt",
["realm"] = "Area 52",
["class"] = "MAGE",
["level"] = 12,
["lastSeen"] = 1700000000,
},
},
["events"] = {
{
["kind"] = "kill",
["name"] = "Ulgrax the \"Devourer\"",
["at"] = 1761955204,
["character"] = "Testchar-Area 52",
["difficulty"] = 16,
}, -- [1]
{
["kind"] = "level",
["level"] = 80,
["at"] = 1761955202,
["character"] = "Testchar-Area 52",
}, -- [2]
},
["addon"] = "1.0.0",
}
OtherAddonDB = nil
"#;

    #[test]
    fn a_saved_variables_file_reads_as_data() {
        let all = parse_saved(SAVED);
        assert_eq!(all.get("OtherAddonDB"), Some(&Value::Null));
        let saved = from_db(&all["LoadoutDB"]);
        let mut chars = saved.characters.clone();
        chars.sort_by_key(|c| std::cmp::Reverse(c.last_seen));
        assert_eq!(
            chars[0],
            WowCharacter {
                name: "Testchar".into(),
                realm: "Area 52".into(),
                class: "PALADIN".into(),
                class_name: "Paladin".into(),
                level: 80,
                zone: "Dornogal".into(),
                money_copper: 123_456_789,
                item_level: Some(608.3),
                played_secs: Some(360_000),
                deaths: 1,
                last_seen: 1_761_955_210,
                ..Default::default()
            }
        );
        assert_eq!(chars[1].level, 12);
        assert_eq!(
            saved.events,
            vec![
                WowEvent {
                    kind: "kill".into(),
                    at: 1_761_955_204,
                    character: "Testchar-Area 52".into(),
                    name: "Ulgrax the \"Devourer\"".into(),
                    level: None,
                },
                WowEvent {
                    kind: "level".into(),
                    at: 1_761_955_202,
                    character: "Testchar-Area 52".into(),
                    name: String::new(),
                    level: Some(80),
                },
            ]
        );
    }

    #[test]
    fn code_and_broken_files_give_nothing_harmful() {
        // A function call isn't data: parsing stops there.
        assert!(parse_saved("LoadoutDB = os.execute(\"rm -rf /\")").is_empty());
        assert!(parse_saved("LoadoutDB = { [\"a\"] = 1, ").is_empty());
        assert!(parse_saved("").is_empty());
        let deep = format!("LoadoutDB = {}{}", "{".repeat(100), "}".repeat(100));
        assert!(parse_saved(&deep).is_empty());
        // What parsed before a bad entry is kept.
        let two = parse_saved("A = { 1, 2 }\nB = print(1)\n");
        assert_eq!(two.get("A"), Some(&serde_json::json!([1, 2])));
        assert_eq!(two.get("B"), None);
    }

    #[test]
    fn lua_escapes_comments_and_bare_keys_parse() {
        let v = parse_saved(
            "X = { name = 'it\\'s', [1] = \"\\65\\66\", --[[ block ]] n = -1.5e2; ok = true }",
        );
        assert_eq!(
            v["X"],
            serde_json::json!({"name": "it's", "1": "AB", "n": -150.0, "ok": true})
        );
    }

    #[test]
    fn every_account_folder_is_read_and_characters_sort_newest_first() {
        let root = std::env::temp_dir().join(format!("loadout-saved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (acct, text) in [
            ("ACCOUNT1", SAVED.to_string()),
            (
                "ACCOUNT2",
                "LoadoutDB = { [\"characters\"] = { [\"New-Realm\"] = { [\"name\"] = \"New\", [\"level\"] = 70, [\"lastSeen\"] = 1800000000 } } }".to_string(),
            ),
        ] {
            let dir = root.join("WTF/Account").join(acct).join("SavedVariables");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Loadout.lua"), text).unwrap();
        }
        std::fs::create_dir_all(root.join("WTF/Account/SavedVariables")).unwrap();
        let saved = read_flavor(&root);
        let names: Vec<&str> = saved.characters.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["New", "Testchar", "Alt"]);
        assert_eq!(saved.events.first().map(|e| e.kind.as_str()), Some("level"));
    }

    #[test]
    fn labels_read_like_the_game() {
        assert_eq!(gold_label(123_456_789), "12,345g");
        assert_eq!(gold_label(9_999), "0g");
        assert_eq!(gold_label(1_000_000_000_000), "100,000,000g");
        let saved = from_db(&parse_saved(SAVED)["LoadoutDB"]);
        let c = saved
            .characters
            .iter()
            .find(|c| c.name == "Testchar")
            .unwrap();
        assert_eq!(
            character_line(c),
            "Testchar · 80 Paladin · Dornogal · 12,345g · ilvl 608"
        );
        let alt = saved.characters.iter().find(|c| c.name == "Alt").unwrap();
        assert_eq!(character_line(alt), "Alt · 12 MAGE · 0g");
    }

    /// What the 1.1.0 addon saves, in WoW's own layout.
    const SAVED_V2: &str = r#"
LoadoutDB = {
["schema"] = 2,
["characters"] = {
["Testchar-Area 52"] = {
["name"] = "Testchar",
["realm"] = "Area 52",
["class"] = "HUNTER",
["className"] = "Hunter",
["spec"] = "Marksmanship",
["level"] = 90,
["zone"] = "Silvermoon City",
["money"] = 51234567,
["itemLevel"] = 701.3,
["itemLevelOverall"] = 703.8,
["played"] = 360000,
["lastSeen"] = 1761955210,
["mplusScore"] = 2456.4,
["keystone"] = {
["level"] = 12,
["map"] = 500,
["name"] = "Magisters' Terrace",
},
["gear"] = {
{
["slot"] = "Head",
["id"] = 1001,
["name"] = "Helm",
["level"] = 700,
["quality"] = 4,
}, -- [1]
},
["professions"] = {
{
["name"] = "Skinning",
["skill"] = 100,
["max"] = 100,
}, -- [1]
},
["currencies"] = {
{
["name"] = "Valorstones",
["quantity"] = 1500,
}, -- [1]
},
["reputations"] = {
{
["name"] = "Silvermoon Court",
["renown"] = 12,
}, -- [1]
{
["name"] = "Shattered Sun",
["standing"] = 5,
["value"] = 3000,
["low"] = 0,
["high"] = 6000,
}, -- [2]
},
["vault"] = {
{
["kind"] = 1,
["progress"] = 4,
["threshold"] = 1,
}, -- [1]
{
["kind"] = 1,
["progress"] = 4,
["threshold"] = 4,
}, -- [2]
{
["kind"] = 1,
["progress"] = 4,
["threshold"] = 8,
}, -- [3]
},
["lockouts"] = {
{
["name"] = "Voidspire",
["difficulty"] = "Heroic",
["raid"] = true,
["done"] = 3,
["total"] = 8,
["resetAt"] = 1762000000,
}, -- [1]
{
["name"] = "Old Raid",
["raid"] = true,
["resetAt"] = 1700000000,
}, -- [2]
},
["achievementPoints"] = 18000,
["mounts"] = 412,
["pets"] = 250,
["questsInLog"] = 22,
["questsCompleted"] = 14000,
},
},
["events"] = {},
}
"#;

    #[test]
    fn everything_the_addon_saves_reads_back_and_shows() {
        let saved = from_db(&parse_saved(SAVED_V2)["LoadoutDB"]);
        let c = &saved.characters[0];
        assert_eq!(c.spec, "Marksmanship");
        assert_eq!(c.gear[0].level, Some(700));
        assert_eq!(c.reputations[1].progress, Some((3000, 6000)));
        assert_eq!(
            c.keystone,
            Some(WowKeystone {
                name: "Magisters' Terrace".into(),
                level: 12
            })
        );
        assert_eq!(
            character_line(c),
            "Testchar · 90 Marksmanship Hunter · Silvermoon City · 5,123g · ilvl 701"
        );
        assert_eq!(
            detail_lines(c, 1_761_955_210),
            vec![
                "Item level 701 equipped, 704 overall",
                "Mythic+ rating 2456 · +12 Magisters' Terrace",
                "Great Vault: Dungeons 2/3",
                "Locked: Voidspire Heroic 3/8",
                "Skinning 100/100",
                "Renown: Silvermoon Court 12",
                "18,000 achievement points · 412 mounts · 250 pets · 1 reputations",
                "22 quests in the log · 14,000 completed · played 4d 4h",
                "Valorstones 1,500",
            ]
        );
    }

    #[test]
    fn a_classic_character_reads_with_its_talent_trees() {
        let db = parse_saved(
            r#"LoadoutDB = {
["characters"] = {
["Testmage-Area 52"] = {
["name"] = "Testmage", ["realm"] = "Area 52", ["class"] = "MAGE", ["className"] = "Mage",
["level"] = 70, ["spec"] = "Arcane", ["talents"] = "40/21/0", ["money"] = 12340000,
["professions"] = { { ["name"] = "Tailoring", ["skill"] = 375, ["max"] = 375 } },
["reputations"] = { { ["name"] = "The Aldor", ["standing"] = 6, ["value"] = 4000, ["low"] = 3000, ["high"] = 9000 } },
},
},
}"#,
        );
        let c = &from_db(&db["LoadoutDB"]).characters[0];
        assert_eq!((c.spec.as_str(), c.talents.as_str()), ("Arcane", "40/21/0"));
        assert_eq!(
            character_line(c),
            "Testmage · 70 Arcane (40/21/0) Mage · 1,234g"
        );
        assert!(detail_lines(c, 0).contains(&"Tailoring 375/375".to_string()));
    }

    #[test]
    fn a_zero_item_level_from_an_old_copy_is_unknown_not_zero() {
        let db = parse_saved(
            "LoadoutDB = { [\"characters\"] = { [\"A-B\"] = { [\"name\"] = \"A\", [\"level\"] = 90, [\"itemLevel\"] = 0 } } }",
        );
        let c = &from_db(&db["LoadoutDB"]).characters[0];
        assert_eq!(c.item_level, None);
        assert!(!character_line(c).contains("ilvl"));
    }
}
