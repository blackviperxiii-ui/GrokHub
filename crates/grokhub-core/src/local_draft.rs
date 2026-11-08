//! Speculative decoding: which draft model goes with which model, and when
//! one is used. The pairs come from the catalog's `drafter` column (Gemma 4
//! E4B/12B/26B/31B draft with E2B); a model whose GGUF carries
//! multi-token-prediction heads drafts with them instead. Any pair, catalog or community file, must hash to the
//! same tokenizer. The engine's own `--help` says which flags it has. A
//! drafter is used only in RampUp or Normal, only when both fit, and only
//! after the speed test found it accepted at least 40% of its guesses and
//! answered at least 10% faster; that verdict is kept per device. Every
//! shell downloads drafters into one folder, `~/GrokHub/models/drafters`.
//! Everything here is pure apart from the verdict file and the home folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::local_setup::{self, LocalModel};

/// The verdicts file, next to `autotune.json`.
pub const VERDICTS_FILE: &str = "draft_bench.json";
/// Share of drafted tokens the model must accept.
pub const MIN_ACCEPT: f64 = 0.40;
/// Speed-up the drafter must give.
pub const MIN_GAIN: f64 = 1.10;
/// Tokens drafted per step for a separate draft model.
pub const DRAFT_MAX: u32 = 16;
/// Tokens drafted per step from a separate MTP heads file: the heads predict
/// a few tokens past the model's own, and acceptance falls off past that.
pub const MTP_FILE_DRAFT_MAX: u32 = 4;
/// Where drafters download, under the models folder, so they never replace
/// the model GrokHub runs.
pub const DRAFTERS_DIR: &str = "drafters";

/// The one models folder every shell downloads into: `~/GrokHub/models`.
pub fn models_root() -> Option<PathBuf> {
    Some(
        crate::paths::user_home()?
            .join(crate::brand::INTERNAL_ID)
            .join("models"),
    )
}

/// Where every shell downloads drafters: `~/GrokHub/models/drafters`.
pub fn drafters_root() -> Option<PathBuf> {
    models_root().map(|r| r.join(DRAFTERS_DIR))
}

/// Every folder a drafter for `model` may sit in, in search order: beside
/// the model, the shared models root, then an older download folder (the
/// desktop app's `{config}/models/drafters`). No repeats.
pub fn drafter_dirs(model: &Path, root: Option<&Path>, legacy: Option<&Path>) -> Vec<PathBuf> {
    let beside = model
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(DRAFTERS_DIR);
    let mut dirs = vec![beside];
    for d in [
        root.map(|r| r.join(DRAFTERS_DIR)),
        legacy.map(Path::to_path_buf),
    ]
    .into_iter()
    .flatten()
    {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

/// How the engine drafts (`--spec-type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DraftKind {
    /// A separate small model of the same family.
    Simple,
    /// A DFlash drafter trained for one model.
    Dflash,
    /// The model's own multi-token-prediction heads; no second file.
    Mtp,
}

impl DraftKind {
    pub fn spec_type(self) -> &'static str {
        match self {
            DraftKind::Simple => "draft-simple",
            DraftKind::Dflash => "draft-dflash",
            DraftKind::Mtp => "draft-mtp",
        }
    }

    /// A drafter row's kind. DFlash and MTP drafters carry it in their id.
    pub fn of(drafter: &LocalModel) -> Self {
        if drafter.id.contains("dflash") {
            DraftKind::Dflash
        } else if drafter.id.contains("mtp") {
            DraftKind::Mtp
        } else {
            DraftKind::Simple
        }
    }
}

/// A model and the catalog drafter it pairs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub target: &'static LocalModel,
    pub drafter: &'static LocalModel,
    pub kind: DraftKind,
}

/// Every catalog pair, biggest model first.
pub fn pairs() -> Vec<Pair> {
    local_setup::local_models()
        .iter()
        .filter_map(|t| {
            let d = local_setup::model_by_id(t.drafter?)?;
            Some(Pair {
                target: t,
                drafter: d,
                kind: DraftKind::of(d),
            })
        })
        .collect()
}

/// The catalog drafter for the model in `file`.
pub fn pair_for_file(file: &str) -> Option<Pair> {
    pairs().into_iter().find(|p| p.target.file == file)
}

/// The draft flags this engine's `llama-server --help` lists, by the name it
/// uses. Flags the help marks as removed or deprecated don't count.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct DraftFlags {
    pub model: Option<String>,
    pub ngl: Option<String>,
    pub max: Option<String>,
    pub spec_type: Option<String>,
    /// The values `--spec-type` accepts.
    pub kinds: Vec<String>,
}

impl DraftFlags {
    /// The engine can run `kind`.
    pub fn supports(&self, kind: DraftKind) -> bool {
        let typed = match &self.spec_type {
            Some(_) => self.kinds.iter().any(|k| k == kind.spec_type()),
            // Older engines draft with `-md` alone and know nothing else.
            None => kind == DraftKind::Simple,
        };
        typed && (kind == DraftKind::Mtp || self.model.is_some())
    }
}

/// One option of `llama-server --help`: the names that lead its line, and
/// whether its description says it was removed or is deprecated.
struct HelpOption<'a> {
    line: &'a str,
    names: Vec<&'a str>,
    retired: bool,
}

/// The option lines of `llama-server --help`, each with its description
/// (the indented lines up to the next option).
fn help_options(help: &str) -> Vec<HelpOption<'_>> {
    let lines: Vec<&str> = help.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with('-') {
            continue;
        }
        let description = lines[i + 1..].iter().take_while(|n| !n.starts_with('-'));
        let retired = std::iter::once(line)
            .chain(description)
            .any(|l| l.contains("has been removed") || l.contains("(DEPRECATED)"));
        // The option names lead the line; a metavar (`N`, `FNAME`) may sit
        // between them; the first plain word starts the description. A
        // section rule (`----- speculative params -----`) names nothing.
        let names: Vec<&str> = line
            .split_whitespace()
            .map(|t| t.trim_end_matches(','))
            .take_while(|t| {
                t.starts_with('-') || t.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            })
            .filter_map(|t| t.strip_prefix('-').map(|rest| (t, rest)))
            .filter(|(_, rest)| {
                rest.trim_start_matches('-')
                    .starts_with(|c: char| c.is_ascii_alphanumeric())
            })
            .map(|(t, _)| t)
            .collect();
        out.push(HelpOption {
            line,
            names,
            retired,
        });
    }
    out
}

/// Every option name `llama-server --help` lists, mapped to `true` when the
/// help says it was removed or is deprecated.
pub fn help_flags(help: &str) -> BTreeMap<String, bool> {
    let mut all = BTreeMap::new();
    for o in help_options(help) {
        // A name a live option also uses stays live.
        for n in o.names {
            all.entry(n.to_string())
                .and_modify(|r: &mut bool| *r &= o.retired)
                .or_insert(o.retired);
        }
    }
    all
}

/// Reads the option lines of `llama-server --help`.
pub fn parse_help(help: &str) -> DraftFlags {
    let mut f = DraftFlags::default();
    for o in help_options(help).into_iter().filter(|o| !o.retired) {
        let pick = |want: &[&str]| {
            want.iter()
                .find(|w| o.names.contains(w))
                .map(|w| w.to_string())
        };
        if let Some(n) = pick(&["--spec-draft-model", "--model-draft", "-md"]) {
            f.model = Some(n);
        }
        if let Some(n) = pick(&[
            "--spec-draft-ngl",
            "--gpu-layers-draft",
            "--n-gpu-layers-draft",
            "-ngld",
        ]) {
            f.ngl = Some(n);
        }
        if let Some(n) = pick(&["--spec-draft-n-max", "--draft-max"]) {
            f.max = Some(n);
        }
        if let Some(rest) = o.line.strip_prefix("--spec-type ") {
            f.spec_type = Some("--spec-type".into());
            let list = rest.split_whitespace().next().unwrap_or("");
            f.kinds = list.split(',').map(str::to_string).collect();
        }
    }
    f
}

/// Two tokenizer hashes from GGUF headers match (and both exist).
pub fn same_tokenizer(a: Option<&str>, b: Option<&str>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a == b)
}

/// One timed reply from the server.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub tok_s: f64,
    pub drafted: u64,
    pub accepted: u64,
}

/// `timings` of a llama-server reply (`predicted_per_second`, `draft_n`,
/// `draft_n_accepted`).
pub fn parse_timings(json: &str) -> Option<Timing> {
    timings_of(&serde_json::from_str(json).ok()?)
}

/// [`parse_timings`] for a reply or stream chunk already parsed.
pub fn timings_of(v: &serde_json::Value) -> Option<Timing> {
    let t = v.get("timings")?;
    Some(Timing {
        tok_s: t.get("predicted_per_second")?.as_f64()?,
        drafted: t.get("draft_n").and_then(|n| n.as_u64()).unwrap_or(0),
        accepted: t
            .get("draft_n_accepted")
            .and_then(|n| n.as_u64())
            .unwrap_or(0),
    })
}

/// The speed test's call on one pair.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub keep: bool,
    /// Share of drafted tokens accepted, 0–1.
    pub acceptance: f64,
    /// Drafted speed over plain speed.
    pub gain: f64,
    /// When a verdict from live replies expires (unix seconds), so the speed
    /// test runs again; a speed test's own verdict stands until the pair
    /// changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retest_at: Option<u64>,
}

impl Verdict {
    /// Whether this verdict still decides, or is due for a new speed test.
    pub fn standing(&self, now_secs: u64) -> bool {
        self.retest_at.is_none_or(|t| now_secs < t)
    }
}

/// How long a draft model turned off by live replies stays off before the
/// speed test tries it again.
pub const RETEST_SECS: u64 = 7 * 24 * 3600;
/// Drafted replies the live check averages before it can turn drafting off.
pub const LIVE_REPLIES: u32 = 20;

/// Acceptance over the last [`LIVE_REPLIES`] drafted replies. The speed test
/// times one short prompt; real chats can draft worse, and this catches it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Live {
    replies: u32,
    drafted: u64,
    accepted: u64,
}

impl Live {
    /// Count one reply's timings. After [`LIVE_REPLIES`] drafted replies it
    /// returns an Off verdict (due for a retest) when under [`MIN_ACCEPT`] of
    /// the guesses were kept, and starts a fresh window either way.
    pub fn add(&mut self, t: &Timing, now_secs: u64) -> Option<Verdict> {
        if t.drafted == 0 {
            return None;
        }
        self.replies += 1;
        self.drafted += t.drafted;
        self.accepted += t.accepted.min(t.drafted);
        if self.replies < LIVE_REPLIES {
            return None;
        }
        let acceptance = self.accepted as f64 / self.drafted as f64;
        *self = Live::default();
        (acceptance < MIN_ACCEPT).then_some(Verdict {
            keep: false,
            acceptance,
            gain: 0.0,
            retest_at: Some(now_secs + RETEST_SECS),
        })
    }
}

/// What the unattended speed test waits for: the machine at rest, on wall
/// power, nobody using the model or the computer, and the model not keeping
/// warm to cover for the cloud (the test stops it for a minute or two).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Quiet {
    /// The governor is at Normal with no reason to slow down.
    pub calm: bool,
    pub on_battery: bool,
    pub game: bool,
    /// Seconds since the last keyboard or mouse input, when the OS says.
    pub user_idle_secs: Option<u64>,
    /// Seconds since the model last answered or was asked.
    pub model_idle_secs: u64,
    pub in_flight: usize,
    pub warm_wanted: bool,
}

/// Idle time, at the keyboard and on the model, before the test may run.
pub const AUTO_TEST_IDLE_SECS: u64 = 600;

/// Whether the unattended speed test may start now (`next_at` is the
/// earliest time it may look again, unix seconds).
pub fn auto_test_due(q: &Quiet, next_at: u64, now_secs: u64) -> bool {
    now_secs >= next_at
        && q.calm
        && !q.on_battery
        && !q.game
        && q.in_flight == 0
        && !q.warm_wanted
        && q.model_idle_secs >= AUTO_TEST_IDLE_SECS
        && q.user_idle_secs.is_none_or(|s| s >= AUTO_TEST_IDLE_SECS)
}

pub fn judge(plain: &Timing, drafted: &Timing) -> Verdict {
    let acceptance = if drafted.drafted == 0 {
        0.0
    } else {
        drafted.accepted as f64 / drafted.drafted as f64
    };
    let gain = if plain.tok_s > 0.0 {
        drafted.tok_s / plain.tok_s
    } else {
        0.0
    };
    Verdict {
        keep: acceptance >= MIN_ACCEPT && gain >= MIN_GAIN,
        acceptance,
        gain,
        retest_at: None,
    }
}

/// The Settings line for a verdict.
pub fn verdict_line(drafter: &str, v: &Verdict) -> String {
    let accepted = (v.acceptance * 100.0).round() as u32;
    if v.keep {
        format!(
            "{drafter} made replies {:.1}× as fast ({accepted}% of its guesses were kept), so it stays on.",
            v.gain
        )
    } else {
        format!(
            "{drafter} made replies {:.1}× as fast with {accepted}% of its guesses kept, so {product} left it off.",
            v.gain, product = crate::brand::product_name()
        )
    }
}

/// Verdicts are per device, model file and drafter file.
pub fn verdict_key(device: &str, target_file: &str, drafter: &str) -> String {
    format!("{device}|{target_file}|{drafter}")
}

pub fn load_verdicts(dir: &Path) -> BTreeMap<String, Verdict> {
    std::fs::read_to_string(dir.join(VERDICTS_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_verdict(dir: &Path, key: &str, v: Verdict) -> std::io::Result<()> {
    let mut all = load_verdicts(dir);
    all.insert(key.to_string(), v);
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(&all).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(VERDICTS_FILE), json)
}

/// Whether a launch drafts, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Off(&'static str),
    /// Everything fits but the speed test hasn't run on this device yet.
    NeedsTest,
    On,
}

/// Inputs to [`decide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    /// The governor allows it (RampUp or Normal).
    pub level_ok: bool,
    pub engine_ok: bool,
    pub tokenizer_ok: bool,
    pub fits: bool,
    /// The stored speed-test verdict, when there is one.
    pub verdict: Option<bool>,
}

pub fn decide(c: Check) -> Decision {
    if !c.engine_ok {
        return Decision::Off("this engine can't run a draft model");
    }
    if !c.level_ok {
        return Decision::Off("the machine is busy, so the draft model is off");
    }
    if !c.tokenizer_ok {
        return Decision::Off("the draft model's tokenizer doesn't match the model");
    }
    if !c.fits {
        return Decision::Off("there isn't room for the model and its draft model together");
    }
    match c.verdict {
        None => Decision::NeedsTest,
        Some(false) => Decision::Off("the speed test found the draft model didn't help"),
        Some(true) => Decision::On,
    }
}

/// The drafter to offer for download: the installed model's catalog drafter
/// when it isn't in `drafters_dir` yet.
pub fn offer(installed_file: &str, drafters_dir: &Path) -> Option<&'static LocalModel> {
    let p = pair_for_file(installed_file)?;
    (!drafters_dir.join(p.drafter.file).is_file()).then_some(p.drafter)
}

#[cfg(test)]
#[path = "local_draft_tests.rs"]
mod tests;
