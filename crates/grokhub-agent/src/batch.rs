//! Night passes through xAI's Batch API (HX-13): 20% off, answered within
//! hours instead of seconds.
//!
//! Only a client built with [`crate::XaiClient::deferrable`] uses it, and only
//! when signed in with an API key (the SuperGrok pool has no batch price).
//! A request qualifies when it is one plain-text ask with no function tools
//! ([`eligible`]). It goes in as a batch of one, then the pass polls until the
//! answer is in. Anything that goes wrong falls back to the normal call, so a
//! pass never loses its answer to the batch:
//!
//! - the batch can't be made or takes the request: a cooldown (a week for a
//!   model xAI says it can't batch, a day for a refused key, an hour else);
//! - no answer within [`MAX_WAIT`]: the batch is cancelled first, so a request
//!   still pending is never paid twice;
//! - a reply this code can't read: the batch rests a week, so a format change
//!   costs at most one doubled call.
//!
//! Each submitted batch is kept in `batch/jobs.json` until it is read. A pass
//! that runs again with the same request picks its batch back up, and a batch
//! left behind (the app closed mid-wait) is cancelled at the next pass.
//! `"batch_background": false` in `app.json` turns all of it off.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::client::{
    is_grok_model, responses_body, CancelToken, ClientError, ContentPart, InputItem,
    ResponsesRequest, TurnOutput,
};

/// `app.json` switch; anything but `false` leaves it on.
pub const SETTING: &str = "batch_background";
/// How long a pass waits for its batch before it cancels and calls normally.
pub const MAX_WAIT: Duration = Duration::from_secs(3 * 60 * 60);
/// Time between status checks.
pub const POLL: Duration = Duration::from_secs(60);
/// A batch older than [`MAX_WAIT`] plus this was left behind and is cancelled.
const STALE_GRACE_MS: u64 = 15 * 60 * 1000;
/// Status checks that may fail in a row before the pass gives up on the batch.
const MAX_POLL_ERRORS: u32 = 5;
const HOUR_MS: u64 = 60 * 60 * 1000;
const DAY_MS: u64 = 24 * HOUR_MS;
const WEEK_MS: u64 = 7 * DAY_MS;
/// Cooldown key for every model at once.
const ALL: &str = "*";
const BATCH_NAME: &str = "loadout-background";

/// How a deferrable client waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchPolicy {
    pub config_dir: PathBuf,
    pub max_wait: Duration,
    pub poll: Duration,
}

impl BatchPolicy {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            config_dir: config_dir.to_path_buf(),
            max_wait: MAX_WAIT,
            poll: POLL,
        }
    }
}

/// Whether `app.json` in `dir` leaves batching on.
pub fn enabled_at(dir: &Path) -> bool {
    let Ok(text) = fs::read_to_string(dir.join("app.json")) else {
        return true;
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get(SETTING).and_then(Value::as_bool))
        != Some(false)
}

/// One plain-text ask to a Grok model xAI can batch: messages of text only and
/// no function tools (a tool round trip would be one batch per step). Hosted
/// search runs on xAI's side, so it may stay.
pub fn eligible(req: &ResponsesRequest) -> bool {
    let model = req.model.trim().to_ascii_lowercase();
    is_grok_model(&model)
        && !model.starts_with("grok-build")
        && !model.contains("imagine")
        && req.tools.is_empty()
        && !req.input.is_empty()
        && req.input.iter().all(|item| match item {
            InputItem::Message { content, .. } => content
                .iter()
                .all(|part| matches!(part, ContentPart::InputText(_))),
            _ => false,
        })
}

/// A batch's counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct BatchState {
    #[serde(default)]
    pub num_requests: u64,
    #[serde(default)]
    pub num_pending: u64,
    #[serde(default)]
    pub num_success: u64,
    #[serde(default)]
    pub num_error: u64,
    #[serde(default)]
    pub num_cancelled: u64,
}

impl BatchState {
    /// Every request has an outcome.
    pub fn settled(&self) -> bool {
        self.num_requests > 0 && self.num_pending == 0
    }

    /// The state inside a batch object (`{"state": {...}}`).
    pub fn of(batch: &Value) -> Self {
        batch
            .get("state")
            .cloned()
            .and_then(|s| serde_json::from_value(s).ok())
            .unwrap_or_default()
    }
}

/// The five Batch API calls. [`crate::XaiClient`] makes them over HTTP; tests fake them.
pub trait BatchApi {
    fn create(&self, name: &str) -> Result<String, ClientError>;
    fn add(&self, batch_id: &str, request_id: &str, body: Value) -> Result<(), ClientError>;
    fn get(&self, batch_id: &str) -> Result<BatchState, ClientError>;
    /// One page of results (the batch holds one request, so one page).
    fn results(&self, batch_id: &str) -> Result<Vec<Value>, ClientError>;
    fn cancel(&self, batch_id: &str) -> Result<BatchState, ClientError>;
}

/// What a deferred call came to.
#[derive(Debug)]
pub enum Deferred {
    /// The batch answered.
    Done(TurnOutput),
    /// Call the normal way; the reason is for the log.
    Fallback(String),
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Job {
    batch_id: String,
    request_id: String,
    /// Hash of the model and body: the same ask picks its batch back up.
    key: String,
    model: String,
    submitted_ms: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    jobs: Vec<Job>,
    /// Model (or `*`) to the time batching it may be tried again.
    #[serde(default)]
    cooldown: BTreeMap<String, u64>,
}

fn store_path(dir: &Path) -> PathBuf {
    dir.join("batch").join("jobs.json")
}

fn load(dir: &Path) -> Store {
    fs::read_to_string(store_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(dir: &Path, store: &Store) {
    let path = store_path(dir);
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(store) else {
        return;
    };
    let part = path.with_extension("json.part");
    if fs::write(&part, text).is_ok() {
        let _ = fs::rename(&part, &path);
    }
}

fn update(dir: &Path, f: impl FnOnce(&mut Store)) {
    let mut store = load(dir);
    f(&mut store);
    save(dir, &store);
}

fn cool(dir: &Path, model: &str, for_ms: u64, now: u64) {
    update(dir, |s| {
        let until = now.saturating_add(for_ms);
        let at = s.cooldown.entry(model.to_string()).or_insert(0);
        *at = (*at).max(until);
    });
}

/// The request as the Batch API takes it: the Responses body, not streamed.
pub fn batch_body(req: &ResponsesRequest) -> Value {
    let mut body = responses_body(req);
    if let Some(map) = body.as_object_mut() {
        map.remove("stream");
    }
    body
}

fn key_of(model: &str, body: &Value) -> String {
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update([0]);
    h.update(body.to_string().as_bytes());
    h.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Cooldown for a failed create or add: a week for a model xAI says it can't
/// batch, a day for a refused key, an hour for anything else.
fn cooldown_for(err: &ClientError, model: &str) -> (String, u64) {
    let text = err.to_string().to_ascii_lowercase();
    if text.contains("not supported for batch") {
        return (model.to_string(), WEEK_MS);
    }
    match err {
        ClientError::Auth { .. } | ClientError::KeyOffer { .. } => (ALL.into(), DAY_MS),
        _ => (ALL.into(), HOUR_MS),
    }
}

/// Send `req` as a batch of one and wait for it. `sleep` waits up to the given
/// time and returns false when `cancel` fired (tests pass one that returns at once).
pub fn run(
    api: &dyn BatchApi,
    policy: &BatchPolicy,
    req: &ResponsesRequest,
    cancel: &CancelToken,
    now: &dyn Fn() -> u64,
    sleep: &dyn Fn(Duration, &CancelToken) -> bool,
) -> Deferred {
    let dir = policy.config_dir.as_path();
    let started = now();
    let body = batch_body(req);
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut store = load(dir);
    store.cooldown.retain(|_, until| *until > started);
    for name in [model.as_str(), ALL] {
        if store.cooldown.contains_key(name) {
            save(dir, &store);
            return Deferred::Fallback(format!("batching {name} rests a while"));
        }
    }
    // A batch left behind by a pass that never came back: cancel it so a
    // request still waiting isn't paid for.
    let wait_ms = policy.max_wait.as_millis() as u64;
    let (stale, keep): (Vec<Job>, Vec<Job>) = store
        .jobs
        .drain(..)
        .partition(|j| started.saturating_sub(j.submitted_ms) > wait_ms + STALE_GRACE_MS);
    store.jobs = keep;
    save(dir, &store);
    for job in stale {
        let _ = api.cancel(&job.batch_id);
    }
    let key = key_of(&model, &body);
    let job = match store.jobs.iter().find(|j| j.key == key).cloned() {
        Some(job) => job,
        None => {
            let request_id = format!("loadout-{key}");
            let batch_id = match api.create(BATCH_NAME) {
                Ok(id) => id,
                Err(err) => {
                    let (name, ms) = cooldown_for(&err, &model);
                    cool(dir, &name, ms, started);
                    return Deferred::Fallback(format!("batch not made: {err}"));
                }
            };
            if let Err(err) = api.add(&batch_id, &request_id, body) {
                let _ = api.cancel(&batch_id);
                let (name, ms) = cooldown_for(&err, &model);
                cool(dir, &name, ms, started);
                return Deferred::Fallback(format!("batch refused the request: {err}"));
            }
            let job = Job {
                batch_id,
                request_id,
                key,
                model: model.clone(),
                submitted_ms: started,
            };
            update(dir, |s| s.jobs.push(job.clone()));
            job
        }
    };
    let forget = |dir: &Path| update(dir, |s| s.jobs.retain(|j| j.batch_id != job.batch_id));
    let deadline = job.submitted_ms.saturating_add(wait_ms);
    let mut errors = 0;
    loop {
        if cancel.is_cancelled() {
            let _ = api.cancel(&job.batch_id);
            forget(dir);
            return Deferred::Cancelled;
        }
        match api.get(&job.batch_id) {
            Ok(state) if state.settled() => break,
            Ok(_) => errors = 0,
            Err(_) => errors += 1,
        }
        if errors >= MAX_POLL_ERRORS || now() >= deadline {
            // Cancel first: a request still pending is then never paid twice.
            match api.cancel(&job.batch_id) {
                Ok(state) if state.num_success > 0 => break,
                _ => {
                    forget(dir);
                    return Deferred::Fallback(if errors >= MAX_POLL_ERRORS {
                        "batch status unreadable".into()
                    } else {
                        "batch took too long".into()
                    });
                }
            }
        }
        if !sleep(policy.poll, cancel) {
            let _ = api.cancel(&job.batch_id);
            forget(dir);
            return Deferred::Cancelled;
        }
    }
    let results = api.results(&job.batch_id);
    forget(dir);
    let entry = match results {
        Ok(rows) => rows.into_iter().find(|r| {
            r.get("batch_request_id").and_then(Value::as_str) == Some(job.request_id.as_str())
        }),
        Err(err) => return Deferred::Fallback(format!("batch results unreadable: {err}")),
    };
    let Some(entry) = entry else {
        return Deferred::Fallback("batch result missing".into());
    };
    match parse_result(&entry, &job.model) {
        Ok(turn) => Deferred::Done(turn),
        Err(ResultError::Failed(why)) => Deferred::Fallback(format!("batch request failed: {why}")),
        Err(ResultError::Unreadable) => {
            cool(dir, ALL, WEEK_MS, now());
            Deferred::Fallback("batch reply in an unknown shape".into())
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResultError {
    /// xAI ran the request and it failed.
    Failed(String),
    /// A reply this code doesn't know how to read.
    Unreadable,
}

/// One row of `/v1/batches/{id}/results` as a [`TurnOutput`]. The reply sits
/// under `batch_result.response`, keyed by its kind; both the Responses shape
/// (`output`) and the Chat Completions shape (`choices`) are read.
pub fn parse_result(entry: &Value, model: &str) -> Result<TurnOutput, ResultError> {
    let response = entry.pointer("/batch_result/response");
    let Some(response) = response.filter(|r| r.is_object()) else {
        let why = entry
            .get("error_message")
            .and_then(Value::as_str)
            .or_else(|| {
                entry
                    .pointer("/batch_result/error/message")
                    .and_then(Value::as_str)
            })
            .unwrap_or("no reply");
        return Err(ResultError::Failed(why.to_string()));
    };
    let shaped = |v: &Value| v.get("output").is_some() || v.get("choices").is_some();
    let body = if shaped(response) {
        response
    } else {
        response
            .as_object()
            .and_then(|m| m.values().find(|v| shaped(v)))
            .ok_or(ResultError::Unreadable)?
    };
    if let Some(output) = body.get("output").and_then(Value::as_array) {
        return Ok(from_responses(body, output, model));
    }
    let choice = body.pointer("/choices/0").ok_or(ResultError::Unreadable)?;
    let text = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or(ResultError::Unreadable)?
        .to_string();
    let reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let truncated = reason == "length";
    Ok(TurnOutput {
        reasoning: choice
            .pointer("/message/reasoning_content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        text,
        usage: chat_usage(body.get("usage").unwrap_or(&Value::Null)),
        truncated,
        truncate_reason: truncated.then(|| "max_output_tokens".to_string()),
        ..TurnOutput::default()
    })
}

fn from_responses(body: &Value, output: &[Value], model: &str) -> TurnOutput {
    let mut text = String::new();
    let mut reasoning_items = Vec::new();
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if part.get("type").and_then(Value::as_str) == Some("output_text") {
                        text.push_str(part.get("text").and_then(Value::as_str).unwrap_or(""));
                    }
                }
            }
            Some("reasoning") => {
                if let Some(item) = serde_json::to_string(item)
                    .ok()
                    .and_then(|raw| InputItem::reasoning(&raw, model))
                {
                    reasoning_items.push(item);
                }
            }
            _ => {}
        }
    }
    if text.is_empty() {
        if let Some(t) = body.get("output_text").and_then(Value::as_str) {
            text = t.to_string();
        }
    }
    let incomplete = body.get("status").and_then(Value::as_str) == Some("incomplete");
    TurnOutput {
        text,
        reasoning_items,
        usage: crate::sse::parse_usage_value(body),
        truncated: incomplete,
        truncate_reason: if incomplete {
            body.pointer("/incomplete_details/reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        } else {
            None
        },
        ..TurnOutput::default()
    }
}

/// Chat Completions usage in this crate's terms.
fn chat_usage(u: &Value) -> crate::client::Usage {
    let n = |p: &str| u.pointer(p).and_then(Value::as_u64).unwrap_or(0);
    crate::client::Usage {
        input_tokens: n("/prompt_tokens"),
        output_tokens: n("/completion_tokens"),
        reasoning_tokens: n("/completion_tokens_details/reasoning_tokens"),
        cached_tokens: n("/prompt_tokens_details/cached_tokens"),
        cost_in_usd_ticks: u
            .get("cost_in_usd_ticks")
            .and_then(|c| c.as_i64().or_else(|| c.as_f64().map(|f| f as i64)))
            .unwrap_or(0),
    }
}

/// A batch id as a URL path segment: letters, digits, `-` and `_` only.
pub fn valid_batch_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests;
