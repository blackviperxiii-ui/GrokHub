//! Devices → Companions: phones and other computers that lend this app their
//! camera, screen, location, texts, voice, panels, MCP servers and skills
//! (docs/design/companion-nodes/README.md).
//!
//! The companion host is off until the user starts it, and starts again at
//! the next launch until they stop it. Each approved companion is one MCP
//! connection, `companion-<id>`, whose key is sealed like any connector's,
//! so its tools reach chats through the same approvals, fencing and
//! Offline switch as every other MCP tool.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use grokhub_agent::mcp;
use grokhub_core::companion_nodes as nodes;
use grokhub_hub::node_host::{CompanionView, NodeHost};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

/// Whether the user left the host on, so the next launch starts it again.
const ON_FILE: &str = "companion-host.json";
/// What a wake or a heard line is sent to the page as.
pub const EVENT: &str = "companion-event";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionsView {
    pub on: bool,
    pub port: u16,
    /// The live pairing code; empty when none is open. Never log it.
    pub code: String,
    /// This computer's number address on the network, for `node pair`;
    /// empty while off or when there is none.
    pub address: String,
    pub companions: Vec<CompanionView>,
}

/// A wake or a heard line from a companion, for the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionEvent {
    pub companion: String,
    /// The companion's name, as it paired.
    pub name: String,
    /// `wake` or `heard`.
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OnFile {
    on: bool,
}

/// What the watcher found since its last look.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tick {
    pub events: Vec<CompanionEvent>,
    /// Connections to open again: their companion just came online, so
    /// the app's MCP client lists its tools afresh.
    pub reconnect: Vec<String>,
}

pub struct Companions {
    config: PathBuf,
    bind: IpAddr,
    port: u16,
    lan: fn() -> Option<IpAddr>,
    host: Mutex<Option<NodeHost>>,
    /// Approved companions that were online at the last look.
    online: Mutex<Vec<String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The MCP connection that reaches one companion.
pub fn connection(id: &str) -> String {
    format!("companion-{id}")
}

impl Companions {
    /// The host on the network, on the companion port.
    pub fn open(config: &Path) -> Self {
        Self::with(
            config,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            nodes::PORT,
            crate::remote::lan_ip,
        )
    }

    fn with(config: &Path, bind: IpAddr, port: u16, lan: fn() -> Option<IpAddr>) -> Self {
        Companions {
            config: config.to_path_buf(),
            bind,
            port,
            lan,
            host: Mutex::new(None),
            online: Mutex::new(vec![]),
        }
    }

    /// Start the host again when the user left it on.
    pub fn resume(&self) {
        if read_on(&self.config) {
            if let Err(e) = self.start_host(false) {
                eprintln!("companions: {e}");
            }
        }
    }

    /// Start: listen for companions and open a pairing code.
    pub fn start(&self) -> Result<CompanionsView, String> {
        self.start_host(true)?;
        write_on(&self.config, true)?;
        Ok(self.view())
    }

    fn start_host(&self, with_code: bool) -> Result<(), String> {
        let mut host = lock(&self.host);
        if host.is_none() {
            let started = NodeHost::start(
                &self.config.join(nodes::FILE),
                self.bind,
                self.port,
                self.key(),
                &crate::remote_devices::computer_name(),
            )
            .map_err(|e| {
                if grokhub_core::pair::lan_bind_in_use(&e) {
                    format!(
                        "Port {} is already in use. Close the other app using it, then try again.",
                        self.port
                    )
                } else {
                    "Companions could not start on this computer's network.".to_string()
                }
            })?;
            *host = Some(started);
        }
        if with_code {
            if let Some(h) = host.as_ref().filter(|h| h.pairing().is_none()) {
                h.open_pairing();
            }
        }
        Ok(())
    }

    /// The key this app's MCP client sends to reach each companion. Every
    /// connection holds the same one, sealed; a new one is made when none
    /// opens.
    fn key(&self) -> String {
        let book = nodes::Book::load(&self.config.join(nodes::FILE));
        let saved = book.companions.iter().filter(|c| c.approved).find_map(|c| {
            grokhub_agent::harness::open_connection_token(&self.config, &connection(&c.id))
        });
        saved.unwrap_or_else(grokhub_core::pkce::oauth_nonce)
    }

    /// Stop: no companion can reach this app until Start again.
    pub fn stop(&self) -> Result<CompanionsView, String> {
        lock(&self.host).take();
        lock(&self.online).clear();
        write_on(&self.config, false)?;
        Ok(self.view())
    }

    pub fn new_code(&self) -> Result<CompanionsView, String> {
        lock(&self.host)
            .as_ref()
            .ok_or("Start companions first.")?
            .open_pairing();
        Ok(self.view())
    }

    pub fn view(&self) -> CompanionsView {
        let host = lock(&self.host);
        let Some(h) = host.as_ref() else {
            return CompanionsView {
                on: false,
                port: self.port,
                code: String::new(),
                address: String::new(),
                companions: nodes::Book::load(&self.config.join(nodes::FILE))
                    .companions
                    .into_iter()
                    .map(|c| CompanionView {
                        id: c.id,
                        name: c.name,
                        platform: c.platform,
                        approved: c.approved,
                        offers: c.offers,
                        allowed: c.allowed,
                        online: false,
                        paired_at: c.paired_at,
                    })
                    .collect(),
            };
        };
        CompanionsView {
            on: true,
            port: h.port(),
            code: h.pairing().map(|p| p.code).unwrap_or_default(),
            address: (self.lan)().map(|ip| ip.to_string()).unwrap_or_default(),
            companions: h.companions(),
        }
    }

    /// The user's yes, with what they allowed. Its connection is added (or
    /// kept) and opened again so chats see what it lends now.
    pub fn approve(&self, id: &str, allowed: &[String]) -> Result<CompanionsView, String> {
        {
            let host = lock(&self.host);
            let h = host.as_ref().ok_or("Start companions first.")?;
            if !h.approve(id, allowed) {
                return Err("That companion isn't paired anymore.".into());
            }
            self.connect(h, id)?;
        }
        Ok(self.view())
    }

    pub fn set_allowed(&self, id: &str, allowed: &[String]) -> Result<CompanionsView, String> {
        {
            let host = lock(&self.host);
            let h = host.as_ref().ok_or("Start companions first.")?;
            if !h.set_allowed(id, allowed) {
                return Err("That companion isn't paired anymore.".into());
            }
            let _ = mcp::restart(&connection(id));
        }
        Ok(self.view())
    }

    /// Remove: its token stops at once and its connection goes.
    pub fn remove(&self, id: &str) -> Result<CompanionsView, String> {
        let removed = match lock(&self.host).as_ref() {
            Some(h) => h.remove(id),
            None => {
                let path = self.config.join(nodes::FILE);
                let mut book = nodes::Book::load(&path);
                let gone = book.remove(id);
                if gone {
                    book.save(&path)?;
                }
                gone
            }
        };
        if !removed {
            return Err("That companion isn't paired anymore.".into());
        }
        lock(&self.online).retain(|o| o != id);
        let name = connection(id);
        if mcp::configured().iter().any(|row| row.name == name) {
            mcp::disconnect(&name)?;
        }
        Ok(self.view())
    }

    /// Add the companion's connection with the host's key sealed in it.
    fn connect(&self, h: &NodeHost, id: &str) -> Result<(), String> {
        let name = connection(id);
        mcp::install(&name, &json!({ "url": h.mcp_url(id) }))?;
        let key = h.local_key();
        if grokhub_agent::harness::open_connection_token(&self.config, &name).as_deref()
            != Some(key)
        {
            mcp::save_key(&name, key)?;
        }
        let _ = mcp::restart(&name);
        Ok(())
    }

    /// Wakes and heard lines since the last look, and the companions that
    /// just came online.
    pub fn tick(&self) -> Tick {
        let host = lock(&self.host);
        let Some(h) = host.as_ref() else {
            return Tick::default();
        };
        let list = h.companions();
        let name_of = |id: &str| {
            list.iter()
                .find(|c| c.id == id)
                .map_or_else(|| id.to_string(), |c| c.name.clone())
        };
        let events = h
            .take_events()
            .into_iter()
            .map(|e| CompanionEvent {
                name: name_of(&e.companion),
                companion: e.companion,
                kind: e.kind,
                text: e.text,
            })
            .collect();
        let now: Vec<String> = list
            .iter()
            .filter(|c| c.approved && c.online)
            .map(|c| c.id.clone())
            .collect();
        let mut online = lock(&self.online);
        let reconnect = now
            .iter()
            .filter(|id| !online.contains(id))
            .map(|id| connection(id))
            .collect();
        *online = now;
        Tick { events, reconnect }
    }
}

fn read_on(config: &Path) -> bool {
    std::fs::read(config.join(ON_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<OnFile>(&b).ok())
        .is_some_and(|f| f.on)
}

fn write_on(config: &Path, on: bool) -> Result<(), String> {
    let bytes = serde_json::to_vec(&OnFile { on }).map_err(|e| e.to_string())?;
    nodes::write_private(&config.join(ON_FILE), &bytes)
}

/// Look for wakes, heard lines and companions coming online, once a
/// second, for as long as the app runs.
pub fn watch(app: tauri::AppHandle, companions: Arc<Companions>) {
    use tauri::Emitter;
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let tick = companions.tick();
        for name in tick.reconnect {
            let _ = mcp::restart(&name);
        }
        for event in tick.events {
            if event.kind == "wake" {
                crate::show_main(&app);
            }
            let _ = app.emit(EVENT, &event);
        }
    });
}

/// Devices → Companions. Each can write `companions.json` or `mcp.json`,
/// so none runs on the main thread.
#[tauri::command]
pub(crate) async fn get_companions(
    companions: State<'_, Arc<Companions>>,
) -> Result<CompanionsView, String> {
    let c = Arc::clone(&companions);
    crate::off_main(move || c.view()).await
}

#[tauri::command]
pub(crate) async fn start_companions(
    companions: State<'_, Arc<Companions>>,
) -> Result<CompanionsView, String> {
    let c = Arc::clone(&companions);
    crate::off_main(move || c.start()).await?
}

#[tauri::command]
pub(crate) async fn stop_companions(
    companions: State<'_, Arc<Companions>>,
) -> Result<CompanionsView, String> {
    let c = Arc::clone(&companions);
    crate::off_main(move || c.stop()).await?
}

#[tauri::command]
pub(crate) async fn new_companion_code(
    companions: State<'_, Arc<Companions>>,
) -> Result<CompanionsView, String> {
    let c = Arc::clone(&companions);
    crate::off_main(move || c.new_code()).await?
}

#[tauri::command]
pub(crate) async fn approve_companion(
    companions: State<'_, Arc<Companions>>,
    id: String,
    allowed: Vec<String>,
) -> Result<CompanionsView, String> {
    let c = Arc::clone(&companions);
    crate::off_main(move || c.approve(&id, &allowed)).await?
}

#[tauri::command]
pub(crate) async fn set_companion_allowed(
    companions: State<'_, Arc<Companions>>,
    id: String,
    allowed: Vec<String>,
) -> Result<CompanionsView, String> {
    let c = Arc::clone(&companions);
    crate::off_main(move || c.set_allowed(&id, &allowed)).await?
}

#[tauri::command]
pub(crate) async fn remove_companion(
    companions: State<'_, Arc<Companions>>,
    id: String,
) -> Result<CompanionsView, String> {
    let c = Arc::clone(&companions);
    crate::off_main(move || c.remove(&id)).await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::harness::{use_key_store_for, MemoryKeyStore};
    use serde_json::Value;
    use std::time::Duration;

    struct Dirs {
        config: PathBuf,
        _guard: grokhub_agent::perm::ConfigGuard,
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            mcp::shutdown_all();
            mcp::invalidate();
            let _ = std::fs::remove_dir_all(&self.config);
        }
    }

    fn dirs() -> Dirs {
        let config = std::env::temp_dir().join(grokhub_core::uid("shell-companions"));
        std::fs::create_dir_all(&config).expect("cfg");
        let guard = grokhub_agent::perm::ConfigGuard::set(&config);
        use_key_store_for(&config, Arc::new(MemoryKeyStore::new()));
        mcp::shutdown_all();
        mcp::invalidate();
        Dirs {
            config,
            _guard: guard,
        }
    }

    fn no_lan() -> Option<IpAddr> {
        None
    }

    fn loopback(config: &Path) -> Companions {
        Companions::with(config, IpAddr::V4(Ipv4Addr::LOCALHOST), 0, no_lan)
    }

    fn call(port: u16, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
        let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
            connect: Duration::from_secs(5),
            read: Some(Duration::from_secs(40)),
            ..Default::default()
        });
        let url = format!("http://127.0.0.1:{port}/node/v1{path}");
        let req = match method {
            "GET" => agent.get(&url),
            _ => agent.post(&url),
        }
        .any_status()
        .header("Authorization", &format!("Bearer {token}"));
        let resp = if method == "GET" {
            req.call()
        } else {
            req.send_json(body)
        }
        .expect("http");
        let status = resp.status();
        (status, resp.into_json().unwrap_or(Value::Null))
    }

    fn mcp_entry(config: &Path, name: &str) -> Option<Value> {
        let text = std::fs::read_to_string(config.join("mcp.json")).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        v["mcpServers"].get(name).cloned()
    }

    #[test]
    fn approving_adds_a_sealed_connection_and_removing_takes_it_away() {
        let d = dirs();
        let c = loopback(&d.config);
        let off = c.view();
        assert!(!off.on && off.code.is_empty() && off.companions.is_empty());
        assert_eq!(c.new_code(), Err("Start companions first.".into()));

        let on = c.start().expect("start");
        assert!(on.on && on.code.len() >= 6, "{on:?}");
        let port = on.port;
        let (status, paired) = call(
            port,
            "POST",
            "/pair",
            "",
            json!({ "code": on.code, "id": "pixel", "name": "Pixel", "platform": "android" }),
        );
        assert_eq!(status, 200, "{paired}");
        let token = paired["token"].as_str().expect("token").to_string();
        call(
            port,
            "POST",
            "/hello",
            &token,
            json!({ "offers": ["camera", "sms", "wake"] }),
        );
        let pending = c.view();
        assert_eq!(pending.code, "", "the code is spent by the pairing");
        assert_eq!(pending.companions.len(), 1);
        assert!(!pending.companions[0].approved);
        assert!(
            mcp_entry(&d.config, "companion-pixel").is_none(),
            "pairing alone adds nothing"
        );

        let approved = c
            .approve("pixel", &["camera".into(), "wake".into()])
            .expect("approve");
        assert_eq!(approved.companions[0].allowed, ["camera", "wake"]);
        let entry = mcp_entry(&d.config, "companion-pixel").expect("entry");
        assert_eq!(
            entry,
            json!({ "url": format!("http://127.0.0.1:{port}/node/v1/mcp/pixel"), "tokenRef": "companion-pixel" })
        );
        let sealed = grokhub_agent::harness::open_connection_token(&d.config, "companion-pixel");
        let key = lock(&c.host)
            .as_ref()
            .expect("host")
            .local_key()
            .to_string();
        assert_eq!(sealed.as_deref(), Some(key.as_str()));
        assert!(
            !std::fs::read_to_string(d.config.join("mcp.json"))
                .expect("mcp.json")
                .contains(&key),
            "the key is sealed, never in mcp.json"
        );

        c.remove("pixel").expect("remove");
        assert!(mcp_entry(&d.config, "companion-pixel").is_none());
        assert_eq!(
            grokhub_agent::harness::open_connection_token(&d.config, "companion-pixel"),
            None
        );
        let (status, _) = call(port, "GET", "/next", &token, Value::Null);
        assert_eq!(status, 401);
        assert_eq!(
            c.remove("pixel"),
            Err("That companion isn't paired anymore.".into())
        );
    }

    #[test]
    fn wakes_reach_the_page_and_a_companion_coming_online_reconnects_once() {
        let d = dirs();
        let c = Arc::new(loopback(&d.config));
        let on = c.start().expect("start");
        let port = on.port;
        let (_, paired) = call(
            port,
            "POST",
            "/pair",
            "",
            json!({ "code": on.code, "id": "pixel", "name": "Pixel", "platform": "android" }),
        );
        let token = paired["token"].as_str().expect("token").to_string();
        call(
            port,
            "POST",
            "/hello",
            &token,
            json!({ "offers": ["wake", "talk"] }),
        );
        c.approve("pixel", &["wake".into()]).expect("approve");
        assert_eq!(c.tick(), Tick::default(), "approved, not polling yet");

        let (status, _) = call(
            port,
            "POST",
            "/event",
            &token,
            json!({ "kind": "wake", "text": "" }),
        );
        assert_eq!(status, 200);
        let (status, _) = call(
            port,
            "POST",
            "/event",
            &token,
            json!({ "kind": "heard", "text": "what's on today" }),
        );
        assert_eq!(status, 403, "talk wasn't allowed");
        let poll = std::thread::spawn({
            let token = token.clone();
            move || call(port, "GET", "/next", &token, Value::Null)
        });
        let mut seen = Tick::default();
        for _ in 0..100 {
            let tick = c.tick();
            seen.events.extend(tick.events);
            seen.reconnect.extend(tick.reconnect);
            if !seen.reconnect.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            seen.events,
            [CompanionEvent {
                companion: "pixel".into(),
                name: "Pixel".into(),
                kind: "wake".into(),
                text: String::new(),
            }]
        );
        assert_eq!(seen.reconnect, ["companion-pixel"]);
        assert_eq!(
            c.tick(),
            Tick::default(),
            "online stays online: no second reconnect"
        );
        c.remove("pixel").expect("remove");
        assert_eq!(
            poll.join().expect("poll").0,
            200,
            "the held poll ends at removal"
        );
    }

    #[test]
    fn the_host_comes_back_at_launch_until_stopped_and_keeps_its_key() {
        let d = dirs();
        let first = loopback(&d.config);
        let on = first.start().expect("start");
        let (_, paired) = call(
            on.port,
            "POST",
            "/pair",
            "",
            json!({ "code": on.code, "id": "pixel", "name": "Pixel", "platform": "linux" }),
        );
        assert!(paired["token"].is_string());
        first.approve("pixel", &[]).expect("approve");
        let key = lock(&first.host)
            .as_ref()
            .expect("host")
            .local_key()
            .to_string();
        drop(first);

        let again = loopback(&d.config);
        again.resume();
        let view = again.view();
        assert!(view.on, "left on, so on again");
        assert_eq!(view.code, "", "no code opens by itself at launch");
        assert_eq!(view.companions[0].id, "pixel");
        assert_eq!(
            lock(&again.host).as_ref().expect("host").local_key(),
            key,
            "the sealed key is reused, so the saved connection still works"
        );
        assert!(!again.stop().expect("stop").on);
        drop(again);

        let third = loopback(&d.config);
        third.resume();
        let view = third.view();
        assert!(!view.on, "stopped stays stopped");
        assert_eq!(view.companions.len(), 1, "the paired list stays while off");
    }
}
