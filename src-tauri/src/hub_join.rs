//! Devices → Join another computer: pair this computer with another
//! computer's hub (its Start share address and code) and send it this
//! computer's machine card every minute, for that computer's Fleet page.
//!
//! - Only an address on this network or this computer is dialed, with no
//!   redirects, so the pairing token never leaves the LAN. The address is
//!   checked again before every send.
//! - The token is kept in the OS keychain, never in a file or the page.
//! - Only the machine card is sent: CPU, memory, app version. No chats,
//!   memory or files.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use grokhub_core::fleet::MachineCard;
use grokhub_core::http;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// How often the card goes out while joined.
pub const SEND_EVERY: Duration = Duration::from_secs(60);
const FILE: &str = "hub-join.json";
const KEYRING_ACCOUNT: &str = "hub-join-token";

/// The hub this computer joined, as the file keeps it. The token is in the
/// keychain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Joined {
    /// `host:port`, checked to be on this network.
    pub address: String,
    pub hub_id: String,
    pub hub_name: String,
    pub joined_at: u64,
}

/// What Devices shows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinView {
    pub joined: Option<Joined>,
    /// When the card last reached the hub, this run.
    pub last_sent_at: Option<u64>,
    /// Why the last send failed, in words for the page; `None` after a send
    /// that worked.
    pub last_error: Option<String>,
}

#[derive(Default)]
struct Sends {
    last_sent_at: Option<u64>,
    last_error: Option<String>,
}

static SENDS: Mutex<Sends> = Mutex::new(Sends {
    last_sent_at: None,
    last_error: None,
});

fn sends() -> std::sync::MutexGuard<'static, Sends> {
    SENDS.lock().unwrap_or_else(|e| e.into_inner())
}

/// `host:port` for what the user typed: the address Start share shows
/// (`http://192.168.1.20:18766`), with or without `http://` or the port.
pub fn lan_address(raw: &str) -> Result<String, String> {
    let text = raw
        .trim()
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if text.is_empty() {
        return Err("Type the address the other computer shows under Sharing.".into());
    }
    let (host, port) = match text.rsplit_once(':') {
        Some((host, port)) => (
            host,
            port.parse::<u16>()
                .map_err(|_| "That port isn't a number.".to_string())?,
        ),
        None => (text, grokhub_core::DEFAULT_PORT),
    };
    let ip: IpAddr = host.parse().map_err(|_| {
        "Type the number address the other computer shows, like 192.168.1.20.".to_string()
    })?;
    if !crate::pc_files::is_lan(&ip) {
        return Err("Only a computer on this network can be joined.".into());
    }
    Ok(format!("{ip}:{port}"))
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

fn load(config_dir: &Path) -> Option<Joined> {
    let text = std::fs::read_to_string(path(config_dir)).ok()?;
    let joined: Joined = serde_json::from_str(&text).ok()?;
    lan_address(&joined.address)
        .ok()
        .map(|address| Joined { address, ..joined })
}

pub fn view(config_dir: &Path) -> JoinView {
    let joined = load(config_dir);
    let sends = sends();
    JoinView {
        last_sent_at: joined.as_ref().and(sends.last_sent_at),
        last_error: joined.as_ref().and(sends.last_error.clone()),
        joined,
    }
}

fn agent() -> http::Agent {
    // No redirects: a redirect could carry the token off the LAN.
    http::agent(http::Opts {
        connect: Duration::from_secs(3),
        read: Some(Duration::from_secs(5)),
        overall: Some(Duration::from_secs(10)),
        redirects: 0,
        ..Default::default()
    })
}

/// The hub's words for a refusal, or a plain line for its status code.
fn refusal(status: u16, body: &Value) -> String {
    if let Some(error) = body.get("error").and_then(Value::as_str) {
        return error.to_string();
    }
    format!("The other computer answered {status}.")
}

/// Pair with the hub at `raw_address` using its pairing `code`, as this
/// computer (`me` is this computer's hub id and name). Replaces any earlier
/// join.
pub fn join(
    config_dir: &Path,
    raw_address: &str,
    code: &str,
    me: (&str, &str),
    now: u64,
) -> Result<JoinView, String> {
    let address = lan_address(raw_address)?;
    let code = code.trim();
    if code.is_empty() {
        return Err("Type the code the other computer shows under Sharing.".into());
    }
    let resp = agent()
        .post(&format!("http://{address}/v1/pair"))
        .any_status()
        .send_json(json!({ "code": code, "deviceId": me.0, "deviceName": me.1 }))
        .map_err(|_| format!("Couldn't reach {address}. Is it sharing, on this network?"))?;
    let status = resp.status();
    let body: Value = resp.into_json().unwrap_or(Value::Null);
    if status != 200 {
        return Err(refusal(status, &body));
    }
    let token = body
        .get("token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or("The other computer didn't send a pairing.")?;
    let hub = body.get("hub").cloned().unwrap_or(Value::Null);
    let text = |key: &str| {
        hub.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let joined = Joined {
        address,
        hub_id: text("id"),
        hub_name: Some(text("name"))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Other computer".into()),
        joined_at: now,
    };
    store_token(Some(token))?;
    let file = serde_json::to_string_pretty(&joined).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(path(config_dir), file).map_err(|e| e.to_string())?;
    *sends() = Sends::default();
    Ok(view(config_dir))
}

/// Stop sending and forget the pairing here. The other computer still lists
/// this one until it's removed there.
pub fn leave(config_dir: &Path) -> Result<JoinView, String> {
    store_token(None)?;
    match std::fs::remove_file(path(config_dir)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    *sends() = Sends::default();
    Ok(view(config_dir))
}

/// Send `card` to the joined hub, if any, and keep how it went for the page.
pub fn send(config_dir: &Path, card: &MachineCard, now: u64) {
    let Some(joined) = load(config_dir) else {
        return;
    };
    let result = send_to(&joined, card);
    let mut sends = sends();
    match result {
        Ok(()) => {
            sends.last_sent_at = Some(now);
            sends.last_error = None;
        }
        Err(why) => sends.last_error = Some(why),
    }
}

fn send_to(joined: &Joined, card: &MachineCard) -> Result<(), String> {
    let token = load_token().ok_or("Join again: this computer's pairing is gone.")?;
    let resp = agent()
        .post(&format!("http://{}/v1/fleet", joined.address))
        .header("Authorization", &format!("Bearer {token}"))
        .any_status()
        .send_json(json!({
            "cpuPct": card.cpu_pct,
            "ramMb": card.ram_mb,
            "freeRamMb": card.free_ram_mb,
            "appVersion": card.app_version,
        }))
        .map_err(|_| format!("Couldn't reach {}.", joined.hub_name))?;
    let status = resp.status();
    if status == 200 {
        return Ok(());
    }
    let body: Value = resp.into_json().unwrap_or(Value::Null);
    Err(match status {
        401 => format!(
            "{} doesn't know this computer any more. Join again.",
            joined.hub_name
        ),
        403 => format!("{} isn't sharing right now.", joined.hub_name),
        _ => refusal(status, &body),
    })
}

/// Sends this computer's card every [`SEND_EVERY`] while it's joined.
pub fn start(config_dir: PathBuf, share: std::sync::Arc<crate::hub_share::HubShare>) {
    std::thread::spawn(move || loop {
        if load(&config_dir).is_some() {
            send_now(&config_dir, &share);
        }
        std::thread::sleep(SEND_EVERY);
    });
}

/// This computer's card, sent now.
pub fn send_now(config_dir: &Path, share: &crate::hub_share::HubShare) {
    let now = grokhub_core::now_ms();
    let (id, name, _) = share.fleet();
    send(config_dir, &crate::fleet::local_card(&id, &name, now), now);
}

/// The token in tests, which never touch the real keychain.
static TEST_TOKEN: Mutex<Option<String>> = Mutex::new(None);

fn load_token() -> Option<String> {
    if cfg!(test) {
        return TEST_TOKEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    }
    match keyring_entry().ok()?.get_password() {
        Ok(token) if !token.trim().is_empty() => Some(token),
        _ => None,
    }
}

fn store_token(token: Option<&str>) -> Result<(), String> {
    if cfg!(test) {
        *TEST_TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = token.map(str::to_string);
        return Ok(());
    }
    let entry = keyring_entry()?;
    match token {
        Some(token) => entry
            .set_password(token)
            .map_err(|e| format!("Couldn't keep the pairing in the system keychain: {e}")),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!(
                "Couldn't remove the pairing from the system keychain: {e}"
            )),
        },
    }
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(crate::credential::SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("The system keychain is not available: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::HubState;
    use std::sync::{Arc, Mutex};

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hub-join-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A sharing hub on loopback with a live code.
    fn hub() -> (Arc<Mutex<HubState>>, String, String) {
        let mut st = HubState::empty();
        st.sharing = true;
        st.device_name = "Desk".into();
        let code = st.rotate_pair().code;
        let state = Arc::new(Mutex::new(st));
        let port = grokhub_hub::serve_background(state.clone(), 0).expect("bind");
        (state, format!("http://127.0.0.1:{port}"), code)
    }

    fn card() -> MachineCard {
        MachineCard {
            id: "ignored".into(),
            name: "ignored".into(),
            cpu_pct: Some(23),
            ram_mb: Some(16_000),
            free_ram_mb: Some(9_000),
            app_version: "3.6.0".into(),
            at_ms: 1,
        }
    }

    #[test]
    fn an_address_must_be_on_this_network_and_the_port_defaults() {
        assert_eq!(
            lan_address(" http://192.168.1.20:18766/ "),
            Ok("192.168.1.20:18766".into())
        );
        assert_eq!(
            lan_address("10.0.0.5"),
            Ok(format!("10.0.0.5:{}", grokhub_core::DEFAULT_PORT))
        );
        assert_eq!(
            lan_address("8.8.8.8:18766"),
            Err("Only a computer on this network can be joined.".into())
        );
        assert_eq!(
            lan_address("desk.example.com:18766"),
            Err("Type the number address the other computer shows, like 192.168.1.20.".into())
        );
        assert_eq!(
            lan_address("https://192.168.1.20:18766"),
            Err("Type the number address the other computer shows, like 192.168.1.20.".into())
        );
        assert_eq!(
            lan_address("192.168.1.20:port"),
            Err("That port isn't a number.".into())
        );
    }

    // One test: the token is held in one static while the tests run.
    #[test]
    fn joining_sends_the_card_under_this_computers_name_and_leaving_forgets_it() {
        let dir = temp("round-trip");
        let (state, address, code) = hub();

        let wrong = join(&dir, &address, "000000", ("d-laptop", "Laptop"), 5);
        assert_eq!(wrong, Err("Pairing code does not match.".into()));
        assert_eq!(view(&dir), JoinView::default());

        let joined = join(&dir, &address, &code, ("d-laptop", "Laptop"), 5).unwrap();
        let held = joined.joined.clone().expect("joined");
        assert_eq!(
            (held.hub_name.as_str(), held.joined_at, held.address),
            ("Desk", 5, address.trim_start_matches("http://").to_string())
        );
        let file = std::fs::read_to_string(dir.join(FILE)).unwrap();
        let token = TEST_TOKEN.lock().unwrap().clone().expect("token kept");
        assert!(!file.contains(&token), "the token never reaches the file");

        send(&dir, &card(), 40);
        let fleet = state.lock().unwrap().fleet.clone();
        let sent: Vec<(&str, &str, Option<u8>, &str)> = fleet
            .iter()
            .map(|c| {
                (
                    c.id.as_str(),
                    c.name.as_str(),
                    c.cpu_pct,
                    c.app_version.as_str(),
                )
            })
            .collect();
        assert_eq!(sent, vec![("d-laptop", "Laptop", Some(23), "3.6.0")]);
        let seen = view(&dir);
        assert_eq!((seen.last_sent_at, seen.last_error), (Some(40), None));

        state.lock().unwrap().sharing = false;
        send(&dir, &card(), 100);
        let seen = view(&dir);
        assert_eq!(
            (seen.last_sent_at, seen.last_error.as_deref()),
            (Some(40), Some("Desk isn't sharing right now."))
        );

        state.lock().unwrap().sharing = true;
        state.lock().unwrap().peers.clear();
        send(&dir, &card(), 160);
        assert_eq!(
            view(&dir).last_error.as_deref(),
            Some("Desk doesn't know this computer any more. Join again.")
        );

        assert_eq!(leave(&dir), Ok(JoinView::default()));
        assert_eq!(*TEST_TOKEN.lock().unwrap(), None);
        assert!(!dir.join(FILE).exists());
        let before = state.lock().unwrap().fleet.clone();
        send(&dir, &card(), 220);
        assert_eq!(
            state.lock().unwrap().fleet,
            before,
            "nothing goes out after leaving"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_naming_a_public_address_is_never_dialed() {
        let dir = temp("public-file");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            r#"{"address":"8.8.8.8:18766","hubId":"h","hubName":"Desk","joinedAt":1}"#,
        )
        .unwrap();
        assert_eq!(view(&dir), JoinView::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
