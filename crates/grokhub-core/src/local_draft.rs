//! Speculative decoding: which draft model goes with which model, and when
//! one is used. The pairs come from the catalog's `drafter` column (Gemma 4
//! E4B/12B/26B/31B draft with E2B); a model whose GGUF carries
//! multi-token-prediction heads drafts with them instead. Any pair, catalog or community file, must hash to the
//! same tokenizer. The engine's own `--help` says which flags it has. A
//! drafter is used only in RampUp or Normal, only when both fit, and only
//! after the speed test found it accepted at least 40% of its guesses and
//! answered at least 10% faster; that verdict is kept per device. Drafter
//! downloads are offered, never automatic. Everything here is pure apart
//! from the verdict file.

use std::collections::BTreeMap;
use std::path::Path;

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
/// Where drafters download, under the models folder, so they never replace
/// the model GrokHub runs.
pub const DRAFTERS_DIR: &str = "drafters";

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

    /// A drafter row's kind. DFlash drafters carry it in their id.
    pub fn of(drafter: &LocalModel) -> Self {
        if drafter.id.contains("dflash") {
            DraftKind::Dflash
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
/// uses. Flags the help marks as removed don't count.
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

/// Reads the option lines of `llama-server --help`.
pub fn parse_help(help: &str) -> DraftFlags {
    let lines: Vec<&str> = help.lines().collect();
    let mut f = DraftFlags::default();
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with('-') {
            continue;
        }
        // A notice on the next line belongs to this option only when it
        // continues the description rather than starting another option.
        let next = lines
            .get(i + 1)
            .copied()
            .filter(|n| !n.starts_with('-'))
            .unwrap_or("");
        if line.contains("has been removed") || next.contains("has been removed") {
            continue;
        }
        // The option names lead the line; a metavar (`N`, `FNAME`) may sit
        // between them; the first plain word starts the description.
        let names: Vec<&str> = line
            .split_whitespace()
            .map(|t| t.trim_end_matches(','))
            .take_while(|t| {
                t.starts_with('-') || t.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            })
            .filter(|t| t.starts_with('-'))
            .collect();
        let pick = |want: &[&str]| {
            want.iter()
                .find(|w| names.contains(w))
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
        if let Some(rest) = line.strip_prefix("--spec-type ") {
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
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
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
