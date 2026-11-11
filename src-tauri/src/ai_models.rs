//! Settings → AI models (cross-platform recode): other AI providers with your
//! own key, the router's routing table, and the spend settings with the paid
//! upgrade grants. It reads and writes the same files the cabin does
//! (`providers.json`, the keyring, `consent.jsonl`, `app.json`), so one
//! profile reads the same in both apps. The keyring is installed once when
//! the app starts (`run` in `lib.rs`), so a chat sent before this page is
//! opened finds the saved keys.
//!
//! A provider key goes through a parked credentials card and is saved only on
//! Approve. It is never returned or logged. A new provider stays off until
//! its destination is allowed on a second card. Spend settings are limits
//! only; nothing here can spend money.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use grokhub_agent::harness::{self as hx, ConsentLedger, ParkRequest, UserClick};
use grokhub_agent::route::providers::{self, Provider, ProviderState};
use grokhub_agent::route::spend::{self, PREMIUM_PREFIX};
use serde::Serialize;
use serde_json::{json, Value};

/// Weekly caps offered, USD. 0 is off. Same list as the cabin.
pub const CAPS: &[u32] = &[0, 5, 10, 25, 50, 100];
/// Price limits offered, USD per million output tokens. Same list as the cabin.
pub const CEILINGS: &[u32] = &[5, 15, 30, 60];

const CAP_KEY: &str = "weeklySpendCapUsd";
const CEILING_KEY: &str = "priceCeilingUsdPerM";
const APP_JSON: &str = "app.json";
/// A bigger `app.json` than this is not read or rewritten.
const APP_JSON_CAP: u64 = 1024 * 1024;
/// How many rows each class shows in the routing table.
const TABLE_ROWS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRow {
    pub id: String,
    /// "OpenAI-compatible" or "Anthropic".
    pub kind: String,
    pub base_url: String,
    /// The host the grant names.
    pub dest: String,
    /// Only that a key is set. The key itself never leaves the keyring.
    pub has_key: bool,
    /// "No key", "Not allowed yet" or "Allowed".
    pub state: String,
    /// The key is set and the destination is allowed: its models can be picked.
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub weekly_cap_usd: u32,
    pub price_ceiling_usd_per_m: u32,
    pub caps: Vec<u32>,
    pub ceilings: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PremiumRow {
    pub id: String,
    /// The options, never the model.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableRowView {
    pub model: String,
    pub effort: Option<String>,
    /// Share of the class's check items passed, 0 to 1, or none when unscored.
    pub quality: Option<f64>,
    pub est_cost_usd: Option<f64>,
    pub p50_latency_ms: Option<u64>,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassView {
    pub class: String,
    pub rows: Vec<TableRowView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelsView {
    pub providers: Vec<ProviderRow>,
    pub spend: Spend,
    pub premium: Vec<PremiumRow>,
    pub table: Vec<ClassView>,
    /// When the routing table was built (Unix ms), 0 when there is none yet.
    pub table_built_at: u64,
    /// Why grants can't be read or changed right now (a locked ledger), else empty.
    pub locked: String,
    /// What the last action did, in plain words.
    pub status: String,
}

/// A card waiting for Save/Allow or Deny. It never holds a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    /// "key" (save a provider and its key) or "allow" (allow its destination).
    pub kind: String,
    pub provider: String,
    /// What the card says, with no key in it.
    pub action: String,
}

fn provider_row(config_dir: &Path, ledger: &ConsentLedger, p: &Provider) -> ProviderRow {
    let state = providers::provider_state(config_dir, ledger, p);
    ProviderRow {
        id: p.id.clone(),
        kind: p.kind.label().to_string(),
        base_url: p.base_url.clone(),
        dest: p.dest(),
        has_key: state != ProviderState::NoKey,
        state: state.tag().to_string(),
        ready: state == ProviderState::Ready,
    }
}

/// How a premium grant reads: the options, never the model.
pub fn premium_label(key: &str) -> String {
    let mut label = "Use a premium model".to_string();
    for (tag, words) in [
        ("+priority", " at priority speed"),
        ("+us", " on the US endpoint"),
        ("+agents16", " with 16 agents"),
    ] {
        if key.contains(tag) {
            label.push_str(words);
        }
    }
    label
}

fn app_json_path(config_dir: &Path) -> PathBuf {
    config_dir.join(APP_JSON)
}

fn read_app_json(config_dir: &Path) -> Value {
    let path = app_json_path(config_dir);
    if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= APP_JSON_CAP) {
        return json!({});
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// A stored dollar amount as the whole dollars the page shows, rounded up, so
/// a limit the router enforces never reads as a smaller one or as off. The
/// router reads the same keys the same way (`route::host::read`).
fn shown_usd(app: &Value, key: &str) -> Option<u32> {
    app.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v.ceil().min(f64::from(u32::MAX)) as u32)
}

/// The values offered, with the stored one added when it is not among them.
fn offered(listed: &[u32], stored: u32) -> Vec<u32> {
    let mut all = listed.to_vec();
    if !all.contains(&stored) {
        all.push(stored);
        all.sort_unstable();
    }
    all
}

/// The spend settings in `app.json`, the cabin's own keys. A stored value that
/// is not on its list (an older or hand-edited file) is shown and offered too.
pub fn load_spend(config_dir: &Path) -> Spend {
    let app = read_app_json(config_dir);
    let cap = shown_usd(&app, CAP_KEY).unwrap_or(0);
    let ceiling = shown_usd(&app, CEILING_KEY).unwrap_or(spend_default_ceiling());
    Spend {
        weekly_cap_usd: cap,
        price_ceiling_usd_per_m: ceiling,
        caps: offered(CAPS, cap),
        ceilings: offered(CEILINGS, ceiling),
    }
}

fn spend_default_ceiling() -> u32 {
    grokhub_core::model_registry::cost_class::DEFAULT_CEILING_USD_PER_M as u32
}

/// Save the weekly cap and price limit into `app.json`, keeping every other
/// key as it is. A cap of 0 is stored as no key, like the cabin does. A value
/// sent back as the page showed it stays as the file holds it.
pub fn save_spend(config_dir: &Path, cap: u32, ceiling: u32) -> Result<Spend, String> {
    let shown = load_spend(config_dir);
    if !shown.caps.contains(&cap) {
        return Err("Pick one of the weekly caps listed.".into());
    }
    if !shown.ceilings.contains(&ceiling) {
        return Err("Pick one of the price limits listed.".into());
    }
    let path = app_json_path(config_dir);
    let mut app = match std::fs::read_to_string(&path) {
        Ok(raw) if raw.len() as u64 > APP_JSON_CAP => {
            return Err("The settings file is too big to change here.".into());
        }
        Ok(raw) if raw.trim().is_empty() => json!({}),
        // A file that doesn't parse is left alone, never overwritten.
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .ok()
            .filter(Value::is_object)
            .ok_or("The settings file can't be read, so it was left as it is.")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.to_string()),
    };
    // 12.5 is shown as 13: sending 13 back with a new price limit keeps 12.5.
    let keep_cap = shown_usd(&app, CAP_KEY) == Some(cap);
    let keep_ceiling = shown_usd(&app, CEILING_KEY) == Some(ceiling);
    let map = app
        .as_object_mut()
        .ok_or("The settings file isn't an object.")?;
    if cap == 0 {
        map.remove(CAP_KEY);
    } else if !keep_cap {
        map.insert(CAP_KEY.into(), json!(cap));
    }
    if !keep_ceiling {
        map.insert(CEILING_KEY.into(), json!(ceiling));
    }
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let tmp = config_dir.join("app.json.tmp");
    let body = serde_json::to_string_pretty(&app).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    // The router takes what the file now holds, cents included.
    spend::set_spend(grokhub_agent::route::host::read(config_dir).spend);
    Ok(load_spend(config_dir))
}

fn table_view(config_dir: &Path) -> (Vec<ClassView>, u64) {
    let table = grokhub_agent::route::live::table_snapshot(config_dir);
    let classes = table
        .classes
        .iter()
        .map(|(class, t)| ClassView {
            class: class.clone(),
            rows: t
                .ranked
                .iter()
                .take(TABLE_ROWS)
                .map(|r| TableRowView {
                    model: r.model.clone(),
                    effort: r.effort.clone(),
                    quality: r.quality,
                    est_cost_usd: r.est_cost_usd,
                    p50_latency_ms: r.p50_latency_ms,
                    why: r.why.clone(),
                })
                .collect(),
        })
        .collect();
    (classes, table.built_at)
}

/// Everything the page shows, read fresh from the cabin's files.
pub fn view(config_dir: &Path, status: &str) -> AiModelsView {
    let ledger = ConsentLedger::load(config_dir);
    let locked = ledger.locked().map(|l| l.message()).unwrap_or_default();
    let rows = providers::load_providers(config_dir)
        .iter()
        .map(|p| provider_row(config_dir, &ledger, p))
        .collect();
    let premium = ledger
        .active()
        .filter(|g| g.scope == "spend" && g.source.starts_with(PREMIUM_PREFIX))
        .map(|g| PremiumRow {
            id: g.id.clone(),
            label: premium_label(&g.source),
        })
        .collect();
    let (table, table_built_at) = table_view(config_dir);
    AiModelsView {
        providers: rows,
        spend: load_spend(config_dir),
        premium,
        table,
        table_built_at,
        locked,
        status: status.to_string(),
    }
}

/// Revoke one premium grant. Auto stops using that route on the next turn.
pub fn revoke_premium(config_dir: &Path, id: &str) -> Result<AiModelsView, String> {
    let is_premium = ConsentLedger::load(config_dir)
        .active()
        .any(|g| g.id == id && g.scope == "spend" && g.source.starts_with(PREMIUM_PREFIX));
    if !is_premium {
        return Err("That grant is already gone.".into());
    }
    let revoked = hx::revoke_grant(config_dir, id)?;
    let status = if revoked {
        "Revoked. Auto stops using it on the next turn."
    } else {
        "That grant is already gone."
    };
    Ok(view(config_dir, status))
}

/// Remove a provider and its key. Its destination grant stays in Settings →
/// Permissions until you revoke it there.
pub fn remove_provider(config_dir: &Path, id: &str) -> Result<AiModelsView, String> {
    providers::remove_provider(config_dir, id)?;
    Ok(view(
        config_dir,
        "Provider removed. Its key is gone from the keyring.",
    ))
}

enum Pending {
    /// The waiter thread holds the key, never this map. It ends with the line
    /// to show (saved, denied, timed out), or with why the key wasn't saved.
    Key(PathBuf, JoinHandle<Result<String, String>>),
    /// The provider whose destination the card would allow, and the
    /// destination the card named.
    Allow(PathBuf, String, String),
}

static CARDS: Mutex<Option<HashMap<String, Pending>>> = Mutex::new(None);

fn park(map: &mut Option<HashMap<String, Pending>>) -> &mut HashMap<String, Pending> {
    map.get_or_insert_with(HashMap::new)
}

/// How long a key card waits before it counts as Deny.
pub const CARD_WAIT: Duration = hx::APPROVAL_TTL;
/// What a denied key card says.
pub const KEY_DENIED: &str = "Nothing saved. The key was denied.";
/// What a key card says when nobody answered it in [`CARD_WAIT`].
pub const CARD_TIMED_OUT: &str = "Nothing saved. The card timed out.";
/// What an Allow card says when its provider no longer points at the host it named.
pub const CARD_STALE: &str = "That card is out of date. Start again.";

/// Wait for the card's answer. `None` when the wait ran out with no answer;
/// the request is cleared then, so nothing can approve it later.
fn wait_card(config_dir: &Path, card_id: &str, wait: Duration) -> Option<bool> {
    let start = std::time::Instant::now();
    loop {
        if let Some(approve) = hx::take_answer(config_dir, card_id) {
            return Some(approve);
        }
        if start.elapsed() >= wait {
            hx::clear_park(config_dir, card_id);
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Post the credentials card for a new provider and wait for its answer on a
/// background thread. Approve saves the key to the keyring and the provider
/// (still off); Deny, a timeout or a closed app saves nothing. A blank or
/// partial key, or an address that isn't a public https URL, posts no card.
pub fn start_add(config_dir: &Path, base_url: &str, key: String) -> Result<Card, String> {
    start_add_within(config_dir, base_url, key, CARD_WAIT)
}

/// [`start_add`] with the time the card waits.
fn start_add_within(
    config_dir: &Path,
    base_url: &str,
    key: String,
    wait: Duration,
) -> Result<Card, String> {
    let (_, _, id) = providers::parse_base_url(base_url)?;
    providers::check_key(&key)?;
    let card_id = format!("ai-key-{id}-{}", grokhub_core::pkce::oauth_nonce());
    let action = format!("Save its API key ({id})");
    hx::post_park(
        config_dir,
        &ParkRequest {
            id: card_id.clone(),
            path: "E".into(),
            tool: "model_provider".into(),
            action: action.clone(),
            class: "credentials".into(),
            ts_ms: grokhub_core::now_ms(),
        },
    )?;
    let waiter = std::thread::spawn({
        let (card_id, base_url, config) = (
            card_id.clone(),
            base_url.to_string(),
            config_dir.to_path_buf(),
        );
        move || {
            let _pin = grokhub_agent::perm::ConfigGuard::set(config.clone());
            match wait_card(&config, &card_id, wait) {
                Some(true) => {}
                Some(false) => return Ok(KEY_DENIED.to_string()),
                None => return Ok(CARD_TIMED_OUT.to_string()),
            }
            let p = providers::add_provider(&config, &base_url, &key, grokhub_core::now_ms())?;
            Ok(format!(
                "{} added, still off. Allow it so Auto can use its models.",
                p.id
            ))
        }
    });
    park(&mut CARDS.lock().unwrap_or_else(|e| e.into_inner())).insert(
        card_id.clone(),
        Pending::Key(config_dir.to_path_buf(), waiter),
    );
    Ok(Card {
        id: card_id,
        kind: "key".into(),
        provider: id,
        action,
    })
}

/// The card telling a person that Approve sends their chats and memory to this
/// provider. Nothing is allowed until [`answer`] is called with Approve.
pub fn start_allow(config_dir: &Path, provider_id: &str) -> Result<Card, String> {
    let p = providers::provider(config_dir, provider_id).ok_or("That provider isn't added.")?;
    if !providers::has_key(config_dir, &p.id) {
        return Err("Add its key first.".into());
    }
    let card_id = format!("ai-allow-{}-{}", p.id, grokhub_core::pkce::oauth_nonce());
    let dest = p.dest();
    park(&mut CARDS.lock().unwrap_or_else(|e| e.into_inner())).insert(
        card_id.clone(),
        Pending::Allow(config_dir.to_path_buf(), p.id.clone(), dest.clone()),
    );
    Ok(Card {
        id: card_id,
        kind: "allow".into(),
        provider: p.id.clone(),
        action: format!("Send chats and memory to {dest}"),
    })
}

/// The card's Save/Allow or Deny.
pub fn answer(card_id: &str, approve: bool) -> Result<AiModelsView, String> {
    let pending = park(&mut CARDS.lock().unwrap_or_else(|e| e.into_inner()))
        .remove(card_id)
        .ok_or("That card is closed. Start again.")?;
    match pending {
        Pending::Key(config, waiter) => {
            // A waiter that gave up has cleared its request: an answer now
            // would be a file nobody reads.
            if !waiter.is_finished() {
                hx::answer_park(&config, card_id, approve)?;
            }
            let ended = waiter.join();
            // The waiter may have given up between the check and the answer.
            hx::clear_park(&config, card_id);
            match ended {
                Ok(Ok(line)) => Ok(view(&config, &line)),
                // The key wasn't saved (no keyring): an error, not a status.
                Ok(Err(err)) => Err(err),
                Err(_) => Ok(view(&config, "Nothing saved.")),
            }
        }
        Pending::Allow(config, id, dest) => {
            if !approve {
                return Ok(view(&config, "Not allowed. Nothing was sent."));
            }
            let p = providers::provider(&config, &id).ok_or("That provider isn't added.")?;
            // The grant goes to the host the card named, or nowhere.
            if p.dest() != dest {
                return Err(CARD_STALE.into());
            }
            // The Approve click on this card is the user's own.
            hx::grant_destination(
                &config,
                &p.dest(),
                providers::CALL_DATA,
                UserClick::from_click(),
            )?;
            Ok(view(
                &config,
                "Allowed. Revoke it in Settings → Permissions any time.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::harness::{use_key_store_for, MemoryKeyStore};
    use grokhub_agent::route::providers::{use_vault_for, MemoryVault};
    use std::sync::Arc;

    /// A config folder for one test, removed when the test ends.
    struct Dir(PathBuf);

    impl std::ops::Deref for Dir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A config folder with no keyring.
    fn dir_without_keyring(name: &str) -> Dir {
        let root = std::env::temp_dir().join(format!(
            "grokhub-ai-models-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        use_key_store_for(&root, Arc::new(MemoryKeyStore::new()));
        Dir(root)
    }

    fn dir(name: &str) -> Dir {
        let root = dir_without_keyring(name);
        use_vault_for(&root, Arc::new(MemoryVault::default()));
        root
    }

    const BASE: &str = "https://openrouter.ai/api/v1";
    const KEY: &str = "test-key-aaaaaaaa";

    fn pending_for(config: &Path) -> usize {
        hx::pending_parks(config).len()
    }

    #[test]
    fn a_provider_without_a_whole_key_posts_no_card_and_stays_off() {
        let config = dir("nokey");
        for key in ["", "   ", "short", "has a space in it"] {
            let err = start_add(&config, BASE, key.to_string()).unwrap_err();
            assert_eq!(err, "Paste the whole key.", "key {key:?}");
        }
        assert_eq!(pending_for(&config), 0);
        let v = view(&config, "");
        assert_eq!(v.providers, Vec::<ProviderRow>::new());
        assert!(!providers::has_key(&config, "openrouter"));
    }

    #[test]
    fn a_plain_http_address_is_refused_before_any_card() {
        let config = dir("http");
        let err = start_add(&config, "http://openrouter.ai/api/v1", KEY.into()).unwrap_err();
        assert_eq!(err, "Use an https:// address.");
        assert_eq!(pending_for(&config), 0);
    }

    #[test]
    fn a_key_goes_through_a_parked_credentials_card_and_stays_off_until_allowed() {
        let config = dir("approve");
        let card = start_add(&config, BASE, KEY.into()).expect("card");
        assert_eq!(card.kind, "key");
        assert_eq!(card.provider, "openrouter");
        assert_eq!(card.action, "Save its API key (openrouter)");
        let parked = hx::pending_parks(&config);
        assert_eq!(parked.len(), 1);
        assert_eq!(parked[0].class, "credentials");
        assert_eq!(parked[0].id, card.id);
        // Nothing is saved while the card waits.
        assert!(providers::load_providers(&config).is_empty());
        assert!(!providers::has_key(&config, "openrouter"));

        let v = answer(&card.id, true).expect("answered");
        assert_eq!(
            v.status,
            "openrouter added, still off. Allow it so Auto can use its models."
        );
        assert_eq!(v.providers.len(), 1);
        let row = &v.providers[0];
        assert_eq!(row.id, "openrouter");
        assert_eq!(row.dest, "openrouter.ai");
        assert!(row.has_key);
        assert!(!row.ready);
        assert_eq!(row.state, "Not allowed yet");
        // The page never carries the key.
        let wire = serde_json::to_string(&v).expect("json");
        assert!(!wire.contains(KEY));
    }

    #[test]
    fn a_deny_saves_nothing() {
        let config = dir("deny");
        let card = start_add(&config, BASE, KEY.into()).expect("card");
        let v = answer(&card.id, false).expect("answered");
        assert_eq!(v.status, "Nothing saved. The key was denied.");
        assert!(v.providers.is_empty());
        assert!(!providers::has_key(&config, "openrouter"));
        assert!(providers::load_providers(&config).is_empty());
        assert_eq!(
            answer(&card.id, true).unwrap_err(),
            "That card is closed. Start again."
        );
    }

    #[test]
    fn an_allowed_provider_is_ready_and_a_denied_allow_is_not() {
        let config = dir("allow");
        let card = start_add(&config, BASE, KEY.into()).expect("card");
        answer(&card.id, true).expect("saved");

        let allow = start_allow(&config, "openrouter").expect("allow card");
        assert_eq!(allow.kind, "allow");
        assert_eq!(allow.action, "Send chats and memory to openrouter.ai");
        let v = answer(&allow.id, false).expect("denied");
        assert_eq!(v.status, "Not allowed. Nothing was sent.");
        assert_eq!(v.providers[0].state, "Not allowed yet");

        let allow = start_allow(&config, "openrouter").expect("allow card");
        let v = answer(&allow.id, true).expect("allowed");
        assert_eq!(v.providers[0].state, "Allowed");
        assert!(v.providers[0].ready);
        assert_eq!(
            providers::usable(&config, providers::CALL_DATA),
            vec!["openrouter".to_string()]
        );
    }

    #[test]
    fn allowing_needs_an_added_provider_with_a_key() {
        let config = dir("allow-none");
        assert_eq!(
            start_allow(&config, "openrouter").unwrap_err(),
            "That provider isn't added."
        );
    }

    #[test]
    fn removing_a_provider_drops_its_row_and_key() {
        let config = dir("remove");
        let card = start_add(&config, BASE, KEY.into()).expect("card");
        answer(&card.id, true).expect("saved");
        let v = remove_provider(&config, "openrouter").expect("removed");
        assert!(v.providers.is_empty());
        assert!(!providers::has_key(&config, "openrouter"));
    }

    #[test]
    fn the_spend_cap_round_trips_through_app_json_and_keeps_other_keys() {
        let config = dir("spend");
        std::fs::write(
            config.join("app.json"),
            r#"{"quietStart":"22:00","weeklySpendCapUsd":5}"#,
        )
        .expect("write");
        assert_eq!(load_spend(&config).weekly_cap_usd, 5);
        assert_eq!(load_spend(&config).price_ceiling_usd_per_m, 15);

        let s = save_spend(&config, 25, 30).expect("saved");
        assert_eq!((s.weekly_cap_usd, s.price_ceiling_usd_per_m), (25, 30));
        let raw: Value =
            serde_json::from_str(&std::fs::read_to_string(config.join("app.json")).expect("read"))
                .expect("json");
        assert_eq!(raw["weeklySpendCapUsd"], 25);
        assert_eq!(raw["priceCeilingUsdPerM"], 30);
        assert_eq!(raw["quietStart"], "22:00");
        assert_eq!(spend::spend_settings().weekly_cap_usd, 25.0);

        // Off removes the key, as the cabin does.
        save_spend(&config, 0, 15).expect("off");
        let raw: Value =
            serde_json::from_str(&std::fs::read_to_string(config.join("app.json")).expect("read"))
                .expect("json");
        assert!(raw.get("weeklySpendCapUsd").is_none());
        assert_eq!(raw["quietStart"], "22:00");
        assert_eq!(load_spend(&config).weekly_cap_usd, 0);
    }

    #[test]
    fn an_unreadable_app_json_is_left_alone() {
        let config = dir("spend-corrupt");
        std::fs::write(config.join("app.json"), "{not json").expect("write");
        assert_eq!(
            save_spend(&config, 5, 15).unwrap_err(),
            "The settings file can't be read, so it was left as it is."
        );
        assert_eq!(
            std::fs::read_to_string(config.join("app.json")).expect("read"),
            "{not json"
        );
    }

    #[test]
    fn a_cap_that_is_not_offered_is_refused_and_nothing_is_written() {
        let config = dir("spend-bad");
        assert_eq!(
            save_spend(&config, 7, 15).unwrap_err(),
            "Pick one of the weekly caps listed."
        );
        assert_eq!(
            save_spend(&config, 5, 7).unwrap_err(),
            "Pick one of the price limits listed."
        );
        assert!(!config.join("app.json").exists());
    }

    #[test]
    fn a_revoked_grant_is_gone_from_the_list_and_the_router() {
        let config = dir("premium");
        let key = "premium:some-model+priority";
        let g = hx::grant_premium(&config, key, UserClick::from_click()).expect("grant");
        let v = view(&config, "");
        assert_eq!(
            v.premium,
            vec![PremiumRow {
                id: g.id.clone(),
                label: "Use a premium model at priority speed".into()
            }]
        );
        assert_eq!(spend::premium_grants(&config), vec![key.to_string()]);

        let v = revoke_premium(&config, &g.id).expect("revoked");
        assert_eq!(v.status, "Revoked. Auto stops using it on the next turn.");
        assert!(v.premium.is_empty());
        assert!(spend::premium_grants(&config).is_empty());
        assert_eq!(
            revoke_premium(&config, &g.id).unwrap_err(),
            "That grant is already gone."
        );
    }

    #[test]
    fn revoking_a_grant_that_is_not_premium_is_refused() {
        let config = dir("premium-other");
        let g = hx::grant_destination(
            &config,
            "example.com",
            providers::CALL_DATA,
            UserClick::from_click(),
        )
        .expect("grant");
        assert_eq!(
            revoke_premium(&config, &g.id).unwrap_err(),
            "That grant is already gone."
        );
    }

    #[test]
    fn premium_labels_name_options_never_the_model() {
        assert_eq!(premium_label("premium:x"), "Use a premium model");
        assert_eq!(
            premium_label("premium:x+priority+us"),
            "Use a premium model at priority speed on the US endpoint"
        );
    }

    #[test]
    fn a_fresh_profile_has_an_empty_routing_table() {
        let config = dir("table");
        let v = view(&config, "");
        assert_eq!(v.table_built_at, 0);
        assert!(v.table.is_empty());
        assert_eq!(v.spend.caps, vec![0, 5, 10, 25, 50, 100]);
    }

    #[test]
    fn a_test_folder_is_removed_when_the_test_ends() {
        let kept = {
            let config = dir("cleanup");
            std::fs::write(config.join("app.json"), "{}").expect("write");
            assert!(config.is_dir());
            config.to_path_buf()
        };
        assert!(!kept.exists());
    }

    /// Whether `setup` (the body of the app's start) calls the keyring
    /// install as a statement of its own, before the state is managed.
    fn installs_keyring_first(setup: &str) -> bool {
        let Some(state) = setup.find("app.manage(ShellState") else {
            return false;
        };
        setup[..state]
            .lines()
            .any(|line| line.trim() == "grokhub_agent::route::providers::use_os_vault();")
    }

    #[test]
    fn the_keyring_is_installed_when_the_app_starts() {
        // A chat sent before Settings → AI models is opened reads the saved keys.
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        let setup = src
            .split("pub fn run() {")
            .nth(1)
            .and_then(|s| s.split(".on_window_event(").next())
            .expect("run setup");
        assert!(installs_keyring_first(setup), "{setup}");
        // The check reads code, not words: a call that is commented out, sits
        // in a string or comes after the state is not an install.
        let call = "grokhub_agent::route::providers::use_os_vault();";
        let state = "app.manage(ShellState::new());";
        assert!(installs_keyring_first(&format!(
            "    {call}\n    {state}\n"
        )));
        assert!(!installs_keyring_first(&format!(
            "    // {call}\n    {state}\n"
        )));
        assert!(!installs_keyring_first(&format!(
            "    let note = \"{call}\";\n    {state}\n"
        )));
        assert!(!installs_keyring_first(&format!(
            "    {state}\n    {call}\n"
        )));
        assert!(!installs_keyring_first(&format!("    {call}\n")));
    }

    #[test]
    fn a_key_the_keyring_cannot_take_is_an_error_in_plain_words() {
        // No keyring for this folder, and nothing on the page installs the OS one.
        let config = dir_without_keyring("keyring-off");
        let card = start_add(&config, BASE, KEY.into()).expect("card");
        assert_eq!(
            answer(&card.id, true).unwrap_err(),
            "The keyring isn't available, so the key wasn't saved."
        );
        assert!(providers::load_providers(&config).is_empty());
        assert_eq!(pending_for(&config), 0);
    }

    #[test]
    fn a_key_card_answered_after_its_wait_ran_out_says_so_and_leaves_no_file() {
        let config = dir("timeout");
        let card =
            start_add_within(&config, BASE, KEY.into(), Duration::from_millis(50)).expect("card");
        // The waiter gives up and clears its request.
        let gone = std::time::Instant::now();
        while pending_for(&config) > 0 && gone.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(100));
        let v = answer(&card.id, true).expect("answered");
        assert_eq!(v.status, "Nothing saved. The card timed out.");
        assert!(v.providers.is_empty());
        assert!(!providers::has_key(&config, "openrouter"));
        let left: Vec<String> = std::fs::read_dir(hx::park_dir(&config))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(left, Vec::<String>::new());
    }

    #[test]
    fn an_allow_card_grants_only_the_host_it_named() {
        let config = dir("stale-allow");
        let card =
            start_add(&config, "https://router.huggingface.co/v1", KEY.into()).expect("card");
        answer(&card.id, true).expect("saved");
        let allow = start_allow(&config, "router").expect("allow card");
        assert_eq!(
            allow.action,
            "Send chats and memory to router.huggingface.co"
        );
        // The same id now points at another host.
        providers::add_provider(&config, "https://router.requesty.ai/v1", KEY, 2).expect("moved");
        assert_eq!(
            answer(&allow.id, true).unwrap_err(),
            "That card is out of date. Start again."
        );
        let ledger = ConsentLedger::load(&config);
        for host in ["router.requesty.ai", "router.huggingface.co"] {
            assert!(
                ledger
                    .destination_grant(host, providers::CALL_DATA)
                    .is_none(),
                "{host} was granted"
            );
        }
        assert_eq!(
            providers::usable(&config, providers::CALL_DATA),
            Vec::<String>::new()
        );
    }

    #[test]
    fn removing_a_provider_that_was_never_added_says_so() {
        let config = dir("remove-unknown");
        assert_eq!(
            remove_provider(&config, "nope").unwrap_err(),
            "That provider isn't added."
        );
    }

    #[test]
    fn a_stored_price_limit_that_is_not_offered_is_shown_and_can_be_kept() {
        let config = dir("spend-old");
        std::fs::write(
            config.join("app.json"),
            r#"{"priceCeilingUsdPerM":20,"weeklySpendCapUsd":5}"#,
        )
        .expect("write");
        let s = load_spend(&config);
        assert_eq!((s.weekly_cap_usd, s.price_ceiling_usd_per_m), (5, 20));
        assert_eq!(s.caps, vec![0, 5, 10, 25, 50, 100]);
        assert_eq!(s.ceilings, vec![5, 15, 20, 30, 60]);

        let s = save_spend(&config, 25, 20).expect("saved");
        assert_eq!((s.weekly_cap_usd, s.price_ceiling_usd_per_m), (25, 20));
        assert_eq!(s.ceilings, vec![5, 15, 20, 30, 60]);
        // A value that is neither offered nor stored is still refused.
        assert_eq!(
            save_spend(&config, 25, 7).unwrap_err(),
            "Pick one of the price limits listed."
        );
        assert_eq!(
            save_spend(&config, 7, 20).unwrap_err(),
            "Pick one of the weekly caps listed."
        );
    }

    #[test]
    fn a_stored_cap_with_cents_is_shown_rounded_up_and_stays_as_stored() {
        let config = dir("spend-cents");
        std::fs::write(config.join("app.json"), r#"{"weeklySpendCapUsd":12.5}"#).expect("write");
        let s = load_spend(&config);
        assert_eq!(s.weekly_cap_usd, 13);
        assert_eq!(s.caps, vec![0, 5, 10, 13, 25, 50, 100]);

        // Changing only the price limit keeps the cap the file holds.
        let s = save_spend(&config, 13, 30).expect("saved");
        assert_eq!((s.weekly_cap_usd, s.price_ceiling_usd_per_m), (13, 30));
        let raw: Value =
            serde_json::from_str(&std::fs::read_to_string(config.join("app.json")).expect("read"))
                .expect("json");
        assert_eq!(raw["weeklySpendCapUsd"], 12.5);
        assert_eq!(raw["priceCeilingUsdPerM"], 30);
        assert_eq!(spend::spend_settings().weekly_cap_usd, 12.5);
        assert_eq!(spend::spend_settings().ceiling_usd_per_m, 30.0);
    }
}
