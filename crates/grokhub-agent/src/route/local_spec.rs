//! Speculative decoding for the sidecar (`grokhub_core::local_draft` holds the
//! pairs and the gates). [`plan`] picks the draft for one launch: the model's
//! own MTP heads when the engine can use them, else the catalog drafter (or
//! any GGUF in `models/drafters/` with the same tokenizer) when it fits next
//! to the model, the governor isn't throttling, and the speed test kept it.
//! [`run_bench`] is that speed test: one reply without the drafter, one with,
//! and the verdict is stored per device.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
    let mut cmd = Command::new(binary);
    cmd.arg("--help").stdin(Stdio::null());
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    let f = cmd
        .output()
        .map(|o| {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            ld::parse_help(&text)
        })
        .unwrap_or_default();
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

/// Manual draft preference from Settings: `Some(true)` forces drafting on,
/// `Some(false)` forces it off, `None` uses the stored speed-test verdict
/// (or NeedsTest until one exists).
pub fn prefer_draft(force: Option<bool>) {
    set_force(force);
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
    let verdict = match forced {
        Some(true) => Some(true),
        _ => ld::load_verdicts(tune_dir).get(&key).map(|v| v.keep),
    };
    let decision = ld::decide(Check {
        // The speed test runs whatever the level; it asked for the drafter.
        level_ok: state != GovState::Throttle || forced == Some(true),
        // A drafter file also needs the flag that loads it.
        engine_ok: flags.supports(c.kind) && (c.file.is_none() || flags.model.is_some()),
        tokenizer_ok,
        fits: on_gpu.is_some(),
        verdict,
    });
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
                && r != "the machine is busy, so the draft model is off" =>
        {
            return None
        }
        _ => {}
    }
    let (key, name, dir) = (planned.key?, planned.name?, autoconfig::tune_dir()?);
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
    Some(line)
}

#[cfg(test)]
#[path = "local_spec_tests.rs"]
mod tests;
