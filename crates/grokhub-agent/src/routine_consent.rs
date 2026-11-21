//! Consent for a routine's change steps (script-first runtime, phase 5,
//! decision 2). Steps that only read run freely. The person approves a
//! routine's *change* steps once, seeing each tool and its argument
//! template, and that consent is bound to the routine's content hash: any
//! edit to `routine.json` asks again. A hard-class step (`harness::decide`
//! parks it) is never pre-approved and is refused on every unattended run.
//!
//! The store is `{config}/routines/consent.json`. Only the person grants
//! consent: from `grokhub-hub routine allow` on a terminal, or a click in the
//! app. No model reply and no routine step reaches [`grant`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use grokhub_core::routine::{self, Routine, StepKind};
use serde::{Deserialize, Serialize};

use crate::routine_run::Approve;

pub const CONSENT_FILE: &str = "consent.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConsentStore {
    #[serde(default)]
    pub routines: BTreeMap<String, Granted>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Granted {
    /// [`content_hash`] of the routine the person saw.
    pub hash: String,
    /// The change steps they allowed, by id.
    pub steps: BTreeSet<String>,
    pub at_ms: u64,
}

/// One change step, as the person sees it before allowing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeStep {
    pub id: String,
    pub tool: String,
    /// The argument templates as JSON, or the desktop ops in a line.
    pub what: String,
}

/// SHA-256 hex of the routine as saved (its canonical JSON).
pub fn content_hash(r: &Routine) -> String {
    let text = serde_json::to_string(r).unwrap_or_default();
    crate::harness::content_hash(text.as_bytes())
}

/// The steps that change something: every tool step whose tool isn't
/// read-only, and every desktop step. Scripts run in the read-only sandbox.
pub fn change_steps(r: &Routine) -> Vec<ChangeStep> {
    r.steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::Tool { tool, args } if !crate::gate::is_readonly(tool) => Some(ChangeStep {
                id: step.id.clone(),
                tool: tool.clone(),
                what: serde_json::to_string(args).unwrap_or_default(),
            }),
            StepKind::Desktop(recipe) => Some(ChangeStep {
                id: step.id.clone(),
                tool: "desktop".into(),
                what: format!("{} screen actions", recipe.ops.len()),
            }),
            _ => None,
        })
        .collect()
}

fn path(config_dir: &Path) -> PathBuf {
    routine::routines_dir(config_dir).join(CONSENT_FILE)
}

pub fn load(config_dir: &Path) -> ConsentStore {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(config_dir: &Path, store: &ConsentStore) -> Result<(), String> {
    let text = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&path(config_dir), &text)
}

/// The person allows `r`'s change steps as they are now.
pub fn grant(config_dir: &Path, r: &Routine, now_ms: u64) -> Result<(), String> {
    let mut store = load(config_dir);
    store.routines.insert(
        r.name.clone(),
        Granted {
            hash: content_hash(r),
            steps: change_steps(r).into_iter().map(|s| s.id).collect(),
            at_ms: now_ms,
        },
    );
    save(config_dir, &store)
}

/// Takes the consent back. `true` when there was one.
pub fn revoke(config_dir: &Path, name: &str) -> Result<bool, String> {
    let mut store = load(config_dir);
    let had = store.routines.remove(name).is_some();
    if had {
        save(config_dir, &store)?;
    }
    Ok(had)
}

/// Where a routine's consent stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Nothing to allow: every step only reads.
    ReadOnly,
    Allowed,
    /// Allowed once, but the routine changed since.
    Edited,
    NotAllowed,
}

pub fn standing(config_dir: &Path, r: &Routine) -> Standing {
    if change_steps(r).is_empty() {
        return Standing::ReadOnly;
    }
    match load(config_dir).routines.get(&r.name) {
        Some(g) if g.hash == content_hash(r) => Standing::Allowed,
        Some(_) => Standing::Edited,
        None => Standing::NotAllowed,
    }
}

/// The approver an unattended run uses: it allows the consented steps and
/// nothing else, and never a hard-class step. `None` when there is no
/// consent for the routine as it is now.
pub fn approver(config_dir: &Path, r: &Routine) -> Option<Consented> {
    let g = load(config_dir).routines.remove(&r.name)?;
    (g.hash == content_hash(r)).then_some(Consented { steps: g.steps })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consented {
    steps: BTreeSet<String>,
}

impl Approve for Consented {
    fn approve(&self, step: &str, tool: &str, arguments: &str) -> bool {
        if !self.steps.contains(step) {
            return false;
        }
        // A hard class parks on every run, consent or not.
        !matches!(
            crate::harness::decide(crate::harness::Step::Tool {
                name: tool,
                arguments,
            }),
            crate::harness::GateOutcome::Park { hard: Some(_), .. }
        )
    }
}

/// `grokhub-hub routine allow|revoke|undo <name>`. Allow shows each change
/// step and asks on the terminal. Undo takes back the routine's newest save.
/// Exit 0 when done, 1 on a no (or nothing to undo), 2 on a bad call.
pub fn run_cli(args: &[String], config_dir: &Path) -> i32 {
    let (Some(verb), Some(name), None) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("usage: grokhub-hub routine allow|revoke|undo <name>");
        return 2;
    };
    if verb == "undo" {
        // A command the person typed.
        let ask = crate::harness::UndoAsk::from_typing();
        return match crate::routine_draft::undo(config_dir, name, ask) {
            Ok(line) => {
                println!("{line}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }
    if verb == "revoke" {
        return match revoke(config_dir, name) {
            Ok(true) => {
                println!("{name}: its change steps are refused on unattended runs again.");
                0
            }
            Ok(false) => {
                println!("{name} had no consent.");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                2
            }
        };
    }
    let dir = routine::routines_dir(config_dir).join(name);
    let r = match Routine::load(&dir, &crate::routine_run::step_tool) {
        Ok(r) => r,
        Err(problems) => {
            eprintln!("{} can't run:\n- {}", dir.display(), problems.join("\n- "));
            return 2;
        }
    };
    let steps = change_steps(&r);
    if steps.is_empty() {
        println!("{name} only reads, so it needs no consent.");
        return 0;
    }
    println!("{name} would run these steps that change something, with no one asking:");
    for s in &steps {
        println!("- {}: {} {}", s.id, s.tool, s.what);
    }
    println!("A step in a hard class (delete, send, money, credentials, and so on) still stops every run.");
    println!("Editing the routine takes this consent away.");
    eprint!("Allow these steps on unattended runs? [y/N] ");
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        println!("Not allowed.");
        return 1;
    }
    match grant(config_dir, &r, grokhub_core::now_ms()) {
        Ok(()) => {
            println!("Allowed.");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            2
        }
    }
}

#[cfg(test)]
#[path = "routine_consent_tests.rs"]
mod tests;
