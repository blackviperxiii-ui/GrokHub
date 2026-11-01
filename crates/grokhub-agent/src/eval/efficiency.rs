//! Efficiency bench (LO-30): what each fixed scenario would send to a model,
//! measured offline with a scripted client. No network, no keys.
//!
//! Per scenario it reports how many model requests the engine made, the bytes
//! of the first request split into system prompt / tool schemas / history, the
//! bytes summed over every request (what an uncached provider re-reads), the
//! share of each request's head that the next request repeats byte for byte
//! (what Grok's prompt cache and llama.cpp's KV cache can reuse), whether facts
//! planted early are still in the last request (recall), wall time and the
//! process's peak RSS. Token cards compare these numbers before and after.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};

use super::{scratch, text_turn, tool_turn, NoHalt, SUITE};
use crate::client::{
    responses_body, ClientError, ContentPart, InputItem, ResponsesRequest, StreamEvent, TurnOutput,
};
use crate::events::Engine;
use crate::gate::{ClosedPermits, Gate, PermMode};
use crate::perm::ConfigGuard;
use crate::{AuthKind, CancelToken, EngineParts, ModelClient, NativeEngine, SteerQueue, Usage};

/// A runaway engine is cut off here so the bench always ends; hitting it is
/// itself the finding.
pub const BENCH_REQUEST_CAP: usize = 60;

/// One planted fact the last request must still carry.
const FACT_NOTE: &str = "FACT-ALPHA: the save folder is D:/Loadout/saves";
const FACT_HISTORY: &str = "FACT-BRAVO: my duo partner is called Kestrel";
const FACT_LOG: &str = "FACT-CHARLIE: the crash dump is in D:/Loadout/dumps";

/// The `re-reads` log: big enough that one read_file hits its 50,000-char cap.
const BIG_LOG: &str = "server.log";
const BIG_LOG_LINES: usize = 4000;

/// A local model's window, small enough that two full log reads pass the
/// history trim's 40% mark.
const LOCAL_CONTEXT_TOKENS: u64 = 64_000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BenchRow {
    pub scenario: &'static str,
    pub requests: usize,
    pub capped: bool,
    pub first_system_bytes: usize,
    pub first_tools_bytes: usize,
    pub first_history_bytes: usize,
    pub first_total_bytes: usize,
    pub total_bytes: usize,
    pub est_tokens_total: u64,
    /// Mean share (0-100) of request k's cacheable head that request k+1 repeats.
    pub stable_prefix_pct: u32,
    pub recall_ok: bool,
    pub wall_ms: u128,
    pub peak_rss_kb: u64,
}

struct Recorder {
    script: Vec<TurnOutput>,
    finish: String,
    n: AtomicUsize,
    bodies: Mutex<Vec<Value>>,
}

impl Recorder {
    fn new(script: Vec<TurnOutput>, finish: &str) -> Self {
        Self {
            script,
            finish: finish.to_string(),
            n: AtomicUsize::new(0),
            bodies: Mutex::new(Vec::new()),
        }
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
}

impl ModelClient for Recorder {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let n = self.n.fetch_add(1, Ordering::SeqCst);
        if n >= BENCH_REQUEST_CAP {
            return Err(ClientError::Cancelled);
        }
        // Side calls (compaction, judges, briefs) count too: they are spend.
        self.bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(responses_body(req));
        Ok(self
            .script
            .get(n)
            .cloned()
            .unwrap_or_else(|| text_turn(&self.finish)))
    }
}

fn json_len(v: &Value) -> usize {
    serde_json::to_vec(v).map(|b| b.len()).unwrap_or(0)
}

/// The part a provider cache can reuse, in the order it is read: tools, then
/// the input (system message first, then history).
fn cache_head(body: &Value) -> String {
    let tools = body.get("tools").cloned().unwrap_or(Value::Null);
    let input = body.get("input").cloned().unwrap_or(Value::Null);
    format!(
        "{}{}",
        serde_json::to_string(&tools).unwrap_or_default(),
        serde_json::to_string(&input).unwrap_or_default()
    )
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

fn split_input(body: &Value) -> (usize, usize) {
    let Some(items) = body.get("input").and_then(Value::as_array) else {
        return (0, 0);
    };
    let mut system = 0;
    let mut rest = 0;
    for item in items {
        let len = json_len(item);
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "system" || role == "developer" {
            system += len;
        } else {
            rest += len;
        }
    }
    (system, rest)
}

fn peak_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

fn row_from(scenario: &'static str, bodies: &[Value], facts: &[&str], wall_ms: u128) -> BenchRow {
    let first = bodies.first().cloned().unwrap_or(Value::Null);
    let (first_system_bytes, first_history_bytes) = split_input(&first);
    let first_tools_bytes = first.get("tools").map(json_len).unwrap_or(0);
    let total_bytes: usize = bodies.iter().map(json_len).sum();
    let heads: Vec<String> = bodies.iter().map(cache_head).collect();
    let pcts: Vec<f64> = heads
        .windows(2)
        .filter(|w| !w[0].is_empty())
        .map(|w| common_prefix(&w[0], &w[1]) as f64 * 100.0 / w[0].len() as f64)
        .collect();
    let stable_prefix_pct = if pcts.is_empty() {
        100
    } else {
        (pcts.iter().sum::<f64>() / pcts.len() as f64).round() as u32
    };
    let last = bodies
        .last()
        .map(|b| serde_json::to_string(b).unwrap_or_default())
        .unwrap_or_default();
    BenchRow {
        scenario,
        requests: bodies.len(),
        capped: bodies.len() >= BENCH_REQUEST_CAP,
        first_system_bytes,
        first_tools_bytes,
        first_history_bytes,
        first_total_bytes: json_len(&first),
        total_bytes,
        est_tokens_total: crate::tokens::estimate_token_bytes(total_bytes as u64),
        stable_prefix_pct,
        recall_ok: facts.iter().all(|f| last.contains(f)),
        wall_ms,
        peak_rss_kb: peak_rss_kb(),
    }
}

fn seed_workspace(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join("notes.txt"), format!("{FACT_NOTE}\n"));
    let mut log = String::new();
    for i in 0..2000 {
        log.push_str(&format!(
            "2026-10-31T20:{:02}:{:02} INFO service[{}] heartbeat ok latency_ms={}\n",
            i / 60 % 60,
            i % 60,
            i % 17,
            i % 97
        ));
    }
    let _ = std::fs::write(dir.join("system.log"), log);
}

fn seed_big_log(dir: &Path) {
    let mut log = format!("{FACT_LOG}\n");
    for i in 1..BIG_LOG_LINES {
        log.push_str(&format!(
            "2026-10-31T21:{:02}:{:02} WARN worker[{}] slow request ms={}\n",
            i / 60 % 60,
            i % 60,
            i % 13,
            16 + i % 41
        ));
    }
    let _ = std::fs::write(dir.join(BIG_LOG), log);
}

fn read(id: &str, file: &str) -> TurnOutput {
    tool_turn(id, "read_file", &json!({ "target_file": file }).to_string())
}

fn list(id: &str) -> TurnOutput {
    tool_turn(id, "list_dir", &json!({ "path": "" }).to_string())
}

/// A stored chat as a session file holds it: the first turn's system
/// message, then the turns.
fn history_turns(n: usize, system: &str) -> Vec<InputItem> {
    let mut out = vec![InputItem::Message {
        role: "system".into(),
        content: vec![ContentPart::InputText(system.to_string())],
    }];
    for i in 0..n {
        let user = if i == 0 {
            format!("Remember this. {FACT_HISTORY}. Also my rig has a 4070.")
        } else {
            format!("Turn {i}: what should I tweak next for frame pacing in my game?")
        };
        let reply = format!(
            "Turn {i} answer: cap the frame rate a few fps under your refresh rate, keep \
             V-Sync off in game, turn on low latency mode, and check shader cache size. {}",
            "Details: ".repeat(20)
        );
        out.push(InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(user)],
        });
        out.push(InputItem::Message {
            role: "assistant".into(),
            content: vec![ContentPart::InputText(reply)],
        });
    }
    out
}

struct Scenario {
    name: &'static str,
    /// Sent one after another in the same chat.
    prompts: Vec<&'static str>,
    script: Vec<TurnOutput>,
    finish: &'static str,
    /// Turns of an earlier chat to resume, before the prompts.
    history_turns: usize,
    facts: Vec<&'static str>,
    /// Model window in tokens; `None` keeps the engine's default.
    context_tokens: Option<u64>,
    /// Write `history_trim.toml` with `enabled = false` (the control row).
    trim_off: bool,
}

/// A log read three times with the same arguments, one turn each (a third
/// identical call in one turn would be refused unrun), then a notes read.
fn re_reads(name: &'static str, trim_off: bool) -> Scenario {
    Scenario {
        name,
        prompts: vec![
            "Read server.log and sum it up.",
            "I restarted the server. Read server.log again.",
            "One more look at server.log, then remind me where my save folder is.",
        ],
        script: vec![
            read("g1", BIG_LOG),
            text_turn("Only slow requests so far."),
            read("g2", BIG_LOG),
            text_turn("Still only slow requests."),
            read("g3", BIG_LOG),
            read("g4", "notes.txt"),
            text_turn("Slow requests only. Your save folder is D:/Loadout/saves."),
        ],
        finish: "Anything else?",
        history_turns: 0,
        facts: vec![FACT_NOTE, FACT_LOG],
        context_tokens: Some(LOCAL_CONTEXT_TOKENS),
        trim_off,
    }
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "chat",
            prompts: vec!["What can you do for my gaming PC?"],
            script: vec![text_turn(
                "I can check your rig, tune games and plan sessions.",
            )],
            finish: "Anything else?",
            history_turns: 0,
            facts: vec![],
            context_tokens: None,
            trim_off: false,
        },
        Scenario {
            name: "tools",
            prompts: vec!["Read my notes and the system log, then sum it up."],
            script: vec![
                read("a", "notes.txt"),
                list("b"),
                read("c", "system.log"),
                text_turn("Your notes name the save folder; the log is all heartbeats."),
            ],
            finish: "Done.",
            history_turns: 0,
            facts: vec![FACT_NOTE],
            context_tokens: None,
            trim_off: false,
        },
        Scenario {
            name: "diagnostics",
            prompts: vec!["run a diagnostics check on my pc"],
            script: vec![
                list("d1"),
                read("d2", "system.log"),
                read("d3", "notes.txt"),
                text_turn("Checked the folder and the system log: nothing failing."),
            ],
            finish: "Checked the folder and the system log: nothing failing.",
            history_turns: 0,
            facts: vec![FACT_NOTE],
            context_tokens: None,
            trim_off: false,
        },
        Scenario {
            name: "long-chat",
            prompts: vec!["Which name did I give my duo partner?"],
            script: vec![text_turn("Kestrel.")],
            finish: "Kestrel.",
            history_turns: 20,
            facts: vec![FACT_HISTORY],
            context_tokens: None,
            trim_off: false,
        },
        Scenario {
            name: "three-turns",
            prompts: vec![
                "Read my notes.",
                "Thanks. What does a frame cap do?",
                "And where is my save folder again?",
            ],
            script: vec![
                read("t1", "notes.txt"),
                text_turn("Your notes name the save folder."),
                text_turn("It holds the game a few fps under refresh so frame times stay even."),
                text_turn("D:/Loadout/saves."),
            ],
            finish: "Anything else?",
            history_turns: 0,
            facts: vec![FACT_NOTE],
            context_tokens: None,
            trim_off: false,
        },
        re_reads("re-reads", false),
        re_reads("re-reads (trim off)", true),
    ]
}

/// The engine the chat tab runs: the real system prompt and the app's
/// unlimited step budget, so a runaway shows up as it would for a user.
fn bench_engine(ws: &Path, id: &str, client: Arc<dyn ModelClient + Send + Sync>) -> NativeEngine {
    let stable = grokhub_core::cabin_rules::cabin_static_rules(false);
    NativeEngine::new(EngineParts {
        client,
        workspace: ws.to_path_buf(),
        model: "grok-4.7".into(),
        effort: Some("high".into()),
        system: crate::system_prompt_parts(&stable, "", ws),
        conversation_id: format!("bench-{id}"),
        auth_kind: AuthKind::ApiKey,
        max_turns: 0,
        cancel: CancelToken::new(),
        steer: SteerQueue::new(),
        halt: Box::new(NoHalt),
        gate: Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: false,
            desktop: false,
        },
        desktop: None,
        permits: Arc::new(ClosedPermits),
    })
}

/// Run every scenario and return one row each.
pub fn run_bench() -> Vec<BenchRow> {
    let _lock = SUITE.lock().unwrap_or_else(|err| err.into_inner());
    let mut rows = Vec::new();
    for sc in scenarios() {
        let Some(dir) = scratch(&format!("eff-{}", sc.name)) else {
            continue;
        };
        let _cfg = ConfigGuard::set(dir.join("cfg"));
        let ws = dir.join("ws");
        seed_workspace(&ws);
        if sc.facts.contains(&FACT_LOG) {
            seed_big_log(&ws);
        }
        if sc.trim_off {
            let _ = std::fs::create_dir_all(dir.join("cfg"));
            let _ = std::fs::write(
                dir.join("cfg").join(crate::compact::HISTORY_TRIM_CONFIG),
                "enabled = false\n",
            );
        }
        let rec = Arc::new(Recorder::new(sc.script, sc.finish));
        let shared: Arc<dyn ModelClient + Send + Sync> = rec.clone();
        let mut engine = bench_engine(&ws, sc.name, shared);
        if let Some(tokens) = sc.context_tokens {
            engine.set_context_length(tokens);
        }
        if sc.history_turns > 0 {
            let system = crate::system_prompt_parts(
                &grokhub_core::cabin_rules::cabin_static_rules(false),
                "",
                &ws,
            );
            engine.resume(history_turns(sc.history_turns, &system), Usage::default());
        }
        let start = Instant::now();
        for prompt in &sc.prompts {
            let _ = engine.prompt(prompt, None, &mut |_| {});
        }
        let wall = start.elapsed().as_millis();
        let bodies = rec.bodies();
        if let Some(out) = std::env::var_os("GROKHUB_BENCH_DUMP") {
            let out = Path::new(&out);
            let _ = std::fs::create_dir_all(out);
            let _ = std::fs::write(
                out.join(format!("{}.json", sc.name)),
                serde_json::to_string_pretty(&bodies).unwrap_or_default(),
            );
        }
        rows.push(row_from(sc.name, &bodies, &sc.facts, wall));
        let _ = std::fs::remove_dir_all(&dir);
    }
    rows
}

/// Markdown table of [`run_bench`] rows.
pub fn render_bench(rows: &[BenchRow]) -> String {
    let mut out = String::from(
        "| scenario | requests | 1st sys B | 1st tools B | 1st history B | 1st total B | all requests B | est tokens | stable prefix % | recall | wall ms | peak RSS so far KB |\n\
         |---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        out.push_str(&format!(
            "| {} | {}{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.scenario,
            r.requests,
            if r.capped { " (hit cap)" } else { "" },
            r.first_system_bytes,
            r.first_tools_bytes,
            r.first_history_bytes,
            r.first_total_bytes,
            r.total_bytes,
            r.est_tokens_total,
            r.stable_prefix_pct,
            if r.recall_ok { "ok" } else { "LOST" },
            r.wall_ms,
            r.peak_rss_kb
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_measures_every_scenario_and_keeps_planted_facts() {
        let rows = run_bench();
        let names: Vec<&str> = rows.iter().map(|r| r.scenario).collect();
        assert_eq!(
            names,
            [
                "chat",
                "tools",
                "diagnostics",
                "long-chat",
                "three-turns",
                "re-reads",
                "re-reads (trim off)"
            ]
        );
        for r in &rows {
            assert!(r.requests >= 1, "{r:?}");
            assert!(r.first_tools_bytes > 0 && r.first_system_bytes > 0, "{r:?}");
            assert!(r.recall_ok, "a planted fact fell out: {r:?}");
        }
        // Dropping the two older log reads sends fewer bytes in the same requests.
        let trimmed = &rows[5];
        let control = &rows[6];
        assert_eq!(trimmed.requests, control.requests);
        assert!(
            trimmed.total_bytes < control.total_bytes,
            "{trimmed:?} vs {control:?}"
        );
        let table = render_bench(&rows);
        assert!(table.contains("| diagnostics |"), "{table}");
    }

    #[test]
    fn prefix_share_and_recall_math() {
        let a = json!({"tools": [1], "input": [{"role": "system", "c": "x"}]});
        let b = json!({"tools": [1], "input": [{"role": "system", "c": "x"}, {"role": "user", "c": "FACT"}]});
        let row = row_from("t", &[a, b], &["FACT"], 1);
        assert_eq!(row.requests, 2);
        assert!(row.stable_prefix_pct >= 90, "{row:?}");
        assert!(row.recall_ok);
        let lost = row_from("t", &[json!({"input": []})], &["FACT"], 1);
        assert!(!lost.recall_ok);
    }
}
