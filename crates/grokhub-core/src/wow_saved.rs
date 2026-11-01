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
#[derive(Debug, Clone, PartialEq)]
pub struct WowCharacter {
    pub name: String,
    pub realm: String,
    /// Blizzard's class token (`PALADIN`), for colors.
    pub class: String,
    /// The class's display name in the game's language.
    pub class_name: String,
    pub level: u32,
    pub zone: String,
    pub money_copper: u64,
    pub item_level: Option<f64>,
    pub played_secs: Option<u64>,
    pub deaths: u64,
    pub last_seen: u64,
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

/// The characters and events in one `LoadoutDB` value.
pub fn from_db(db: &Value) -> WowSaved {
    let mut out = WowSaved::default();
    if let Some(chars) = db.get("characters").and_then(Value::as_object) {
        for c in chars.values() {
            out.characters.push(WowCharacter {
                name: s(c, "name"),
                realm: s(c, "realm"),
                class: s(c, "class"),
                class_name: s(c, "className"),
                level: u(c, "level")
                    .and_then(|l| u32::try_from(l).ok())
                    .unwrap_or(0),
                zone: s(c, "zone"),
                money_copper: u(c, "money").unwrap_or(0),
                item_level: c.get("itemLevel").and_then(Value::as_f64),
                played_secs: u(c, "played"),
                deaths: u(c, "deaths").unwrap_or(0),
                last_seen: u(c, "lastSeen").unwrap_or(0),
            });
        }
    }
    let events: Vec<&Value> = match db.get("events") {
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
    };
    for e in events {
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
    let Ok(rd) = std::fs::read_dir(flavor_dir.join("WTF").join("Account")) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path().join("SavedVariables").join("Loadout.lua"))
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
    parts.push(format!("{} {}", c.level, class).trim().to_owned());
    if !c.zone.is_empty() {
        parts.push(c.zone.clone());
    }
    parts.push(gold_label(c.money_copper));
    if let Some(ilvl) = c.item_level {
        parts.push(format!("ilvl {}", ilvl.round() as u64));
    }
    parts.join(" · ")
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
}
