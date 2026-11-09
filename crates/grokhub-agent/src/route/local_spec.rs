//! Speculative decoding for the sidecar (`grokhub_core::local_draft` holds the
//! pairs and the gates). [`plan`] picks the draft for one launch: the model's
//! own MTP heads when the engine can use them, else the catalog drafter (or
//! any GGUF in `models/drafters/` with the same tokenizer) when it fits next
//! to the model, the governor isn't throttling, and the speed test kept it.
//! [`run_bench`] is that speed test: one reply without the drafter, one with,
//! and the verdict is stored per device.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use grokhub_core::gguf::{self, GgufMeta};
use grokhub_core::local_draft::{self as ld, Check, Decision, DraftFlags, DraftKind, Timing};
use grokhub_core::local_profile::LocalProfile;

use super::local_autoconfig::{self as autoconfig, GovState, LaunchArgs};

/// The draft half of one launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftArgs {
    pub kind: DraftKind,
    /// The drafter file; `None` for MTP heads.
    pub model: Option<PathBuf>,
    pub on_gpu: bool,
    pub flags: DraftFlags,
}

impl DraftArgs {
    pub fn to_args(&self) -> Vec<String> {
        let mut a = Vec::new();
        if let Some(t) = &self.flags.spec_type {
            a.extend([t.clone(), self.kind.spec_type().to_string()]);
        }
        if let (Some(f), Some(m)) = (&self.flags.model, &self.model) {
            a.extend([f.clone(), m.to_string_lossy().into_owned()]);
            if let Some(n) = &self.flags.ngl {
                a.extend([n.clone(), if self.on_gpu { "all" } else { "0" }.to_string()]);
            }
        }
        // DFlash and MTP heads inside the model know their own draft length;
        // a separate MTP heads file drafts a few tokens ahead.
        let max = match self.kind {
            DraftKind::Simple => Some(ld::DRAFT_MAX),
            DraftKind::Mtp if self.model.is_some() => Some(ld::MTP_FILE_DRAFT_MAX),
            _ => None,
        };
        if let (Some(f), Some(n)) = (&self.flags.max, max) {
            a.extend([f.clone(), n.to_string()]);
        }
        a
    }
}

/// Whether the drafter fits next to the planned launch: `Some(true)` on the
/// GPU with the whole model, `Some(false)` in RAM for a CPU launch, `None`
/// when there isn't room (a partly offloaded model has none to spare). The
/// drafter brings its own KV cache at the same context.
pub fn fits(
    p: &LocalProfile,
    meta: &GgufMeta,
    a: &LaunchArgs,
    state: GovState,
    drafter: &GgufMeta,
) -> Option<bool> {
    let (gpu, ram) = autoconfig::budgets(p, a.backend, state);
    let model =
        meta.file_mb + autoconfig::kv_mb(meta, a.ctx, a.kv) + autoconfig::overhead_mb(a.ctx);
    let need = model
        + drafter.file_mb
        + autoconfig::kv_mb(drafter, a.ctx, a.kv)
        + autoconfig::overhead_mb(a.ctx);
    if gpu == 0 {
        return (need <= ram).then_some(false);
    }
    let whole = a.ngl >= a.all_layers && a.n_cpu_moe.is_none();
    (whole && need <= gpu).then_some(true)
}

/// One launch's draft plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub decision: Decision,
    pub draft: Option<DraftArgs>,
    /// The verdict key and the drafter's name, when there is a candidate.
    pub key: Option<String>,
    pub name: Option<String>,
}

impl Plan {
    fn off(reason: &'static str) -> Self {
        Plan {
            decision: Decision::Off(reason),
            draft: None,
            key: None,
            name: None,
        }
    }
}

/// A candidate drafter: its kind, file and header.
struct Candidate {
    kind: DraftKind,
    file: Option<PathBuf>,
    meta: Option<GgufMeta>,
    name: String,
}

/// Every folder a drafter for `model` may sit in: beside the model, the
/// shared `~/GrokHub/models/drafters`, then the desktop app's older
/// `{config}/models/drafters` (`tune_dir` is the config folder).
pub fn drafter_dirs(model: &Path, tune_dir: &Path) -> Vec<PathBuf> {
    let legacy = tune_dir.join("models").join(ld::DRAFTERS_DIR);
    ld::drafter_dirs(model, ld::models_root().as_deref(), Some(&legacy))
}

fn candidate(
    model: &Path,
    meta: &GgufMeta,
    flags: &DraftFlags,
    dirs: &[PathBuf],
) -> Option<Candidate> {
    if meta.nextn_layers > 0 && flags.supports(DraftKind::Mtp) {
        return Some(Candidate {
            kind: DraftKind::Mtp,
            file: None,
            meta: None,
            name: "the model's own prediction heads".into(),
        });
    }
    let file = model.file_name()?.to_str()?;
    if let Some(pair) = ld::pair_for_file(file) {
        // The first folder that holds the file; an unreadable header can't
        // pass the tokenizer and room checks.
        let path = dirs
            .iter()
            .map(|d| d.join(pair.drafter.file))
            .find(|p| p.is_file())?;
        // An MTP heads file whose header this reader can't parse still runs
        // (llama.cpp loads it) when it's the catalog's size; size it by that.
        let m = header(&path).or_else(|| {
            let mb = std::fs::metadata(&path).ok()?.len().div_ceil(1024 * 1024);
            (pair.kind == DraftKind::Mtp && mb == pair.drafter.download_mb).then(|| GgufMeta {
                file_mb: mb,
                ..GgufMeta::default()
            })
        })?;
        return Some(Candidate {
            kind: pair.kind,
            meta: Some(m),
            file: Some(path),
            name: pair.drafter.name.to_string(),
        });
    }
    // A model the catalog doesn't list: any drafter that tokenizes the same.
    let mut found: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("gguf"))
        .collect();
    found.sort();
    found.into_iter().find_map(|path| {
        let m = header(&path)?;
        if !ld::same_tokenizer(meta.tokenizer.as_deref(), m.tokenizer.as_deref()) {
            return None;
        }
        Some(Candidate {
            kind: DraftKind::Simple,
            name: path.file_name()?.to_string_lossy().into_owned(),
            meta: Some(m),
            file: Some(path),
        })
    })
}

#[derive(Default)]
struct Cache {
    flags: BTreeMap<PathBuf, DraftFlags>,
    headers: BTreeMap<(PathBuf, u64), GgufMeta>,
    /// The speed test's override: `Some(false)` never drafts, `Some(true)`
    /// drafts without a stored verdict.
    force: Option<bool>,
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Cache::default()))
}

/// A drafter's header, read once per file and size.
fn header(path: &Path) -> Option<GgufMeta> {
    let len = std::fs::metadata(path).ok()?.len();
    let key = (path.to_path_buf(), len);
    if let Some(m) = cache().lock().ok()?.headers.get(&key) {
        return Some(m.clone());
    }
    let m = gguf::read_meta(path).ok()?;
    cache().lock().ok()?.headers.insert(key, m.clone());
    Some(m)
}

/// The engine's draft flags, from `--help`, read once per binary.
pub fn help_flags(binary: &Path) -> DraftFlags {
    if let Some(f) = cache()
        .lock()
        .ok()
        .and_then(|c| c.flags.get(binary).cloned())
    {
        return f;
    }
    let f = ld::parse_help(&super::local_flags::help_text(binary));
    if let Ok(mut c) = cache().lock() {
        c.flags.insert(binary.to_path_buf(), f.clone());
    }
    f
}

fn force() -> Option<bool> {
    cache().lock().ok().and_then(|c| c.force)
}

fn set_force(f: Option<bool>) {
    if let Ok(mut c) = cache().lock() {
        c.force = f;
    }
}

/// What [`plan`] reads about one launch.
pub struct Launch<'a> {
    pub flags: &'a DraftFlags,
    pub model: &'a Path,
    pub meta: &'a GgufMeta,
    pub profile: &'a LocalProfile,
    pub state: GovState,
    pub args: &'a LaunchArgs,
    /// Where `draft_bench.json` lives (next to `autotune.json`).
    pub tune_dir: &'a Path,
    /// The device part of the verdict key.
    pub device: &'a str,
}

/// The draft for one launch.
pub fn plan(l: &Launch) -> Plan {
    let Launch {
        flags,
        model,
        meta,
        profile: p,
        state,
        args,
        tune_dir,
        device,
    } = *l;
    let forced = force();
    if forced == Some(false) {
        return Plan::off("the speed test is timing the model alone");
    }
    let Some(c) = candidate(model, meta, flags, &drafter_dirs(model, tune_dir)) else {
        return Plan::off("no draft model is installed");
    };
    let target = model
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let drafter = c
        .file
        .as_ref()
        .and_then(|f| f.file_name())
        .map_or("mtp".to_string(), |f| f.to_string_lossy().into_owned());
    let key = ld::verdict_key(device, &target, &drafter);
    // MTP heads are trained for this one model and carry no tokenizer of
    // their own, so only a separate small model needs the tokenizer check.
    let (tokenizer_ok, on_gpu) = match &c.meta {
        None => (true, Some(args.ngl > 0)),
        Some(m) => (
            c.kind == DraftKind::Mtp
                || ld::same_tokenizer(meta.tokenizer.as_deref(), m.tokenizer.as_deref()),
            fits(p, meta, args, state, m),
        ),
    };
    // A verdict live replies set expires, and the speed test runs again.
    let stored = ld::load_verdicts(tune_dir)
        .get(&key)
        .copied()
        .filter(|v| v.standing(now_secs()));
    let verdict = match forced {
        Some(true) => Some(true),
        _ => stored.map(|v| v.keep),
    };
    let live_off = forced.is_none() && stored.is_some_and(|v| !v.keep && v.retest_at.is_some());
    let decision = ld::decide(Check {
        // The speed test runs whatever the level; it asked for the drafter.
        level_ok: state != GovState::Throttle || forced == Some(true),
        // A drafter file also needs the flag that loads it.
        engine_ok: flags.supports(c.kind) && (c.file.is_none() || flags.model.is_some()),
        tokenizer_ok,
        fits: on_gpu.is_some(),
        verdict,
    });
    // Say which test turned it off.
    let decision = match decision {
        Decision::Off("the speed test found the draft model didn't help") if live_off => {
            Decision::Off(LIVE_OFF)
        }
        d => d,
    };
    let draft = (decision == Decision::On).then(|| DraftArgs {
        kind: c.kind,
        model: c.file.clone(),
        on_gpu: on_gpu.unwrap_or(false),
        flags: flags.clone(),
    });
    Plan {
        decision,
        draft,
        key: Some(key),
        name: Some(c.name),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The running server's draft: what [`draft_status`] reports and what live
/// replies count toward.
#[derive(Default)]
struct Running {
    /// The drafter's name and the launch's decision.
    status: Option<(String, Decision)>,
    /// The verdict key and folder while it drafts.
    key: Option<(String, PathBuf)>,
    live: ld::Live,
    /// The last reply's speed, and the guesses drafted and kept since launch.
    tok_s: Option<f64>,
    drafted: u64,
    accepted: u64,
}

fn running() -> &'static Mutex<Running> {
    static R: OnceLock<Mutex<Running>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Running::default()))
}

/// A launch went ahead with `plan`: keep what it decided for Settings, and
/// count its replies when it drafts.
pub fn launched(plan: &Plan, tune_dir: &Path) {
    if let Ok(mut r) = running().lock() {
        r.status = plan.name.clone().map(|n| (n, plan.decision));
        r.key = match (&plan.draft, &plan.key) {
            (Some(_), Some(k)) => Some((k.clone(), tune_dir.to_path_buf())),
            _ => None,
        };
        r.live = ld::Live::default();
        r.tok_s = None;
        r.drafted = 0;
        r.accepted = 0;
    }
}

/// The last reply's tokens per second, and the share of the drafter's
/// guesses kept since the model loaded.
pub fn reply_stats() -> (Option<f64>, Option<f64>) {
    let Ok(r) = running().lock() else {
        return (None, None);
    };
    let kept = (r.drafted > 0).then(|| r.accepted as f64 / r.drafted as f64);
    (r.tok_s, kept)
}

/// The test's own servers are gone; the next launch says what runs.
fn forget_running() {
    if let Ok(mut r) = running().lock() {
        *r = Running::default();
    }
}

/// The drafter the running model launched with and whether it drafts, or why
/// not. `None` before a launch, or with no drafter to consider.
pub fn draft_status() -> Option<(String, Decision)> {
    running().lock().ok()?.status.clone()
}

/// One finished reply's timings. When live replies keep too few of the
/// drafter's guesses, drafting turns off until a retest, from the next
/// launch (between requests).
pub fn note_reply(t: &Timing) {
    let Some((dir, key, v)) = live_verdict(t, now_secs()) else {
        return;
    };
    if ld::save_verdict(&dir, &key, v).is_ok() {
        super::local_gov::request_restart();
    }
}

/// [`note_reply`]'s count: the verdict to save, once, when the window ends
/// poorly. The running status says why from then on.
fn live_verdict(t: &Timing, now: u64) -> Option<(PathBuf, String, ld::Verdict)> {
    let mut r = running().lock().ok()?;
    r.tok_s = Some(t.tok_s);
    r.drafted += t.drafted;
    r.accepted += t.accepted.min(t.drafted);
    let (key, dir) = r.key.clone()?;
    let v = r.live.add(t, now)?;
    r.key = None;
    if let Some((_, d)) = r.status.as_mut() {
        *d = Decision::Off(LIVE_OFF);
    }
    Some((dir, key, v))
}

/// Why drafting stopped after live replies.
pub const LIVE_OFF: &str = "too few of the draft model's guesses were kept in your chats";

/// When the unattended speed test may look again, and what it said last.
#[derive(Default)]
struct Auto {
    running: bool,
    next_at: u64,
    line: Option<String>,
}

fn auto() -> &'static Mutex<Auto> {
    static A: OnceLock<Mutex<Auto>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(Auto::default()))
}

/// Seconds between looks while the plan doesn't need a test.
const AUTO_LOOK_SECS: u64 = 15 * 60;
/// Seconds before trying again after a test that ran.
const AUTO_RETRY_SECS: u64 = 6 * 3600;

/// Run the draft speed test on its own, once, while the machine is quiet
/// (see [`ld::auto_test_due`]) and the model's drafter still needs one. The
/// test runs on its own thread; a request that comes in meanwhile waits for
/// the current timing and the test stops early.
pub fn tick_auto_test(config_dir: &Path, q: &ld::Quiet) {
    let now = now_secs();
    {
        let Ok(mut a) = auto().lock() else {
            return;
        };
        if a.running || !ld::auto_test_due(q, a.next_at, now) {
            return;
        }
        a.next_at = now + AUTO_LOOK_SECS;
    }
    let Some(paths) = super::local_sidecar::resolve_paths(config_dir) else {
        return;
    };
    let needs = autoconfig::draft_plan(&paths.binary, &paths.model)
        .is_some_and(|p| p.decision == Decision::NeedsTest);
    // With the drafter settled, a model that hasn't had its refusal check
    // gets it next.
    let refusal = !needs && refusal_due(&paths.model);
    if !needs && !refusal {
        return;
    }
    if let Ok(mut a) = auto().lock() {
        a.running = true;
        a.next_at = now + AUTO_RETRY_SECS;
    }
    let dir = config_dir.to_path_buf();
    std::thread::spawn(move || {
        let line = if refusal {
            run_refusal_check(&dir)
        } else {
            run_bench(&dir)
        };
        if let Ok(mut a) = auto().lock() {
            a.running = false;
            if line.is_some() {
                a.line = line;
            }
        }
    });
}

/// What the last unattended speed test found, in words.
pub fn auto_test_line() -> Option<String> {
    auto().lock().ok()?.line.clone()
}

const BENCH_PROMPT: &str =
    "Write a short paragraph about why the sky looks blue. Plain prose, no lists.";

/// One timed reply from a running server.
pub fn measure(base_url: &str, token: &str) -> Result<Timing, String> {
    let body = serde_json::json!({
        "messages": [{"role": "user", "content": BENCH_PROMPT}],
        "max_tokens": 128,
        "temperature": 0,
        "stream": false,
    });
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(5),
        read: Some(Duration::from_secs(300)),
        ..Default::default()
    });
    let text = agent
        .post(&format!("{base_url}/v1/chat/completions"))
        .header("Authorization", &format!("Bearer {token}"))
        .send_json(body)
        .map_err(|e| format!("speed test request failed: {e}"))?
        .into_string()
        .map_err(|e| format!("speed test reply: {e}"))?;
    ld::parse_timings(&text).ok_or_else(|| "the server didn't report its speed".to_string())
}

/// Time the model with and without its drafter and keep the verdict. `None`
/// when there's no drafter to test.
pub fn run_bench(config_dir: &Path) -> Option<String> {
    let paths = super::local_sidecar::resolve_paths(config_dir)?;
    let planned = autoconfig::draft_plan(&paths.binary, &paths.model)?;
    match planned.decision {
        Decision::Off(r)
            if r != "the speed test found the draft model didn't help"
                && r != LIVE_OFF
                && r != "the machine is busy, so the draft model is off" =>
        {
            return None
        }
        _ => {}
    }
    let (key, name, dir) = (planned.key?, planned.name?, autoconfig::tune_dir()?);
    // Requests wait for the test instead of loading the model beside it.
    let _gate = super::local_sidecar::speed_test_gate();
    super::local_sidecar::stop_global();
    let timed = |force: Option<bool>| -> Result<(Timing, bool), String> {
        set_force(force);
        let side = super::local_sidecar::start(&paths)?;
        let drafting = side.drafting();
        let t = measure(&side.base_url(), &side.token);
        side.stop();
        t.map(|t| (t, drafting))
    };
    let plain = timed(Some(false));
    // A reply is waiting: stop here and try another time.
    if super::local_sidecar::speed_test_wanted_back() {
        set_force(None);
        forget_running();
        return Some(format!(
            "The {name} test stopped for a reply and runs again later."
        ));
    }
    let with = timed(Some(true));
    set_force(None);
    let line = match (plain, with) {
        (Ok((plain, _)), Ok((with, true))) => {
            let v = ld::judge(&plain, &with);
            if let Err(e) = ld::save_verdict(&dir, &key, v) {
                return Some(format!("Couldn't save the draft model test: {e}"));
            }
            ld::verdict_line(&name, &v)
        }
        (Ok(_), Ok((_, false))) => {
            format!("{name} couldn't run next to the model, so it stays off.")
        }
        (Err(e), _) | (_, Err(e)) => format!("The draft model test stopped: {e}"),
    };
    forget_running();
    Some(line)
}

fn model_file(model: &Path) -> Option<String> {
    model.file_name().map(|f| f.to_string_lossy().into_owned())
}

fn refusal_due(model: &Path) -> bool {
    match (autoconfig::tune_dir(), model_file(model)) {
        (Some(dir), Some(file)) => grokhub_core::local_refusal::due(&dir, &file),
        _ => false,
    }
}

/// The refusal check's result for the model in `model`, as its row reads.
pub fn refusal_line(model: &Path) -> Option<String> {
    let o = grokhub_core::local_refusal::result_for(&autoconfig::tune_dir()?, &model_file(model)?)?;
    Some(o.line())
}

/// One reply to `prompt` from a running server, thinking off, or `None` for
/// an empty one.
pub fn ask(base_url: &str, token: &str, prompt: &str) -> Result<Option<String>, String> {
    let body = serde_json::json!({
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 96,
        "temperature": 0,
        "stream": false,
        // A thinking model answers in its reply, not in its reasoning.
        "chat_template_kwargs": {"enable_thinking": false},
    });
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(5),
        read: Some(Duration::from_secs(300)),
        ..Default::default()
    });
    let text = agent
        .post(&format!("{base_url}/v1/chat/completions"))
        .header("Authorization", &format!("Bearer {token}"))
        .send_json(body)
        .map_err(|e| format!("refusal check request failed: {e}"))?
        .into_string()
        .map_err(|e| format!("refusal check reply: {e}"))?;
    Ok(reply_text(&text))
}

/// The assistant's reply in a chat completion, if it has one.
pub fn reply_text(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let c = v["choices"][0]["message"]["content"].as_str()?.trim();
    (!c.is_empty()).then(|| c.to_string())
}

/// Ask the running model's fixed everyday asks once and keep how many it
/// answered. Like the speed test, a reply that comes in stops it, and it runs
/// again later.
pub fn run_refusal_check(config_dir: &Path) -> Option<String> {
    use grokhub_core::local_refusal as lr;
    let paths = super::local_sidecar::resolve_paths(config_dir)?;
    let (dir, file) = (autoconfig::tune_dir()?, model_file(&paths.model)?);
    if !lr::due(&dir, &file) {
        return None;
    }
    let _gate = super::local_sidecar::speed_test_gate();
    super::local_sidecar::stop_global();
    let side = match super::local_sidecar::start(&paths) {
        Ok(s) => s,
        Err(e) => return Some(format!("The refusal check couldn't start the model: {e}")),
    };
    let mut replies = Vec::with_capacity(lr::PROMPTS.len());
    for p in lr::PROMPTS {
        if super::local_sidecar::speed_test_wanted_back() {
            side.stop();
            return Some("The refusal check stopped for a reply and runs again later.".into());
        }
        match ask(&side.base_url(), &side.token, p) {
            Ok(r) => replies.push(r),
            Err(e) => {
                side.stop();
                return Some(format!("The refusal check stopped: {e}"));
            }
        }
    }
    side.stop();
    let o = lr::Outcome::from_replies(&replies, now_secs());
    let line = o.line();
    if let Err(e) = lr::save(&dir, &file, o) {
        return Some(format!("Couldn't save the refusal check: {e}"));
    }
    Some(line)
}

#[cfg(test)]
#[path = "local_spec_tests.rs"]
mod tests;
