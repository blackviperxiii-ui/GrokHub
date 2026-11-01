//! World of Warcraft through Blizzard's Battle.net profile API
//! (`grokhub_core::battlenet_api`).
//!
//! Off until the player connects it. Under the game watcher in Settings: a
//! short how-to for registering their own Battle.net client, its Client ID
//! (kept beside the snapshot, not secret) and secret (system keychain
//! only), the region, and Connect. Connect opens the system browser at
//! Blizzard's own sign-in, listens on `localhost:47615` for its redirect,
//! and also takes the address pasted from the browser bar. The token goes to
//! the keychain only. While connected, the profile is read off the UI thread
//! every half hour and on Refresh, each call through the egress guard (so
//! Offline only stops it), and the WoW page in Library shows it. Nothing
//! touches the game.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::*;
use grokhub_core::battlenet_api::{
    self as bn, BnetProfile, Call, FetchError, Region, SendError, Token,
};

const KEYRING_SERVICE: &str = "GrokHub";
const SECRET_ACCOUNT: &str = "battlenet-client-secret";
const TOKEN_ACCOUNT: &str = "battlenet-wow-token";
const HOW_TO: &str = "Create a client at develop.battle.net (API Access) with the redirect URL http://localhost:47615/callback, then add its Client ID and secret here and press Connect. Blizzard gives profile data only to an app holding its own client secret, so Loadout can't ship one. Only your own WoW profile is read; your password goes to Blizzard's page, never to Loadout.";
/// How long Connect waits for Blizzard's redirect.
const CONNECT_SECS: u64 = 300;
const READ_CAP: usize = 16 * 1024;

#[derive(Default)]
pub(super) struct BnetUi {
    loaded: bool,
    client_id: Option<String>,
    region: Region,
    has_secret: Option<bool>,
    token: Option<Token>,
    profile: Option<BnetProfile>,
    id_draft: String,
    secret_draft: String,
    paste_draft: String,
    /// The `state` of the sign-in under way, and the flag that stops its
    /// loopback listener.
    signing_in: Option<(String, Arc<AtomicBool>)>,
    connect: Option<mpsc::Receiver<Result<Token, String>>>,
    pending: Option<mpsc::Receiver<Result<BnetProfile, FetchError>>>,
    next_ms: u64,
    note: String,
}

impl BnetUi {
    /// The profile snapshot to show, if any.
    pub(super) fn profile(&self) -> Option<&BnetProfile> {
        self.profile.as_ref()
    }
}

fn keyring_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, account).map_err(|e| e.to_string())
}

fn load_secret_value(account: &str) -> Option<String> {
    if cfg!(test) {
        return None;
    }
    match keyring_entry(account).ok()?.get_password() {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

fn store_secret_value(account: &str, value: Option<&str>) -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    let entry = keyring_entry(account)?;
    match value {
        Some(v) => entry.set_password(v).map_err(|e| e.to_string()),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        },
    }
}

fn load_token() -> Option<Token> {
    serde_json::from_str(&load_secret_value(TOKEN_ACCOUNT)?).ok()
}

fn store_token(token: Option<&Token>) -> Result<(), String> {
    let raw = token
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?;
    store_secret_value(TOKEN_ACCOUNT, raw.as_deref())
}

/// The client id and region aren't secret: they sit beside the snapshot.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct ClientFile {
    client_id: String,
    region: Region,
}

fn client_path(config_dir: &std::path::Path) -> PathBuf {
    config_dir.join("games").join("battlenet-client.json")
}

fn load_client() -> Option<ClientFile> {
    if cfg!(test) {
        return None;
    }
    let text = std::fs::read_to_string(client_path(&crate::config::config_dir())).ok()?;
    serde_json::from_str(&text).ok()
}

fn store_client(client: Option<&ClientFile>) {
    if cfg!(test) {
        return;
    }
    let path = client_path(&crate::config::config_dir());
    match client.and_then(|c| serde_json::to_string(c).ok()) {
        Some(text) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, text);
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn agent() -> grokhub_core::http::Agent {
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(8),
        overall: Some(Duration::from_secs(20)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

fn read_capped(resp: grokhub_core::http::Response) -> Result<String, String> {
    let mut text = String::new();
    resp.into_reader()
        .take(bn::MAX_BODY)
        .read_to_string(&mut text)
        .map_err(|_| "Battle.net's answer didn't arrive whole.".to_string())?;
    Ok(text)
}

/// One profile API call with the token. Only Blizzard's API host for the
/// token's region, never from tests.
fn api_send(
    agent: &grokhub_core::http::Agent,
    call: &Call,
    token: &Token,
) -> Result<String, SendError> {
    let base = format!("{}/profile/", token.region.api_base());
    if cfg!(test) || !call.url.starts_with(&base) {
        return Err(SendError::Other("Not sent.".into()));
    }
    crate::xai::egress_ok(&call.url, &[]).map_err(|_| {
        SendError::Other(
            "Offline only or the network guard held the Battle.net refresh back.".into(),
        )
    })?;
    let answer = agent
        .get(&call.url)
        .header("authorization", &format!("Bearer {}", token.access_token))
        .call();
    match answer {
        Ok(resp) => read_capped(resp).map_err(SendError::Other),
        Err(grokhub_core::http::Error::Status {
            code, retry_after, ..
        }) => Err(SendError::Status {
            code,
            retry_after_s: retry_after.and_then(|r| r.trim().parse().ok()),
        }),
        Err(_) => Err(SendError::Other("Couldn't reach Battle.net.".into())),
    }
}

/// Trade the code for a token at Blizzard's token endpoint, with the
/// client's id and secret as basic auth. Never from tests.
fn exchange(code: &str, client_id: &str, secret: &str, region: Region) -> Result<Token, String> {
    if cfg!(test) {
        return Err("Not sent.".into());
    }
    crate::xai::egress_ok(bn::TOKEN_URL, &[]).map_err(|_| {
        "Offline only or the network guard held the Battle.net sign-in back.".to_string()
    })?;
    let answer = agent()
        .post(bn::TOKEN_URL)
        .header("authorization", &bn::basic_auth(client_id, secret))
        .header("content-type", "application/x-www-form-urlencoded")
        .send_string(&bn::token_form(code, bn::REDIRECT_URI));
    let body = match answer {
        Ok(resp) => read_capped(resp)?,
        // Blizzard explains a refusal in the body.
        Err(grokhub_core::http::Error::Status { body, .. }) if body.contains("\"error\"") => body,
        Err(grokhub_core::http::Error::Status { code, .. }) => {
            return Err(format!("Battle.net's sign-in answered {code}."))
        }
        Err(_) => return Err("Couldn't reach Battle.net.".into()),
    };
    let token = bn::parse_token(&body, region, now_ms())?;
    store_token(Some(&token))
        .map_err(|e| format!("Couldn't save the Battle.net sign-in to the keychain: {e}"))?;
    Ok(token)
}

fn close_page() -> Vec<u8> {
    let body = b"<p>Battle.net is connected to Loadout. You can close this tab.</p>";
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

/// Wait on the loopback port for Blizzard's redirect, until it comes, the
/// wait runs out, or `stop` is set (the player pasted the address instead).
fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    stop: &AtomicBool,
) -> Option<Result<String, String>> {
    let deadline = Instant::now() + Duration::from_secs(CONNECT_SECS);
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Some(Err(e.to_string())),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; READ_CAP];
        let n = stream.read(&mut buf).unwrap_or(0).min(READ_CAP);
        let head = String::from_utf8_lossy(&buf[..n]);
        if !head.starts_with("GET /callback") {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            continue;
        }
        let _ = stream.write_all(&close_page());
        return Some(bn::callback_code(&head, state));
    }
    if stop.load(Ordering::Relaxed) {
        None
    } else {
        Some(Err(
            "Battle.net sign-in timed out. Press Connect to try again.".into(),
        ))
    }
}

impl Cabin {
    fn load_bnet(&mut self) {
        let b = &mut self.focus.bnet;
        if b.loaded {
            return;
        }
        b.loaded = true;
        if let Some(c) = load_client() {
            b.client_id = Some(c.client_id);
            b.region = c.region;
        }
        b.token = load_token();
        b.profile = if cfg!(test) {
            None
        } else {
            bn::load_cache(&crate::config::config_dir(), now_ms())
        };
    }

    pub(super) fn tick_bnet(&mut self) {
        self.load_bnet();
        let b = &mut self.focus.bnet;
        if let Some(rx) = &b.connect {
            match rx.try_recv() {
                Ok(done) => {
                    b.connect = None;
                    b.signing_in = None;
                    self.finish_bnet_connect(done);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    b.connect = None;
                    b.signing_in = None;
                }
            }
        }
        let b = &mut self.focus.bnet;
        if let Some(rx) = &b.pending {
            match rx.try_recv() {
                Ok(done) => {
                    b.pending = None;
                    self.finish_bnet(done, now_ms());
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => b.pending = None,
            }
        }
        let now = now_ms();
        let b = &self.focus.bnet;
        // Off until connected: no token, no call.
        if !self.cfg.game_watch || now < b.next_ms || !b.token.as_ref().is_some_and(|t| t.live(now))
        {
            return;
        }
        self.start_bnet_refresh();
    }

    fn start_bnet_refresh(&mut self) {
        let b = &mut self.focus.bnet;
        if b.pending.is_some() {
            return;
        }
        let now = now_ms();
        b.next_ms = now + bn::REFRESH_MS;
        let Some(token) = b.token.clone().filter(|t| t.live(now)) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        b.pending = Some(rx);
        std::thread::spawn(move || {
            let agent = agent();
            let region = token.region;
            let got =
                bn::fetch_profile(&mut |call| api_send(&agent, call, &token), region, now_ms());
            let _ = tx.send(got);
        });
    }

    fn finish_bnet(&mut self, done: Result<BnetProfile, FetchError>, now: u64) {
        let b = &mut self.focus.bnet;
        match done {
            Ok(p) => {
                if !cfg!(test) {
                    let _ = bn::save_cache(&crate::config::config_dir(), &p);
                }
                b.note.clear();
                b.profile = Some(p);
            }
            Err(e) => {
                match &e {
                    FetchError::RateLimited { retry_after_s } => {
                        b.next_ms = now + retry_after_s * 1000;
                    }
                    FetchError::SignIn(_) => {
                        let _ = store_token(None);
                        b.token = None;
                    }
                    FetchError::Other(_) => {}
                }
                b.note = e.message();
            }
        }
    }

    fn finish_bnet_connect(&mut self, done: Result<Token, String>) {
        let b = &mut self.focus.bnet;
        match done {
            Ok(token) => {
                b.token = Some(token);
                b.next_ms = 0;
                b.paste_draft.clear();
                b.note = "Battle.net connected. Reading your WoW profile...".into();
            }
            Err(e) => b.note = e,
        }
    }

    /// Open Blizzard's sign-in in the system browser and wait for its
    /// redirect on the loopback port, off the UI thread.
    fn connect_bnet(&mut self) {
        let b = &mut self.focus.bnet;
        let (Some(client_id), Some(secret)) =
            (b.client_id.clone(), load_secret_value(SECRET_ACCOUNT))
        else {
            b.note = "Add the client's ID and secret first.".into();
            return;
        };
        // A sign-in still waiting from an earlier press gives up its port.
        if let Some((_, old)) = b.signing_in.take() {
            old.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(120));
        }
        let state = grokhub_core::oauth_nonce();
        let stop = Arc::new(AtomicBool::new(false));
        let listener = TcpListener::bind(("127.0.0.1", bn::LOOPBACK_PORT))
            .ok()
            .filter(|l| l.set_nonblocking(true).is_ok());
        let url = bn::authorize_url(&client_id, bn::REDIRECT_URI, &state);
        if let Err(e) = crate::desktop::open_url(&url) {
            b.note = format!("Couldn't open the browser: {e}");
            return;
        }
        b.note = if listener.is_some() {
            "Sign in on Battle.net's page in your browser. If it sends you to a page that doesn't load, paste that page's address below.".into()
        } else {
            "Sign in on Battle.net's page in your browser, then paste the address it sends you to below (port 47615 is busy).".into()
        };
        b.signing_in = Some((state.clone(), stop.clone()));
        let region = b.region;
        // Without the port, only a pasted address answers.
        let Some(listener) = listener else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        b.connect = Some(rx);
        std::thread::spawn(move || {
            let Some(code) = wait_for_code(&listener, &state, &stop) else {
                return;
            };
            let _ = tx.send(code.and_then(|c| exchange(&c, &client_id, &secret, region)));
        });
    }

    /// The address the player pasted from the browser bar after signing in.
    fn take_bnet_paste(&mut self) {
        let b = &mut self.focus.bnet;
        let Some((state, stop)) = b.signing_in.clone() else {
            b.note = "Press Connect first.".into();
            return;
        };
        let code = match bn::callback_code(&b.paste_draft, &state) {
            Ok(code) => code,
            Err(e) => {
                b.note = e;
                return;
            }
        };
        let (Some(client_id), Some(secret)) =
            (b.client_id.clone(), load_secret_value(SECRET_ACCOUNT))
        else {
            b.note = "Add the client's ID and secret first.".into();
            return;
        };
        stop.store(true, Ordering::Relaxed);
        let region = b.region;
        let (tx, rx) = mpsc::channel();
        b.connect = Some(rx);
        b.note = "Connecting...".into();
        std::thread::spawn(move || {
            let _ = tx.send(exchange(&code, &client_id, &secret, region));
        });
    }

    fn save_bnet_client(&mut self) {
        let b = &mut self.focus.bnet;
        let id = b.id_draft.trim().to_string();
        let secret = b.secret_draft.trim().to_string();
        if !bn::looks_like_client_value(&id) {
            b.note = "That doesn't look like a Battle.net Client ID: copy it from your client's page on develop.battle.net.".into();
            return;
        }
        if !bn::looks_like_client_value(&secret) {
            b.note = "That doesn't look like a Battle.net client secret: copy it from your client's page on develop.battle.net.".into();
            return;
        }
        if let Err(e) = store_secret_value(SECRET_ACCOUNT, Some(&secret)) {
            b.note = format!("Couldn't save the secret to the keychain: {e}");
            return;
        }
        store_client(Some(&ClientFile {
            client_id: id.clone(),
            region: b.region,
        }));
        b.client_id = Some(id);
        b.has_secret = Some(true);
        b.id_draft.clear();
        b.secret_draft.clear();
        b.note = "Client saved. Press Connect to sign in on Battle.net.".into();
    }

    fn set_bnet_region(&mut self, region: Region) {
        let b = &mut self.focus.bnet;
        if b.region == region {
            return;
        }
        b.region = region;
        if let Some(id) = b.client_id.clone() {
            store_client(Some(&ClientFile {
                client_id: id,
                region,
            }));
        }
        // A token is for one region's profile: connect again there.
        let _ = store_token(None);
        b.token = None;
        b.profile = None;
        if !cfg!(test) {
            let _ = std::fs::remove_file(bn::cache_path(&crate::config::config_dir()));
        }
    }

    /// Drop the token and the snapshot; with `client` also the client.
    fn disconnect_bnet(&mut self, client: bool) {
        let b = &mut self.focus.bnet;
        if let Err(e) = store_token(None) {
            b.note = format!("Couldn't remove the sign-in: {e}");
            return;
        }
        if let Some((_, stop)) = b.signing_in.take() {
            stop.store(true, Ordering::Relaxed);
        }
        b.connect = None;
        b.token = None;
        b.profile = None;
        if !cfg!(test) {
            let _ = std::fs::remove_file(bn::cache_path(&crate::config::config_dir()));
        }
        b.note = "Battle.net disconnected; its snapshot is deleted.".into();
        if client {
            if let Err(e) = store_secret_value(SECRET_ACCOUNT, None) {
                b.note = format!("Couldn't remove the client secret: {e}");
                return;
            }
            store_client(None);
            b.client_id = None;
            b.has_secret = Some(false);
            b.note = "Battle.net disconnected; client and snapshot removed.".into();
        }
    }

    /// The Battle.net rows under the game watcher.
    pub(super) fn ui_bnet(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.game_watch {
            return;
        }
        self.load_bnet();
        let now = now_ms();
        let has_secret = *self
            .focus
            .bnet
            .has_secret
            .get_or_insert_with(|| load_secret_value(SECRET_ACCOUNT).is_some());
        let b = &self.focus.bnet;
        let has_client = has_secret && b.client_id.is_some();
        let connected = b.token.as_ref().is_some_and(|t| t.live(now));
        let line = bnet_status(b.profile.as_ref(), has_client, connected, now);
        let (note, busy, signing_in, region) = (
            b.note.clone(),
            b.pending.is_some(),
            b.signing_in.is_some(),
            b.region,
        );
        let mut act = BnetAct::None;
        ui.indent("battlenet-api", |ui| {
            crate::cards::help_text(ui, &line);
            if !has_client {
                crate::cards::help_text(ui, HOW_TO);
                crate::cards::settings_field(
                    ui,
                    "Battle.net Client ID",
                    "From your client's page on develop.battle.net.",
                    &mut self.focus.bnet.id_draft,
                    false,
                );
                crate::cards::settings_field(
                    ui,
                    "Battle.net client secret",
                    "It stays in your system keychain.",
                    &mut self.focus.bnet.secret_draft,
                    true,
                );
                if crate::cards::settings_action(
                    ui,
                    "Battle.net client",
                    "Saved on this PC only.",
                    "Save client",
                ) {
                    act = BnetAct::SaveClient;
                }
            }
            ui.horizontal(|ui| {
                crate::cards::help_text(ui, "Region:");
                for r in Region::ALL {
                    if ui.selectable_label(region == r, r.label()).clicked() {
                        act = BnetAct::Region(r);
                    }
                }
            });
            if has_client {
                ui.horizontal(|ui| {
                    if busy {
                        crate::cards::help_text(ui, "Refreshing...");
                    } else if connected {
                        if crate::cards::ghost_pill(ui, "Refresh Battle.net") {
                            act = BnetAct::Refresh;
                        }
                    } else if crate::cards::white_pill(ui, "Connect Battle.net") {
                        act = BnetAct::Connect;
                    }
                    if (connected || self.focus.bnet.profile.is_some())
                        && crate::cards::ghost_pill(ui, "Disconnect")
                    {
                        act = BnetAct::Disconnect;
                    }
                    if crate::cards::ghost_pill(ui, "Remove client") {
                        act = BnetAct::Forget;
                    }
                });
                if signing_in {
                    crate::cards::settings_field(
                        ui,
                        "Address after signing in",
                        "Only if the browser didn't come back by itself.",
                        &mut self.focus.bnet.paste_draft,
                        true,
                    );
                    if crate::cards::ghost_pill(ui, "Use this address") {
                        act = BnetAct::Paste;
                    }
                }
            }
            if !note.is_empty() {
                crate::cards::help_text(ui, &note);
            }
        });
        match act {
            BnetAct::None => {}
            BnetAct::SaveClient => self.save_bnet_client(),
            BnetAct::Region(r) => self.set_bnet_region(r),
            BnetAct::Connect => self.connect_bnet(),
            BnetAct::Paste => self.take_bnet_paste(),
            BnetAct::Refresh => {
                self.focus.bnet.next_ms = 0;
                self.start_bnet_refresh();
            }
            BnetAct::Disconnect => self.disconnect_bnet(false),
            BnetAct::Forget => self.disconnect_bnet(true),
        }
    }

    /// The Battle.net snapshot on the WoW page in Library: characters,
    /// collections, and the top characters' raids and gear.
    pub(super) fn ui_bnet_profile(&mut self, ui: &mut egui::Ui) {
        self.load_bnet();
        let Some(p) = self.focus.bnet.profile().cloned() else {
            return;
        };
        let muted = |ui: &mut egui::Ui, text: &str| {
            ui.label(
                egui::RichText::new(text)
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
            );
        };
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!(
                "Battle.net · {} · {}",
                p.summary_line(),
                age(now_ms(), p.fetched_ms)
            ))
            .size(crate::theme::TYPE_CAPTION)
            .color(crate::theme::fg()),
        );
        for (i, c) in p.characters.iter().enumerate() {
            let detail = c.gear.len() + c.raids.len();
            if i >= bn::DETAILED || detail == 0 {
                if i < 8 {
                    muted(ui, &c.line());
                }
                continue;
            }
            egui::CollapsingHeader::new(
                egui::RichText::new(c.line())
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::fg()),
            )
            .id_salt(("bnet-char", &c.realm_slug, &c.name))
            .show(ui, |ui| {
                for line in c.raid_lines() {
                    muted(ui, &line);
                }
                for item in &c.gear {
                    let level = item.level.map(|l| format!(" · {l}")).unwrap_or_default();
                    muted(ui, &format!("{}: {}{level}", item.slot, item.name));
                }
            });
        }
    }
}

enum BnetAct {
    None,
    SaveClient,
    Region(Region),
    Connect,
    Paste,
    Refresh,
    Disconnect,
    Forget,
}

fn age(now: u64, then: u64) -> String {
    match now.saturating_sub(then) / 60_000 {
        0 => "updated just now".to_string(),
        1 => "updated a minute ago".to_string(),
        m if m < 120 => format!("updated {m} minutes ago"),
        m if m < 48 * 60 => format!("updated {} hours ago", m / 60),
        m => format!("updated {} days ago", m / (24 * 60)),
    }
}

/// Where setup and the snapshot stand, in one line.
fn bnet_status(p: Option<&BnetProfile>, has_client: bool, connected: bool, now: u64) -> String {
    match (p, has_client, connected) {
        (_, false, _) => "Battle.net (WoW profile): off. Add your own Battle.net client to read your characters, collections, gear and raid progress without the game running.".into(),
        (None, true, false) => "Battle.net (WoW profile): client saved, not connected.".into(),
        (None, true, true) => "Battle.net (WoW profile): connected, nothing read yet.".into(),
        (Some(p), true, live) => format!(
            "Battle.net (WoW profile, {}): {}, {}.{}",
            p.region.label(),
            p.summary_line(),
            age(now, p.fetched_ms),
            if live { "" } else { " Connect again to refresh it: Blizzard's sign-in lasts about a day." }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(region: Region, expires_at_ms: u64) -> Token {
        Token {
            access_token: "tok".into(),
            expires_at_ms,
            region,
        }
    }

    #[test]
    fn nothing_is_sent_from_tests_or_to_other_hosts() {
        let agent = agent();
        let t = token(Region::Us, u64::MAX);
        for url in [
            "https://example.com/profile/user/wow",
            "https://eu.api.blizzard.com/profile/user/wow",
            "https://us.api.blizzard.com/profile/user/wow",
        ] {
            let call = Call { url: url.into() };
            assert!(matches!(
                api_send(&agent, &call, &t),
                Err(SendError::Other(_))
            ));
        }
        assert!(exchange("code", "id", "secret", Region::Us).is_err());
    }

    #[test]
    fn it_stays_off_until_connected() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_watch = true;
        cabin.tick_bnet();
        assert!(cabin.focus.bnet.pending.is_none());
        // An ended token doesn't refresh either.
        cabin.focus.bnet.token = Some(token(Region::Us, 1));
        cabin.tick_bnet();
        assert!(cabin.focus.bnet.pending.is_none());
        // With the game watcher off nothing runs, even connected.
        cabin.cfg.game_watch = false;
        cabin.focus.bnet.token = Some(token(Region::Us, u64::MAX));
        cabin.tick_bnet();
        assert!(cabin.focus.bnet.pending.is_none());
    }

    #[test]
    fn the_client_is_checked_before_it_is_kept_and_removed_on_forget() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.bnet.loaded = true;
        cabin.focus.bnet.id_draft = "short".into();
        cabin.focus.bnet.secret_draft = "0123456789abcdef0123456789abcdef".into();
        cabin.save_bnet_client();
        assert_eq!(cabin.focus.bnet.client_id, None);
        assert!(cabin.focus.bnet.note.contains("Client ID"));
        cabin.focus.bnet.id_draft = "0123456789abcdef0123456789abcdef".into();
        cabin.save_bnet_client();
        assert_eq!(
            cabin.focus.bnet.client_id.as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(cabin.focus.bnet.has_secret, Some(true));
        assert!(cabin.focus.bnet.secret_draft.is_empty());
        cabin.focus.bnet.token = Some(token(Region::Us, u64::MAX));
        cabin.focus.bnet.profile = Some(BnetProfile::default());
        cabin.disconnect_bnet(true);
        assert_eq!(cabin.focus.bnet.client_id, None);
        assert_eq!(cabin.focus.bnet.token, None);
        assert_eq!(cabin.focus.bnet.profile, None);
    }

    #[test]
    fn an_ended_sign_in_clears_the_token_and_a_rate_limit_waits() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.bnet.token = Some(token(Region::Us, u64::MAX));
        cabin.finish_bnet(Err(FetchError::SignIn("ended".into())), 1_000);
        assert_eq!(cabin.focus.bnet.token, None);
        assert_eq!(cabin.focus.bnet.note, "ended");
        cabin.finish_bnet(Err(FetchError::RateLimited { retry_after_s: 30 }), 1_000);
        assert_eq!(cabin.focus.bnet.next_ms, 31_000);
        cabin.finish_bnet(Ok(BnetProfile::default()), 2_000);
        assert!(cabin.focus.bnet.profile.is_some());
        assert!(cabin.focus.bnet.note.is_empty());
    }

    #[test]
    fn a_pasted_address_needs_a_sign_in_under_way_and_its_state() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.bnet.paste_draft = "https://localhost/callback?code=c&state=s1".into();
        cabin.take_bnet_paste();
        assert_eq!(cabin.focus.bnet.note, "Press Connect first.");
        let stop = Arc::new(AtomicBool::new(false));
        cabin.focus.bnet.signing_in = Some(("s2".into(), stop.clone()));
        cabin.take_bnet_paste();
        assert!(cabin.focus.bnet.note.contains("another attempt"));
        assert!(!stop.load(Ordering::Relaxed));
    }

    #[test]
    fn changing_region_drops_the_token_and_the_snapshot() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.bnet.token = Some(token(Region::Us, u64::MAX));
        cabin.focus.bnet.profile = Some(BnetProfile::default());
        cabin.set_bnet_region(Region::Eu);
        assert_eq!(cabin.focus.bnet.region, Region::Eu);
        assert_eq!(cabin.focus.bnet.token, None);
        assert_eq!(cabin.focus.bnet.profile, None);
    }

    #[test]
    fn the_loopback_answers_the_callback_and_stops_when_asked() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let waiter = std::thread::spawn(move || wait_for_code(&listener, "s1", &s2));
        let mut other = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        other
            .write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n")
            .unwrap();
        let mut reply = String::new();
        other.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 404"));
        let mut cb = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        cb.write_all(b"GET /callback?code=c-9&state=s1 HTTP/1.1\r\n\r\n")
            .unwrap();
        let mut reply = String::new();
        cb.read_to_string(&mut reply).unwrap();
        assert!(reply.contains("You can close this tab"));
        assert_eq!(waiter.join().unwrap(), Some(Ok("c-9".to_string())));
        // A stop ends the wait without an answer.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        stop.store(true, Ordering::Relaxed);
        assert_eq!(wait_for_code(&listener, "s1", &stop), None);
    }

    #[test]
    fn the_status_line_says_where_setup_and_snapshot_stand() {
        assert!(bnet_status(None, false, false, 0).contains("off"));
        assert!(bnet_status(None, true, false, 0).contains("not connected"));
        let p = BnetProfile {
            characters: vec![Default::default(); 2],
            mounts: Some(300),
            fetched_ms: 0,
            ..Default::default()
        };
        assert_eq!(
            bnet_status(Some(&p), true, true, 3 * 60_000),
            "Battle.net (WoW profile, Americas): 2 characters · 300 mounts, updated 3 minutes ago."
        );
        assert!(bnet_status(Some(&p), true, false, 0).contains("Connect again"));
    }
}
