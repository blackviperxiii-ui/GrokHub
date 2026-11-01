//! Guard for Offline only (LU-13): every place the app can open a network
//! connection is listed here with how it obeys the switch.
//!
//! The scan reads `crates/` and `src-tauri/src` and counts what starts an
//! outbound connection: an HTTP client (`ureq`, `reqwest`), a raw socket, a
//! websocket, or a spawned download / git remote step. A file whose count
//! doesn't match its [`GUARDED`] row fails, so a new exit can't skip the
//! switch. Test modules (`#[cfg(test)] mod … { }` blocks, `tests.rs`,
//! `*_tests.rs`, `tests/`) are skipped: their fake servers are on loopback.
//! Only the test block is skipped, never the rest of the file after it.
//!
//! Hermetic: reads files under the repo root only (CRLF normalized).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// What starts an outbound connection.
const EXITS: &[&str] = &[
    "ureq::get(",
    "ureq::post(",
    "ureq::put(",
    "ureq::delete(",
    "ureq::head(",
    "ureq::patch(",
    "ureq::request(",
    "ureq::request_url(",
    "ureq::agent(",
    "ureq::builder(",
    "ureq::AgentBuilder::new(",
    "ureq::Agent::new(",
    // The app's own client (`grokhub_core::http`): each agent counts where it's built.
    "http::agent(",
    "reqwest::blocking::Client::builder(",
    "reqwest::Client::builder(",
    "reqwest::blocking::Client::new(",
    "reqwest::Client::new(",
    "reqwest::get(",
    "reqwest::blocking::get(",
    "TcpStream::connect(",
    "TcpStream::connect_timeout(",
    "UdpSocket::bind(",
    "tungstenite::client(",
    "tungstenite::connect(",
    "Invoke-WebRequest",
    "Command::new(\"curl",
    "Command::new(\"wget",
    "\"clone\"",
    "\"fetch\"",
    "\"pull\"",
    "\"ls-remote\"",
];

/// `(path from the repo root, exits in it, how it obeys Offline only)`.
///
/// "egress guard" means `guard_egress` / `guard_provider` (and the
/// `guard_quiet`, `guard_or_park`, `provider_or_park`, cabin `egress_ok`
/// wrappers): it refuses any host that isn't loopback or the LAN while the
/// switch is on. "loopback" and "LAN" sites stay allowed by design.
const GUARDED: &[(&str, usize, &str)] = &[
    // The HTTP facade: a factory, counted again at every caller listed here.
    ("crates/grokhub-core/src/http.rs", 1, "agent factory (LU-12): no guard of its own; each caller below runs its guard first, and public_only dials public_only_resolve (refuses all while on)"),
    // Model providers.
    ("crates/grokhub-agent/src/client.rs", 1, "XaiClient::once: egress guard (api.x.ai); a refusal is Protocol, never retried"),
    ("crates/grokhub-agent/src/route/providers.rs", 1, "providers you added: guard_provider / provider_or_park"),
    ("crates/grokhub-app/src/xai.rs", 1, "cabin Grok calls (STT, TTS, Imagine): egress_ok"),
    // Model registry, probes, connectivity.
    ("crates/grokhub-agent/src/route/sources.rs", 1, "model listing: egress guard; token probe: offline::check_url"),
    ("crates/grokhub-core/src/connectivity.rs", 1, "reqwest client for the re-probe: run_probe_if_due sends nothing while on"),
    ("crates/grokhub-app/src/app/net_log_ui.rs", 1, "resolver connect probe and speed test: offline::refuse first"),
    // Web tools, Pulse, GitHub, plugins.
    ("crates/grokhub-agent/src/tools/web_fetch.rs", 1, "every hop: egress guard + public_only_resolve (refuses all while on)"),
    ("crates/grokhub-agent/src/tools/web_search.rs", 1, "Brave: egress guard; DDG / SearXNG go through web_fetch"),
    ("crates/grokhub-agent/src/tools/media.rs", 1, "Imagine media: egress guard"),
    ("crates/grokhub-app/src/app/pulse_ui.rs", 1, "Pulse previews: egress_ok"),
    ("crates/grokhub-app/src/app/pulse_sources_ui.rs", 1, "Pulse sources: egress_ok on every hop"),
    ("src-tauri/src/oauth.rs", 1, "Sign in with Grok to xAI's auth host: guard_quiet (codes and tokens only); the test override is loopback only"),
    ("src-tauri/src/pulse_feed.rs", 2, "Pulse feed: guard_quiet + public_only_resolve; LoopbackNet is loopback only"),
    ("crates/grokhub-app/src/github.rs", 1, "GitHub tool: egress_ok"),
    ("crates/grokhub-agent/src/plugins.rs", 2, "marketplace index: egress guard; plugin git clone: offline::check_url"),
    ("crates/grokhub-app/src/app/library_ui/game_art.rs", 1, "store art: egress_ok"),
    ("src-tauri/src/library.rs", 1, "Library store art: guard_quiet + public_only resolve, no redirects; Play only spawns the OS opener for steam://rungameid/<digits>"),
    ("crates/grokhub-app/src/app/gw2_ui.rs", 1, "Guild Wars 2 API: egress_ok"),
    ("crates/grokhub-app/src/app/destiny_ui.rs", 1, "Bungie.net API: egress_ok"),
    // MCP (HTTP transport + OAuth) and the MCP registry.
    ("crates/grokhub-agent/src/mcp/http.rs", 1, "connect / ping / call and OAuth sign-in (mcp/oauth.rs): egress guard; loopback servers still go"),
    ("crates/grokhub-core/src/mcp_catalog.rs", 1, "registry refresh: guard_mcp_registry_url refuses while on"),
    // Sign-in, updates, downloads.
    ("crates/grokhub-app/src/oauth.rs", 1, "xAI sign-in (auth_agent callers): egress_ok"),
    ("crates/grokhub-app/src/imagine_auth.rs", 1, "Imagine sign-in (auth_agent callers): egress_ok"),
    ("crates/grokhub-app/src/update.rs", 1, "latest tag check: egress_ok"),
    ("crates/grokhub-app/src/release_update.rs", 1, "release download: egress_ok; the plan is refused first (update_cmds_*)"),
    ("crates/grokhub-core/src/update.rs", 2, "git fetch of the channel tips: offline::refuse"),
    ("crates/grokhub-app/src/app/model_download_ui.rs", 1, "Hugging Face model: egress_ok + model_download::download refuses while on"),
    ("crates/grokhub-app/src/app/local_ai_ui.rs", 1, "engine download: runtime_download_url_ok + download_engine refuse while on"),
    ("src-tauri/src/shell_update.rs", 1, "shell self-update release list: guard_quiet + public_only; the plugin's check and download refuse first (offline::refuse)"),
    ("src-tauri/src/local_engine.rs", 2, "engine and HF downloads: runtime_download_url_ok, download_engine and model_download::download refuse while on"),
    // On-device model (loopback only).
    ("crates/grokhub-agent/src/route/local_http.rs", 2, "llama-server chat and the /props context-window read, both loopback only"),
    ("crates/grokhub-agent/src/route/local_sidecar.rs", 1, "llama-server /health on loopback"),
    ("crates/grokhub-agent/src/route/local_effort.rs", 1, "llama-server /props on loopback (refuses any other host)"),
    ("crates/grokhub-agent/src/route/local_spec.rs", 1, "speed test against the loopback sidecar"),
    // The app's own desktop, CDP, hub and LAN features.
    ("crates/grokhub-app/src/desktop.rs", 3, "hub health and CDP HTTP (loopback_agent), CDP websocket: loopback via egress_ok"),
    ("crates/grokhub-app/src/app/print_alerts_ui.rs", 1, "the user's printer: LAN, allowed (a public host is refused by egress_ok)"),
    ("crates/grokhub-app/src/app/lan_host.rs", 1, "UDP connect to TEST-NET-1 to read the LAN address: sends nothing"),
    ("src-tauri/src/remote.rs", 1, "UDP connect to TEST-NET-1 to read the LAN address: sends nothing"),
    ("src-tauri/src/remote_tls.rs", 1, "Web access TLS front proxies to its own loopback server"),
    // Game companions: loopback game clients only (game-data paths are not changed).
    ("crates/grokhub-core/src/game_watch_league.rs", 1, "League live client API on 127.0.0.1"),
    ("crates/grokhub-core/src/game_watch_gsi.rs", 1, "game state integration listener's self-wake on 127.0.0.1"),
    // Words in lists, not spawns.
    ("crates/grokhub-agent/src/perm/risk.rs", 1, "downloader word list for shell risk"),
    ("crates/grokhub-agent/src/auto_review/routine_git.rs", 2, "routine git verbs list"),
    ("crates/grokhub-app/src/app/loadout_home.rs", 1, "home feed word list"),
];

/// Scanners name every exit pattern themselves.
const SKIP_FILES: &[&str] = &["crates/grokhub-agent/src/harness/egress_coverage.rs"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The part of a source file that ships: CRLF normalized, each
/// `#[cfg(test)] mod name { … }` block cut out (the code after it stays).
fn production_text(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let next = lines.get(i + 1).map(|l| l.trim_start()).unwrap_or("");
        let decl = next
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub(super) ")
            .trim_start_matches("pub ");
        let opens_mod = decl.starts_with("mod ") && next.trim_end().ends_with('{');
        if lines[i].trim() == "#[cfg(test)]" && opens_mod {
            let mut depth: i64 = 0;
            i += 1;
            while i < lines.len() {
                depth += lines[i].matches('{').count() as i64;
                depth -= lines[i].matches('}').count() as i64;
                i += 1;
                if depth <= 0 {
                    break;
                }
            }
            continue;
        }
        out.push_str(lines[i]);
        out.push('\n');
        i += 1;
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "tests" | "target" | "fixtures") {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            out.push(path);
        }
    }
}

/// Exits per file under `roots`, keyed by the path from `root` with `/`.
fn scan(root: &Path, roots: &[&str]) -> BTreeMap<String, usize> {
    let mut files = Vec::new();
    for sub in roots {
        rust_files(&root.join(sub), &mut files);
    }
    let mut found = BTreeMap::new();
    for path in files {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let key = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if SKIP_FILES.contains(&key.as_str()) {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let text = production_text(&raw);
        let n: usize = EXITS.iter().map(|p| text.matches(p).count()).sum();
        if n > 0 {
            found.insert(key, n);
        }
    }
    found
}

fn unguarded(found: &BTreeMap<String, usize>, guarded: &[(&str, usize, &str)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (path, n) in found {
        match guarded.iter().find(|(p, _, _)| p == path) {
            None => bad.push(format!(
                "{path}: {n} network exit(s) with no Offline only entry"
            )),
            Some((_, listed, _)) if listed != n => bad.push(format!(
                "{path}: {n} network exit(s), {listed} listed as guarded"
            )),
            Some(_) => {}
        }
    }
    for (path, _, _) in guarded {
        if !found.contains_key(*path) {
            bad.push(format!(
                "{path}: listed but opens no connection (stale entry)"
            ));
        }
    }
    bad
}

#[test]
fn every_network_exit_obeys_offline_only() {
    let found = scan(&repo_root(), &["crates", "src-tauri/src"]);
    assert!(
        found.len() >= GUARDED.len(),
        "the walk found too little: {found:?}"
    );
    assert_eq!(unguarded(&found, GUARDED), Vec::<String>::new());
}

#[test]
fn an_agent_from_the_http_facade_counts_as_an_exit() {
    let root = std::env::temp_dir().join(format!("offline-guard-facade-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let src = root.join("crates").join("demo").join("src");
    fs::create_dir_all(&src).unwrap();
    let fixture = "pub fn a() { let _ = grokhub_core::http::agent(Default::default()); }\n\
                   pub fn b() { let _ = crate::http::agent(Default::default()); }\n";
    fs::write(src.join("feed.rs"), fixture).unwrap();
    let found = scan(&root, &["crates"]);
    assert_eq!(
        found,
        BTreeMap::from([("crates/demo/src/feed.rs".to_string(), 2)])
    );
    assert_eq!(
        unguarded(&found, &[]),
        vec!["crates/demo/src/feed.rs: 2 network exit(s) with no Offline only entry".to_string()]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_new_exit_fails_and_code_after_a_test_module_still_counts() {
    let root = std::env::temp_dir().join(format!("offline-guard-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let src = root.join("crates").join("demo").join("src");
    fs::create_dir_all(&src).unwrap();
    // CRLF, a test module with a loopback fake, and one real exit after it.
    let fixture = "#[cfg(test)]\r\nmod tests {\r\n    fn fake() { let _ = ureq::get(\"http://127.0.0.1:9/\"); }\r\n}\r\n\r\npub fn fetch() {\r\n    let _ = std::net::TcpStream::connect((\"1.1.1.1\", 443));\r\n}\r\n";
    fs::write(src.join("feed.rs"), fixture).unwrap();
    fs::write(
        src.join("feed_tests.rs"),
        "fn t() { let _ = ureq::post(\"http://127.0.0.1:9/\"); }\n",
    )
    .unwrap();
    let found = scan(&root, &["crates"]);
    assert_eq!(
        found,
        BTreeMap::from([("crates/demo/src/feed.rs".to_string(), 1)])
    );
    assert_eq!(
        unguarded(&found, &[]),
        vec!["crates/demo/src/feed.rs: 1 network exit(s) with no Offline only entry".to_string()]
    );
    let listed = [("crates/demo/src/feed.rs", 1, "guarded")];
    assert_eq!(unguarded(&found, &listed), Vec::<String>::new());
    let stale = [("crates/demo/src/gone.rs", 1, "guarded")];
    assert_eq!(
        unguarded(&BTreeMap::new(), &stale),
        vec!["crates/demo/src/gone.rs: listed but opens no connection (stale entry)".to_string()]
    );
    let _ = fs::remove_dir_all(root);
}
