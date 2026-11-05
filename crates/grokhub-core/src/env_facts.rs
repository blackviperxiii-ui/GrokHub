//! Round 3 item 39: proven micro-facts.
//!
//! A micro-fact is one short thing about the environment the agent would
//! otherwise rediscover every run: the test command, the build command, the
//! GPU. Each kind comes with its own check, and a fact is only kept while
//! its check agrees:
//!
//! - **Test runner** and **build command**: read from the project's own
//!   files (`Cargo.toml`, `.config/nextest.toml`, `package.json` scripts and
//!   its lockfile, `pyproject.toml`, `go.mod`, a `Makefile`). No command runs.
//! - **GPU**: the app's hardware probe, read once a launch.
//!
//! Every check confirms the fact it matches. One that sees something else
//! retires the old fact (kept, with what contradicted it) and records the new
//! one. A check that can't tell leaves the fact alone, and a fact not
//! confirmed for [`MAX_AGE_DAYS`] stops showing. Only facts a check produced
//! are stored: nothing here takes a fact from a model or from a chat.
//!
//! The prompt gets a short `<facts>` block of the shown facts, with no dates
//! in it, so the cached prefix only changes when a fact does.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "env_facts.json";
/// A fact not confirmed for this long is not shown.
pub const MAX_AGE_DAYS: u64 = 30;
/// Retired facts kept for the record.
const RETIRED_KEEP: usize = 50;
/// A confirmation is written at most this often per fact.
const CONFIRM_EVERY_MS: u64 = 24 * 60 * 60 * 1000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// The scope of a fact about the machine rather than a project.
pub const MACHINE: &str = "machine";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    TestRunner,
    BuildCommand,
    Gpu,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::TestRunner => "Tests",
            Kind::BuildCommand => "Build",
            Kind::Gpu => "GPU",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub kind: Kind,
    /// [`MACHINE`], or a workspace key ([`scope_of`]).
    pub scope: String,
    pub value: String,
    /// What the check read it from, in a few words.
    pub from: String,
    pub confirmed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_ms: Option<u64>,
    /// The value the check saw instead, when retired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contradicted_by: Option<String>,
}

impl Fact {
    pub fn active(&self) -> bool {
        self.retired_ms.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub facts: Vec<Fact>,
}

/// What one check did to the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No fact of this kind yet: the check's reading is now one.
    New,
    /// The check agreed.
    Confirmed,
    /// The check saw something else: the old fact is retired.
    Contradicted { old: String },
    /// The check couldn't tell; nothing changed.
    Unknown,
}

/// A workspace's scope key: a hash, so the file holds no paths.
pub fn scope_of(workspace: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in workspace.display().to_string().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("ws:{hash:016x}")
}

impl Store {
    fn active_mut(&mut self, kind: Kind, scope: &str) -> Option<&mut Fact> {
        self.facts
            .iter_mut()
            .find(|f| f.active() && f.kind == kind && f.scope == scope)
    }

    /// Apply one check's reading. `seen` is `(value, from)`, or `None` when
    /// the check couldn't tell. True in the second place when the store
    /// changed and should be saved.
    pub fn check(
        &mut self,
        kind: Kind,
        scope: &str,
        seen: Option<(&str, &str)>,
        now: u64,
    ) -> (Outcome, bool) {
        let Some((value, from)) = seen.filter(|(v, _)| !v.trim().is_empty()) else {
            return (Outcome::Unknown, false);
        };
        let outcome = match self.active_mut(kind, scope) {
            Some(f) if f.value == value => {
                if now.saturating_sub(f.confirmed_ms) < CONFIRM_EVERY_MS && f.from == from {
                    return (Outcome::Confirmed, false);
                }
                f.confirmed_ms = now;
                f.from = from.to_string();
                return (Outcome::Confirmed, true);
            }
            Some(f) => {
                let old = f.value.clone();
                f.retired_ms = Some(now);
                f.contradicted_by = Some(value.to_string());
                Outcome::Contradicted { old }
            }
            None => Outcome::New,
        };
        self.facts.push(Fact {
            kind,
            scope: scope.to_string(),
            value: value.to_string(),
            from: from.to_string(),
            confirmed_ms: now,
            retired_ms: None,
            contradicted_by: None,
        });
        self.trim();
        (outcome, true)
    }

    fn trim(&mut self) {
        let retired = self.facts.iter().filter(|f| !f.active()).count();
        let mut drop = retired.saturating_sub(RETIRED_KEEP);
        self.facts.retain(|f| {
            if drop > 0 && !f.active() {
                drop -= 1;
                false
            } else {
                true
            }
        });
    }

    /// The facts to show for `scope` (plus the machine's), confirmed within
    /// [`MAX_AGE_DAYS`].
    pub fn shown(&self, scope: &str, now: u64) -> Vec<&Fact> {
        let mut out: Vec<&Fact> = self
            .facts
            .iter()
            .filter(|f| f.active() && (f.scope == scope || f.scope == MACHINE))
            .filter(|f| now.saturating_sub(f.confirmed_ms) <= MAX_AGE_DAYS * DAY_MS)
            .collect();
        out.sort_by_key(|f| f.kind as u8);
        out
    }
}

pub fn load(config_dir: &Path) -> Store {
    std::fs::read_to_string(config_dir.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, store: &Store) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, config_dir.join(FILE)).map_err(|e| e.to_string())
}

fn package_json(ws: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(ws.join("package.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// The package manager a JS project's lockfile names.
fn js_runner(ws: &Path) -> &'static str {
    if ws.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if ws.join("yarn.lock").is_file() {
        "yarn"
    } else if ws.join("bun.lock").is_file() || ws.join("bun.lockb").is_file() {
        "bun"
    } else {
        "npm"
    }
}

fn js_script(ws: &Path, script: &str) -> Option<(String, String)> {
    let pkg = package_json(ws)?;
    let body = pkg.get("scripts")?.get(script)?.as_str()?;
    // npm init's placeholder test script fails on purpose.
    if body.contains("no test specified") {
        return None;
    }
    let pm = js_runner(ws);
    let cmd = match (pm, script) {
        ("npm", "test") => "npm test".to_string(),
        (pm, s) => format!("{pm} run {s}"),
    };
    Some((cmd, format!("package.json `{script}` script")))
}

fn file_has(path: &Path, needle: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| t.contains(needle))
}

/// The test command `ws`'s own files name, and where it came from.
pub fn detect_test_runner(ws: &Path) -> Option<(String, String)> {
    if ws.join("Cargo.toml").is_file() {
        if ws.join(".config").join("nextest.toml").is_file() {
            return Some(("cargo nextest run".into(), ".config/nextest.toml".into()));
        }
        return Some(("cargo test".into(), "Cargo.toml".into()));
    }
    if let Some(found) = js_script(ws, "test") {
        return Some(found);
    }
    let pyproject = ws.join("pyproject.toml");
    if ws.join("pytest.ini").is_file()
        || ws.join("conftest.py").is_file()
        || file_has(&pyproject, "[tool.pytest")
    {
        let cmd = if ws.join("uv.lock").is_file() {
            "uv run pytest"
        } else {
            "pytest"
        };
        return Some((cmd.into(), "pytest config".into()));
    }
    if ws.join("go.mod").is_file() {
        return Some(("go test ./...".into(), "go.mod".into()));
    }
    None
}

/// The build command `ws`'s own files name, and where it came from.
pub fn detect_build_command(ws: &Path) -> Option<(String, String)> {
    if ws.join("Cargo.toml").is_file() {
        return Some(("cargo build".into(), "Cargo.toml".into()));
    }
    if let Some(found) = js_script(ws, "build") {
        return Some(found);
    }
    if ws.join("go.mod").is_file() {
        return Some(("go build ./...".into(), "go.mod".into()));
    }
    if ws.join("Makefile").is_file() || ws.join("makefile").is_file() {
        return Some(("make".into(), "Makefile".into()));
    }
    None
}

/// Run the project checks for `ws` and save any change. The facts to show.
pub fn observe_workspace(config_dir: &Path, ws: &Path, now: u64) -> Vec<Fact> {
    let scope = scope_of(ws);
    let mut store = load(config_dir);
    let mut changed = false;
    for (kind, seen) in [
        (Kind::TestRunner, detect_test_runner(ws)),
        (Kind::BuildCommand, detect_build_command(ws)),
    ] {
        let seen = seen.as_ref().map(|(v, f)| (v.as_str(), f.as_str()));
        changed |= store.check(kind, &scope, seen, now).1;
    }
    if changed {
        let _ = save(config_dir, &store);
    }
    store.shown(&scope, now).into_iter().cloned().collect()
}

/// Record a machine check's reading (the GPU from the hardware probe).
pub fn observe_machine(
    config_dir: &Path,
    kind: Kind,
    seen: Option<(&str, &str)>,
    now: u64,
) -> Outcome {
    let mut store = load(config_dir);
    let (outcome, changed) = store.check(kind, MACHINE, seen, now);
    if changed {
        let _ = save(config_dir, &store);
    }
    outcome
}

/// The GPU reading from the hardware probe: the discrete card with the most
/// VRAM, else unified memory, else the integrated one. `None` when the probe
/// found no GPU it could name.
pub fn gpu_reading(gpus: &[crate::local_profile::Gpu]) -> Option<String> {
    let named = || gpus.iter().filter(|g| !g.name.trim().is_empty());
    if let Some(g) = named()
        .filter(|g| !g.integrated && !g.unified)
        .max_by_key(|g| g.vram_mb)
    {
        if g.vram_mb == 0 {
            return Some(g.name.trim().to_string());
        }
        let gb = (g.vram_mb + 512) / 1024;
        return Some(format!("{}, {gb} GB VRAM", g.name.trim()));
    }
    if let Some(g) = named().find(|g| g.unified) {
        return Some(format!("{} (unified memory)", g.name.trim()));
    }
    named()
        .next()
        .map(|g| format!("{} (integrated)", g.name.trim()))
}

/// The prompt block for `facts`, or empty. No dates, so it stays put while
/// the facts do.
pub fn prompt_block(facts: &[Fact]) -> String {
    if facts.is_empty() {
        return String::new();
    }
    let mut out = String::from("<facts>\nChecked on this computer; each is re-checked and dropped if it stops being true.\n");
    for f in facts {
        let value = if f.kind == Kind::Gpu {
            f.value.clone()
        } else {
            format!("`{}`", f.value)
        };
        out.push_str(&format!(
            "- {}: {value} (from {})\n",
            f.kind.label(),
            f.from
        ));
    }
    out.push_str("</facts>");
    out
}

#[cfg(test)]
#[path = "env_facts_tests.rs"]
mod tests;
