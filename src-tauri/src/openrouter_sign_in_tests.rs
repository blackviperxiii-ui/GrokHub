use super::*;
use grokhub_agent::harness::{self as hx, use_key_store_for, MemoryKeyStore};
use grokhub_agent::route::providers::{
    use_transport_for, use_vault_for, HttpReply, MemoryVault, ProviderTransport,
};
use std::path::PathBuf;
use std::sync::Arc;

const KEY: &str = "sk-abcdefghijklmnopqrstuv";

/// One sign-in at a time is the rule under test, so the tests take turns.
static TURN: Mutex<()> = Mutex::new(());

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

/// OpenRouter's key exchange, faked: a key for `good-code`, else a 403.
#[derive(Default)]
struct KeyWire(Mutex<Vec<String>>);

impl ProviderTransport for KeyWire {
    fn get(&self, url: &str, _: &[(String, String)]) -> Result<HttpReply, String> {
        Err(format!("no GET expected: {url}"))
    }
    fn post(&self, url: &str, _: &[(String, String)], body: &str) -> Result<HttpReply, String> {
        self.0.lock().unwrap().push(url.to_string());
        let (status, body) = if body.contains(r#""code":"good-code""#) {
            (200, serde_json::json!({ "key": KEY }).to_string())
        } else {
            (403, "{}".to_string())
        };
        Ok(HttpReply {
            status,
            body,
            headers: Vec::new(),
        })
    }
}

fn dir(name: &str) -> (Dir, Arc<KeyWire>) {
    let root = std::env::temp_dir().join(format!(
        "grokhub-openrouter-sign-in-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("dir");
    use_key_store_for(&root, Arc::new(MemoryKeyStore::new()));
    use_vault_for(&root, Arc::new(MemoryVault::default()));
    let wire = Arc::new(KeyWire::default());
    use_transport_for(&root, wire.clone());
    (Dir(root), wire)
}

/// A query value from OpenRouter's page link.
fn param(link: &str, name: &str) -> String {
    let query = link.split_once('?').map(|(_, q)| q).unwrap_or("");
    query
        .split('&')
        .find_map(|p| p.strip_prefix(&format!("{name}=")))
        .unwrap_or("")
        .replace("%3A", ":")
        .replace("%2F", "/")
}

/// The person approving on OpenRouter's page: their browser calls the
/// callback with `code` and the link's `state`.
fn approve_in_browser(link: &str, code: &'static str) {
    let callback = param(link, "callback_url");
    let state = param(link, "state");
    let (host, path) = callback
        .trim_start_matches("http://")
        .split_once('/')
        .map(|(h, p)| (h.to_string(), p.to_string()))
        .expect("callback");
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut sock = std::net::TcpStream::connect(&host).expect("callback port");
        let _ = write!(
            sock,
            "GET /{path}?code={code}&state={state} HTTP/1.1\r\nHost: {host}\r\n\r\n"
        );
        let mut sink = String::new();
        let _ = sock.read_to_string(&mut sink);
    });
}

/// A browser that opens and approves at once, recording what it opened.
fn approving(
    code: &'static str,
    opened: Arc<Mutex<Vec<String>>>,
) -> impl Fn(&str) -> Result<(), String> + Send + 'static {
    move |link: &str| {
        opened.lock().unwrap().push(link.to_string());
        approve_in_browser(link, code);
        Ok(())
    }
}

#[test]
fn only_openrouters_own_sign_in_page_opens() {
    for ok in [
        "https://openrouter.ai/auth?callback_url=x",
        "https://openrouter.ai/auth?a=1&b=2",
    ] {
        assert!(sign_in_page_ok(ok), "{ok}");
    }
    for bad in [
        "https://openrouter.ai/auth?",
        "https://openrouter.ai/auth",
        "https://openrouter.ai/settings/keys?x=1",
        "http://openrouter.ai/auth?x=1",
        "https://openrouter.ai.example.com/auth?x=1",
        "https://example.com/?https://openrouter.ai/auth?x=1",
        "https://openrouter.ai/auth?x=1 y",
        "",
    ] {
        assert!(!sign_in_page_ok(bad), "{bad}");
    }
}

#[test]
fn a_sign_in_ends_on_a_key_card_and_saves_nothing_before_save() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (config, wire) = dir("card");
    let opened = Arc::new(Mutex::new(Vec::new()));
    let started = start(&config, approving("good-code", opened.clone())).expect("started");
    assert!(started.opened);
    assert_eq!(started.why, "");
    assert!(
        started.url.starts_with("https://openrouter.ai/auth?callback_url="),
        "{}",
        started.url
    );
    assert_eq!(*opened.lock().unwrap(), vec![started.url.clone()]);

    let card = wait().expect("card");
    assert_eq!(card.kind, "key");
    assert_eq!(card.provider, "openrouter");
    assert_eq!(card.action, "Save its API key (openrouter)");
    assert_eq!(
        *wire.0.lock().unwrap(),
        vec!["https://openrouter.ai/api/v1/auth/keys".to_string()]
    );
    // The card waits: no key and no provider until Save.
    assert_eq!(hx::pending_parks(&config).len(), 1);
    assert!(!providers::has_key(&config, "openrouter"));
    assert!(providers::load_providers(&config).is_empty());
    let wire_json = serde_json::to_string(&card).expect("json");
    assert!(!wire_json.contains(KEY), "{wire_json}");

    let v = ai_models::answer(&card.id, true).expect("saved");
    assert_eq!(
        v.status,
        "openrouter added, still off. Allow it so Auto can use its models."
    );
    assert!(providers::has_key(&config, "openrouter"));
    assert_eq!(v.providers[0].state, "Not allowed yet");
    assert!(!v.providers[0].ready);
    // The sign-in is over.
    assert_eq!(wait().unwrap_err(), NO_SIGN_IN);
}

#[test]
fn a_browser_that_does_not_open_still_signs_in_by_hand() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (config, _wire) = dir("by-hand");
    let started = start(&config, |link: &str| {
        // The page shows the link; the person opens it themselves.
        approve_in_browser(link, "good-code");
        Err("No browser opened".to_string())
    })
    .expect("started");
    assert!(!started.opened);
    assert_eq!(started.why, "No browser opened");
    assert!(sign_in_page_ok(&started.url), "{}", started.url);

    let card = wait().expect("card");
    assert_eq!(card.kind, "key");
    let v = ai_models::answer(&card.id, false).expect("denied");
    assert_eq!(v.status, "Nothing saved. The key was denied.");
    assert!(!providers::has_key(&config, "openrouter"));
}

#[test]
fn a_second_start_waits_its_turn_and_an_unanswered_one_times_out() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (config, wire) = dir("busy");
    let opened = Arc::new(Mutex::new(Vec::new()));
    let silent = {
        let opened = opened.clone();
        move |link: &str| {
            opened.lock().unwrap().push(link.to_string());
            Ok(())
        }
    };
    start_within(&config, silent, Duration::from_millis(600)).expect("started");
    let again = start(&config, approving("good-code", opened.clone())).unwrap_err();
    assert_eq!(again, OPENROUTER_WAIT);
    // The second start opened nothing.
    assert_eq!(opened.lock().unwrap().len(), 1);

    assert_eq!(wait().unwrap_err(), "OpenRouter sign-in timed out");
    assert!(wire.0.lock().unwrap().is_empty());
    assert!(hx::pending_parks(&config).is_empty());
    assert_eq!(wait().unwrap_err(), NO_SIGN_IN);
}

#[test]
fn a_code_openrouter_refuses_posts_no_card() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (config, _wire) = dir("refused");
    let opened = Arc::new(Mutex::new(Vec::new()));
    start(&config, approving("bad-code", opened)).expect("started");
    assert_eq!(
        wait().unwrap_err(),
        "OpenRouter didn't hand over a key (HTTP 403)"
    );
    assert!(hx::pending_parks(&config).is_empty());
    assert!(!providers::has_key(&config, "openrouter"));
}

#[test]
fn offline_only_opens_no_page() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (config, wire) = dir("offline");
    let _on = grokhub_core::offline::force_for_thread(true);
    let opened = Arc::new(Mutex::new(Vec::new()));
    let err = start(&config, approving("good-code", opened.clone())).unwrap_err();
    assert_eq!(err, *grokhub_core::offline::OFFLINE_MSG);
    assert!(opened.lock().unwrap().is_empty());
    assert!(wire.0.lock().unwrap().is_empty());
    assert_eq!(wait().unwrap_err(), NO_SIGN_IN);
}

#[test]
fn a_wait_with_no_sign_in_says_to_start_first() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(wait().unwrap_err(), "Start signing in first.");
}
