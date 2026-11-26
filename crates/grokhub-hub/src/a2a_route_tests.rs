use super::*;
use crate::server::serve_background_with;
use grokhub_core::agent_team::{Kind, Profile, Team, TeamFile, VERSION};
use grokhub_core::HubState;
use std::io::Write as _;
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const UNKNOWN_ROUTE: &str = r#"{"error":"unknown hub route","ok":false}"#;

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "gh-a2a-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `front-desk` is shared with `laptop` and `tablet`; `planner` with nobody.
fn desk() -> Profile {
    desk_for(&["laptop", "tablet"])
}

fn desk_for(computers: &[&str]) -> Profile {
    let mut desk = Profile::new(Kind::Customer, "front-desk", "Front desk");
    desk.about = "Answers opening hours.".into();
    desk.share = true;
    desk.share_with = computers.iter().map(|c| c.to_string()).collect();
    desk
}

fn write_team(cfg: &Path) {
    write_team_for(cfg, &["laptop", "tablet"]);
}

fn write_team_for(cfg: &Path, computers: &[&str]) {
    let mut planner = Profile::new(Kind::Role, "planner", "Planner");
    planner.team = "home".into();
    TeamFile {
        version: VERSION,
        agents: vec![desk_for(computers), planner],
        teams: vec![Team {
            id: "home".into(),
            name: "Home".into(),
            lead: "planner".into(),
        }],
    }
    .save(cfg)
    .unwrap();
}

/// Answers with `reply`, or waits while `hold` is on until it is released or canceled.
struct Scripted {
    reply: &'static str,
    hold: AtomicBool,
    asks: Mutex<Vec<(String, String, String)>>,
    canceled: AtomicUsize,
    limit: Duration,
}

impl Scripted {
    fn new(hold: bool) -> Arc<Scripted> {
        Scripted::limited(hold, Duration::from_secs(60))
    }

    fn limited(hold: bool, limit: Duration) -> Arc<Scripted> {
        Arc::new(Scripted {
            reply: "open 9 to 5",
            hold: AtomicBool::new(hold),
            asks: Mutex::new(Vec::new()),
            canceled: AtomicUsize::new(0),
            limit,
        })
    }
}

impl PeerTurns for Scripted {
    fn time_limit(&self) -> Duration {
        self.limit
    }

    fn run(&self, ask: PeerAsk) -> Outcome {
        self.asks
            .lock()
            .unwrap()
            .push((ask.peer.clone(), ask.agent.clone(), ask.text.clone()));
        while self.hold.load(Ordering::SeqCst) {
            if ask.cancel.is_cancelled() {
                self.canceled.fetch_add(1, Ordering::SeqCst);
                return Outcome::Failed("canceled".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Outcome::Completed(self.reply.into())
    }
}

struct Hub {
    port: u16,
    state: Arc<Mutex<HubState>>,
    door: Arc<A2aDoor>,
    cfg: PathBuf,
    laptop: String,
    tablet: String,
    desk_pc: String,
}

fn pair(state: &Arc<Mutex<HubState>>, id: &str) -> String {
    let mut st = state.lock().unwrap();
    let code = st.rotate_pair().code;
    st.pair_with(&code, id, id).unwrap().token
}

fn no_lan() -> Option<IpAddr> {
    None
}

fn hub(tag: &str, turns: impl FnOnce(&Path) -> Arc<dyn PeerTurns>, with_door: bool) -> Hub {
    let cfg = scratch(tag);
    write_team(&cfg);
    let turns = turns(&cfg);
    let mut st = HubState::empty();
    st.sharing = true;
    let state = Arc::new(Mutex::new(st));
    let laptop = pair(&state, "laptop");
    let tablet = pair(&state, "tablet");
    let desk_pc = pair(&state, "desk-pc");
    let door = Arc::new(A2aDoor::new(cfg.clone(), turns, no_lan, "3.6.0"));
    let port = serve_background_with(state.clone(), 0, with_door.then(|| door.clone())).unwrap();
    state.lock().unwrap().port = port;
    Hub {
        port,
        state,
        door,
        cfg,
        laptop,
        tablet,
        desk_pc,
    }
}

fn http(port: u16, req: &str) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = match text.find("\r\n\r\n") {
        Some(i) => buf[i + 4..].to_vec(),
        None => buf,
    };
    (status, body)
}

fn get(port: u16, token: &str, path: &str) -> (u16, Vec<u8>) {
    http(
        port,
        &format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\n\r\n"),
    )
}

fn post(port: u16, token: &str, path: &str, headers: &str, body: &str) -> (u16, Value) {
    let (status, out) = http(
        port,
        &format!(
            "POST {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    (status, serde_json::from_slice(&out).unwrap_or(Value::Null))
}

fn rpc(port: u16, token: &str, method: &str, params: Value) -> Value {
    let body = json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params });
    let (status, reply) = post(port, token, "/a2a/v1/front-desk", "", &body.to_string());
    assert_eq!(status, 200, "{reply}");
    reply
}

fn send(port: u16, token: &str, text: &str, immediately: bool) -> Value {
    rpc(
        port,
        token,
        "SendMessage",
        json!({
            "message": { "messageId": "m-1", "role": "ROLE_USER", "parts": [{ "text": text }] },
            "configuration": { "returnImmediately": immediately },
        }),
    )
}

fn state_of(port: u16, token: &str, id: &str) -> Value {
    rpc(port, token, "GetTask", json!({ "id": id }))["result"]["status"]["state"].clone()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_hub_without_a_door_has_no_agent_routes() {
    let hub = hub("nodoor", |_| Scripted::new(false), false);
    for path in ["/a2a/v1", "/a2a/v1/front-desk"] {
        let (status, body) = get(hub.port, &hub.laptop, path);
        assert_eq!(
            (status, String::from_utf8(body).unwrap()),
            (404, UNKNOWN_ROUTE.into())
        );
    }
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn the_door_needs_a_token_sharing_on_and_no_web_page() {
    let hub = hub("gates", |_| Scripted::new(false), true);
    let (status, _) = get(hub.port, "", "/a2a/v1/front-desk");
    assert_eq!(status, 401);
    let (status, body) = http(
        hub.port,
        &format!(
            "GET /a2a/v1/front-desk HTTP/1.0\r\nHost: 127.0.0.1\r\nOrigin: http://evil.example\r\nAuthorization: Bearer {}\r\n\r\n",
            hub.laptop
        ),
    );
    assert_eq!(status, 403);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["error"],
        "Agents answer the app on a paired computer, not a web page."
    );
    hub.state.lock().unwrap().sharing = false;
    let (status, body) = get(hub.port, &hub.laptop, "/a2a/v1/front-desk");
    assert_eq!(status, 403);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["error"],
        "Sharing is off on this computer."
    );
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn a_computer_sees_only_the_agents_shared_with_it() {
    let hub = hub("cards", |_| Scripted::new(false), true);
    let url = format!("http://127.0.0.1:{}/a2a/v1/front-desk", hub.port);
    let (status, body) = get(hub.port, &hub.laptop, "/a2a/v1");
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({ "ok": true, "agents": [{ "id": "front-desk", "name": "Front desk", "url": url }] })
    );
    let (_, body) = get(hub.port, &hub.desk_pc, "/a2a/v1/");
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({ "ok": true, "agents": [] })
    );
    let (status, body) = get(hub.port, &hub.laptop, "/a2a/v1/front-desk");
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        a2a::card(&desk(), &url, "3.6.0")
    );
    // Not shared with this computer, shared with nobody, or missing: the same 404.
    for (token, path) in [
        (&hub.desk_pc, "/a2a/v1/front-desk"),
        (&hub.laptop, "/a2a/v1/planner"),
        (&hub.laptop, "/a2a/v1/ghost"),
        (&hub.laptop, "/a2a/v1/front-desk/extra"),
    ] {
        let (status, body) = get(hub.port, token, path);
        assert_eq!(
            (status, String::from_utf8(body).unwrap()),
            (404, UNKNOWN_ROUTE.into()),
            "{path}"
        );
    }
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn bad_requests_get_their_json_rpc_errors_and_never_reach_the_agent() {
    let turns = Scripted::new(false);
    let hub = hub("errors", |_| turns.clone(), true);
    let code = |reply: &Value| reply["error"]["code"].as_i64();
    let big = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{{"id":"{}"}}}}"#,
        "x".repeat(97 * 1024)
    );
    let (_, reply) = post(hub.port, &hub.laptop, "/a2a/v1/front-desk", "", &big);
    assert_eq!(code(&reply), Some(-32600), "{reply}");
    let (_, reply) = post(
        hub.port,
        &hub.laptop,
        "/a2a/v1/front-desk",
        "A2A-Version: 2.0\r\n",
        r#"{"jsonrpc":"2.0","id":1,"method":"ListTasks"}"#,
    );
    assert_eq!(code(&reply), Some(-32009), "{reply}");
    let reply = rpc(hub.port, &hub.laptop, "Foo", Value::Null);
    assert_eq!(reply["error"]["code"], -32601);
    assert_eq!(reply["id"], 7);
    let reply = send(hub.port, &hub.laptop, &"a".repeat(8_001), false);
    assert_eq!(
        reply["error"],
        json!({ "code": -32602, "message": "Invalid params: a message to an agent is at most 8000 characters" })
    );
    let reply = rpc(hub.port, &hub.laptop, "GetTask", json!({ "id": "task-1" }));
    assert_eq!(reply["error"]["code"], -32001);
    assert!(turns.asks.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn a_message_is_answered_and_only_the_computer_that_sent_it_sees_the_task() {
    let turns = Scripted::new(true);
    let hub = hub("answer", |_| turns.clone(), true);
    let reply = send(hub.port, &hub.laptop, "When are you open?", true);
    assert_eq!(reply["result"]["task"]["id"], "task-1", "{reply}");
    assert_eq!(
        reply["result"]["task"]["status"]["state"],
        "TASK_STATE_WORKING"
    );
    turns.hold.store(false, Ordering::SeqCst);
    wait_for("the answer", || {
        state_of(hub.port, &hub.laptop, "task-1") == "TASK_STATE_COMPLETED"
    });
    let task = rpc(hub.port, &hub.laptop, "GetTask", json!({ "id": "task-1" }))["result"].clone();
    assert_eq!(task["artifacts"][0]["parts"][0]["text"], "open 9 to 5");
    assert_eq!(
        turns.asks.lock().unwrap().clone(),
        [(
            "laptop".to_string(),
            "front-desk".to_string(),
            "When are you open?".to_string()
        )]
    );
    // A message that isn't returned at once waits for its answer.
    let reply = send(hub.port, &hub.laptop, "And on Sunday?", false);
    assert_eq!(reply["result"]["task"]["id"], "task-2");
    assert_eq!(
        reply["result"]["task"]["status"]["state"],
        "TASK_STATE_COMPLETED"
    );
    let listed = rpc(hub.port, &hub.laptop, "ListTasks", Value::Null)["result"].clone();
    let ids: Vec<&str> = listed["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["task-2", "task-1"]);
    // Another computer the agent is shared with, or this one paired again,
    // can't see them.
    let other = rpc(hub.port, &hub.tablet, "GetTask", json!({ "id": "task-1" }));
    assert_eq!(other["error"]["code"], -32001);
    let again = pair(&hub.state, "laptop");
    assert_eq!(
        rpc(hub.port, &again, "GetTask", json!({ "id": "task-1" }))["error"]["code"],
        -32001
    );
    assert_eq!(get(hub.port, &hub.laptop, "/a2a/v1").0, 401);
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn a_third_message_waits_and_cancel_stops_a_turn() {
    let turns = Scripted::new(true);
    let hub = hub("caps", |_| turns.clone(), true);
    for id in ["task-1", "task-2"] {
        let reply = send(hub.port, &hub.laptop, "When are you open?", true);
        assert_eq!(reply["result"]["task"]["id"], id);
    }
    let reply = send(hub.port, &hub.laptop, "When are you open?", true);
    assert_eq!(
        reply["error"],
        json!({ "code": -32004, "message": "Unsupported operation: 2 messages from this computer are already being answered; wait for one to end" })
    );
    let canceled = rpc(
        hub.port,
        &hub.laptop,
        "CancelTask",
        json!({ "id": "task-1" }),
    );
    assert_eq!(canceled["result"]["status"]["state"], "TASK_STATE_CANCELED");
    wait_for("the turn to see the cancel", || {
        turns.canceled.load(Ordering::SeqCst) == 1
    });
    // A canceled task stays canceled when its turn ends.
    assert_eq!(
        state_of(hub.port, &hub.laptop, "task-1"),
        "TASK_STATE_CANCELED"
    );
    // Stopping share cancels whatever is still running.
    hub.state.lock().unwrap().sharing = false;
    hub.door.recheck(&hub.state);
    wait_for("every turn to stop", || {
        turns.canceled.load(Ordering::SeqCst) == 2
    });
    wait_for("every place to free", || hub.door.desk().running.is_empty());
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

struct Panics;

impl PeerTurns for Panics {
    fn run(&self, _ask: PeerAsk) -> Outcome {
        panic!("a turn that breaks");
    }
}

#[test]
fn a_turn_that_panics_frees_its_place_and_fails_its_task() {
    let hub = hub("panic", |_| Arc::new(Panics), true);
    for id in ["task-1", "task-2"] {
        assert_eq!(
            send(hub.port, &hub.laptop, "When are you open?", true)["result"]["task"]["id"],
            id
        );
    }
    wait_for("both places to free", || hub.door.desk().running.is_empty());
    for id in ["task-1", "task-2"] {
        assert_eq!(state_of(hub.port, &hub.laptop, id), "TASK_STATE_FAILED");
    }
    let task = rpc(hub.port, &hub.laptop, "GetTask", json!({ "id": "task-1" }))["result"].clone();
    assert_eq!(
        task["status"]["message"]["parts"][0]["text"],
        "the agent couldn't answer"
    );
    let third = send(hub.port, &hub.laptop, "When are you open?", false);
    assert_eq!(third["result"]["task"]["id"], "task-3", "{third}");
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn unsharing_or_pairing_again_stops_a_turn_and_its_answer_never_goes_out() {
    let turns = Scripted::new(true);
    let hub = hub("revoke", |_| turns.clone(), true);
    let (port, laptop) = (hub.port, hub.laptop.clone());
    let held = std::thread::spawn(move || send(port, &laptop, "When are you open?", false));
    wait_for("the turn to start", || {
        turns.asks.lock().unwrap().len() == 1
    });
    write_team_for(&hub.cfg, &["tablet"]);
    hub.door.recheck(&hub.state);
    wait_for("the turn to see the cancel", || {
        turns.canceled.load(Ordering::SeqCst) == 1
    });
    let reply = held.join().unwrap();
    assert_eq!(
        reply["error"],
        json!({ "code": -32001, "message": "Task not found: task-1" }),
        "{reply}"
    );
    assert_eq!(reply.get("result"), None);
    // Shared again, then the laptop pairs again over the hub's own route:
    // the turn its old token started stops, with no other request.
    write_team(&hub.cfg);
    let started = send(hub.port, &hub.laptop, "And on Sunday?", true);
    assert_eq!(started["result"]["task"]["id"], "task-1", "{started}");
    let code = hub.state.lock().unwrap().rotate_pair().code;
    let body = json!({ "code": code, "deviceId": "laptop", "deviceName": "Laptop" }).to_string();
    let (status, _) = http(
        hub.port,
        &format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert_eq!(status, 200);
    wait_for("the old token's turn to stop", || {
        turns.canceled.load(Ordering::SeqCst) == 2
    });
    wait_for("its place to free", || hub.door.desk().running.is_empty());
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn an_answer_never_goes_to_a_computer_the_agent_stopped_being_shared_with() {
    let turns = Scripted::new(true);
    let hub = hub("unshared", |_| turns.clone(), true);
    let (port, laptop) = (hub.port, hub.laptop.clone());
    let held = std::thread::spawn(move || send(port, &laptop, "When are you open?", false));
    wait_for("the turn to start", || {
        turns.asks.lock().unwrap().len() == 1
    });
    // The team is saved without the laptop, and nothing tells the door.
    write_team_for(&hub.cfg, &["tablet"]);
    turns.hold.store(false, Ordering::SeqCst);
    let reply = held.join().unwrap();
    assert_eq!(
        reply["error"],
        json!({ "code": -32001, "message": "Task not found: task-1" }),
        "{reply}"
    );
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn a_message_still_arriving_when_sharing_stops_never_starts() {
    let turns = Scripted::new(false);
    let hub = hub("midbody", |_| turns.clone(), true);
    let body = json!({
        "jsonrpc": "2.0", "id": 7, "method": "SendMessage",
        "params": { "message": { "messageId": "m-1", "role": "ROLE_USER", "parts": [{ "text": "a".repeat(2_000) }] } },
    })
    .to_string();
    let mut s = TcpStream::connect(("127.0.0.1", hub.port)).unwrap();
    s.write_all(
        format!(
            "POST /a2a/v1/front-desk HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            hub.laptop,
            body.len(),
            &body[..10]
        )
        .as_bytes(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    hub.state.lock().unwrap().sharing = false;
    hub.door.recheck(&hub.state);
    s.write_all(&body.as_bytes()[10..]).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    let reply: Value = serde_json::from_str(&text[text.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(
        reply["error"],
        json!({ "code": -32004, "message": "Unsupported operation: this computer stopped sharing that agent with yours" })
    );
    assert!(turns.asks.lock().unwrap().is_empty());
    assert!(hub.door.desk().running.is_empty());
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn a_blocking_message_waits_for_the_end_and_errs_past_the_time_limit() {
    let turns = Scripted::new(true);
    let quick = hub("block", |_| turns.clone(), true);
    let release = {
        let turns = turns.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            turns.hold.store(false, Ordering::SeqCst);
        })
    };
    let reply = send(quick.port, &quick.laptop, "When are you open?", false);
    release.join().unwrap();
    assert_eq!(
        reply["result"]["task"]["status"]["state"], "TASK_STATE_COMPLETED",
        "{reply}"
    );
    let slow = Scripted::limited(true, Duration::from_millis(200));
    let hub2 = hub("block-limit", |_| slow.clone(), true);
    let reply = send(hub2.port, &hub2.laptop, "When are you open?", false);
    assert_eq!(
        reply["error"],
        json!({ "code": -32603, "message": "Internal error: the agent is still working on task-1; get it with GetTask" })
    );
    slow.hold.store(false, Ordering::SeqCst);
    wait_for("the answer", || {
        state_of(hub2.port, &hub2.laptop, "task-1") == "TASK_STATE_COMPLETED"
    });
    for cfg in [&quick.cfg, &hub2.cfg] {
        let _ = std::fs::remove_dir_all(cfg);
    }
}

#[test]
fn a_task_list_pages_by_token_and_leaves_artifacts_out_unless_asked() {
    let hub = hub("pages", |_| Scripted::new(false), true);
    for _ in 0..3 {
        send(hub.port, &hub.laptop, "When are you open?", false);
    }
    let ids = |page: &Value| -> Vec<String> {
        page["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_string())
            .collect()
    };
    let first = rpc(hub.port, &hub.laptop, "ListTasks", json!({ "pageSize": 2 }))["result"].clone();
    assert_eq!(ids(&first), ["task-3", "task-2"]);
    assert_eq!(
        (
            first["nextPageToken"].clone(),
            first["pageSize"].clone(),
            first["totalSize"].clone()
        ),
        (json!("task-2"), json!(2), json!(3))
    );
    assert_eq!(first["tasks"][0].get("artifacts"), None, "{first}");
    let next = rpc(
        hub.port,
        &hub.laptop,
        "ListTasks",
        json!({ "pageSize": 2, "pageToken": "task-2", "includeArtifacts": true }),
    )["result"]
        .clone();
    assert_eq!(ids(&next), ["task-1"]);
    assert_eq!(next["nextPageToken"], "");
    assert_eq!(
        next["tasks"][0]["artifacts"][0]["parts"][0]["text"],
        "open 9 to 5"
    );
    let working = rpc(
        hub.port,
        &hub.laptop,
        "ListTasks",
        json!({ "status": "TASK_STATE_WORKING", "contextId": "" }),
    )["result"]
        .clone();
    assert_eq!(
        (ids(&working).len(), working["totalSize"].clone()),
        (0, json!(0))
    );
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn a_card_points_at_the_address_the_computer_used() {
    let hub = hub("host", |_| Scripted::new(false), true);
    let url_for = |host: &str| {
        let (status, body) = http(
            hub.port,
            &format!(
                "GET /a2a/v1 HTTP/1.0\r\nHost: {host}\r\nAuthorization: Bearer {}\r\n\r\n",
                hub.laptop
            ),
        );
        assert_eq!(status, 200, "{host}");
        serde_json::from_slice::<Value>(&body).unwrap()["agents"][0]["url"].clone()
    };
    let port = hub.port;
    assert_eq!(
        url_for(&format!("192.168.1.50:{port}")),
        format!("http://192.168.1.50:{port}/a2a/v1/front-desk")
    );
    assert_eq!(
        url_for("my-pc.local"),
        format!("http://my-pc.local:{port}/a2a/v1/front-desk")
    );
    assert_eq!(
        url_for("[fe80::1]:8080"),
        "http://[fe80::1]:8080/a2a/v1/front-desk"
    );
    // A host that isn't a plain host and port falls back to this computer's address.
    assert_eq!(
        url_for("evil.example/x"),
        format!("http://127.0.0.1:{port}/a2a/v1/front-desk")
    );
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

#[test]
fn only_a_plain_host_and_port_count_as_an_address() {
    assert_eq!(
        authority("pc.local:7878"),
        Some(("pc.local:7878", Some(7878)))
    );
    assert_eq!(authority("10.0.0.2"), Some(("10.0.0.2", None)));
    assert_eq!(authority("[::1]"), Some(("[::1]", None)));
    assert_eq!(authority("[::1]:9"), Some(("[::1]:9", Some(9))));
    for bad in [
        "",
        "::1",
        "a:b:c",
        "pc:+80",
        "pc:0",
        "pc:65536",
        "pc:",
        "[]",
        "[::1]x",
        "[zz]:1",
        "pc/a",
        "pc@x",
        "user@pc:1",
    ] {
        assert_eq!(authority(bad), None, "{bad}");
    }
}

#[test]
fn a_computer_sends_at_most_twenty_messages_an_hour() {
    let hub = hub("hourly", |_| Scripted::new(false), true);
    for n in 1..=20 {
        let reply = send(hub.port, &hub.laptop, "When are you open?", false);
        assert_eq!(
            reply["result"]["task"]["status"]["state"], "TASK_STATE_COMPLETED",
            "{n}: {reply}"
        );
    }
    let reply = send(hub.port, &hub.laptop, "When are you open?", false);
    assert_eq!(
        reply["error"]["message"],
        "Unsupported operation: this computer takes 20 messages an hour from each paired computer"
    );
    let _ = std::fs::remove_dir_all(&hub.cfg);
}

/// The app's turns: the real `answer_peer`, with a model that always says one thing.
struct RealTurns {
    config: PathBuf,
}

struct SaysHours;

impl grokhub_agent::ModelClient for SaysHours {
    fn stream(
        &self,
        _req: &grokhub_agent::ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(grokhub_agent::StreamEvent),
    ) -> Result<grokhub_agent::TurnOutput, grokhub_agent::ClientError> {
        Ok(grokhub_agent::TurnOutput {
            text: "We're open 9 to 5.".into(),
            ..Default::default()
        })
    }
}

impl PeerTurns for RealTurns {
    fn run(&self, ask: PeerAsk) -> Outcome {
        grokhub_agent::answer_peer(grokhub_agent::PeerTurn {
            config: self.config.clone(),
            peer: ask.peer,
            agent: ask.agent,
            text: ask.text,
            client: Arc::new(SaysHours),
            model: "grok-4".into(),
            cancel: ask.cancel,
        })
    }
}

#[test]
fn a_shared_agent_answers_through_the_app_turn_and_the_log_names_the_computer() {
    let hub = hub(
        "e2e",
        |cfg| {
            Arc::new(RealTurns {
                config: cfg.to_path_buf(),
            })
        },
        true,
    );
    let cfg = hub.cfg.clone();
    let reply = send(hub.port, &hub.laptop, "When are you open?", false);
    let task = &reply["result"]["task"];
    assert_eq!(task["status"]["state"], "TASK_STATE_COMPLETED", "{reply}");
    assert_eq!(
        task["artifacts"][0]["parts"][0]["text"],
        "We're open 9 to 5."
    );
    let log = grokhub_core::agent_tasks::recent(&cfg, 10);
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(
        serde_json::to_value(&log[0].from).unwrap(),
        json!({ "peer": "laptop" })
    );
    assert_eq!(log[0].to, "front-desk");
    assert_eq!(log[0].asked, "When are you open?");
    let _ = std::fs::remove_dir_all(&cfg);
}
