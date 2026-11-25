//! Companion devices: a phone, another computer or a headless box that pairs
//! with this app and lends it a camera, a screen, its location, text
//! messages, a voice, a canvas panel, and the MCP servers and skills it
//! hosts. The protocol is in docs/design/companion-nodes/README.md; the
//! server is `grokhub-hub`'s `node_host`.
//!
//! This module is the paired list and the consent rules, with no I/O but the
//! list's own file. A companion is useful only after two yeses: the companion
//! offers a capability (nothing is offered by default), and the user allows
//! it for that companion in the app. Pairing alone allows nothing.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::pair::{ct_eq, make_pair_code, normalize_code, PAIR_MAX_TRIES, PAIR_TTL_MS};
use crate::state::PairCode;

/// The companion port. The hub (18766) is for computers that share chats,
/// so companions get a door of their own that leads only to the relay.
pub const PORT: u16 = 18767;
/// The paired list, under the config dir. Holds token hashes, not tokens.
pub const FILE: &str = "companions.json";
/// Protocol version a node and host agree on in `/pair` and `/hello`.
pub const VERSION: u32 = 1;

/// What a companion can lend, with the words the approval card shows.
pub const OFFERS: &[(&str, &str)] = &[
    ("camera", "Take photos with its camera"),
    ("screen", "Take screenshots of its screen"),
    ("location", "Share where it is"),
    ("sms", "Read and send its text messages"),
    ("talk", "Speak out loud and listen on its microphone"),
    ("wake", "Wake this app with a phrase or a button"),
    ("canvas", "Show panels on its screen"),
    ("mcp", "Lend the tools of its MCP servers"),
    ("skills", "Lend its skills"),
];

/// The companion's own tools and the capability each needs. Any other tool
/// a companion lists is one of its MCP servers' tools.
const TOOL_OFFERS: &[(&str, &str)] = &[
    ("camera_snap", "camera"),
    ("screen_snap", "screen"),
    ("location_get", "location"),
    ("sms_list", "sms"),
    ("sms_send", "sms"),
    ("talk_say", "talk"),
    ("talk_listen", "talk"),
    ("canvas_show", "canvas"),
    ("canvas_clear", "canvas"),
    ("skills_list", "skills"),
    ("skill_read", "skills"),
];

/// The capability a companion tool needs.
pub fn offer_for_tool(tool: &str) -> &'static str {
    TOOL_OFFERS
        .iter()
        .find(|(name, _)| *name == tool)
        .map_or("mcp", |(_, offer)| offer)
}

/// The known offers in `wanted`, in [`OFFERS`] order, each once.
pub fn known_offers<S: AsRef<str>>(wanted: &[S]) -> Vec<String> {
    OFFERS
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| wanted.iter().any(|w| w.as_ref() == *name))
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Companion {
    pub id: String,
    pub name: String,
    /// `android`, `linux`, `macos`, `windows`, `ios`, `browser`, or what the
    /// node said, cut to [`MAX_PLATFORM`] characters.
    pub platform: String,
    /// SHA-256 of the bearer token, hex. The token itself is only ever held
    /// by the companion.
    pub token_sha256: String,
    /// The user said yes on this computer. Until then the relay is shut.
    pub approved: bool,
    /// What the companion says it can lend, from its last `/hello`.
    #[serde(default)]
    pub offers: Vec<String>,
    /// What the user allowed. A capability is usable when it is in both.
    #[serde(default)]
    pub allowed: Vec<String>,
    pub paired_at: u64,
}

impl Companion {
    /// The capability is offered by the companion and allowed by the user.
    pub fn may(&self, offer: &str) -> bool {
        self.approved
            && self.offers.iter().any(|o| o == offer)
            && self.allowed.iter().any(|o| o == offer)
    }

    pub fn may_call(&self, tool: &str) -> bool {
        self.may(offer_for_tool(tool))
    }
}

const MAX_ID: usize = 40;
const MAX_NAME: usize = 48;
const MAX_PLATFORM: usize = 24;
/// Paired companions kept at once; a new pairing past this is refused.
pub const MAX_COMPANIONS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairError {
    NoCode,
    Mismatch,
    BadId,
    Full,
}

impl PairError {
    pub fn text(self) -> &'static str {
        match self {
            PairError::NoCode => {
                "No pairing code is open. Make one in the app under Devices → Companions."
            }
            PairError::Mismatch => "That pairing code doesn't match.",
            PairError::BadId => "The companion id must be 1 to 40 letters, digits or dashes.",
            PairError::Full => {
                "This app already has as many companions as it keeps. Remove one first."
            }
        }
    }
}

/// A companion id is the node's own: lowercase letters, digits and dashes.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn clean(text: &str, max: usize, fallback: &str) -> String {
    let text: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect();
    if text.is_empty() {
        fallback.to_string()
    } else {
        text
    }
}

pub fn token_hash(token: &str) -> String {
    crate::model_download::sha256_hex(token.as_bytes())
}

/// The paired companions and the open pairing code.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Book {
    #[serde(default)]
    pub companions: Vec<Companion>,
    /// In memory only, like the hub's: never written to the file.
    #[serde(skip)]
    pub pair: Option<PairCode>,
}

impl Book {
    pub fn load(path: &Path) -> Book {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Book>(&text).ok())
            .map(|mut book| {
                book.companions.retain(|c| valid_id(&c.id));
                book
            })
            .unwrap_or_default()
    }

    /// Written readable by this user only: a token hash is enough to tell a
    /// guess from the real token offline.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        write_private(path, &bytes)
    }

    /// A new one-time code for the next companion.
    pub fn open_pairing(&mut self, now: u64) -> PairCode {
        let code = PairCode {
            code: make_pair_code(),
            expires_at: now + PAIR_TTL_MS,
            tries: 0,
        };
        self.pair = Some(code.clone());
        code
    }

    /// Pair with the open code. Returns the companion's new token, once; the
    /// companion waits for the user's yes before anything else works. The
    /// code is spent on success and burned after [`PAIR_MAX_TRIES`] misses.
    /// The same id pairing again starts over: new token, not approved.
    pub fn pair_with(
        &mut self,
        code: &str,
        id: &str,
        name: &str,
        platform: &str,
        now: u64,
    ) -> Result<String, PairError> {
        let want = self
            .pair
            .as_ref()
            .filter(|p| p.expires_at >= now)
            .map(|p| normalize_code(&p.code))
            .unwrap_or_default();
        if want.is_empty() {
            return Err(PairError::NoCode);
        }
        if !ct_eq(normalize_code(code).as_bytes(), want.as_bytes()) {
            if let Some(p) = self.pair.as_mut() {
                p.tries = p.tries.saturating_add(1);
                if p.tries >= PAIR_MAX_TRIES {
                    self.pair = None;
                }
            }
            return Err(PairError::Mismatch);
        }
        if !valid_id(id) {
            return Err(PairError::BadId);
        }
        let again = self.companions.iter().any(|c| c.id == id);
        if !again && self.companions.len() >= MAX_COMPANIONS {
            return Err(PairError::Full);
        }
        self.pair = None;
        self.companions.retain(|c| c.id != id);
        let token = crate::new_token();
        self.companions.push(Companion {
            id: id.to_string(),
            name: clean(name, MAX_NAME, "Companion"),
            platform: clean(platform, MAX_PLATFORM, "unknown").to_ascii_lowercase(),
            token_sha256: token_hash(&token),
            approved: false,
            offers: vec![],
            allowed: vec![],
            paired_at: now,
        });
        Ok(token)
    }

    pub fn by_token(&self, token: &str) -> Option<&Companion> {
        if token.is_empty() {
            return None;
        }
        let hash = token_hash(token);
        self.companions
            .iter()
            .find(|c| ct_eq(c.token_sha256.as_bytes(), hash.as_bytes()))
    }

    pub fn get(&self, id: &str) -> Option<&Companion> {
        self.companions.iter().find(|c| c.id == id)
    }

    /// The companion's `/hello`: what it can lend now. Known offers only.
    pub fn hello(&mut self, id: &str, offers: &[String]) -> bool {
        let Some(c) = self.companions.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        c.offers = known_offers(offers);
        true
    }

    /// The user's yes, with the capabilities they allowed.
    pub fn approve(&mut self, id: &str, allowed: &[String]) -> bool {
        let Some(c) = self.companions.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        c.approved = true;
        c.allowed = known_offers(allowed);
        true
    }

    pub fn set_allowed(&mut self, id: &str, allowed: &[String]) -> bool {
        let Some(c) = self.companions.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        c.allowed = known_offers(allowed);
        true
    }

    /// Forget a companion: its token stops working at once.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.companions.len();
        self.companions.retain(|c| c.id != id);
        self.companions.len() != before
    }
}

/// Write `bytes` to `path` readable by this user only, through a synced
/// temp file so a crash leaves the old file or the new one.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    crate::state::write_private_synced(&tmp, bytes)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    #[cfg(windows)]
    crate::win_acl::restrict_user_only(path);
    Ok(())
}

/// Companions and their host talk only over the home network, link-local,
/// a VPN such as Tailscale (100.64.0.0/10), or this computer.
pub fn private_address(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return private_address(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// The companion's side of a pairing, in its own config dir (0600): where
/// the host is, the token, and what its person offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeFile {
    /// `ip:port` of the host.
    pub host: String,
    pub host_name: String,
    pub id: String,
    pub name: String,
    pub token: String,
    #[serde(default)]
    pub offers: Vec<String>,
}

/// The companion's file, under its config dir.
pub const NODE_FILE: &str = "companion-node.json";

impl NodeFile {
    pub fn load(path: &Path) -> Option<NodeFile> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        write_private(path, &bytes)
    }
}

/// The MCP methods the relay carries. Anything else is answered by the host
/// and never reaches the companion.
pub fn relayed_method(method: &str) -> bool {
    matches!(method, "initialize" | "ping" | "tools/list" | "tools/call")
        || method.starts_with("notifications/")
}

/// Why the host refuses an MCP request for this companion before relaying
/// it, as a JSON-RPC error; `None` when it may go.
pub fn refusal(request: &Value, c: &Companion) -> Option<Value> {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    if !relayed_method(method) {
        return Some(
            json!({ "code": -32601, "message": format!("{method} isn't available on a companion.") }),
        );
    }
    if method == "tools/call" {
        let tool = request
            .pointer("/params/name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !c.may_call(tool) {
            let offer = offer_for_tool(tool);
            return Some(json!({
                "code": -32602,
                "message": format!("{} isn't allowed to lend `{offer}`. Allow it under Devices → Companions.", c.name),
            }));
        }
    }
    None
}

/// The companion's `tools/list` answer with the tools the user didn't allow
/// taken out, so the model never sees them.
pub fn filter_tools(response: &mut Value, c: &Companion) {
    if let Some(tools) = response
        .pointer_mut("/result/tools")
        .and_then(Value::as_array_mut)
    {
        tools.retain(|tool| {
            tool.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| c.may_call(name))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn paired() -> (Book, String) {
        let mut book = Book::default();
        let code = book.open_pairing(NOW).code;
        let token = book
            .pair_with(&code, "pixel-8", "Pixel 8", "Android", NOW)
            .expect("pair");
        (book, token)
    }

    #[test]
    fn pairing_spends_the_code_and_starts_unapproved() {
        let (book, token) = paired();
        assert!(book.pair.is_none(), "the code is one-time");
        let c = book.by_token(&token).expect("token finds it");
        assert_eq!(c.id, "pixel-8");
        assert_eq!(c.platform, "android");
        assert!(!c.approved);
        assert_eq!(c.token_sha256, token_hash(&token));
        assert_ne!(c.token_sha256, token, "the file holds a hash");
        assert_eq!(book.by_token("nope"), None);
        assert_eq!(book.by_token(""), None);
    }

    #[test]
    fn wrong_codes_burn_the_code() {
        let mut book = Book::default();
        book.open_pairing(NOW);
        for _ in 0..PAIR_MAX_TRIES {
            assert_eq!(
                book.pair_with("AAA-AAA", "x", "X", "linux", NOW),
                Err(PairError::Mismatch)
            );
        }
        assert!(book.pair.is_none());
        assert_eq!(
            book.pair_with("AAA-AAA", "x", "X", "linux", NOW),
            Err(PairError::NoCode)
        );
    }

    #[test]
    fn an_expired_code_or_bad_id_pairs_nothing() {
        let mut book = Book::default();
        let code = book.open_pairing(NOW).code;
        assert_eq!(
            book.pair_with(&code, "Bad Id", "X", "linux", NOW),
            Err(PairError::BadId)
        );
        assert_eq!(
            book.pair_with(&code, "ok", "X", "linux", NOW + PAIR_TTL_MS + 1),
            Err(PairError::NoCode)
        );
        assert!(book.companions.is_empty());
    }

    #[test]
    fn pairing_again_replaces_the_token_and_the_yes() {
        let (mut book, old) = paired();
        book.approve("pixel-8", &["camera".into()]);
        let code = book.open_pairing(NOW).code;
        let new = book
            .pair_with(&code, "pixel-8", "Pixel", "android", NOW)
            .expect("pair again");
        assert_eq!(book.companions.len(), 1);
        assert_eq!(book.by_token(&old), None);
        let c = book.by_token(&new).expect("new token");
        assert!(!c.approved);
        assert!(c.allowed.is_empty());
    }

    #[test]
    fn a_capability_needs_the_offer_the_yes_and_the_allow() {
        let (mut book, token) = paired();
        book.hello(
            "pixel-8",
            &["camera".into(), "sms".into(), "teleport".into()],
        );
        let c = book.by_token(&token).expect("c");
        assert_eq!(
            c.offers,
            vec!["camera", "sms"],
            "unknown offers are dropped"
        );
        assert!(!c.may("camera"), "not approved yet");
        book.approve("pixel-8", &["camera".into(), "location".into()]);
        let c = book.by_token(&token).expect("c");
        assert!(c.may("camera"));
        assert!(!c.may("sms"), "offered but not allowed");
        assert!(!c.may("location"), "allowed but not offered");
        assert!(c.may_call("camera_snap"));
        assert!(!c.may_call("sms_send"));
        assert!(!c.may_call("github-create_issue"), "hosted MCP needs `mcp`");
    }

    #[test]
    fn the_host_refuses_tools_and_methods_the_user_didnt_allow() {
        let (mut book, _) = paired();
        book.hello("pixel-8", &["camera".into(), "sms".into()]);
        book.approve("pixel-8", &["camera".into()]);
        let c = book.get("pixel-8").expect("c").clone();
        let call = |name: &str| json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":{}}});
        assert_eq!(refusal(&call("camera_snap"), &c), None);
        let sms = refusal(&call("sms_send"), &c).expect("refused");
        assert_eq!(sms["code"], -32602);
        assert_eq!(
            sms["message"],
            "Pixel 8 isn't allowed to lend `sms`. Allow it under Devices → Companions."
        );
        let other = refusal(
            &json!({"jsonrpc":"2.0","id":2,"method":"resources/read"}),
            &c,
        )
        .expect("refused");
        assert_eq!(other["code"], -32601);
        assert_eq!(
            refusal(
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                &c
            ),
            None
        );

        let mut list = json!({"jsonrpc":"2.0","id":3,"result":{"tools":[
            {"name":"camera_snap"},{"name":"sms_send"},{"name":"github-create_issue"},{"nope":1}
        ]}});
        filter_tools(&mut list, &c);
        assert_eq!(list["result"]["tools"], json!([{"name":"camera_snap"}]));
    }

    #[test]
    fn the_file_keeps_hashes_and_drops_the_code() {
        let dir = std::env::temp_dir().join(format!("companions-{}", crate::uid("t")));
        let path = dir.join(FILE);
        let (mut book, token) = paired();
        book.open_pairing(NOW);
        book.save(&path).expect("save");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(!text.contains(&token), "no token on disk: {text}");
        assert!(!text.contains("\"pair\""), "no code on disk: {text}");
        let back = Book::load(&path);
        assert_eq!(back.companions, book.companions);
        assert!(back.pair.is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_full_book_refuses_new_companions_but_not_old_ones() {
        let mut book = Book::default();
        for n in 0..MAX_COMPANIONS {
            let code = book.open_pairing(NOW).code;
            book.pair_with(&code, &format!("n{n}"), "N", "linux", NOW)
                .expect("pair");
        }
        let code = book.open_pairing(NOW).code;
        assert_eq!(
            book.pair_with(&code, "one-more", "N", "linux", NOW),
            Err(PairError::Full)
        );
        assert!(book.pair_with(&code, "n3", "N", "linux", NOW).is_ok());
    }
}
