//! Consent for a routine's change steps (script-first runtime, phase 5,
//! decision 2). Steps that only read run freely. The person approves a
//! routine's *change* steps once, seeing each tool and its argument
//! template, and that consent is bound to the routine's content hash and its
//! scripts ([`consent_hash`]): any edit to `routine.json` or a script asks
//! again. A hard-class step (`harness::decide`
//! parks it) is never pre-approved and is refused on every unattended run.
//!
//! The store is `{config}/routines/consent.json`. Only the person grants
//! consent: from `grokhub-hub routine allow` on a terminal, or a click in the
//! app. No model reply and no routine step reaches [`grant`]. Each grant is
//! sealed with the learned-tier key in the OS keyring
//! ([`crate::harness::seal_with`]), so a grant written into the file any
//! other way allows nothing. With no keyring, nothing is allowed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use grokhub_core::routine::{self, Routine, StepKind};
use serde::{Deserialize, Serialize};

use crate::routine_run::Approve;

pub const CONSENT_FILE: &str = "consent.json";
/// Associated data for a grant's seal.
const AAD_GRANT: &str = "grokhub:routine-consent:v1";

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
    /// The routine's name, `hash` and `steps` sealed with the learned-tier
    /// key. A grant whose seal doesn't open to them allows nothing.
    #[serde(default)]
    pub seal: String,
}

/// What a grant's seal holds.
fn bound(name: &str, hash: &str, steps: &BTreeSet<String>) -> String {
    let steps: Vec<&str> = steps.iter().map(String::as_str).collect();
    format!("{name}\n{hash}\n{}", steps.join("\n"))
}

/// Whether `g` is a grant the person made for `name`: its seal opens with
/// the key and holds its hash and steps.
fn sealed(config_dir: &Path, name: &str, g: &Granted) -> bool {
    let Some(Ok(Some(key))) = crate::harness::read_key(config_dir, true) else {
        return false;
    };
    crate::harness::open_with(&key, AAD_GRANT, &g.seal)
        .is_some_and(|plain| plain.as_slice() == bound(name, &g.hash, &g.steps).as_bytes())
}

/// One change step, as the person sees it before allowing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeStep {
    pub id: String,
    pub tool: String,
    /// The argument templates as JSON, or the desktop ops in a line.
    pub what: String,
}

/// SHA-256 hex of the routine as saved (its canonical JSON): which version
/// of `routine.json` a run or a fix belongs to.
pub fn content_hash(r: &Routine) -> String {
    let text = serde_json::to_string(r).unwrap_or_default();
    crate::harness::content_hash(text.as_bytes())
}

/// What consent is bound to: the routine as saved and every script it runs,
/// as they are in its folder now. A script's output can fill a change step's
/// arguments, so editing a script asks again, like editing `routine.json`.
pub fn consent_hash(config_dir: &Path, r: &Routine) -> String {
    let dir = routine::routines_dir(config_dir).join(&r.name);
    let mut text = content_hash(r);
    for step in &r.steps {
        if let StepKind::Script { file, .. } = &step.kind {
            let body = std::fs::read(dir.join(file))
                .map(|b| crate::harness::content_hash(&b))
                .unwrap_or_else(|_| "missing".into());
            text.push_str(&format!("\n{file} {body}"));
        }
    }
    crate::harness::content_hash(text.as_bytes())
}

/// The steps that change something: every tool step whose tool isn't
/// read-only, and every desktop step, each shown with exactly what it does.
/// A script runs in the read-only sandbox where it holds; on Windows it
/// doesn't, so there a script step is a change step too.
pub fn change_steps(r: &Routine) -> Vec<ChangeStep> {
    change_steps_where(r, cfg!(windows))
}

/// [`change_steps`], with script steps among them when `scripts` (where
/// the script sandbox doesn't hold).
fn change_steps_where(r: &Routine, scripts: bool) -> Vec<ChangeStep> {
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
                what: serde_json::to_string(&recipe.ops).unwrap_or_default(),
            }),
            StepKind::Script { file, args } if scripts => Some(ChangeStep {
                id: step.id.clone(),
                tool: format!("script {file}"),
                what: serde_json::to_string(args).unwrap_or_default(),
            }),
            _ => None,
        })
        .collect()
}

/// [`change_steps`] as the person sees them before allowing. A routine
/// that changes something also lists each of its script steps with the
/// script itself, since a script's output can fill a change step (and on
/// Windows the script is a change step), so that is code the consent
/// covers. Control and direction characters in what is shown (a script, a
/// file name, an argument) show escaped, so a carriage return or a
/// terminal escape can't hide a line.
pub fn shown_steps(config_dir: &Path, r: &Routine) -> Vec<ChangeStep> {
    shown_steps_where(config_dir, r, cfg!(windows))
}

fn shown_steps_where(config_dir: &Path, r: &Routine, scripts: bool) -> Vec<ChangeStep> {
    let dir = routine::routines_dir(config_dir).join(&r.name);
    let mut changes = change_steps_where(r, scripts).into_iter().peekable();
    if changes.peek().is_none() {
        return Vec::new();
    }
    let mut shown = Vec::new();
    for step in &r.steps {
        let change = changes.next_if(|c| c.id == step.id);
        let StepKind::Script { file, args } = &step.kind else {
            shown.extend(change);
            continue;
        };
        let mut s = change.unwrap_or_else(|| ChangeStep {
            id: step.id.clone(),
            tool: format!("script {file}"),
            what: serde_json::to_string(args).unwrap_or_default(),
        });
        let body = std::fs::read_to_string(dir.join(file))
            .unwrap_or_else(|_| "(the script is missing)".into());
        for line in body.lines() {
            s.what.push_str("\n    ");
            s.what.push_str(line);
        }
        shown.push(s);
    }
    for s in &mut shown {
        s.tool = s.tool.chars().map(shown_char).collect();
        s.what = s
            .what
            .split('\n')
            .map(|line| line.chars().map(shown_char).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
    }
    shown
}

/// A script character as the Allow list shows it: a control character
/// other than tab, or a bidi mark, as its `\u{…}` escape.
fn shown_char(c: char) -> String {
    let hidden = (c.is_control() && c != '\t')
        || matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    if hidden {
        c.escape_unicode().to_string()
    } else {
        c.to_string()
    }
}

pub(crate) fn path(config_dir: &Path) -> PathBuf {
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
    grant_seen(config_dir, r, &consent_hash(config_dir, r), now_ms)
}

/// The person allows `r`'s change steps as they saw them: `seen` is the
/// [`consent_hash`] of what was shown. A routine or script saved again since
/// then isn't allowed, since they haven't seen what it does now.
pub fn grant_seen(config_dir: &Path, r: &Routine, seen: &str, now_ms: u64) -> Result<(), String> {
    let hash = consent_hash(config_dir, r);
    if hash != seen {
        return Err(format!(
            "{} changed since its steps were shown. Look over them again, then allow it.",
            r.name
        ));
    }
    let steps: BTreeSet<String> = change_steps(r).into_iter().map(|s| s.id).collect();
    let key = crate::harness::write_key(config_dir).map_err(|locked| locked.message())?;
    let seal =
        crate::harness::seal_with(&key, AAD_GRANT, bound(&r.name, &hash, &steps).as_bytes())?;
    let mut store = load(config_dir);
    store.routines.insert(
        r.name.clone(),
        Granted {
            hash,
            steps,
            at_ms: now_ms,
            seal,
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
        // Not one the person made here (or made before grants were sealed).
        Some(g) if !sealed(config_dir, &r.name, g) => Standing::NotAllowed,
        Some(g) if g.hash == consent_hash(config_dir, r) => Standing::Allowed,
        Some(_) => Standing::Edited,
        None => Standing::NotAllowed,
    }
}

/// The approver an unattended run uses: it allows the consented steps and
/// nothing else, and never a hard-class step. `None` when there is no
/// consent for the routine as it is now.
pub fn approver(config_dir: &Path, r: &Routine) -> Option<Consented> {
    let g = load(config_dir).routines.remove(&r.name)?;
    (sealed(config_dir, &r.name, &g) && g.hash == consent_hash(config_dir, r))
        .then_some(Consented { steps: g.steps })
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
    // What the person sees is what they allow.
    let seen = consent_hash(config_dir, &r);
    let steps = shown_steps(config_dir, &r);
    if steps.is_empty() {
        println!("{name} only reads, so it needs no consent.");
        return 0;
    }
    println!("{name} would run these steps that change something, with no one asking:");
    for s in &steps {
        println!("- {}: {} {}", s.id, s.tool, s.what);
    }
    println!(
        "A tool step in a hard class (delete, send, money, credentials, and so on) still stops every run. A script runs as shown."
    );
    println!("Editing the routine or one of its scripts takes this consent away.");
    eprint!("Allow these steps on unattended runs? [y/N] ");
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        println!("Not allowed.");
        return 1;
    }
    match grant_seen(config_dir, &r, &seen, grokhub_core::now_ms()) {
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
