//! Destiny 2 through Bungie.net's official web API
//! (`https://www.bungie.net/Platform`), read with an API key from an
//! application the user registers on Bungie.net themselves.
//!
//! Bungie's terms make a key personal, so Loadout ships none. Only public
//! profile data is read (no sign-in): the player found by Bungie Name, their
//! characters, triumph score and lifetime PvE and PvP stats. Nothing touches
//! the game or BattlEye. This module is pure: the app hands in the function
//! that sends a request, so tests run with no network. The key never goes
//! into a [`DestinyProfile`], a card or anything saved.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Destiny 2 on Steam.
pub const APP_ID: u32 = 1085660;
pub const NAME: &str = "Destiny 2";
pub const API_BASE: &str = "https://www.bungie.net/Platform";
/// Where the user registers an application for a key.
pub const KEY_PAGE: &str = "https://www.bungie.net/en/Application";
/// Biggest answer read from one call.
pub const MAX_BODY: u64 = 2 * 1024 * 1024;
/// How often a snapshot is refreshed while Loadout runs.
pub const REFRESH_MS: u64 = 15 * 60 * 1000;

/// `Name#1234` split into its two parts.
pub fn parse_bungie_name(name: &str) -> Option<(String, u32)> {
    let (n, code) = name.trim().rsplit_once('#')?;
    let n = n.trim();
    let code = code.trim();
    if n.is_empty() || n.chars().count() > 26 || code.len() != 4 {
        return None;
    }
    Some((n.to_owned(), code.parse().ok()?))
}

/// An API key from Bungie.net is 32 hex characters.
pub fn looks_like_key(key: &str) -> bool {
    let k = key.trim();
    k.len() == 32 && k.chars().all(|c| c.is_ascii_hexdigit())
}

/// One request: a GET, or a POST with a JSON body.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub url: String,
    pub body: Option<Value>,
}

pub fn search_call(name: &str, code: u32) -> Call {
    Call {
        url: format!("{API_BASE}/Destiny2/SearchDestinyPlayerByBungieName/-1/"),
        body: Some(json!({ "displayName": name, "displayNameCode": code })),
    }
}

pub fn profile_call(membership_type: i64, membership_id: &str) -> Call {
    Call {
        url: format!(
            "{API_BASE}/Destiny2/{membership_type}/Profile/{membership_id}/?components=100,200,900"
        ),
        body: None,
    }
}

pub fn stats_call(membership_type: i64, membership_id: &str) -> Call {
    Call {
        url: format!(
            "{API_BASE}/Destiny2/{membership_type}/Account/{membership_id}/Stats/?groups=1"
        ),
        body: None,
    }
}

/// Bungie's envelope: `Response` when `ErrorCode` is 1, otherwise its
/// message. `ThrottleSeconds` is handed back either way.
pub fn unwrap_envelope(text: &str) -> Result<(Value, u64), FetchError> {
    let v: Value = serde_json::from_str(text)
        .map_err(|_| FetchError::Other("Bungie.net sent something unexpected.".into()))?;
    let throttle = v
        .get("ThrottleSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    match v.get("ErrorCode").and_then(Value::as_i64) {
        Some(1) => Ok((v.get("Response").cloned().unwrap_or(Value::Null), throttle)),
        Some(code) => {
            let status = v.get("ErrorStatus").and_then(Value::as_str).unwrap_or("");
            let msg = v
                .get("Message")
                .and_then(Value::as_str)
                .unwrap_or("Bungie.net refused the request.");
            Err(match (code, status) {
                (2101..=2111, _) | (_, "ApiInvalidOrExpiredKey") | (_, "ApiKeyMissingFromRequest") => {
                    FetchError::BadKey(
                        "Bungie.net refused that API key. Check it on your Bungie.net application page."
                            .into(),
                    )
                }
                (5, _) | (_, "SystemDisabled") => FetchError::Other(
                    "Bungie.net is down for maintenance; Loadout tries again later.".into(),
                ),
                (36 | 51, _) | (_, "ThrottleLimitExceeded") => FetchError::RateLimited {
                    retry_after_s: throttle.max(60),
                },
                _ => FetchError::Other(format!("Bungie.net: {msg}")),
            })
        }
        None => Err(FetchError::Other(
            "Bungie.net sent something unexpected.".into(),
        )),
    }
}

/// One character.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DestinyCharacter {
    pub class: String,
    pub light: Option<u64>,
    pub minutes_played: u64,
    pub last_played: Option<String>,
}

/// Lifetime totals for PvE or PvP.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModeStats {
    pub kills: u64,
    pub deaths: u64,
    pub activities: u64,
    pub kd: Option<f64>,
}

/// What Bungie.net shows publicly. A component the player keeps private
/// stays `None` and its name goes in `hidden`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DestinyProfile {
    pub bungie_name: String,
    pub characters: Option<Vec<DestinyCharacter>>,
    pub triumph_score: Option<u64>,
    pub pve: Option<ModeStats>,
    pub pvp: Option<ModeStats>,
    #[serde(default)]
    pub hidden: Vec<String>,
    #[serde(default)]
    pub fetched_ms: u64,
}

fn class_name(t: i64) -> &'static str {
    match t {
        0 => "Titan",
        1 => "Hunter",
        2 => "Warlock",
        _ => "Guardian",
    }
}

fn num(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}

/// The account to read from a Bungie Name search: the cross-save primary
/// when there is one, else the first.
pub fn pick_membership(search: &Value) -> Option<(i64, String)> {
    let list = search.as_array()?;
    let parse = |m: &Value| {
        Some((
            m.get("membershipType")?.as_i64()?,
            m.get("membershipId")?.as_str()?.to_owned(),
        ))
    };
    list.iter()
        .find(|m| {
            let over = m
                .get("crossSaveOverride")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            over != 0 && Some(over) == m.get("membershipType").and_then(Value::as_i64)
        })
        .or_else(|| list.first())
        .and_then(parse)
}

impl DestinyProfile {
    /// Take GetProfile's `Response` (components 100, 200 and 900).
    pub fn take_profile(&mut self, r: &Value) {
        match r.pointer("/characters/data").and_then(Value::as_object) {
            Some(chars) => {
                let mut list: Vec<DestinyCharacter> = chars
                    .values()
                    .map(|c| DestinyCharacter {
                        class: class_name(c.get("classType").and_then(Value::as_i64).unwrap_or(-1))
                            .to_owned(),
                        light: c.get("light").and_then(num),
                        minutes_played: c.get("minutesPlayedTotal").and_then(num).unwrap_or(0),
                        last_played: c
                            .get("dateLastPlayed")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    })
                    .collect();
                list.sort_by(|a, b| b.last_played.cmp(&a.last_played));
                self.characters = Some(list);
            }
            None => self.hidden.push("characters".into()),
        }
        match r.pointer("/profileRecords/data") {
            Some(rec) => {
                self.triumph_score = rec
                    .get("activeScore")
                    .or_else(|| rec.get("score"))
                    .and_then(num)
            }
            None => self.hidden.push("triumphs".into()),
        }
    }

    /// Take the account stats `Response` (`groups=1`, all characters).
    pub fn take_stats(&mut self, r: &Value) {
        let mode = |key: &str| -> Option<ModeStats> {
            let t = r.pointer(&format!("/mergedAllCharacters/results/{key}/allTime"))?;
            let val = |k: &str| {
                t.pointer(&format!("/{k}/basic/value"))
                    .and_then(Value::as_f64)
            };
            Some(ModeStats {
                kills: val("kills").unwrap_or(0.0) as u64,
                deaths: val("deaths").unwrap_or(0.0) as u64,
                activities: val("activitiesCleared")
                    .or_else(|| val("activitiesEntered"))
                    .unwrap_or(0.0) as u64,
                kd: val("killsDeathsRatio"),
            })
        };
        self.pve = mode("allPvE");
        self.pvp = mode("allPvP");
    }

    /// `3 characters, last played a 2010 Light Warlock. 412 hours played.
    /// Triumph score 98,200. PvE: 120,400 kills, 900 activities. PvP: 1.24 K/D
    /// over 3,100 kills.`
    pub fn profile_line(&self) -> String {
        let mut out = Vec::new();
        if let Some(chars) = &self.characters {
            let n = chars.len();
            let mut line = format!("{n} {}", if n == 1 { "character" } else { "characters" });
            if let Some(c) = chars.first() {
                match c.light {
                    Some(l) => line.push_str(&format!(", last played a {l} Light {}", c.class)),
                    None => line.push_str(&format!(", last played a {}", c.class)),
                }
            }
            out.push(line);
            let mins: u64 = chars.iter().map(|c| c.minutes_played).sum();
            if mins > 0 {
                out.push(format!("{} hours played", thousands(mins / 60)));
            }
        }
        if let Some(s) = self.triumph_score {
            out.push(format!("Triumph score {}", thousands(s)));
        }
        if let Some(p) = self
            .pve
            .as_ref()
            .filter(|p| p.kills > 0 || p.activities > 0)
        {
            out.push(format!(
                "PvE: {} kills, {} activities cleared",
                thousands(p.kills),
                thousands(p.activities)
            ));
        }
        if let Some(p) = self.pvp.as_ref().filter(|p| p.kills > 0) {
            let kd =
                p.kd.map(|k| format!("{k:.2} K/D over "))
                    .unwrap_or_default();
            out.push(format!("PvP: {kd}{} kills", thousands(p.kills)));
        }
        if out.is_empty() {
            return "Nothing public to read yet.".into();
        }
        out.join(". ") + "."
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

/// Why one request failed before Bungie.net's envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    Status {
        code: u16,
        retry_after_s: Option<u64>,
    },
    Other(String),
}

/// Why a refresh failed, said plainly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    BadKey(String),
    NotFound(String),
    RateLimited { retry_after_s: u64 },
    Other(String),
}

impl FetchError {
    pub fn message(&self) -> String {
        match self {
            Self::BadKey(m) | Self::NotFound(m) | Self::Other(m) => m.clone(),
            Self::RateLimited { retry_after_s } => format!(
                "Bungie.net asked Loadout to slow down, so it waits {} before trying again.",
                if *retry_after_s >= 60 {
                    format!("{} minutes", retry_after_s.div_ceil(60))
                } else {
                    format!("{retry_after_s} seconds")
                }
            ),
        }
    }
}

/// Send one call and open the envelope. Bungie answers most errors with a
/// JSON envelope even on a 4xx/5xx, so the app passes those bodies through
/// as `Ok`; a bare status here means there was no envelope.
fn send(
    send: &mut dyn FnMut(&Call) -> Result<String, SendError>,
    call: &Call,
) -> Result<Value, FetchError> {
    let body = send(call).map_err(|e| match e {
        SendError::Status {
            code: 401 | 403, ..
        } => FetchError::BadKey(
            "Bungie.net refused that API key. Check it on your Bungie.net application page.".into(),
        ),
        SendError::Status {
            code: 429,
            retry_after_s,
        } => FetchError::RateLimited {
            retry_after_s: retry_after_s.unwrap_or(60).clamp(10, 3600),
        },
        SendError::Status { code, .. } => {
            FetchError::Other(format!("Bungie.net answered {code}; try again later."))
        }
        SendError::Other(m) => FetchError::Other(m),
    })?;
    unwrap_envelope(&body).map(|(v, _)| v)
}

/// Find the player by Bungie Name, then read their public profile and
/// stats. A stats call that fails only drops the stats, except a refused
/// key or a rate limit.
pub fn fetch_profile(
    sender: &mut dyn FnMut(&Call) -> Result<String, SendError>,
    bungie_name: &str,
    now_ms: u64,
) -> Result<DestinyProfile, FetchError> {
    let Some((name, code)) = parse_bungie_name(bungie_name) else {
        return Err(FetchError::NotFound(
            "Enter your Bungie Name with its four-digit code, like Guardian#1234.".into(),
        ));
    };
    let found = send(sender, &search_call(&name, code))?;
    let Some((mtype, mid)) = pick_membership(&found) else {
        return Err(FetchError::NotFound(format!(
            "Bungie.net found no Destiny 2 player named {name}#{code:04}."
        )));
    };
    let mut p = DestinyProfile {
        bungie_name: format!("{name}#{code:04}"),
        fetched_ms: now_ms,
        ..Default::default()
    };
    let profile = send(sender, &profile_call(mtype, &mid))?;
    p.take_profile(&profile);
    match send(sender, &stats_call(mtype, &mid)) {
        Ok(stats) => p.take_stats(&stats),
        Err(e @ (FetchError::BadKey(_) | FetchError::RateLimited { .. })) => return Err(e),
        Err(_) => p.hidden.push("stats".into()),
    }
    Ok(p)
}

/// Where the last good snapshot is kept (no key in it).
pub fn cache_path(config_dir: &Path) -> PathBuf {
    config_dir.join("games").join("destiny2-profile.json")
}

pub fn load_cache(config_dir: &Path) -> Option<DestinyProfile> {
    let path = cache_path(config_dir);
    if std::fs::metadata(&path).ok()?.len() > 256 * 1024 {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn save_cache(config_dir: &Path, p: &DestinyProfile) -> std::io::Result<()> {
    let path = cache_path(config_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string(p).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes from Bungie.net's API docs, made-up player.
    const SEARCH: &str = r#"{"Response":[{"iconPath":"/img.png","crossSaveOverride":3,"applicableMembershipTypes":[3,2],"membershipType":2,"membershipId":"4611686018400000001","displayName":"Tester","bungieGlobalDisplayName":"Tester","bungieGlobalDisplayNameCode":1234},{"crossSaveOverride":3,"membershipType":3,"membershipId":"4611686018400000002","displayName":"Tester"}],"ErrorCode":1,"ThrottleSeconds":0,"ErrorStatus":"Success","Message":"Ok","MessageData":{}}"#;
    const PROFILE: &str = r#"{"Response":{"profile":{"data":{"dateLastPlayed":"2026-10-30T20:00:00Z","characterIds":["1","2"]},"privacy":1},"characters":{"data":{"1":{"classType":2,"light":2010,"minutesPlayedTotal":"18000","dateLastPlayed":"2026-10-30T20:00:00Z"},"2":{"classType":0,"light":1990,"minutesPlayedTotal":"6720","dateLastPlayed":"2026-09-01T10:00:00Z"}},"privacy":1},"profileRecords":{"data":{"score":120000,"activeScore":98200,"legacyScore":21800},"privacy":1}},"ErrorCode":1,"ThrottleSeconds":0,"ErrorStatus":"Success","Message":"Ok"}"#;
    const PRIVATE_PROFILE: &str = r#"{"Response":{"profile":{"data":{},"privacy":1},"characters":{"privacy":2},"profileRecords":{"privacy":2}},"ErrorCode":1,"ThrottleSeconds":0,"ErrorStatus":"Success","Message":"Ok"}"#;
    const STATS: &str = r#"{"Response":{"mergedAllCharacters":{"results":{"allPvE":{"allTime":{"kills":{"basic":{"value":120400.0}},"deaths":{"basic":{"value":2100.0}},"activitiesCleared":{"basic":{"value":900.0}}}},"allPvP":{"allTime":{"kills":{"basic":{"value":3100.0}},"deaths":{"basic":{"value":2500.0}},"killsDeathsRatio":{"basic":{"value":1.24}}}}}}},"ErrorCode":1,"ThrottleSeconds":0,"ErrorStatus":"Success","Message":"Ok"}"#;
    const BAD_KEY: &str = r#"{"ErrorCode":2101,"ThrottleSeconds":0,"ErrorStatus":"ApiInvalidOrExpiredKey","Message":"Please use a valid key."}"#;
    const THROTTLED: &str = r#"{"ErrorCode":51,"ThrottleSeconds":90,"ErrorStatus":"PerEndpointRequestThrottleExceeded","Message":"Slow down"}"#;
    const MAINTENANCE: &str = r#"{"ErrorCode":5,"ThrottleSeconds":0,"ErrorStatus":"SystemDisabled","Message":"This system is temporarily disabled for maintenance."}"#;

    fn run(answers: &[(&str, &'static str)]) -> (Result<DestinyProfile, FetchError>, Vec<Call>) {
        let mut asked = Vec::new();
        let got = fetch_profile(
            &mut |call| {
                asked.push(call.clone());
                answers
                    .iter()
                    .find(|(part, _)| call.url.contains(part))
                    .map(|(_, body)| body.to_string())
                    .ok_or(SendError::Status {
                        code: 404,
                        retry_after_s: None,
                    })
            },
            "Tester#1234",
            7,
        );
        (got, asked)
    }

    #[test]
    fn names_and_keys_are_checked_for_shape() {
        assert_eq!(
            parse_bungie_name(" Tester#1234 "),
            Some(("Tester".into(), 1234))
        );
        assert_eq!(
            parse_bungie_name("Two Words#0042"),
            Some(("Two Words".into(), 42))
        );
        assert_eq!(parse_bungie_name("Tester"), None);
        assert_eq!(parse_bungie_name("Tester#12"), None);
        assert!(looks_like_key("0123456789abcdef0123456789ABCDEF"));
        assert!(!looks_like_key("hunter2"));
    }

    #[test]
    fn a_public_profile_reads_plainly() {
        let (got, asked) = run(&[
            ("SearchDestinyPlayerByBungieName", SEARCH),
            ("/Profile/", PROFILE),
            ("/Stats/", STATS),
        ]);
        let p = got.unwrap();
        assert_eq!(
            asked[0].body,
            Some(json!({"displayName":"Tester","displayNameCode":1234}))
        );
        // The cross-save primary, Steam (3), is read.
        assert!(asked[1]
            .url
            .contains("/Destiny2/3/Profile/4611686018400000002/"));
        assert!(asked.iter().all(|c| c.url.starts_with(API_BASE)));
        assert_eq!(p.bungie_name, "Tester#1234");
        assert!(p.hidden.is_empty());
        assert_eq!(
            p.profile_line(),
            "2 characters, last played a 2010 Light Warlock. 412 hours played. Triumph score 98,200. PvE: 120,400 kills, 900 activities cleared. PvP: 1.24 K/D over 3,100 kills."
        );
    }

    #[test]
    fn private_components_are_named_not_fatal() {
        let (got, _) = run(&[
            ("SearchDestinyPlayerByBungieName", SEARCH),
            ("/Profile/", PRIVATE_PROFILE),
        ]);
        let p = got.unwrap();
        assert_eq!(p.hidden, vec!["characters", "triumphs", "stats"]);
        assert_eq!(p.profile_line(), "Nothing public to read yet.");
    }

    #[test]
    fn errors_say_what_happened() {
        let (got, _) = run(&[("SearchDestinyPlayerByBungieName", BAD_KEY)]);
        assert!(matches!(got, Err(FetchError::BadKey(_))));
        let (got, _) = run(&[("SearchDestinyPlayerByBungieName", THROTTLED)]);
        let e = got.unwrap_err();
        assert_eq!(e, FetchError::RateLimited { retry_after_s: 90 });
        assert!(e.message().contains("2 minutes"));
        let (got, _) = run(&[("SearchDestinyPlayerByBungieName", MAINTENANCE)]);
        assert!(got.unwrap_err().message().contains("maintenance"));
        let none = r#"{"Response":[],"ErrorCode":1,"ThrottleSeconds":0}"#;
        let (got, _) = run(&[("SearchDestinyPlayerByBungieName", none)]);
        assert!(matches!(got, Err(FetchError::NotFound(_))));
        assert!(matches!(
            fetch_profile(&mut |_| unreachable!(), "nobody", 0),
            Err(FetchError::NotFound(_))
        ));
    }

    #[test]
    fn the_snapshot_round_trips() {
        let dir = std::env::temp_dir().join(format!("loadout-destiny-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut p = DestinyProfile {
            bungie_name: "Tester#1234".into(),
            ..Default::default()
        };
        p.take_profile(&unwrap_envelope(PROFILE).unwrap().0);
        save_cache(&dir, &p).unwrap();
        assert_eq!(load_cache(&dir), Some(p));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
