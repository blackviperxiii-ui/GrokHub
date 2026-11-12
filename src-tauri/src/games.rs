//! Per-game companion screens for the Library slot (card 712).
//!
//! Thin wrappers over `grokhub_core::{wow_addon, wow_saved, game_watch_wow,
//! gw2_api, eso_addon, osrs_hiscores, battlenet}`. View models mirror the
//! cabin's wording. The GW2 API key goes through a credentials park into the
//! OS keychain and never appears in a response or cache file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use grokhub_agent::harness::{self as hx, ParkRequest};
use grokhub_core::battlenet::{self as bnet, BnetGame, WowFlavor};
use grokhub_core::eso_addon::{self as eso, AddonState as EsoAddonState};
use grokhub_core::game_watch_wow as wow_watch;
use grokhub_core::gw2_api::{self as gw2, FetchError, GetError, Gw2Account};
use grokhub_core::osrs_hiscores::{self as osrs, AccountType, OsrsSaved, OsrsStats};
use grokhub_core::wow_addon::{self as wow_addon, AddonState as WowAddonState};
use grokhub_core::wow_saved::{self, WowSaved};
use serde::Serialize;

use crate::credential;

const GW2_KEYRING_ACCOUNT: &str = "gw2-api-key";
const GW2_OFFLINE: &str = "Offline only or the network guard held the Guild Wars 2 refresh back.";
const OSRS_OFFLINE: &str = "Offline only or the network guard held the hiscores refresh back.";
const WATCHER_OFF: &str = "Turn on the game watcher in Settings → Your PC → App & extras to follow boss pulls from WoW's own combat log (type /combatlog in game).";
const WOW_EMPTY_SAVED: &str =
    "Log in, then /reload or log out: WoW saves addon data then, and it shows here.";

/// How long a GW2 key card waits before it counts as Deny.
pub const KEY_CARD_WAIT: Duration = hx::APPROVAL_TTL;

// --- WoW -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WowView {
    pub title: String,
    pub running: bool,
    pub caption: String,
    pub logging: Option<String>,
    pub watcher_note: Option<String>,
    pub flavors: Vec<WowFlavorRow>,
    pub install_all: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WowFlavorRow {
    pub install: String,
    pub flavor: String,
    pub label: String,
    pub addon_line: String,
    pub action: Option<String>,
    pub characters: Vec<String>,
    pub events: Vec<String>,
    pub empty_saved: Option<String>,
}

fn wow_addon_line(addon: &WowAddonState) -> String {
    match addon {
        WowAddonState::NotInstalled => "Loadout addon not installed".into(),
        WowAddonState::Current { version } => format!("Loadout addon {version}"),
        WowAddonState::Outdated { version } => format!("Loadout addon {version}, update ready"),
        WowAddonState::Foreign => "Another addon named Loadout is installed".into(),
    }
}

fn wow_addon_button(addon: &WowAddonState) -> Option<&'static str> {
    match addon {
        WowAddonState::NotInstalled => Some("Install addon"),
        WowAddonState::Outdated { .. } => Some("Update addon"),
        WowAddonState::Current { .. } => Some("Remove addon"),
        WowAddonState::Foreign => None,
    }
}

fn wow_recent_lines(saved: &WowSaved, max: usize) -> Vec<String> {
    saved
        .events
        .iter()
        .rev()
        .filter_map(|e| {
            let who = e.character.split('-').next().unwrap_or("");
            match e.kind.as_str() {
                "kill" => Some(format!("Killed {} ({who})", e.name)),
                "level" => e.level.map(|l| format!("{who} reached level {l}")),
                "achievement" if !e.name.is_empty() => Some(format!("{who} earned {}", e.name)),
                _ => None,
            }
        })
        .take(max)
        .collect()
}

fn wow_flavor_row(install: &Path, flavor: WowFlavor) -> WowFlavorRow {
    let dir = flavor.dir(install);
    let _ = wow_addon::sync(install, &dir);
    let addon = wow_addon::state(&dir);
    let saved = wow_saved::read_flavor(&dir);
    let characters: Vec<String> = saved
        .characters
        .iter()
        .take(3)
        .map(wow_saved::character_line)
        .collect();
    let events = wow_recent_lines(&saved, 3);
    let empty_saved = if characters.is_empty() && events.is_empty() {
        Some(WOW_EMPTY_SAVED.to_string())
    } else {
        None
    };
    WowFlavorRow {
        install: install.to_string_lossy().into_owned(),
        flavor: flavor.folder().to_string(),
        label: flavor.label().to_string(),
        addon_line: wow_addon_line(&addon),
        action: wow_addon_button(&addon).map(str::to_string),
        characters,
        events,
        empty_saved,
    }
}

fn wow_installs(games: &[BnetGame]) -> Vec<&BnetGame> {
    let mut seen = std::collections::BTreeSet::new();
    games
        .iter()
        .filter(|g| g.is_wow() && seen.insert(g.install.clone()))
        .collect()
}

/// Build the WoW companion view from detected Battle.net installs.
///
/// `running_dir` is the flavor folder currently running, when known.
pub fn wow_view_from(
    games: &[BnetGame],
    game_watch: bool,
    running_dir: Option<&Path>,
    now: SystemTime,
) -> WowView {
    let installs = wow_installs(games);
    let mut flavors = Vec::new();
    let mut labels = Vec::new();
    for g in &installs {
        for f in bnet::wow_flavors(&g.install) {
            labels.push(f.label().to_string());
            flavors.push(wow_flavor_row(&g.install, f));
        }
    }
    let running = running_dir.is_some();
    let logging = if !game_watch {
        None
    } else if let Some(dir) = running_dir {
        let on = wow_watch::logging_since(dir, now - Duration::from_secs(600));
        Some(
            if on {
                "Combat log on: finished keys and boss pulls post to Pulse."
            } else {
                "Combat log off: type /combatlog in game so Loadout can follow keys and pulls."
            }
            .to_string(),
        )
    } else {
        None
    };
    let missing = flavors
        .iter()
        .filter(|f| {
            f.action.as_deref() == Some("Install addon")
                || f.action.as_deref() == Some("Update addon")
        })
        .count();
    let install_all = if missing > 1 {
        Some(format!("Install addon in all {missing} versions"))
    } else {
        None
    };
    let caption = if labels.is_empty() {
        "Battle.net".into()
    } else {
        let wine = installs.iter().any(|g| g.prefix.is_some());
        let joined = labels.join(", ");
        if wine {
            format!("{joined} · Battle.net in a Wine prefix")
        } else {
            format!("{joined} · Battle.net")
        }
    };
    WowView {
        title: "WORLD OF WARCRAFT".into(),
        running,
        caption,
        logging,
        watcher_note: (!game_watch).then(|| WATCHER_OFF.to_string()),
        flavors,
        install_all,
        note: String::new(),
    }
}

/// Live process sample for WoW (Linux `/proc`; empty elsewhere).
fn wow_running_dir(installs: &[&BnetGame]) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let mut procs = Vec::new();
        let Ok(rd) = std::fs::read_dir("/proc") else {
            return None;
        };
        for e in rd.flatten() {
            let pid = e.file_name();
            if !pid.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let base = e.path();
            let comm = std::fs::read_to_string(base.join("comm")).unwrap_or_default();
            let cmdline = std::fs::read(base.join("cmdline"))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            procs.push((comm.trim().to_string(), cmdline));
        }
        let running = bnet::wow_running(&procs)?;
        installs
            .iter()
            .find_map(|g| bnet::running_flavor_dir(&g.install, running).map(|(_, d)| d))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = installs;
        None
    }
}

/// Live WoW companion read.
pub fn wow_view(game_watch: bool) -> WowView {
    let games = bnet::detect();
    let installs = wow_installs(&games);
    let running = wow_running_dir(&installs);
    wow_view_from(&games, game_watch, running.as_deref(), SystemTime::now())
}

fn parse_flavor(folder: &str) -> Option<WowFlavor> {
    WowFlavor::ALL.into_iter().find(|f| f.folder() == folder)
}

/// Install, update, remove, or install-all for a WoW flavor.
pub fn wow_addon_action(
    install: &Path,
    flavor_folder: &str,
    action: &str,
) -> Result<WowView, String> {
    match action {
        "install_all" => {
            let results = wow_addon::install_all(install);
            let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
            let mut view = wow_view(true);
            view.note = if ok == 0 {
                results
                    .into_iter()
                    .find_map(|(_, r)| r.err())
                    .unwrap_or_else(|| "Couldn't install the Loadout addon.".into())
            } else {
                format!(
                    "Installed the Loadout addon for WoW. Quit and restart WoW so it loads; then open the minimap menu or type /loadout."
                )
            };
            Ok(view)
        }
        "install" | "update" => {
            let flavor = parse_flavor(flavor_folder)
                .ok_or_else(|| "That isn't a WoW version.".to_string())?;
            let dir = flavor.dir(install);
            wow_addon::install(install, &dir)?;
            let mut view = wow_view(true);
            view.note = format!(
                "Installed the Loadout addon for WoW {}. Quit and restart WoW so it loads; then open the minimap menu or type /loadout.",
                flavor.label()
            );
            Ok(view)
        }
        "remove" => {
            let flavor = parse_flavor(flavor_folder)
                .ok_or_else(|| "That isn't a WoW version.".to_string())?;
            let dir = flavor.dir(install);
            wow_addon::remove(&dir)?;
            let mut view = wow_view(true);
            view.note = format!(
                "Removed the Loadout addon from WoW {}. What it saved stays in WoW's WTF folder.",
                flavor.label()
            );
            Ok(view)
        }
        _ => Err("That isn't a WoW addon action.".into()),
    }
}

// --- GW2 -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gw2View {
    pub status: String,
    pub has_key: bool,
    pub account_line: Option<String>,
    pub note: String,
    pub key_hint: String,
}

fn gw2_status(acct: Option<&Gw2Account>, has_key: bool, now: u64) -> String {
    match (acct, has_key) {
        (_, false) => {
            "Guild Wars 2: add a read-only API key to read your account through ArenaNet's official API."
                .into()
        }
        (None, true) => "Guild Wars 2: key saved, nothing read yet.".into(),
        (Some(a), true) => {
            let mins = now.saturating_sub(a.fetched_ms) / 60_000;
            let age = match mins {
                0 => "just now".to_string(),
                1 => "a minute ago".to_string(),
                m if m < 120 => format!("{m} minutes ago"),
                m => format!("{} hours ago", m / 60),
            };
            format!("{}: {} Updated {age}.", gw2::NAME, a.account_line())
        }
    }
}

fn gw2_keyring() -> Result<keyring::Entry, String> {
    keyring::Entry::new(credential::SERVICE, GW2_KEYRING_ACCOUNT)
        .map_err(|e| format!("The system keychain is not available: {e}"))
}

fn gw2_load_key() -> Option<String> {
    if cfg!(test) {
        return TEST_GW2_KEY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
    }
    match gw2_keyring().ok()?.get_password() {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    }
}

fn gw2_store_key(key: Option<&str>) -> Result<(), String> {
    if cfg!(test) {
        *TEST_GW2_KEY.lock().unwrap_or_else(|e| e.into_inner()) =
            key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        return Ok(());
    }
    let entry = gw2_keyring()?;
    match key {
        Some(k) => entry
            .set_password(k.trim())
            .map_err(|e| format!("Couldn't save the Guild Wars 2 key: {e}")),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Couldn't remove the Guild Wars 2 key: {e}")),
        },
    }
}

static TEST_GW2_KEY: Mutex<Option<String>> = Mutex::new(None);

/// Read-only GW2 companion view (key never returned).
pub fn gw2_view(config_dir: &Path, now_ms: u64) -> Gw2View {
    let has_key = gw2_load_key().is_some();
    let account = gw2::load_cache(config_dir);
    Gw2View {
        status: gw2_status(account.as_ref(), has_key, now_ms),
        has_key,
        account_line: account.as_ref().map(|a| a.account_line()),
        note: String::new(),
        key_hint: "Make a key at account.arena.net under Applications with the account, characters, progression and wallet scopes. It stays in your system keychain and only reads.".into(),
    }
}

fn gw2_api_agent() -> grokhub_core::http::Agent {
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(8),
        overall: Some(Duration::from_secs(20)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

fn gw2_egress(config_dir: &Path, url: &str) -> Result<(), String> {
    if !url.starts_with(gw2::API_BASE) {
        return Err("Not sent.".into());
    }
    hx::guard_quiet(config_dir, &hx::EgressReq::new(url, &[])).map_err(|_| GW2_OFFLINE.to_string())
}

fn gw2_api_get(config_dir: &Path, url: &str, key: &str) -> Result<String, GetError> {
    if cfg!(test) {
        return Err(GetError::Other("Not sent.".into()));
    }
    gw2_egress(config_dir, url).map_err(GetError::Other)?;
    let resp = gw2_api_agent()
        .get(url)
        .header("authorization", &format!("Bearer {key}"))
        .call()
        .map_err(|e| match e {
            grokhub_core::http::Error::Status {
                code, retry_after, ..
            } => GetError::Status {
                code,
                retry_after_s: retry_after.and_then(|r| r.trim().parse().ok()),
            },
            _ => GetError::Other("Couldn't reach ArenaNet's API.".into()),
        })?;
    let mut body = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(resp.into_reader(), gw2::MAX_BODY),
        &mut body,
    )
    .map_err(|_| GetError::Other("ArenaNet's API answer didn't arrive whole.".into()))?;
    Ok(body)
}

/// Refresh the GW2 account through ArenaNet (egress-guarded).
pub fn gw2_refresh(config_dir: &Path, now_ms: u64) -> Result<Gw2View, String> {
    let Some(key) = gw2_load_key() else {
        let mut v = gw2_view(config_dir, now_ms);
        v.note = "Add a Guild Wars 2 API key first.".into();
        return Ok(v);
    };
    let cfg = config_dir.to_path_buf();
    let got = gw2::fetch_account(&mut |url| gw2_api_get(&cfg, url, &key), now_ms);
    let mut view = gw2_view(config_dir, now_ms);
    match got {
        Ok(acct) => {
            let _ = gw2::save_cache(config_dir, &acct);
            view.note = if acct.missing.is_empty() {
                String::new()
            } else {
                format!(
                    "The key has no {} scope, so that part isn't read.",
                    acct.missing.join(", ")
                )
            };
            view.status = gw2_status(Some(&acct), true, now_ms);
            view.account_line = Some(acct.account_line());
            view.has_key = true;
            Ok(view)
        }
        Err(e) => {
            view.note = match e {
                FetchError::RateLimited { .. } => e.message(),
                FetchError::Other(m) if m.contains("Offline only") || m == "Not sent." => {
                    if m == "Not sent." {
                        "Couldn't refresh Guild Wars 2 just now.".into()
                    } else {
                        m
                    }
                }
                other => other.message(),
            };
            Ok(view)
        }
    }
}

/// Offline-only path used by tests: forces the cabin offline row without dialing.
pub fn gw2_refresh_offline_row(config_dir: &Path, now_ms: u64) -> Gw2View {
    let mut v = gw2_view(config_dir, now_ms);
    v.note = GW2_OFFLINE.into();
    v
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gw2KeyCard {
    pub id: String,
    pub action: String,
}

type Gw2Waiter = (PathBuf, JoinHandle<Result<(), String>>);
static GW2_KEY_WAITERS: Mutex<Option<HashMap<String, Gw2Waiter>>> = Mutex::new(None);

/// Post the credentials card; Approve seals the key into the keychain.
pub fn gw2_start_key(config_dir: &Path, key: String) -> Result<Gw2KeyCard, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Type the Guild Wars 2 API key first.".into());
    }
    if !gw2::looks_like_key(&key) {
        return Err("That doesn't look like a Guild Wars 2 API key.".into());
    }
    let id = format!("gw2-key-{}", grokhub_core::pkce::oauth_nonce());
    let action = "Save the Guild Wars 2 API key".to_string();
    hx::post_park(
        config_dir,
        &ParkRequest {
            id: id.clone(),
            path: "E".into(),
            tool: "game_gw2".into(),
            action: action.clone(),
            class: "credentials".into(),
            ts_ms: grokhub_core::now_ms(),
        },
    )?;
    let waiter = std::thread::spawn({
        let (id, config) = (id.clone(), config_dir.to_path_buf());
        move || {
            let _pin = grokhub_agent::perm::ConfigGuard::set(config.clone());
            let ok = hx::wait_park(
                &config,
                &id,
                KEY_CARD_WAIT,
                Duration::from_millis(100),
                &mut || false,
            );
            if !ok {
                return Err("Nothing saved. The Guild Wars 2 key was denied.".into());
            }
            gw2_store_key(Some(&key))
        }
    });
    GW2_KEY_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(id.clone(), (config_dir.to_path_buf(), waiter));
    Ok(Gw2KeyCard { id, action })
}

/// Save or Deny on the GW2 credentials card.
pub fn gw2_answer_key(
    config_dir: &Path,
    id: &str,
    approve: bool,
    now_ms: u64,
) -> Result<Gw2View, String> {
    let (park_dir, waiter) = GW2_KEY_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .remove(id)
        .ok_or("That key card is closed. Type the key again.")?;
    hx::answer_park(&park_dir, id, approve)?;
    let status = match waiter.join() {
        Ok(Ok(())) => {
            if approve {
                "Key saved. Reading your account...".to_string()
            } else {
                "Nothing saved. The Guild Wars 2 key was denied.".to_string()
            }
        }
        Ok(Err(err)) => err,
        Err(_) => "Nothing saved.".to_string(),
    };
    let mut view = if approve && gw2_load_key().is_some() {
        match gw2_refresh(config_dir, now_ms) {
            Ok(mut v) => {
                if v.note.is_empty() {
                    v.note = status;
                }
                v
            }
            Err(_) => {
                let mut v = gw2_view(config_dir, now_ms);
                v.note = status;
                v
            }
        }
    } else {
        let mut v = gw2_view(config_dir, now_ms);
        v.note = status;
        v
    };
    if !approve {
        view.has_key = gw2_load_key().is_some();
    }
    Ok(view)
}

/// Remove the GW2 key and cache.
pub fn gw2_remove_key(config_dir: &Path, now_ms: u64) -> Result<Gw2View, String> {
    gw2_store_key(None)?;
    let _ = std::fs::remove_file(gw2::cache_path(config_dir));
    let mut view = gw2_view(config_dir, now_ms);
    view.note = "Key removed.".into();
    Ok(view)
}

// --- ESO -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsoView {
    pub servers: Vec<EsoServerRow>,
    pub character: Option<String>,
    pub note: String,
    pub empty: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsoServerRow {
    pub dir: String,
    pub label: String,
    pub status: String,
    pub action: Option<String>,
}

fn eso_status(state: &EsoAddonState, label: &str) -> String {
    match state {
        EsoAddonState::NotInstalled => format!("ESO addon{label}: not installed."),
        EsoAddonState::Current { version } => {
            format!("ESO addon{label}: installed ({version}).")
        }
        EsoAddonState::Outdated { version } => {
            format!("ESO addon{label}: {version}, an update is ready.")
        }
        EsoAddonState::Foreign => format!(
            "ESO{label}: an addon named Loadout that Loadout didn't install is there, so it's left alone."
        ),
    }
}

fn eso_action(state: &EsoAddonState, label: &str) -> Option<String> {
    match state {
        EsoAddonState::NotInstalled => Some(format!("Install ESO addon{label}")),
        EsoAddonState::Outdated { .. } => Some(format!("Update ESO addon{label}")),
        EsoAddonState::Current { .. } => Some(format!("Remove ESO addon{label}")),
        EsoAddonState::Foreign => None,
    }
}

/// Build the ESO companion view from server folders.
pub fn eso_view_from(dirs: &[PathBuf]) -> EsoView {
    let many = dirs.len() > 1;
    let mut servers = Vec::new();
    let mut characters = Vec::new();
    for dir in dirs {
        let _ = eso::sync(dir);
        let state = eso::state(dir);
        let label = if many {
            format!(" ({})", eso::server_label(dir))
        } else {
            String::new()
        };
        characters.extend(eso::read_characters(dir));
        servers.push(EsoServerRow {
            dir: dir.to_string_lossy().into_owned(),
            label: label.clone(),
            status: eso_status(&state, &label),
            action: eso_action(&state, &label),
        });
    }
    characters.sort_by_key(|c| std::cmp::Reverse(c.seen));
    EsoView {
        servers,
        character: characters.first().map(eso::character_line),
        note: String::new(),
        empty: dirs
            .is_empty()
            .then(|| "No Elder Scrolls Online add-on folders found on this computer.".into()),
    }
}

pub fn eso_view(steam_root: Option<&Path>) -> EsoView {
    eso_view_from(&eso::find_server_dirs(steam_root))
}

pub fn eso_addon_action(server_dir: &Path, install: bool) -> Result<EsoView, String> {
    let result = if install {
        eso::install(server_dir)
    } else {
        eso::remove(server_dir)
    };
    let note = match result {
        Ok(()) if install => {
            "Installed. Turn on Loadout in ESO's Add-Ons menu, then it saves when you log out or reload the UI.".into()
        }
        Ok(()) => "Removed.".into(),
        Err(e) => return Err(e),
    };
    let mut view = eso_view(grokhub_core::steam::detect_root().as_deref());
    // Re-scan including this dir if detection missed a fixture path.
    if !view.servers.iter().any(|s| Path::new(&s.dir) == server_dir) {
        let mut dirs: Vec<PathBuf> = view.servers.iter().map(|s| PathBuf::from(&s.dir)).collect();
        dirs.push(server_dir.to_path_buf());
        view = eso_view_from(&dirs);
    }
    view.note = note;
    Ok(view)
}

// --- OSRS ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsrsView {
    pub status: String,
    pub gains: Option<String>,
    pub has_player: bool,
    pub name: Option<String>,
    pub kind: String,
    pub kinds: Vec<String>,
    pub note: String,
    pub name_hint: String,
}

fn osrs_status(saved: Option<&OsrsSaved>, now: u64) -> String {
    let Some(s) = saved else {
        return "Old School RuneScape: add your display name to read Jagex's official hiscores."
            .into();
    };
    let who = match s.kind {
        AccountType::Normal => s.name.clone(),
        k => format!("{}, {}", s.name, k.label().to_lowercase()),
    };
    match s.latest() {
        None => format!("{} ({who}): nothing read yet.", osrs::NAME),
        Some(l) => {
            let mins = now.saturating_sub(l.fetched_ms) / 60_000;
            let age = match mins {
                0 => "just now".to_string(),
                1 => "a minute ago".to_string(),
                m if m < 120 => format!("{m} minutes ago"),
                m => format!("{} hours ago", m / 60),
            };
            format!("{} ({who}): {} Updated {age}.", osrs::NAME, l.stats_line())
        }
    }
}

fn osrs_kind_label(kind: &str) -> Option<AccountType> {
    AccountType::ALL
        .into_iter()
        .find(|k| k.label().eq_ignore_ascii_case(kind))
}

pub fn osrs_view(config_dir: &Path, now_ms: u64) -> OsrsView {
    let saved = osrs::load(config_dir);
    OsrsView {
        status: osrs_status(saved.as_ref(), now_ms),
        gains: saved.as_ref().and_then(|s| osrs::gains_line(&s.snapshots)),
        has_player: saved.is_some(),
        name: saved.as_ref().map(|s| s.name.clone()),
        kind: saved
            .as_ref()
            .map(|s| s.kind.label().to_string())
            .unwrap_or_else(|| AccountType::Normal.label().to_string()),
        kinds: AccountType::ALL
            .into_iter()
            .map(|k| k.label().to_string())
            .collect(),
        note: String::new(),
        name_hint: "Your display name on Jagex's public hiscores. No password or key.".into(),
    }
}

pub fn osrs_save(
    config_dir: &Path,
    name: &str,
    kind: &str,
    now_ms: u64,
) -> Result<OsrsView, String> {
    if !osrs::valid_name(name) {
        return Err("That isn't a display name Jagex allows.".into());
    }
    let kind = osrs_kind_label(kind).unwrap_or(AccountType::Normal);
    let saved = OsrsSaved {
        name: name.trim().to_string(),
        kind,
        snapshots: Vec::new(),
    };
    osrs::save(config_dir, &saved).map_err(|e| e.to_string())?;
    let mut view = osrs_view(config_dir, now_ms);
    view.note = "Read from the hiscores at most once an hour.".into();
    Ok(view)
}

pub fn osrs_forget(config_dir: &Path, now_ms: u64) -> Result<OsrsView, String> {
    let _ = std::fs::remove_file(osrs::saved_path(config_dir));
    let mut view = osrs_view(config_dir, now_ms);
    view.note = "Player forgotten.".into();
    Ok(view)
}

fn osrs_api_agent() -> grokhub_core::http::Agent {
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(8),
        overall: Some(Duration::from_secs(20)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

fn osrs_fetch(config_dir: &Path, name: &str, kind: AccountType) -> Result<OsrsStats, String> {
    let url = osrs::url(name, kind);
    if cfg!(test) || !url.starts_with(osrs::HOST) {
        return Err("Not sent.".into());
    }
    hx::guard_quiet(config_dir, &hx::EgressReq::new(&url, &[]))
        .map_err(|_| OSRS_OFFLINE.to_string())?;
    let resp = osrs_api_agent().get(&url).call().map_err(|e| match e {
        grokhub_core::http::Error::Status { code: 404, .. } => format!(
            "Jagex's {} hiscores have no player named {name}. If you changed your name, enter the new one.",
            kind.label().to_lowercase()
        ),
        grokhub_core::http::Error::Status { code, .. } => {
            format!("The hiscores answered {code}; Loadout tries again later.")
        }
        _ => "Couldn't reach Jagex's hiscores.".into(),
    })?;
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(resp.into_reader(), osrs::MAX_BODY),
        &mut text,
    )
    .map_err(|_| "The hiscores answer didn't arrive whole.".to_string())?;
    osrs::parse(&text, grokhub_core::now_ms())
        .ok_or_else(|| "The hiscores sent something unexpected.".into())
}

pub fn osrs_refresh(config_dir: &Path, now_ms: u64) -> Result<OsrsView, String> {
    let Some(mut saved) = osrs::load(config_dir) else {
        let mut v = osrs_view(config_dir, now_ms);
        v.note = "Add your Old School RuneScape name first.".into();
        return Ok(v);
    };
    match osrs_fetch(config_dir, &saved.name, saved.kind) {
        Ok(stats) => {
            saved.push(stats);
            osrs::save(config_dir, &saved).map_err(|e| e.to_string())?;
            Ok(osrs_view(config_dir, now_ms))
        }
        Err(e) => {
            let mut view = osrs_view(config_dir, now_ms);
            view.note = if e == "Not sent." {
                "Couldn't refresh the hiscores just now.".into()
            } else {
                e
            };
            Ok(view)
        }
    }
}

/// Offline-only path used by tests.
pub fn osrs_refresh_offline_row(config_dir: &Path, now_ms: u64) -> OsrsView {
    let mut v = osrs_view(config_dir, now_ms);
    v.note = OSRS_OFFLINE.into();
    v
}

#[cfg(test)]
#[path = "games_tests.rs"]
mod tests;
