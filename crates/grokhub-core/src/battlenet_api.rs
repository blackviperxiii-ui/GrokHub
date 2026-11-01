//! Blizzard's Battle.net profile API for World of Warcraft: your own
//! characters, collections, equipment and raid and Mythic+ progress, read
//! from Blizzard's web API so they show without the game running.
//!
//! Blizzard only hands out profile data through the OAuth authorization
//! code flow with the `wow.profile` scope. It has no PKCE and no refresh
//! tokens: the token exchange needs the developer client's secret, and the
//! token lasts about a day. So the player registers their own client on
//! Blizzard's developer site (like Destiny 2's key); the app keeps its
//! secret and the token in the system keychain and nothing here touches
//! either beyond building requests. The snapshot cached on disk holds no
//! secret and is dropped after 30 days, as Blizzard's API terms ask.
//!
//! No HTTP here: the app sends each [`Call`] and hands the body back, so
//! everything is tested with fixtures. Nothing touches the game.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pkce::{ct_eq, form, urlencode};

pub const NAME: &str = "Battle.net";
pub const AUTHORIZE_URL: &str = "https://oauth.battle.net/authorize";
pub const TOKEN_URL: &str = "https://oauth.battle.net/token";
pub const SCOPE: &str = "wow.profile";
/// The loopback port the app listens on for Blizzard's redirect. Fixed,
/// because Blizzard only redirects to an address registered on the client.
pub const LOOPBACK_PORT: u16 = 47615;
pub const REDIRECT_URI: &str = "http://localhost:47615/callback";
/// Where a player registers their own client.
pub const CLIENT_PAGE: &str = "https://develop.battle.net/access/clients";
/// Most bytes one answer may take.
pub const MAX_BODY: u64 = 2 * 1024 * 1024;
/// How often a connected profile is read again.
pub const REFRESH_MS: u64 = 30 * 60 * 1000;
/// Blizzard's API terms: data kept no longer than 30 days.
pub const KEEP_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// Characters read in detail (gear, raids, Mythic+), highest level first.
pub const DETAILED: usize = 3;
/// Characters listed at most.
const MAX_CHARACTERS: usize = 50;
/// A token this close to its end counts as ended.
const TOKEN_MARGIN_MS: u64 = 5 * 60 * 1000;

/// The Battle.net region a profile lives in. China has its own hosts and
/// isn't offered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    #[default]
    Us,
    Eu,
    Kr,
    Tw,
}

impl Region {
    pub const ALL: [Region; 4] = [Region::Us, Region::Eu, Region::Kr, Region::Tw];

    pub fn code(self) -> &'static str {
        match self {
            Region::Us => "us",
            Region::Eu => "eu",
            Region::Kr => "kr",
            Region::Tw => "tw",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Region::Us => "Americas",
            Region::Eu => "Europe",
            Region::Kr => "Korea",
            Region::Tw => "Taiwan",
        }
    }

    pub fn parse(s: &str) -> Option<Region> {
        Region::ALL
            .into_iter()
            .find(|r| r.code().eq_ignore_ascii_case(s.trim()))
    }

    /// The API host for this region.
    pub fn api_base(self) -> String {
        format!("https://{}.api.blizzard.com", self.code())
    }

    /// The locale names come back in: one each region serves.
    fn locale(self) -> &'static str {
        match self {
            Region::Us => "en_US",
            Region::Eu => "en_GB",
            Region::Kr => "ko_KR",
            Region::Tw => "zh_TW",
        }
    }
}

/// A developer client id is 32 hex characters; the secret is 32 letters
/// and digits. Checked loosely so a pasted value with a typo is caught.
pub fn looks_like_client_value(s: &str) -> bool {
    let s = s.trim();
    (20..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The page the system browser opens to ask the player for `wow.profile`.
pub fn authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let query = form(&[
        ("response_type", "code"),
        ("client_id", client_id.trim()),
        ("scope", SCOPE),
        ("state", state),
        ("redirect_uri", redirect_uri),
    ]);
    format!("{AUTHORIZE_URL}?{query}")
}

/// The token request's form body. The client id and secret go in the
/// `Authorization` header ([`basic_auth`]), as Blizzard asks.
pub fn token_form(code: &str, redirect_uri: &str) -> String {
    form(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
    ])
}

/// `Basic base64(id:secret)`.
pub fn basic_auth(client_id: &str, secret: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let data = format!("{}:{}", client_id.trim(), secret.trim()).into_bytes();
    let mut out = String::from("Basic ");
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(T[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn query_pairs(query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// The code from Blizzard's redirect: either the loopback request's first
/// line (`GET /callback?code=..&state=.. HTTP/1.1`) or the address the
/// player pasted from the browser bar (when the redirect page didn't load,
/// as with `https://localhost/callback`). The state must match.
pub fn callback_code(input: &str, expected_state: &str) -> Result<String, String> {
    let first = input.lines().next().unwrap_or("").trim();
    let target = match first.split_whitespace().collect::<Vec<_>>().as_slice() {
        [method, target, ..] if method.eq_ignore_ascii_case("GET") => (*target).to_string(),
        _ => first.to_string(),
    };
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let query = query.split('#').next().unwrap_or("");
    let q = query_pairs(query);
    let get = |key: &str| {
        q.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    };
    if q.is_empty() {
        return Err("That address has no sign-in answer in it. Copy the whole address from the browser bar after Battle.net sends you back.".into());
    }
    if !ct_eq(get("state"), expected_state) {
        return Err("That sign-in answer is from another attempt. Press Connect again.".into());
    }
    if !get("error").is_empty() {
        let why = Some(get("error_description"))
            .filter(|d| !d.is_empty())
            .unwrap_or(get("error"));
        return Err(format!("Battle.net didn't connect: {why}"));
    }
    let code = get("code");
    if code.is_empty() {
        return Err("Battle.net's answer had no code in it. Press Connect again.".into());
    }
    Ok(code.to_string())
}

/// The access token as kept in the keychain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    pub expires_at_ms: u64,
    pub region: Region,
}

impl Token {
    pub fn live(&self, now_ms: u64) -> bool {
        now_ms + TOKEN_MARGIN_MS < self.expires_at_ms
    }
}

/// The token endpoint's answer. Blizzard sends `access_token` and
/// `expires_in` (about a day); a refusal comes as `error`.
pub fn parse_token(body: &str, region: Region, now_ms: u64) -> Result<Token, String> {
    let v: Value = serde_json::from_str(body)
        .map_err(|_| "Battle.net's token answer wasn't readable.".to_string())?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        let why = v
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or(err);
        return Err(format!(
            "Battle.net refused the sign-in: {why}. Check the client id, secret and redirect URL."
        ));
    }
    let access_token = v
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .ok_or("Battle.net's token answer had no token.")?
        .to_string();
    let granted = v.get("scope").and_then(Value::as_str).unwrap_or(SCOPE);
    if !granted.split_whitespace().any(|s| s == SCOPE) {
        return Err(
            "You connected without World of Warcraft profile access. Connect again and keep it ticked."
                .into(),
        );
    }
    let secs = v
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(86_399);
    Ok(Token {
        access_token,
        expires_at_ms: now_ms + secs * 1000,
        region,
    })
}

/// One GET to the profile API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub url: String,
}

fn call(region: Region, path: &str) -> Call {
    Call {
        url: format!(
            "{}{path}?namespace=profile-{}&locale={}",
            region.api_base(),
            region.code(),
            region.locale()
        ),
    }
}

fn character_path(c: &BnetCharacter, tail: &str) -> String {
    format!(
        "/profile/wow/character/{}/{}{tail}",
        urlencode(&c.realm_slug),
        urlencode(&c.name.to_lowercase())
    )
}

pub fn account_call(region: Region) -> Call {
    call(region, "/profile/user/wow")
}

pub fn mounts_call(region: Region) -> Call {
    call(region, "/profile/user/wow/collections/mounts")
}

pub fn pets_call(region: Region) -> Call {
    call(region, "/profile/user/wow/collections/pets")
}

pub fn summary_call(region: Region, c: &BnetCharacter) -> Call {
    call(region, &character_path(c, ""))
}

pub fn equipment_call(region: Region, c: &BnetCharacter) -> Call {
    call(region, &character_path(c, "/equipment"))
}

pub fn raids_call(region: Region, c: &BnetCharacter) -> Call {
    call(region, &character_path(c, "/encounters/raids"))
}

pub fn mythic_call(region: Region, c: &BnetCharacter) -> Call {
    call(region, &character_path(c, "/mythic-keystone-profile"))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GearItem {
    pub slot: String,
    pub name: String,
    pub level: Option<u32>,
}

/// One raid of the newest expansion, at the hardest difficulty with a kill.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RaidProgress {
    pub name: String,
    pub difficulty: String,
    pub killed: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BnetCharacter {
    pub name: String,
    pub realm: String,
    pub realm_slug: String,
    pub level: u32,
    pub class: String,
    pub race: String,
    pub faction: String,
    pub item_level: Option<u32>,
    pub gear: Vec<GearItem>,
    pub raids: Vec<RaidProgress>,
    pub mythic_rating: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BnetProfile {
    pub region: Region,
    pub characters: Vec<BnetCharacter>,
    pub mounts: Option<u32>,
    pub pets: Option<u32>,
    pub fetched_ms: u64,
}

fn text(v: &Value, path: &[&str]) -> String {
    path.iter()
        .try_fold(v, |v, k| v.get(k))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn number(v: &Value, path: &[&str]) -> Option<u64> {
    path.iter().try_fold(v, |v, k| v.get(k))?.as_u64()
}

/// The characters on every WoW account of the profile, highest level first.
pub fn parse_account(v: &Value) -> Vec<BnetCharacter> {
    let mut out: Vec<BnetCharacter> = v
        .get("wow_accounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|a| a.get("characters").and_then(Value::as_array))
        .flatten()
        .filter_map(|c| {
            let name = text(c, &["name"]);
            let realm_slug = text(c, &["realm", "slug"]);
            if name.is_empty() || realm_slug.is_empty() {
                return None;
            }
            Some(BnetCharacter {
                name,
                realm: text(c, &["realm", "name"]),
                realm_slug,
                level: number(c, &["level"]).unwrap_or(0) as u32,
                class: text(c, &["playable_class", "name"]),
                race: text(c, &["playable_race", "name"]),
                faction: text(c, &["faction", "name"]),
                ..Default::default()
            })
        })
        .collect();
    out.sort_by(|a, b| b.level.cmp(&a.level).then_with(|| a.name.cmp(&b.name)));
    out.truncate(MAX_CHARACTERS);
    out
}

pub fn parse_gear(v: &Value) -> Vec<GearItem> {
    v.get("equipped_items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|i| {
            let name = text(i, &["name"]);
            (!name.is_empty()).then(|| GearItem {
                slot: text(i, &["slot", "name"]),
                name,
                level: number(i, &["level", "value"]).map(|l| l as u32),
            })
        })
        .collect()
}

/// The newest expansion's raids, each at its hardest difficulty with at
/// least one kill (or its first difficulty when none has one).
pub fn parse_raids(v: &Value) -> Vec<RaidProgress> {
    let Some(newest) = v
        .get("expansions")
        .and_then(Value::as_array)
        .and_then(|e| e.last())
    else {
        return Vec::new();
    };
    newest
        .get("instances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|inst| {
            let modes: Vec<RaidProgress> = inst
                .get("modes")
                .and_then(Value::as_array)?
                .iter()
                .map(|m| RaidProgress {
                    name: text(inst, &["instance", "name"]),
                    difficulty: text(m, &["difficulty", "name"]),
                    killed: number(m, &["progress", "completed_count"]).unwrap_or(0) as u32,
                    total: number(m, &["progress", "total_count"]).unwrap_or(0) as u32,
                })
                .collect();
            // Blizzard lists modes easiest first.
            modes
                .iter()
                .rev()
                .find(|m| m.killed > 0)
                .or(modes.first())
                .cloned()
        })
        .collect()
}

pub fn parse_rating(v: &Value) -> Option<f64> {
    v.get("current_mythic_rating")?
        .get("rating")?
        .as_f64()
        .filter(|r| *r > 0.0)
}

fn count(v: &Value, key: &str) -> Option<u32> {
    v.get(key).and_then(Value::as_array).map(|a| a.len() as u32)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    Status {
        code: u16,
        retry_after_s: Option<u64>,
    },
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The token ended or was revoked: connect again.
    SignIn(String),
    RateLimited {
        retry_after_s: u64,
    },
    Other(String),
}

impl FetchError {
    pub fn message(&self) -> String {
        match self {
            Self::SignIn(m) | Self::Other(m) => m.clone(),
            Self::RateLimited { retry_after_s } => format!(
                "Battle.net asked Loadout to slow down, so it waits {} before trying again.",
                if *retry_after_s >= 60 {
                    format!("{} minutes", retry_after_s.div_ceil(60))
                } else {
                    format!("{retry_after_s} seconds")
                }
            ),
        }
    }
}

const SIGN_IN_AGAIN: &str =
    "Your Battle.net connection ended (Blizzard's lasts about a day). Press Connect to read it again.";

fn get(
    send: &mut dyn FnMut(&Call) -> Result<String, SendError>,
    call: &Call,
) -> Result<Value, FetchError> {
    let body = send(call).map_err(|e| match e {
        SendError::Status { code: 401, .. } => FetchError::SignIn(SIGN_IN_AGAIN.into()),
        SendError::Status {
            code: 429,
            retry_after_s,
        } => FetchError::RateLimited {
            retry_after_s: retry_after_s.unwrap_or(60).clamp(10, 3600),
        },
        SendError::Status { code, .. } => FetchError::Other(format!(
            "Battle.net answered {code}. Loadout tries again later."
        )),
        SendError::Other(m) => FetchError::Other(m),
    })?;
    serde_json::from_str(&body)
        .map_err(|_| FetchError::Other("Battle.net's answer wasn't readable.".into()))
}

/// A part that may be missing (a character Blizzard hasn't indexed, a
/// collection it won't show) is left out; a token or rate problem stops.
fn optional(got: Result<Value, FetchError>) -> Result<Option<Value>, FetchError> {
    match got {
        Ok(v) => Ok(Some(v)),
        Err(e @ (FetchError::SignIn(_) | FetchError::RateLimited { .. })) => Err(e),
        Err(FetchError::Other(_)) => Ok(None),
    }
}

/// Read the profile: the account's characters, the mount and pet counts,
/// and for the highest [`DETAILED`] characters their item level, gear,
/// newest raids and Mythic+ rating.
pub fn fetch_profile(
    send: &mut dyn FnMut(&Call) -> Result<String, SendError>,
    region: Region,
    now_ms: u64,
) -> Result<BnetProfile, FetchError> {
    let account = get(send, &account_call(region))?;
    let mut characters = parse_account(&account);
    let mounts = optional(get(send, &mounts_call(region)))?.and_then(|v| count(&v, "mounts"));
    let pets = optional(get(send, &pets_call(region)))?.and_then(|v| count(&v, "pets"));
    for c in characters.iter_mut().take(DETAILED) {
        if let Some(v) = optional(get(send, &summary_call(region, c)))? {
            c.item_level = number(&v, &["equipped_item_level"]).map(|l| l as u32);
        }
        if let Some(v) = optional(get(send, &equipment_call(region, c)))? {
            c.gear = parse_gear(&v);
        }
        if let Some(v) = optional(get(send, &raids_call(region, c)))? {
            c.raids = parse_raids(&v);
        }
        if let Some(v) = optional(get(send, &mythic_call(region, c)))? {
            c.mythic_rating = parse_rating(&v);
        }
    }
    Ok(BnetProfile {
        region,
        characters,
        mounts,
        pets,
        fetched_ms: now_ms,
    })
}

impl BnetCharacter {
    /// `Testchar (Area 52) · 80 Human Paladin · ilvl 639 · M+ 2766`.
    pub fn line(&self) -> String {
        let mut who = format!("{} {} {}", self.level, self.race, self.class);
        who = who.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut parts = vec![format!("{} ({})", self.name, self.realm), who];
        if let Some(l) = self.item_level {
            parts.push(format!("ilvl {l}"));
        }
        if let Some(r) = self.mythic_rating {
            parts.push(format!("M+ {}", r.round() as u64));
        }
        parts.join(" · ")
    }

    /// `Nerub-ar Palace 8/8 Heroic`, one per raid.
    pub fn raid_lines(&self) -> Vec<String> {
        self.raids
            .iter()
            .map(|r| {
                format!("{} {}/{} {}", r.name, r.killed, r.total, r.difficulty)
                    .trim()
                    .to_string()
            })
            .collect()
    }
}

impl BnetProfile {
    /// `4 characters · 312 mounts · 870 pets`.
    pub fn summary_line(&self) -> String {
        let n = self.characters.len();
        let mut parts = vec![if n == 1 {
            "1 character".to_string()
        } else {
            format!("{n} characters")
        }];
        if let Some(m) = self.mounts {
            parts.push(format!("{m} mounts"));
        }
        if let Some(p) = self.pets {
            parts.push(format!("{p} pets"));
        }
        parts.join(" · ")
    }
}

pub fn cache_path(config_dir: &Path) -> PathBuf {
    config_dir.join("games").join("battlenet-wow.json")
}

/// The last snapshot, unless it is older than Blizzard's 30 days; an old
/// one is deleted.
pub fn load_cache(config_dir: &Path, now_ms: u64) -> Option<BnetProfile> {
    let path = cache_path(config_dir);
    let p: BnetProfile = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
    if now_ms.saturating_sub(p.fetched_ms) > KEEP_MS {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    Some(p)
}

pub fn save_cache(config_dir: &Path, p: &BnetProfile) -> std::io::Result<()> {
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

    // Shapes from Blizzard's profile API docs, made-up characters.
    const ACCOUNT: &str = r#"{"_links":{"self":{"href":"x"}},"id":1,"wow_accounts":[{"id":11,"characters":[
        {"character":{"href":"x"},"protected_character":{"href":"x"},"name":"Altchar","id":2,"realm":{"key":{"href":"x"},"name":"Area 52","id":3676,"slug":"area-52"},"playable_class":{"key":{"href":"x"},"name":"Mage","id":8},"playable_race":{"key":{"href":"x"},"name":"Gnome","id":7},"gender":{"type":"FEMALE","name":"Female"},"faction":{"type":"ALLIANCE","name":"Alliance"},"level":42},
        {"character":{"href":"x"},"name":"Tëstchar","id":1,"realm":{"key":{"href":"x"},"name":"Area 52","id":3676,"slug":"area-52"},"playable_class":{"name":"Paladin","id":2},"playable_race":{"name":"Human","id":1},"faction":{"type":"ALLIANCE","name":"Alliance"},"level":80},
        {"character":{"href":"x"},"name":"","id":3,"realm":{"slug":"x"},"level":1}
    ]}],"collections":{"href":"x"}}"#;
    const MOUNTS: &str = r#"{"mounts":[{"mount":{"name":"Swift Brown Horse","id":6},"is_useable":true},{"mount":{"name":"Felsteed","id":17},"is_useable":true}]}"#;
    const PETS: &str = r#"{"pets":[{"species":{"name":"Mechanical Squirrel","id":39},"level":1}],"unlocked_battle_pet_slots":3}"#;
    const SUMMARY: &str =
        r#"{"name":"Tëstchar","equipped_item_level":639,"average_item_level":641}"#;
    const EQUIPMENT: &str = r#"{"equipped_items":[{"item":{"id":1},"slot":{"type":"HEAD","name":"Head"},"name":"Helm of Testing","level":{"value":639,"display_string":"Item Level 639"}},{"slot":{"type":"SHIRT","name":"Shirt"},"name":"Plain Shirt"}]}"#;
    const RAIDS: &str = r#"{"expansions":[
        {"expansion":{"name":"Dragonflight","id":503},"instances":[{"instance":{"name":"Old Raid","id":1},"modes":[{"difficulty":{"type":"NORMAL","name":"Normal"},"status":{"type":"COMPLETE"},"progress":{"completed_count":9,"total_count":9}}]}]},
        {"expansion":{"name":"Current Season","id":514},"instances":[
            {"instance":{"name":"Nerub-ar Palace","id":1273},"modes":[
                {"difficulty":{"type":"NORMAL","name":"Normal"},"progress":{"completed_count":8,"total_count":8}},
                {"difficulty":{"type":"HEROIC","name":"Heroic"},"progress":{"completed_count":5,"total_count":8}},
                {"difficulty":{"type":"MYTHIC","name":"Mythic"},"progress":{"completed_count":0,"total_count":8}}]},
            {"instance":{"name":"Liberation of Undermine","id":1296},"modes":[
                {"difficulty":{"type":"LFR","name":"Raid Finder"},"progress":{"completed_count":0,"total_count":8}}]}
        ]}]}"#;
    const MYTHIC: &str =
        r#"{"current_period":{"period":{"id":1}},"current_mythic_rating":{"rating":2765.8}}"#;
    const TOKEN: &str = r#"{"access_token":"tok-1","token_type":"bearer","expires_in":86399,"scope":"wow.profile","sub":"1"}"#;

    #[test]
    fn the_authorize_url_and_token_request_follow_blizzards_docs() {
        let url = authorize_url("abc123", REDIRECT_URI, "st8");
        assert_eq!(
            url,
            "https://oauth.battle.net/authorize?response_type=code&client_id=abc123&scope=wow.profile&state=st8&redirect_uri=http%3A%2F%2Flocalhost%3A47615%2Fcallback"
        );
        assert_eq!(
            token_form("c-1", REDIRECT_URI),
            "grant_type=authorization_code&code=c-1&redirect_uri=http%3A%2F%2Flocalhost%3A47615%2Fcallback"
        );
        // RFC 7617's example pair.
        assert_eq!(
            basic_auth("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
        assert_eq!(basic_auth("a", "b"), "Basic YTpi");
        assert_eq!(basic_auth("ab", "c"), "Basic YWI6Yw==");
    }

    #[test]
    fn the_callback_comes_from_the_loopback_or_a_pasted_address_and_checks_state() {
        let raw = "GET /callback?code=c%2D1&state=s1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(callback_code(raw, "s1").as_deref(), Ok("c-1"));
        assert_eq!(
            callback_code("https://localhost/callback?code=c2&state=s1#frag", "s1").as_deref(),
            Ok("c2")
        );
        assert!(
            callback_code("https://localhost/callback?code=c2&state=s9", "s1")
                .unwrap_err()
                .contains("another attempt")
        );
        assert!(callback_code(
            "https://localhost/callback?error=access_denied&state=s1",
            "s1"
        )
        .unwrap_err()
        .contains("access_denied"));
        assert!(callback_code("https://localhost/", "s1")
            .unwrap_err()
            .contains("whole address"));
        assert!(callback_code("https://localhost/callback?state=s1", "s1")
            .unwrap_err()
            .contains("no code"));
    }

    #[test]
    fn a_token_answer_needs_the_wow_profile_scope() {
        let t = parse_token(TOKEN, Region::Eu, 1_000).unwrap();
        assert_eq!(t.access_token, "tok-1");
        assert_eq!(t.expires_at_ms, 1_000 + 86_399_000);
        assert_eq!(t.region, Region::Eu);
        assert!(t.live(1_000));
        assert!(!t.live(t.expires_at_ms - 60_000));
        let no_scope = r#"{"access_token":"t","expires_in":86399,"scope":"openid"}"#;
        assert!(parse_token(no_scope, Region::Us, 0)
            .unwrap_err()
            .contains("profile access"));
        let refused = r#"{"error":"invalid_client","error_description":"Bad client credentials"}"#;
        assert!(parse_token(refused, Region::Us, 0)
            .unwrap_err()
            .contains("Bad client credentials"));
        assert!(parse_token("<html>", Region::Us, 0).is_err());
    }

    fn run(
        answers: &[(&str, Result<&'static str, SendError>)],
    ) -> (Result<BnetProfile, FetchError>, Vec<String>) {
        let mut asked = Vec::new();
        let got = fetch_profile(
            &mut |call| {
                asked.push(call.url.clone());
                answers
                    .iter()
                    .find(|(path, _)| {
                        call.url
                            .split('?')
                            .next()
                            .is_some_and(|u| u.ends_with(path))
                    })
                    .map(|(_, a)| a.clone().map(str::to_string))
                    .unwrap_or(Err(SendError::Status {
                        code: 404,
                        retry_after_s: None,
                    }))
            },
            Region::Us,
            42,
        );
        (got, asked)
    }

    fn all_answers() -> Vec<(&'static str, Result<&'static str, SendError>)> {
        vec![
            ("/profile/user/wow", Ok(ACCOUNT)),
            ("/collections/mounts", Ok(MOUNTS)),
            ("/collections/pets", Ok(PETS)),
            ("/t%C3%ABstchar", Ok(SUMMARY)),
            ("/t%C3%ABstchar/equipment", Ok(EQUIPMENT)),
            ("/t%C3%ABstchar/encounters/raids", Ok(RAIDS)),
            ("/t%C3%ABstchar/mythic-keystone-profile", Ok(MYTHIC)),
        ]
    }

    #[test]
    fn a_profile_reads_characters_collections_gear_raids_and_rating() {
        let (got, asked) = run(&all_answers());
        let p = got.unwrap();
        assert_eq!(p.fetched_ms, 42);
        assert_eq!(p.summary_line(), "2 characters · 2 mounts · 1 pets");
        let main = &p.characters[0];
        assert_eq!(
            main.line(),
            "Tëstchar (Area 52) · 80 Human Paladin · ilvl 639 · M+ 2766"
        );
        assert_eq!(
            main.gear,
            vec![
                GearItem {
                    slot: "Head".into(),
                    name: "Helm of Testing".into(),
                    level: Some(639),
                },
                GearItem {
                    slot: "Shirt".into(),
                    name: "Plain Shirt".into(),
                    level: None,
                },
            ]
        );
        // Only the newest expansion, at the hardest difficulty with a kill.
        assert_eq!(
            main.raid_lines(),
            vec![
                "Nerub-ar Palace 5/8 Heroic",
                "Liberation of Undermine 0/8 Raid Finder"
            ]
        );
        // The alt has no detail answers (404s) and is still listed.
        assert_eq!(p.characters[1].line(), "Altchar (Area 52) · 42 Gnome Mage");
        // Every call is on the region's API host with its profile namespace.
        assert!(asked
            .iter()
            .all(|u| u.starts_with("https://us.api.blizzard.com/profile/")
                && u.ends_with("?namespace=profile-us&locale=en_US")));
        assert!(asked.contains(
            &"https://us.api.blizzard.com/profile/wow/character/area-52/t%C3%ABstchar/equipment?namespace=profile-us&locale=en_US".to_string()
        ));
    }

    #[test]
    fn an_ended_token_or_a_rate_limit_stops_and_says_so() {
        let mut answers = all_answers();
        answers[0].1 = Err(SendError::Status {
            code: 401,
            retry_after_s: None,
        });
        let (got, asked) = run(&answers);
        assert!(matches!(got, Err(FetchError::SignIn(_))));
        assert_eq!(asked.len(), 1);
        let mut answers = all_answers();
        answers[4].1 = Err(SendError::Status {
            code: 429,
            retry_after_s: Some(5),
        });
        let (got, _) = run(&answers);
        assert_eq!(got, Err(FetchError::RateLimited { retry_after_s: 10 }));
        assert!(got.unwrap_err().message().contains("10 seconds"));
        // A missing collection is just left out.
        let mut answers = all_answers();
        answers[1].1 = Err(SendError::Status {
            code: 403,
            retry_after_s: None,
        });
        let (got, _) = run(&answers);
        assert_eq!(got.unwrap().mounts, None);
    }

    #[test]
    fn the_cache_holds_no_token_and_is_dropped_after_thirty_days() {
        let dir = std::env::temp_dir().join(format!("grokhub-bnet-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (got, _) = run(&all_answers());
        let p = got.unwrap();
        save_cache(&dir, &p).unwrap();
        let text = std::fs::read_to_string(cache_path(&dir)).unwrap();
        assert!(!text.contains("tok-1"));
        assert_eq!(load_cache(&dir, 42 + KEEP_MS), Some(p));
        assert_eq!(load_cache(&dir, 43 + KEEP_MS), None);
        assert!(!cache_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn regions_and_client_values_parse() {
        assert_eq!(Region::parse(" EU "), Some(Region::Eu));
        assert_eq!(Region::parse("cn"), None);
        assert_eq!(Region::Kr.api_base(), "https://kr.api.blizzard.com");
        assert!(looks_like_client_value("0123456789abcdef0123456789abcdef"));
        assert!(!looks_like_client_value("short"));
        assert!(!looks_like_client_value(
            "has spaces in it but is long enough"
        ));
    }
}
