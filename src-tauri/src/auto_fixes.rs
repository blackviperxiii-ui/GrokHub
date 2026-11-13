//! Settings → Pulse switches the cabin had (parity with its `ui_auto_fixes`
//! and Behavior's reminder switch). Every switch starts off.
//!
//! - Fixes that run on their own (`autoFixes` in `app.json`): a kind of fix
//!   switched on skips its tap. Each kind is read only; anything that changes
//!   the computer still asks on its card. Today the one kind is a crash card's
//!   look into its program's crash history, which runs as the card arrives and
//!   puts its receipt on the card.
//! - Reminder notifications (`proactiveReminders`): one OS notice for a
//!   proactive card due within the hour.

use std::path::Path;
use std::sync::Mutex;

use grokhub_agent::harness as hx;
use grokhub_agent::repair;
use grokhub_core::auto_fix::{auto_offer, AutoFixes, FixType};
use grokhub_core::card_report::{add_receipt, CardReceipt, SubjectKind};
use grokhub_core::UpdateCard;
use serde::Serialize;
use serde_json::json;

/// The cabin's `app.json` keys.
pub const FIXES_KEY: &str = "autoFixes";
pub const REMINDERS_KEY: &str = "proactiveReminders";

/// A crash look's receipt when reading system state is not allowed yet.
pub const LOOK_NEEDS_SCOPE: &str = "I need your OK to read system state first. Turn on \"System state\" in Settings → Permissions, then tap Look into the crashes.";
/// A crash look that found nothing to report.
pub const LOOK_FOUND_NOTHING: &str = "Nothing stood out.";

/// One read-change-write of the switch list at a time.
static SWITCHES: Mutex<()> = Mutex::new(());

/// One switch as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixRow {
    /// The switch's id, as `app.json` stores it (`crashCheck`).
    pub id: String,
    pub label: String,
    pub note: String,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub fixes: Vec<FixRow>,
    pub reminders: bool,
}

fn fix_id(fix: FixType) -> String {
    serde_json::to_value(fix)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The kinds of fix switched on. A missing, broken or unknown value is all off.
pub fn fixes(config_dir: &Path) -> AutoFixes {
    grokhub_core::app_json::read(config_dir)
        .ok()
        .and_then(|m| m.get(FIXES_KEY).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn reminders_on(config_dir: &Path) -> bool {
    grokhub_core::app_json::bool_or(config_dir, REMINDERS_KEY, false)
}

pub fn view(config_dir: &Path) -> View {
    let on = fixes(config_dir);
    View {
        fixes: FixType::ALL
            .iter()
            .map(|fix| FixRow {
                id: fix_id(*fix),
                label: fix.label().to_string(),
                note: fix.note().to_string(),
                on: on.is_on(*fix),
            })
            .collect(),
        reminders: reminders_on(config_dir),
    }
}

/// Turn one kind of fix on or off, keeping the others as they are.
pub fn set_fix(config_dir: &Path, id: &str, on: bool) -> Result<View, String> {
    let fix: FixType = serde_json::from_value(json!(id))
        .map_err(|_| "That switch is gone. Settings now shows what is left.".to_string())?;
    let _lock = SWITCHES.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = fixes(config_dir);
    all.set(fix, on);
    let value = serde_json::to_value(&all).map_err(|e| e.to_string())?;
    grokhub_core::app_json::patch(config_dir, &[(FIXES_KEY, value)])?;
    Ok(view(config_dir))
}

pub fn set_reminders(config_dir: &Path, on: bool) -> Result<View, String> {
    grokhub_core::app_json::patch(config_dir, &[(REMINDERS_KEY, json!(on))])?;
    Ok(view(config_dir))
}

/// The program a card's look reads, when the card names one.
fn program(card: &UpdateCard) -> Option<repair::ProgramName> {
    card.pulse
        .report
        .as_ref()?
        .subject
        .as_ref()
        .filter(|s| s.kind == SubjectKind::Program)
        .and_then(|s| repair::ProgramName::parse(&s.name))
}

/// The look's receipt lines: what it found, or why it couldn't look.
fn receipt_lines(report: &repair::DiagnoseReport) -> Vec<String> {
    if report.ask.is_some() {
        return vec![LOOK_NEEDS_SCOPE.to_string()];
    }
    let found: Vec<String> = report.findings.iter().map(|f| f.plain.clone()).collect();
    if found.is_empty() {
        vec![LOOK_FOUND_NOTHING.to_string()]
    } else {
        found
    }
}

/// For each card just posted whose fix is switched on, run its read-only look
/// and put the receipt on the card. `look` reads the program's crash history
/// (split out so a test can hand it a fixed report). Returns how many cards
/// got a receipt.
pub fn after_post(
    config_dir: &Path,
    posted: &[UpdateCard],
    now_ms: u64,
    look: &dyn Fn(&repair::ProgramName) -> repair::DiagnoseReport,
) -> usize {
    let on = fixes(config_dir);
    if on.is_empty() {
        return 0;
    }
    let mut done = 0;
    for card in posted {
        if auto_offer(card, &on).is_none() {
            continue;
        }
        let Some(name) = program(card) else {
            continue;
        };
        let receipt = CardReceipt {
            at_ms: now_ms,
            what: format!("Checked {}'s crash history", name.as_str()),
            result: receipt_lines(&look(&name)),
        };
        // The card may be gone, or have run already, by the time the look ends.
        let added = crate::pulse::edit_cards(config_dir, &mut |cards| {
            let Some(stored) = cards.iter_mut().find(|c| c.id == card.id) else {
                return 0;
            };
            if auto_offer(stored, &on).is_none() {
                return 0;
            }
            add_receipt(stored, receipt.clone());
            1
        });
        done += added.unwrap_or(0);
    }
    done
}

/// The real look: the program's crash history and the background services,
/// read only, under the system-state grant.
pub fn crash_look(config_dir: &Path, name: &repair::ProgramName) -> repair::DiagnoseReport {
    let os = repair::Os::current();
    let mut probes = repair::ProbeId::for_subject(os).to_vec();
    probes.push(if os == repair::Os::Windows {
        repair::ProbeId::ServicesStoppedAuto
    } else {
        repair::ProbeId::FailedServices
    });
    let ledger = hx::ConsentLedger::load(config_dir);
    let session = format!("pulse-look-{}", name.as_str());
    let ctx = repair::DiagnoseCtx {
        config_dir,
        session_id: &session,
        ledger: &ledger,
        access: hx::AccessMode::Supervised,
        os,
        has_bin: &repair::on_path,
        runner: &repair::run_spec,
        subject: Some(name),
    };
    repair::diagnose(&ctx, &probes)
}

/// A proactive card due within the hour, as its reminder's words. `None` when
/// reminders are off or the card is not one to remind about.
pub fn reminder(card: &UpdateCard, now_ms: u64, on: bool) -> Option<String> {
    card.pulse.proactive.as_ref()?;
    let due = card.pulse.due_at?;
    (on && due >= now_ms && due - now_ms < grokhub_core::proactive::NOTIFY_DUE_MS)
        .then(|| grokhub_core::pulse::i_can_title(card))
}

#[cfg(test)]
#[path = "auto_fixes_tests.rs"]
mod tests;
