//! Turn a screen or audio diagnosis into a harness fix plan (Phase C, SC1).
//! Pure builder: no execution, no disk writes, no gate calls. The app classifies
//! each step with the same hard-class vocabulary the harness uses.

use crate::audio_check::AudioReport;
use crate::host_plan::strip_host_cmd_line;
use crate::recipe::{computer_cmd_line, parse_computer_op};
use crate::screen_record::ScreenReport;

/// Where the diagnosis came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosisKind {
    Screen,
    Audio,
}

/// Hard approval classes (money, send, delete, credentials, irreversible OS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardApproval {
    Money,
    Send,
    Delete,
    Credentials,
    IrreversibleOs,
}

impl HardApproval {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Money => "money",
            Self::Send => "send",
            Self::Delete => "delete",
            Self::Credentials => "credentials",
            Self::IrreversibleOs => "irreversible_os",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "money" => Some(Self::Money),
            "send" => Some(Self::Send),
            "delete" => Some(Self::Delete),
            "credentials" => Some(Self::Credentials),
            "irreversible_os" => Some(Self::IrreversibleOs),
            _ => None,
        }
    }
}

/// Whether a step needs a hard approval card, or can run under the soft path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalNeed {
    None,
    Hard(HardApproval),
}

impl ApprovalNeed {
    pub fn hard_class(self) -> Option<HardApproval> {
        match self {
            Self::None => None,
            Self::Hard(c) => Some(c),
        }
    }
}

/// One ordered harness step with a rollback note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixPlanStep {
    /// Plain words for the approval card.
    pub description: String,
    /// Shell command for `run_terminal_command`, when the step names one.
    pub shell: Option<String>,
    /// A `COMPUTER_CMD:` line for desktop tools, when the step names one.
    pub desktop_cmd: Option<String>,
    pub approval: ApprovalNeed,
    /// How to undo this step if something goes wrong.
    pub rollback: String,
}

/// A built plan ready for the harness job (SC2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixPlan {
    pub kind: DiagnosisKind,
    pub steps: Vec<FixPlanStep>,
}

/// Outcome of the builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanBuild {
    Plan(FixPlan),
    Empty { reason: String },
}

/// Build from a parsed screen recording report (Phase A).
pub fn from_screen(report: &ScreenReport) -> PlanBuild {
    if unfixable_text(&report.cause) || report.steps.iter().any(|s| unfixable_text(s)) {
        return PlanBuild::Empty {
            reason: "The diagnosis says this can't be fixed from here.".into(),
        };
    }
    if report.steps.is_empty() {
        return PlanBuild::Empty {
            reason: "The diagnosis didn't suggest any fix steps.".into(),
        };
    }
    let steps = report.steps.iter().map(|s| step_from_text(s)).collect();
    PlanBuild::Plan(FixPlan {
        kind: DiagnosisKind::Screen,
        steps,
    })
}

/// Build from a finished audio check report (Phase B).
pub fn from_audio(report: &AudioReport) -> PlanBuild {
    if report.text.contains("No problems found.") {
        return PlanBuild::Empty {
            reason: "The audio check found no problems to fix.".into(),
        };
    }
    let fixes = numbered_lines_after(&report.text, "Suggested fixes:");
    if fixes.is_empty() {
        return PlanBuild::Empty {
            reason: "The audio check didn't suggest any fix steps.".into(),
        };
    }
    if fixes.iter().any(|s| unfixable_text(s)) {
        return PlanBuild::Empty {
            reason: "The audio diagnosis says this can't be fixed from here.".into(),
        };
    }
    let steps = fixes.iter().map(|s| step_from_text(s)).collect();
    PlanBuild::Plan(FixPlan {
        kind: DiagnosisKind::Audio,
        steps,
    })
}

fn step_from_text(raw: &str) -> FixPlanStep {
    let description = raw.trim().to_string();
    let shell = strip_host_cmd_line(raw).map(str::to_string);
    let desktop_cmd = parse_computer_op(raw).map(|op| computer_cmd_line(&op));
    let approval = classify_step(&description, shell.as_deref(), desktop_cmd.as_deref());
    let rollback = rollback_note(&description, shell.as_deref(), approval);
    FixPlanStep {
        description,
        shell,
        desktop_cmd,
        approval,
        rollback,
    }
}

/// Classify a step from its plain words and any machine command it carries.
pub fn classify_step(
    description: &str,
    shell: Option<&str>,
    desktop_cmd: Option<&str>,
) -> ApprovalNeed {
    if let Some(cmd) = shell {
        if let Some(class) = shell_class(cmd) {
            return ApprovalNeed::Hard(class);
        }
    }
    if let Some(line) = desktop_cmd {
        if let Some(class) = desktop_class(line) {
            return ApprovalNeed::Hard(class);
        }
    }
    if let Some(class) = phrase_class(description) {
        return ApprovalNeed::Hard(class);
    }
    if looks_read_only(description) && shell.is_none() && desktop_cmd.is_none() {
        return ApprovalNeed::None;
    }
    if shell.is_some() || desktop_cmd.is_some() {
        return ApprovalNeed::None;
    }
    // Unknown UI steps default to hard (approval card), same as the harness gate.
    ApprovalNeed::Hard(HardApproval::IrreversibleOs)
}

fn rollback_note(description: &str, shell: Option<&str>, approval: ApprovalNeed) -> String {
    let low = description.to_ascii_lowercase();
    if low.contains("unmute") {
        return "Mute it again in System Settings → Sound if you want the previous state.".into();
    }
    if low.contains("volume") || low.contains("gain") {
        return "Lower the volume or gain again to where it was.".into();
    }
    if let Some(cmd) = shell {
        let c = cmd.to_ascii_lowercase();
        if c.contains("pactl") && c.contains("mute") && c.contains("0") {
            return "Run the same command with mute 1, or mute again in Sound settings.".into();
        }
        if c.contains("wpctl") && c.contains("unmute") {
            return "Mute the device again with wpctl or in Sound settings.".into();
        }
    }
    match approval {
        ApprovalNeed::Hard(HardApproval::Delete) => {
            "If you deleted something, restore it from a backup or the app's trash.".into()
        }
        ApprovalNeed::Hard(HardApproval::Credentials) => {
            "Change the password or secret back in the app or account settings.".into()
        }
        ApprovalNeed::Hard(HardApproval::Send) => {
            "You can't unsend a message; follow up with a correction if needed.".into()
        }
        ApprovalNeed::Hard(HardApproval::Money) => {
            "Contact the seller or bank about reversing the charge if needed.".into()
        }
        ApprovalNeed::Hard(HardApproval::IrreversibleOs) => {
            "Reverse the change manually in the same app or settings screen.".into()
        }
        ApprovalNeed::None => {
            if low.contains("restart") {
                "Reopen the app or sign out and back in if you need the old state.".into()
            } else {
                "Undo the change in the same place you made it.".into()
            }
        }
    }
}

fn numbered_lines_after(text: &str, heading: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        let plain = line.trim();
        let lower = plain.to_ascii_lowercase();
        if lower.starts_with(&heading.to_ascii_lowercase()) {
            in_section = true;
            let rest = plain[heading.len()..].trim();
            if !rest.is_empty() {
                out.push(strip_numbered(rest));
            }
            continue;
        }
        if !in_section {
            continue;
        }
        if plain.is_empty()
            || lower.starts_with("nothing was changed")
            || lower.starts_with("problems:")
            || lower.starts_with("outputs:")
            || lower.starts_with("other inputs:")
        {
            break;
        }
        out.push(strip_numbered(plain));
    }
    out.retain(|s| !s.is_empty());
    out
}

fn strip_numbered(line: &str) -> String {
    let t = line.trim().trim_start_matches(['-', '*', '•']).trim();
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            return r.trim().to_string();
        }
    }
    t.to_string()
}

fn unfixable_text(s: &str) -> bool {
    let low = s.to_ascii_lowercase();
    [
        "can't be fixed",
        "cannot be fixed",
        "not fixable",
        "no software fix",
        "replace the hardware",
        "contact the manufacturer",
        "contact support",
        "take it to a repair",
        "beyond repair",
    ]
    .iter()
    .any(|p| low.contains(p))
}

fn looks_read_only(text: &str) -> bool {
    let low = text.to_ascii_lowercase();
    [
        "open ",
        "check ",
        "look ",
        "verify ",
        "see if",
        "make sure",
        "confirm ",
        "about:",
        "settings",
        "system settings",
    ]
    .iter()
    .any(|p| low.contains(p))
}

fn shell_class(cmd: &str) -> Option<HardApproval> {
    let cmd = cmd.replace("$(", ";").replace("<(", ";");
    for seg in cmd
        .split([';', '|', '&', '\n', '\r', '`'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let words: Vec<String> = seg
            .split_whitespace()
            .map(|w| w.trim_matches(['\'', '"']).to_ascii_lowercase())
            .collect();
        let head = words
            .iter()
            .find(|w| *w != "sudo" && *w != "doas" && !w.starts_with('-'))
            .map(|w| w.as_str())
            .unwrap_or("");
        if IRREVERSIBLE_HEADS.contains(&head)
            || IRREVERSIBLE_PHRASES
                .iter()
                .any(|p| seg.to_ascii_lowercase().contains(p))
        {
            return Some(HardApproval::IrreversibleOs);
        }
        if DELETE_HEADS.contains(&head)
            || DELETE_PHRASES
                .iter()
                .any(|p| seg.to_ascii_lowercase().contains(p))
        {
            return Some(HardApproval::Delete);
        }
        if SEND_HEADS.contains(&head) {
            return Some(HardApproval::Send);
        }
        if CREDENTIAL_HEADS.contains(&head)
            || CREDENTIAL_PHRASES
                .iter()
                .any(|p| seg.to_ascii_lowercase().contains(p))
        {
            return Some(HardApproval::Credentials);
        }
        if MONEY_PHRASES
            .iter()
            .any(|p| seg.to_ascii_lowercase().contains(p))
        {
            return Some(HardApproval::Money);
        }
    }
    None
}

fn desktop_class(line: &str) -> Option<HardApproval> {
    let low = line.to_ascii_lowercase();
    if low.contains("type ") {
        let typed = line.split_once("type ").map(|(_, t)| t).unwrap_or("");
        if credential_hint(typed) {
            return Some(HardApproval::Credentials);
        }
        if shell_class(typed).is_some() {
            return shell_class(typed);
        }
    }
    if low.contains("key ") {
        let keys = line.split_once("key ").map(|(_, k)| k).unwrap_or("");
        let k = keys.to_ascii_lowercase();
        if k.contains("delete") || k.contains("backspace") {
            return Some(HardApproval::Delete);
        }
        if k.contains("ctrl+alt+delete") || k.contains("control+alt+delete") {
            return Some(HardApproval::IrreversibleOs);
        }
    }
    if low.contains("click ") || low.contains("act ") {
        let tail = line.rsplit_once(' ').map(|(_, t)| t).unwrap_or(line);
        if phrase_class(tail).is_some() {
            return phrase_class(tail);
        }
    }
    None
}

fn phrase_class(text: &str) -> Option<HardApproval> {
    let low = text.to_ascii_lowercase();
    if MONEY_PHRASES.iter().any(|p| low.contains(p)) {
        return Some(HardApproval::Money);
    }
    if SEND_PHRASES.iter().any(|p| low.contains(p)) {
        return Some(HardApproval::Send);
    }
    if DELETE_PHRASES.iter().any(|p| low.contains(p))
        || low.contains("delete ")
        || low.contains("remove ")
        || low.contains("uninstall")
        || low.contains("erase ")
    {
        return Some(HardApproval::Delete);
    }
    if credential_hint(&low) {
        return Some(HardApproval::Credentials);
    }
    if IRREVERSIBLE_PHRASES.iter().any(|p| low.contains(p))
        || low.contains("shut down")
        || low.contains("shutdown")
        || low.contains("reboot")
        || low.contains("format ")
        || low.contains("reinstall")
    {
        return Some(HardApproval::IrreversibleOs);
    }
    None
}

fn credential_hint(text: &str) -> bool {
    let low = text.to_ascii_lowercase();
    [
        "password",
        "passphrase",
        "api key",
        "apikey",
        "secret",
        "credential",
        "ssh key",
        ".ssh",
        "token",
        "sign in",
        "log in",
    ]
    .iter()
    .any(|p| low.contains(p))
}

const IRREVERSIBLE_HEADS: &[&str] = &[
    "shutdown",
    "reboot",
    "poweroff",
    "halt",
    "wipefs",
    "shred",
    "diskpart",
    "format",
    "stop-computer",
    "restart-computer",
    "format-volume",
    "clear-disk",
];
const DELETE_HEADS: &[&str] = &[
    "rm",
    "rmdir",
    "unlink",
    "trash",
    "trash-put",
    "del",
    "erase",
    "rd",
    "remove-item",
    "clear-recyclebin",
];
const SEND_HEADS: &[&str] = &["sendmail", "mail", "mutt"];
const CREDENTIAL_HEADS: &[&str] = &["passwd", "chpasswd"];
const IRREVERSIBLE_PHRASES: &[&str] = &["systemctl poweroff", "systemctl reboot"];
const CREDENTIAL_PHRASES: &[&str] = &[
    "secret-tool",
    "gpg --export-secret",
    "security find-generic-password",
    ".password-store",
];
const DELETE_PHRASES: &[&str] = &[
    "gio trash",
    "gio remove",
    "trash:/",
    "sendtorecyclebin",
    "-exec rm ",
];
const MONEY_PHRASES: &[&str] = &["checkout", "purchase", "pay now", "place order", "buy now"];
const SEND_PHRASES: &[&str] = &["send email", "send message", "publish post", "post to"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_check::{audio_report, AudioDevice, Levels};
    use crate::screen_record::{parse_report, ScreenReport};

    #[test]
    fn screen_diagnosis_maps_to_ordered_steps() {
        let raw = "**Seen:** Firefox plays a 4K YouTube video.\n**Likely cause:** Hardware video decoding is off.\n**Fix steps:**\n1. Open about:support and check Hardware Video Decoding.\n2) Set media.ffmpeg.vaapi.enabled to true.\n3. Restart Firefox.\n";
        let report = parse_report(raw);
        let built = from_screen(&report);
        let plan = match built {
            PlanBuild::Plan(p) => p,
            PlanBuild::Empty { reason } => panic!("expected a plan, got empty: {reason}"),
        };
        assert_eq!(plan.kind, DiagnosisKind::Screen);
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(
            plan.steps[0].description,
            "Open about:support and check Hardware Video Decoding."
        );
        assert_eq!(plan.steps[0].approval, ApprovalNeed::None);
        assert_eq!(plan.steps[2].description, "Restart Firefox.");
        assert!(!plan.steps[0].rollback.is_empty());
    }

    #[test]
    fn hard_classes_tag_delete_credentials_and_irreversible_steps() {
        let delete = step_from_text("HOST_CMD: rm -f ~/Downloads/old.pkg");
        assert_eq!(delete.approval, ApprovalNeed::Hard(HardApproval::Delete));
        let cred = step_from_text("Type your API key into the sign-in field");
        assert_eq!(cred.approval, ApprovalNeed::Hard(HardApproval::Credentials));
        let os = step_from_text("HOST_CMD: sudo shutdown -h now");
        assert_eq!(
            os.approval,
            ApprovalNeed::Hard(HardApproval::IrreversibleOs)
        );
    }

    #[test]
    fn unknown_ui_step_defaults_to_hard_class() {
        let step = step_from_text("Click around until the dialog goes away");
        assert_eq!(
            step.approval,
            ApprovalNeed::Hard(HardApproval::IrreversibleOs)
        );
    }

    #[test]
    fn empty_plan_when_nothing_to_fix() {
        let screen = ScreenReport {
            seen: "The screen looks fine.".into(),
            cause: String::new(),
            steps: vec![],
        };
        match from_screen(&screen) {
            PlanBuild::Empty { reason } => {
                assert!(reason.contains("didn't suggest any fix steps"));
            }
            PlanBuild::Plan(_) => panic!("expected empty plan"),
        }
        let (outputs, inputs) = {
            let info = "Default Sink: hdmi\nDefault Source: mic\n";
            let (s, t) = crate::audio_check::parse_pactl_defaults(info);
            (
                crate::audio_check::parse_pactl_list("", s.as_deref()),
                crate::audio_check::parse_pactl_list("", t.as_deref()),
            )
        };
        let levels = Levels {
            peak_dbfs: -12.0,
            rms_dbfs: -20.0,
            clipped: 0,
            clipped_pct: 0.0,
            dropouts: 0,
            clicks: 0,
            secs: 5.0,
        };
        let mic = AudioDevice {
            name: "mic".into(),
            description: "Mic".into(),
            muted: false,
            volume_pct: Some(80),
            is_default: true,
        };
        let report = audio_report(&outputs, &inputs, Some(&mic), &levels);
        match from_audio(&report) {
            PlanBuild::Empty { reason } => {
                assert!(reason.contains("no problems"));
            }
            PlanBuild::Plan(_) => panic!("expected empty plan for clean audio"),
        }
    }

    #[test]
    fn unfixable_diagnosis_yields_empty_plan_with_reason() {
        let report = ScreenReport {
            seen: "The laptop screen is cracked.".into(),
            cause: "Physical damage; no software fix.".into(),
            steps: vec!["Replace the display panel.".into()],
        };
        match from_screen(&report) {
            PlanBuild::Empty { reason } => {
                assert!(reason.contains("can't be fixed"));
            }
            PlanBuild::Plan(_) => panic!("expected empty plan"),
        }
    }

    #[test]
    fn audio_fix_steps_parse_from_report_text() {
        let text = "Audio check: mic\n\nInput: Mic\nClip: 5.0 s\n\nProblems:\n- Mic muted.\nSuggested fixes:\n1. Unmute Mic in System Settings → Sound.\n2. HOST_CMD: pactl set-source-mute mic 0\n\nNothing was changed on this computer.";
        let report = AudioReport {
            title: "t".into(),
            card_title: "t".into(),
            summary: "s".into(),
            text: text.into(),
        };
        let plan = match from_audio(&report) {
            PlanBuild::Plan(p) => p,
            PlanBuild::Empty { reason } => panic!("expected plan: {reason}"),
        };
        assert_eq!(plan.kind, DiagnosisKind::Audio);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(
            plan.steps[1].shell.as_deref(),
            Some("pactl set-source-mute mic 0")
        );
        assert_eq!(plan.steps[1].approval, ApprovalNeed::None);
    }
}
