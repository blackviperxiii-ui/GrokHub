use super::*;
use grokhub_agent::route::providers::{use_vault_for, MemoryVault, KEYRING_OFF};
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

const FAKE: &str = "sk-abcdefghijklmnopqrstuv";

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-github-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A config dir with its own in-memory keyring.
fn config(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    use_vault_for(&dir, Arc::new(MemoryVault::default()));
    dir
}

/// A server on this computer that answers one request with `status` and
/// `body`, and hands back the request head it got.
fn mock(
    status: &'static str,
    extra: &'static str,
    body: String,
) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let mut got = Vec::new();
        let mut byte = [0u8; 1];
        while !got.ends_with(b"\r\n\r\n") && sock.read(&mut byte).unwrap_or(0) == 1 {
            got.push(byte[0]);
        }
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        // The client may stop reading a body that is too large.
        let _ = sock.write_all(reply.as_bytes());
        let _ = sock.write_all(body.as_bytes());
        String::from_utf8_lossy(&got).to_string()
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

#[test]
fn who_am_i_sends_the_token_to_the_user_endpoint_and_names_the_account() {
    let dir = config("who");
    save_token(&dir, FAKE).expect("save");
    let (base, got) = mock(
        "200 OK",
        "",
        r#"{"login":"octo-user","name":"Robin Lee","public_repos":3}"#.into(),
    );
    let ran = run_at(&dir, &base, Tool::User, false).expect("run");
    assert_eq!(
        ran,
        Ran {
            summary: "Signed in as octo-user (Robin Lee) · 3 public repos".into(),
            repos: Vec::new(),
        }
    );
    let head = got.join().expect("server").to_ascii_lowercase();
    assert!(head.starts_with("get /user http/1.1\r\n"), "{head}");
    assert!(
        head.contains(&format!("authorization: bearer {FAKE}\r\n")),
        "{head}"
    );
    assert!(
        head.contains("accept: application/vnd.github+json\r\n"),
        "{head}"
    );
    assert!(
        head.contains("x-github-api-version: 2022-11-28\r\n"),
        "{head}"
    );
}

#[test]
fn an_account_with_no_name_shows_only_its_login() {
    let dir = config("noname");
    save_token(&dir, FAKE).expect("save");
    let (base, _got) = mock(
        "200 OK",
        "",
        r#"{"login":"octo","name":null,"public_repos":1}"#.into(),
    );
    assert_eq!(
        run_at(&dir, &base, Tool::User, false).map(|r| r.summary),
        Ok("Signed in as octo · 1 public repo".to_string())
    );
}

#[test]
fn list_repos_asks_for_the_twenty_most_recently_updated() {
    let dir = config("repos");
    save_token(&dir, FAKE).expect("save");
    let (base, got) = mock(
        "200 OK",
        "",
        r#"[
            {"full_name":"octo/my-app","private":false,"description":"An app","html_url":"https://github.com/octo/my-app"},
            {"full_name":"octo/notes","private":true,"description":null,"html_url":"https://github.com/octo/notes"}
        ]"#
        .into(),
    );
    let ran = run_at(&dir, &base, Tool::ListRepos, false).expect("run");
    assert_eq!(ran.summary, "2 repositories, most recently updated first.");
    assert_eq!(
        ran.repos,
        vec![
            RepoRow {
                name: "octo/my-app".into(),
                private: false,
                description: "An app".into(),
            },
            RepoRow {
                name: "octo/notes".into(),
                private: true,
                description: String::new(),
            },
        ]
    );
    let head = got.join().expect("server");
    assert!(
        head.starts_with("GET /user/repos?per_page=20&sort=updated HTTP/1.1\r\n"),
        "{head}"
    );
}

#[test]
fn a_rejected_token_says_why_and_asks_for_a_new_one() {
    let dir = config("401");
    save_token(&dir, FAKE).expect("save");
    let (base, _got) = mock(
        "401 Unauthorized",
        "",
        r#"{"message":"Bad credentials","documentation_url":"https://docs.github.com/rest"}"#
            .into(),
    );
    assert_eq!(
        run_at(&dir, &base, Tool::User, false),
        Err("GitHub didn't accept this token (401 Bad credentials). Paste a new one.".to_string())
    );
    let (base, _got) = mock("403 Forbidden", "", String::new());
    assert_eq!(
        run_at(&dir, &base, Tool::ListRepos, false),
        Err("GitHub refused this (403).".to_string())
    );
}

#[test]
fn a_redirect_is_not_followed_so_the_token_goes_nowhere_else() {
    let dir = config("302");
    save_token(&dir, FAKE).expect("save");
    // Nothing listens on port 9, so following it would fail differently.
    let (base, _got) = mock(
        "302 Found",
        "Location: http://127.0.0.1:9/user\r\n",
        String::new(),
    );
    assert_eq!(
        run_at(&dir, &base, Tool::User, false),
        Err("GitHub answered 302.".to_string())
    );
}

#[test]
fn an_answer_over_the_cap_is_not_read() {
    let dir = config("big");
    save_token(&dir, FAKE).expect("save");
    let (base, _got) = mock("200 OK", "", "x".repeat(ANSWER_CAP as usize + 1));
    assert_eq!(
        run_at(&dir, &base, Tool::ListRepos, false),
        Err("GitHub's answer was too large to read.".to_string())
    );
}

#[test]
fn nothing_is_sent_until_a_token_is_saved() {
    let dir = config("none");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let base = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    assert_eq!(
        run_at(&dir, &base, Tool::User, false),
        Err(SAVE_FIRST.to_string())
    );
    assert_eq!(
        listener.accept().map_err(|e| e.kind()).err(),
        Some(std::io::ErrorKind::WouldBlock)
    );
}

#[test]
fn saving_checks_the_token_and_never_shows_it() {
    let dir = config("save");
    assert_eq!(
        view(&dir),
        View {
            token_set: false,
            status: String::new()
        }
    );
    assert_eq!(save_token(&dir, "sk-abc def"), Err(NO_SPACES.to_string()));
    assert!(!token_present(&dir));

    let saved = save_token(&dir, &format!("  {FAKE}\n")).expect("save");
    assert_eq!(
        saved,
        View {
            token_set: true,
            status: "GitHub token saved.".into()
        }
    );
    assert_eq!(
        named_key(&dir, TOKEN_ACCOUNT)
            .expect("read")
            .as_deref()
            .map(String::as_str),
        Some(FAKE)
    );
    let shown = serde_json::to_string(&saved).expect("json");
    assert_eq!(shown, r#"{"tokenSet":true,"status":"GitHub token saved."}"#);

    let removed = save_token(&dir, " ").expect("remove");
    assert_eq!(
        removed,
        View {
            token_set: false,
            status: "GitHub token removed.".into()
        }
    );
    assert!(!token_present(&dir));
}

#[test]
fn without_a_keyring_nothing_is_saved() {
    let dir = temp_dir("nokeyring");
    assert_eq!(save_token(&dir, FAKE), Err(KEYRING_OFF.to_string()));
    assert!(!token_present(&dir));
}

#[test]
fn tools_arrive_from_the_page_by_their_camel_case_names() {
    assert_eq!(
        serde_json::from_str::<Tool>(r#""user""#).ok(),
        Some(Tool::User)
    );
    assert_eq!(
        serde_json::from_str::<Tool>(r#""listRepos""#).ok(),
        Some(Tool::ListRepos)
    );
    assert!(serde_json::from_str::<Tool>(r#""deleteRepo""#).is_err());
}
