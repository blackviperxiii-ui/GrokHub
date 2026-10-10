//! Spike-4c coverage: every outbound request builder in `crates/` (a `ureq`
//! agent or request, a websocket client) sits behind the EgressGuard. Each file
//! that builds one is listed in [`WRAPPED`] with how many it builds and the
//! guard call in front of them. A new builder fails this test until it is
//! wrapped and listed, so the next fetch can't skip the guard.
//!
//! Sources are read with `std::fs` (CRLF normalized), not `include_str!`, so the
//! check is the same on Windows. Test modules, `tests/` folders, and this file
//! are skipped: their fake servers are on loopback.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// What starts an outbound request.
const BUILDERS: &[&str] = &[
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
    "tungstenite::client(",
    "tungstenite::connect(",
];

/// `(path under crates/, builders in it, the guard in front of them)`.
const WRAPPED: &[(&str, usize, &str)] = &[
    ("grokhub-agent/src/client.rs", 1, "XaiClient::once: guard_egress (chat, personal) to api.x.ai"),
    ("grokhub-agent/src/mcp/http.rs", 1, "connect / ping: guard_quiet (no user data); call: guard_or_park"),
    ("grokhub-agent/src/route/providers.rs", 1, "ProviderSource::fetch / call_model: guard_provider / provider_or_park (your grant, else a hard send card)"),
    ("grokhub-agent/src/route/sources.rs", 1, "XaiApiSource::get / XaiProbeTransport::send: guard_egress (no user data) to api.x.ai"),
    ("grokhub-agent/src/plugins.rs", 1, "fetch_marketplace: guard_quiet (no user data)"),
    ("grokhub-agent/src/tools/media.rs", 1, "run_call / poll_video / save_results: guard_or_park"),
    ("grokhub-agent/src/tools/web_fetch.rs", 1, "fetch_markdown: guard_or_park on every hop"),
    ("grokhub-app/src/app/model_download_ui.rs", 1, "HfFetch::open / sha256: egress_ok (no user data) to Hugging Face"),
    ("grokhub-app/src/app/pulse_ui.rs", 1, "source_preview / cache_image: egress_ok (chat)"),
    ("grokhub-app/src/desktop.rs", 3, "hub health, CDP HTTP and websocket: egress_ok (loopback)"),
    ("grokhub-app/src/github.rs", 1, "run_github_tool: egress_ok (chat)"),
    ("grokhub-app/src/imagine_auth.rs", 2, "post_form / imagine_discovery: egress_ok (no user data)"),
    ("grokhub-app/src/oauth.rs", 4, "discovery / post_form / userinfo / photo: egress_ok (no user data)"),
    ("grokhub-app/src/update.rs", 1, "latest cabin tag: egress_ok (no user data)"),
    ("grokhub-app/src/xai.rs", 1, "xai_agent callers: grok_json, STT, TTS, polls, downloads"),
];

/// The part of a source file that ships: CRLF normalized, `#[cfg(test)] mod …` cut off.
fn production_text(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let next = lines.get(i + 1).map(|l| l.trim_start()).unwrap_or("");
        if line.trim() == "#[cfg(test)]" && (next.starts_with("mod ") || next.starts_with("pub(crate) mod ")) {
            break;
        }
        out.push_str(line);
        out.push('\n');
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
        } else if name.ends_with(".rs") && !matches!(name.as_str(), "tests.rs" | "egress_coverage.rs") {
            out.push(path);
        }
    }
}

/// Builders per file, keyed by the path under `crates_dir` with `/` separators.
pub(crate) fn scan(crates_dir: &Path) -> BTreeMap<String, usize> {
    let mut files = Vec::new();
    rust_files(crates_dir, &mut files);
    let mut found = BTreeMap::new();
    for path in files {
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let text = production_text(&raw);
        let n: usize = BUILDERS.iter().map(|b| text.matches(b).count()).sum();
        if n > 0 {
            let rel = path.strip_prefix(crates_dir).unwrap_or(&path);
            let key = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            found.insert(key, n);
        }
    }
    found
}

/// Every file whose builders don't match its [`WRAPPED`] entry (or has none).
pub(crate) fn unwrapped(found: &BTreeMap<String, usize>, wrapped: &[(&str, usize, &str)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (path, n) in found {
        match wrapped.iter().find(|(p, _, _)| p == path) {
            None => bad.push(format!("{path}: {n} outbound builder(s) with no EgressGuard entry")),
            Some((_, listed, _)) if listed != n => {
                bad.push(format!("{path}: {n} outbound builder(s), {listed} listed as wrapped"))
            }
            Some(_) => {}
        }
    }
    for (path, _, _) in wrapped {
        if !found.contains_key(*path) {
            bad.push(format!("{path}: listed as wrapped but builds no request (stale entry)"));
        }
    }
    bad
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn every_outbound_builder_in_crates_is_behind_the_egress_guard() {
    let found = scan(&crates_dir());
    assert!(found.len() >= WRAPPED.len(), "the walk found too little: {found:?}");
    assert_eq!(unwrapped(&found, WRAPPED), Vec::<String>::new());
}

#[test]
fn an_unwrapped_fetch_fails_the_coverage_check() {
    let root = crate::harness::test_dir("egress-coverage");
    let src = root.join("grokhub-demo").join("src");
    fs::create_dir_all(&src).unwrap();
    // CRLF, a test module with a loopback fake, and one real fetch above it.
    let fixture = "pub fn fetch() -> String {\r\n    ureq::get(\"https://example.org/feed\").call().unwrap().into_string().unwrap()\r\n}\r\n\r\n#[cfg(test)]\r\nmod tests {\r\n    fn fake() { let _ = ureq::get(\"http://127.0.0.1:9/\"); }\r\n}\r\n";
    fs::write(src.join("feed.rs"), fixture).unwrap();
    fs::write(src.join("tests.rs"), "fn t() { let _ = ureq::post(\"http://127.0.0.1:9/\"); }\n").unwrap();
    let found = scan(&root);
    assert_eq!(found, BTreeMap::from([("grokhub-demo/src/feed.rs".to_string(), 1)]));
    assert_eq!(
        unwrapped(&found, &[]),
        vec!["grokhub-demo/src/feed.rs: 1 outbound builder(s) with no EgressGuard entry".to_string()]
    );
    let listed = [("grokhub-demo/src/feed.rs", 1, "guarded")];
    assert_eq!(unwrapped(&found, &listed), Vec::<String>::new());
    // A second fetch in a wrapped file fails until it is counted too.
    fs::write(src.join("feed.rs"), fixture.replace("\r\n}\r\n\r\n#[cfg", "\r\n}\r\npub fn more() { let _ = ureq::agent(); }\r\n#[cfg")).unwrap();
    assert_eq!(
        unwrapped(&scan(&root), &listed),
        vec!["grokhub-demo/src/feed.rs: 2 outbound builder(s), 1 listed as wrapped".to_string()]
    );
    let _ = fs::remove_dir_all(root);
}
