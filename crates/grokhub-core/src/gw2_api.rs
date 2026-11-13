//! Guild Wars 2 through ArenaNet's official web API
//! (`https://api.guildwars2.com/v2`), read with an API key the user makes on
//! their ArenaNet account.
//!
//! Nothing here touches the game: no client add-on, overlay, MumbleLink or
//! process read, since ArenaNet's third-party policy covers client changes.
//! The key only reads. `/v2/tokeninfo` comes first, and only the endpoints
//! the key's scopes allow are asked for. This module is pure: the app hands
//! in the one function that sends a GET, so tests run with no network. The
//! key never goes into a [`Gw2Account`], a card or anything saved.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Guild Wars 2 on Steam.
pub const APP_ID: u32 = 1284210;
pub const NAME: &str = "Guild Wars 2";
pub const API_BASE: &str = "https://api.guildwars2.com";
/// Where the user makes a key.
pub const KEY_PAGE: &str = "https://account.arena.net/applications";
/// The scopes Loadout uses. All read-only.
pub const SCOPES: [&str; 4] = ["account", "characters", "progression", "wallet"];
/// Biggest answer read from one endpoint (achievements is the big one).
pub const MAX_BODY: u64 = 4 * 1024 * 1024;
/// How often a snapshot is refreshed while Loadout runs.
pub const REFRESH_MS: u64 = 10 * 60 * 1000;

/// One endpoint, the scope it needs and its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    TokenInfo,
    Account,
    Characters,
    Wallet,
    Achievements,
    Raids,
}

impl Endpoint {
    pub fn path(self) -> &'static str {
        match self {
            Self::TokenInfo => "/v2/tokeninfo",
            Self::Account => "/v2/account?v=2019-12-19T00:00:00Z",
            Self::Characters => "/v2/characters?ids=all",
            Self::Wallet => "/v2/account/wallet",
            Self::Achievements => "/v2/account/achievements",
            Self::Raids => "/v2/account/raids",
        }
    }

    /// The scope besides `account` this endpoint needs.
    pub fn scope(self) -> Option<&'static str> {
        match self {
            Self::TokenInfo | Self::Account => None,
            Self::Characters => Some("characters"),
            Self::Wallet => Some("wallet"),
            Self::Achievements | Self::Raids => Some("progression"),
        }
    }

    pub fn url(self) -> String {
        format!("{API_BASE}{}", self.path())
    }
}

/// Does this look like a GW2 API key: 72 characters of hex in the
/// 8-4-4-4-20-4-4-4-12 pattern?
pub fn looks_like_key(key: &str) -> bool {
    let groups: Vec<&str> = key.trim().split('-').collect();
    let lens = [8, 4, 4, 4, 20, 4, 4, 4, 12];
    groups.len() == lens.len()
        && groups
            .iter()
            .zip(lens)
            .all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The scopes a key has, from `/v2/tokeninfo`.
pub fn parse_tokeninfo(text: &str) -> Option<Vec<String>> {
    let v: Value = serde_json::from_str(text).ok()?;
    let perms = v.get("permissions")?.as_array()?;
    Some(
        perms
            .iter()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect(),
    )
}

/// The endpoints a key with these scopes may read, in order.
pub fn plan(scopes: &[String]) -> Vec<Endpoint> {
    if !scopes.iter().any(|s| s == "account") {
        return Vec::new();
    }
    [
        Endpoint::Account,
        Endpoint::Characters,
        Endpoint::Wallet,
        Endpoint::Achievements,
        Endpoint::Raids,
    ]
    .into_iter()
    .filter(|e| e.scope().is_none_or(|s| scopes.iter().any(|k| k == s)))
    .collect()
}

/// One character.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Gw2Character {
    pub name: String,
    pub profession: Option<String>,
    pub level: Option<u64>,
    pub age_s: Option<u64>,
    pub deaths: Option<u64>,
}

/// What the key let Loadout read. Missing scopes and fields stay `None`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Gw2Account {
    pub name: Option<String>,
    pub age_s: Option<u64>,
    pub fractal_level: Option<u64>,
    pub wvw_rank: Option<u64>,
    pub characters: Option<Vec<Gw2Character>>,
    pub coin: Option<u64>,
    pub karma: Option<u64>,
    pub achievements_done: Option<u64>,
    pub raid_clears: Option<Vec<String>>,
    /// The scopes Loadout uses that the key didn't have.
    #[serde(default)]
    pub missing: Vec<String>,
    /// When it was read, in ms since the epoch.
    #[serde(default)]
    pub fetched_ms: u64,
}

fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k)?.as_u64()
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

impl Gw2Account {
    fn take_account(&mut self, v: &Value) {
        self.name = s(v, "name");
        self.age_s = u(v, "age");
        self.fractal_level = u(v, "fractal_level");
        self.wvw_rank = u(v, "wvw_rank").or_else(|| v.get("wvw").and_then(|w| u(w, "rank")));
    }

    fn take_characters(&mut self, v: &Value) {
        let Some(list) = v.as_array() else {
            return;
        };
        let mut chars: Vec<Gw2Character> = list
            .iter()
            .filter_map(|c| {
                Some(Gw2Character {
                    name: s(c, "name")?,
                    profession: s(c, "profession"),
                    level: u(c, "level"),
                    age_s: u(c, "age"),
                    deaths: u(c, "deaths"),
                })
            })
            .collect();
        chars.sort_by(|a, b| b.age_s.cmp(&a.age_s).then_with(|| a.name.cmp(&b.name)));
        self.characters = Some(chars);
    }

    fn take_wallet(&mut self, v: &Value) {
        let Some(list) = v.as_array() else {
            return;
        };
        let value = |id: u64| {
            list.iter()
                .find(|c| u(c, "id") == Some(id))
                .and_then(|c| u(c, "value"))
        };
        // Currency 1 is coin (in copper), 2 is karma.
        self.coin = Some(value(1).unwrap_or(0));
        self.karma = Some(value(2).unwrap_or(0));
    }

    fn take_achievements(&mut self, v: &Value) {
        if let Some(list) = v.as_array() {
            let done = list
                .iter()
                .filter(|a| a.get("done").and_then(Value::as_bool) == Some(true))
                .count();
            self.achievements_done = Some(done as u64);
        }
    }

    fn take_raids(&mut self, v: &Value) {
        if let Some(list) = v.as_array() {
            self.raid_clears = Some(
                list.iter()
                    .filter_map(|e| e.as_str().map(str::to_owned))
                    .collect(),
            );
        }
    }

    /// Feed one endpoint's answer in. A body that isn't JSON changes nothing.
    pub fn take(&mut self, endpoint: Endpoint, text: &str) {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return;
        };
        match endpoint {
            Endpoint::TokenInfo => {}
            Endpoint::Account => self.take_account(&v),
            Endpoint::Characters => self.take_characters(&v),
            Endpoint::Wallet => self.take_wallet(&v),
            Endpoint::Achievements => self.take_achievements(&v),
            Endpoint::Raids => self.take_raids(&v),
        }
    }

    /// `142 days old, fractal level 62, WvW rank 1,204. 4 characters, most
    /// played a level 80 Guardian. 1,234g 56s 78c, 2,100,000 karma. 812
    /// achievements done. 3 raid bosses cleared this week.`
    pub fn account_line(&self) -> String {
        let mut out = Vec::new();
        let mut first = Vec::new();
        if let Some(a) = self.age_s {
            first.push(format!("{} hours played", thousands(a / 3600)));
        }
        if let Some(f) = self.fractal_level.filter(|f| *f > 0) {
            first.push(format!("fractal level {f}"));
        }
        if let Some(r) = self.wvw_rank.filter(|r| *r > 0) {
            first.push(format!("WvW rank {}", thousands(r)));
        }
        if !first.is_empty() {
            out.push(first.join(", "));
        }
        if let Some(chars) = &self.characters {
            let n = chars.len();
            let mut line = format!("{n} {}", if n == 1 { "character" } else { "characters" });
            if let Some(top) = chars.first() {
                let mut what = Vec::new();
                if let Some(l) = top.level {
                    what.push(format!("level {l}"));
                }
                if let Some(p) = &top.profession {
                    what.push(p.clone());
                }
                if !what.is_empty() {
                    line.push_str(&format!(", most played a {}", what.join(" ")));
                }
            }
            out.push(line);
        }
        if let Some(c) = self.coin {
            let mut line = gold(c);
            if let Some(k) = self.karma.filter(|k| *k > 0) {
                line.push_str(&format!(", {} karma", thousands(k)));
            }
            out.push(line);
        }
        if let Some(a) = self.achievements_done {
            out.push(format!("{} achievements done", thousands(a)));
        }
        if let Some(r) = &self.raid_clears {
            out.push(match r.len() {
                0 => "No raid bosses cleared this week".into(),
                1 => "1 raid boss cleared this week".into(),
                n => format!("{n} raid bosses cleared this week"),
            });
        }
        if out.is_empty() {
            return "Nothing read yet.".into();
        }
        let mut line = out
            .into_iter()
            .map(|p| {
                let mut c = p.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => p,
                }
            })
            .collect::<Vec<_>>()
            .join(". ");
        line.push('.');
        line
    }
}

/// `1,234g 56s 78c`, from copper.
pub fn gold(copper: u64) -> String {
    let (g, s, c) = (copper / 10_000, copper / 100 % 100, copper % 100);
    if g > 0 {
        format!("{}g {s}s {c}c", thousands(g))
    } else if s > 0 {
        format!("{s}s {c}c")
    } else {
        format!("{c}c")
    }
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

/// Why one GET failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetError {
    /// The API answered with this status; `retry_after_s` from a 429.
    Status {
        code: u16,
        retry_after_s: Option<u64>,
    },
    /// Refused before sending (Offline only, the egress guard) or no answer.
    Other(String),
}

/// Why a refresh failed, said plainly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The key was refused or has no `account` scope.
    BadKey(String),
    /// Too many requests: wait this long before trying again.
    RateLimited {
        retry_after_s: u64,
    },
    Other(String),
}

impl FetchError {
    pub fn message(&self) -> String {
        match self {
            Self::BadKey(m) | Self::Other(m) => m.clone(),
            Self::RateLimited { retry_after_s } => format!(
                "ArenaNet's API asked Loadout to slow down, so it waits {} before trying again.",
                if *retry_after_s >= 60 {
                    format!("{} minutes", retry_after_s.div_ceil(60))
                } else {
                    format!("{retry_after_s} seconds")
                }
            ),
        }
    }
}

fn map_err(e: GetError) -> FetchError {
    match e {
        GetError::Status {
            code: 401 | 403, ..
        } => FetchError::BadKey(
            "ArenaNet refused that key. Make a new one with the account scope.".into(),
        ),
        GetError::Status {
            code: 429,
            retry_after_s,
        } => FetchError::RateLimited {
            retry_after_s: retry_after_s.unwrap_or(60).clamp(10, 3600),
        },
        GetError::Status { code, .. } => {
            FetchError::Other(format!("ArenaNet's API answered {code}; try again later."))
        }
        GetError::Other(m) => FetchError::Other(m),
    }
}

/// Read the account: tokeninfo first, then only what the key allows.
/// `get` sends one GET to a full URL and hands back the body. An endpoint
/// other than tokeninfo or account that fails is skipped, not fatal,
/// except a rate limit, which stops the refresh.
pub fn fetch_account(
    get: &mut dyn FnMut(&str) -> Result<String, GetError>,
    now_ms: u64,
) -> Result<Gw2Account, FetchError> {
    let info = get(&Endpoint::TokenInfo.url()).map_err(map_err)?;
    let scopes = parse_tokeninfo(&info)
        .ok_or_else(|| FetchError::Other("ArenaNet's API sent something unexpected.".into()))?;
    let endpoints = plan(&scopes);
    if endpoints.is_empty() {
        return Err(FetchError::BadKey(
            "That key can't read your account. Make one with the account scope.".into(),
        ));
    }
    let mut acct = Gw2Account {
        missing: SCOPES
            .iter()
            .filter(|s| !scopes.iter().any(|k| k == *s))
            .map(|s| s.to_string())
            .collect(),
        fetched_ms: now_ms,
        ..Default::default()
    };
    for e in endpoints {
        match get(&e.url()) {
            Ok(body) => acct.take(e, &body),
            Err(err) => match map_err(err) {
                stop @ FetchError::RateLimited { .. } => return Err(stop),
                err if e == Endpoint::Account => return Err(err),
                _ => {}
            },
        }
    }
    Ok(acct)
}

/// Where the last good snapshot is kept (no key in it).
pub fn cache_path(config_dir: &Path) -> PathBuf {
    config_dir.join("games").join("gw2-account.json")
}

pub fn load_cache(config_dir: &Path) -> Option<Gw2Account> {
    let path = cache_path(config_dir);
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > 256 * 1024 {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn save_cache(config_dir: &Path, acct: &Gw2Account) -> std::io::Result<()> {
    let path = cache_path(config_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string(acct).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // Shapes from ArenaNet's API docs, with made-up values.
    const TOKEN_ALL: &str = r#"{"id":"0000","name":"Loadout","permissions":["account","characters","progression","wallet","builds"]}"#;
    const TOKEN_ACCOUNT: &str = r#"{"id":"0000","name":"Loadout","permissions":["account"]}"#;
    const ACCOUNT: &str = r#"{"id":"A-B","name":"Tester.1234","age":7200000,"world":1001,"fractal_level":62,"wvw_rank":1204,"commander":false}"#;
    const CHARACTERS: &str = r#"[{"name":"Alt One","profession":"Thief","level":40,"age":36000,"deaths":3},{"name":"Main Guard","profession":"Guardian","level":80,"age":3600000,"deaths":120}]"#;
    const WALLET: &str =
        r#"[{"id":1,"value":12345678},{"id":2,"value":2100000},{"id":4,"value":400}]"#;
    const ACHIEVEMENTS: &str = r#"[{"id":1,"current":5,"max":5,"done":true},{"id":2,"current":1,"max":3,"done":false},{"id":3,"done":true}]"#;
    const RAIDS: &str = r#"["vale_guardian","spirit_woods","gorseval"]"#;
    const KEY: &str = "ABCDEF01-2345-6789-ABCD-EF0123456789ABCDEF01-2345-6789-ABCD-EF0123456789";

    fn server(
        routes: &[(Endpoint, Result<&'static str, GetError>)],
    ) -> (HashMap<String, Result<String, GetError>>, Vec<String>) {
        let map = routes
            .iter()
            .map(|(e, r)| (e.url(), r.clone().map(str::to_owned)))
            .collect();
        (map, Vec::new())
    }

    fn run(
        routes: &[(Endpoint, Result<&'static str, GetError>)],
    ) -> (Result<Gw2Account, FetchError>, Vec<String>) {
        let (map, mut asked) = server(routes);
        let got = fetch_account(
            &mut |url| {
                asked.push(url.to_owned());
                map.get(url).cloned().unwrap_or(Err(GetError::Status {
                    code: 404,
                    retry_after_s: None,
                }))
            },
            5,
        );
        (got, asked)
    }

    #[test]
    fn keys_are_checked_for_shape_only() {
        assert!(looks_like_key(KEY));
        assert!(looks_like_key(&format!("  {KEY}\n")));
        assert!(!looks_like_key("hunter2"));
        assert!(!looks_like_key(&KEY.replace('A', "G")));
    }

    #[test]
    fn a_full_key_reads_everything_and_says_it_plainly() {
        let (got, asked) = run(&[
            (Endpoint::TokenInfo, Ok(TOKEN_ALL)),
            (Endpoint::Account, Ok(ACCOUNT)),
            (Endpoint::Characters, Ok(CHARACTERS)),
            (Endpoint::Wallet, Ok(WALLET)),
            (Endpoint::Achievements, Ok(ACHIEVEMENTS)),
            (Endpoint::Raids, Ok(RAIDS)),
        ]);
        let acct = got.unwrap();
        assert_eq!(asked.len(), 6);
        assert!(asked
            .iter()
            .all(|u| u.starts_with("https://api.guildwars2.com/v2/")));
        assert!(asked.iter().all(|u| !u.contains(KEY)));
        assert!(acct.missing.is_empty());
        assert_eq!(acct.fetched_ms, 5);
        assert_eq!(
            acct.account_line(),
            "2,000 hours played, fractal level 62, WvW rank 1,204. 2 characters, most played a level 80 Guardian. 1,234g 56s 78c, 2,100,000 karma. 2 achievements done. 3 raid bosses cleared this week."
        );
    }

    #[test]
    fn a_key_without_a_scope_never_asks_for_that_endpoint() {
        let (got, asked) = run(&[
            (Endpoint::TokenInfo, Ok(TOKEN_ACCOUNT)),
            (Endpoint::Account, Ok(ACCOUNT)),
        ]);
        let acct = got.unwrap();
        assert_eq!(
            asked,
            vec![Endpoint::TokenInfo.url(), Endpoint::Account.url()]
        );
        assert_eq!(acct.characters, None);
        assert_eq!(acct.coin, None);
        assert_eq!(acct.missing, vec!["characters", "progression", "wallet"]);
        assert_eq!(
            acct.account_line(),
            "2,000 hours played, fractal level 62, WvW rank 1,204."
        );
        let no_account = r#"{"permissions":["characters"]}"#;
        let (got, _) = run(&[(Endpoint::TokenInfo, Ok(no_account))]);
        assert!(matches!(got, Err(FetchError::BadKey(_))));
    }

    #[test]
    fn refused_keys_rate_limits_and_side_failures_are_told_apart() {
        let refused = GetError::Status {
            code: 401,
            retry_after_s: None,
        };
        let (got, _) = run(&[(Endpoint::TokenInfo, Err(refused))]);
        assert!(matches!(got, Err(FetchError::BadKey(_))));

        let slow = GetError::Status {
            code: 429,
            retry_after_s: Some(120),
        };
        let (got, asked) = run(&[
            (Endpoint::TokenInfo, Ok(TOKEN_ALL)),
            (Endpoint::Account, Ok(ACCOUNT)),
            (Endpoint::Characters, Err(slow)),
        ]);
        let err = got.unwrap_err();
        assert_eq!(err, FetchError::RateLimited { retry_after_s: 120 });
        assert!(err.message().contains("2 minutes"));
        assert_eq!(asked.len(), 3);

        // Achievements failing only drops achievements.
        let (got, _) = run(&[
            (Endpoint::TokenInfo, Ok(TOKEN_ALL)),
            (Endpoint::Account, Ok(ACCOUNT)),
            (Endpoint::Characters, Ok(CHARACTERS)),
            (Endpoint::Wallet, Ok(WALLET)),
            (Endpoint::Raids, Ok(RAIDS)),
        ]);
        let acct = got.unwrap();
        assert_eq!(acct.achievements_done, None);
        assert_eq!(acct.raid_clears.as_ref().map(Vec::len), Some(3));
    }

    #[test]
    fn newer_account_schema_and_odd_bodies_are_handled() {
        let mut a = Gw2Account::default();
        a.take(
            Endpoint::Account,
            r#"{"name":"X.1","age":60,"wvw":{"rank":7}}"#,
        );
        assert_eq!(a.wvw_rank, Some(7));
        a.take(Endpoint::Characters, "not json");
        assert_eq!(a.characters, None);
        a.take(Endpoint::Wallet, "[]");
        assert_eq!(gold(a.coin.unwrap()), "0c");
        assert_eq!(gold(507), "5s 7c");
    }

    #[test]
    fn the_snapshot_round_trips_without_the_key() {
        let dir = std::env::temp_dir().join(format!("loadout-gw2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = Gw2Account {
            fetched_ms: 9,
            ..Default::default()
        };
        a.take(Endpoint::Account, ACCOUNT);
        save_cache(&dir, &a).unwrap();
        assert_eq!(load_cache(&dir), Some(a));
        let text = std::fs::read_to_string(cache_path(&dir)).unwrap();
        assert!(!text.contains(KEY));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
