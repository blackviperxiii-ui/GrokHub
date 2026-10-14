//! Watch-and-learn step extractor (Phase D, SD1).
//!
//! Pure builder: turns a vision-model reply about a screen recording into
//! ordered, classified steps and an **unsaved** skill draft. No network, no
//! filesystem writes, no model call — SD2 owns the vision call, save, and UI.

use crate::fix_plan::{classify_step, ApprovalNeed};
use crate::screen_record::clock;
use crate::skill::{
    is_junk_skill, parse_skill_md, render_skill_md, skill_dir_name, skill_safe, SkillMd,
};

/// At most this many steps from one recording (keeps drafts replayable).
pub const MAX_STEPS: usize = 12;
/// Characters kept per step description.
pub const MAX_STEP_CHARS: usize = 200;

const SYSTEM_PROMPT: &str = concat!(
    "You are GrokHub's watch-and-learn extractor. You get stills from a short ",
    "screen recording, in order, and the user's note. The user is showing you ",
    "how to do a task; list the steps they took, in order, one per line, ",
    "numbered, at most 12. Say only what the stills show. Do not invent clicks ",
    "or menus that are not visible. Never write secrets or typed passwords into ",
    "a step — write \"enter your password\" (or similar) instead of a value. ",
    "Do not take or suggest automatic actions; the user decides."
);

/// One watched step with the approval class SC1 would assign on replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedStep {
    pub description: String,
    pub approval: ApprovalNeed,
}

/// System and user messages for the vision call (same shape as
/// [`crate::screen_record::diagnosis_messages`]). The app (SD2) sends these
/// with keyframe stills; this module never calls the model.
pub fn steps_messages(note: &str, secs: u64, stills: usize) -> Vec<(String, String)> {
    let note = note.trim();
    let note = if note.is_empty() {
        "(no note: name the task from what the stills show)"
    } else {
        note
    };
    vec![
        ("system".into(), SYSTEM_PROMPT.into()),
        (
            "user".into(),
            format!(
                "Recording: {}, {stills} stills in order.\nThe user is showing: {note}",
                clock(secs)
            ),
        ),
    ]
}

/// Parse a model reply into ordered watched steps. Tolerates numbered (`1.` /
/// `2)`), bulleted (`-` / `*` / `•`), bold headers, and blank lines. Caps at
/// [`MAX_STEPS`] and [`MAX_STEP_CHARS`] each. Approval comes from
/// [`classify_step`] and is never downgraded. Bare prose without a list marker
/// is ignored (garbage replies yield no steps).
pub fn parse_steps(raw: &str) -> Vec<WatchedStep> {
    let mut out = Vec::new();
    for line in raw.lines() {
        if out.len() >= MAX_STEPS {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Peel a single **bold** wrap so `**1. Open…**` still parses; do not
        // strip lone `*` or we eat bulleted `* item` lines.
        let plain = peel_bold(trimmed);
        if plain.is_empty() {
            continue;
        }
        let lower = plain.to_ascii_lowercase();
        if is_header_line(&lower) {
            continue;
        }
        let Some(text) = list_item_text(plain) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let description = clip_step(text);
        if description.is_empty() {
            continue;
        }
        let approval = classify_step(&description, None, None);
        out.push(WatchedStep {
            description,
            approval,
        });
    }
    out
}

/// Build an unsaved [`SkillMd`] from a note and watched steps. Returns `None`
/// for zero steps, an empty skill name, unsafe text, or junk skills. Hard
/// approval steps stay in the draft; [`SkillMd::pitfalls`] lists them so replay
/// keeps asking.
pub fn skill_draft(note: &str, steps: &[WatchedStep]) -> Option<SkillMd> {
    if steps.is_empty() {
        return None;
    }
    let note = note.trim();
    let name_src = if note.is_empty() {
        steps.first().map(|s| s.description.as_str()).unwrap_or("")
    } else {
        note
    };
    let name = skill_dir_name(name_src);
    if name.is_empty() {
        return None;
    }
    let description = if note.is_empty() {
        format!("Replay the watched steps for {}.", name.replace('-', " "))
    } else {
        format!("Replay the watched steps: {note}")
    };
    let trigger = if note.is_empty() {
        name.replace('-', " ")
    } else {
        note.to_string()
    };
    let instructions = steps
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}", i + 1, s.description))
        .collect::<Vec<_>>()
        .join("\n");
    let pitfalls = pitfalls_for(steps);
    let verify = steps
        .last()
        .map(|s| format!("After the last step: {}", s.description))
        .unwrap_or_default();

    if !skill_safe(&instructions) || !skill_safe(&trigger) || !skill_safe(&description) {
        return None;
    }

    let draft = SkillMd {
        name: name.clone(),
        description,
        slash: format!("/{name}"),
        trigger,
        instructions,
        pitfalls,
        verify,
        runs: 0,
    };
    if is_junk_skill(&draft) {
        return None;
    }
    // Round-trip must hold for SD2 save.
    let rendered = render_skill_md(&draft);
    if !skill_safe(&rendered) {
        return None;
    }
    let round = parse_skill_md(&rendered);
    if round.name != draft.name || round.runs != 0 {
        return None;
    }
    Some(draft)
}

fn pitfalls_for(steps: &[WatchedStep]) -> String {
    let hard: Vec<String> = steps
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.approval.hard_class().map(|c| {
                format!(
                    "Step {} ({}) asks for approval on replay ({}).",
                    i + 1,
                    s.description,
                    c.as_str()
                )
            })
        })
        .collect();
    if hard.is_empty() {
        "No hard approval steps; soft path still goes through the existing gates on replay.".into()
    } else {
        hard.join("\n")
    }
}

fn is_header_line(lower: &str) -> bool {
    let bare = lower.trim_end_matches(':').trim();
    matches!(
        bare,
        "steps" | "fix steps" | "watched steps" | "the steps" | "task" | "what i saw" | "summary"
    ) || bare.starts_with("steps ")
        || bare.ends_with(" steps")
}

fn peel_bold(line: &str) -> &str {
    let t = line.trim();
    if let Some(inner) = t.strip_prefix("**").and_then(|s| s.strip_suffix("**")) {
        return inner.trim();
    }
    t
}

/// Body of a numbered or bulleted list line, or `None` if the line is not a
/// list item. Same number/bullet strip idea as `screen_record::strip_step`
/// (copied so that module stays untouched), but requires a real list marker.
fn list_item_text(line: &str) -> Option<&str> {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix('•') {
        let body = rest.trim();
        return if body.is_empty() { None } else { Some(body) };
    }
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("-\t")) {
        let body = rest.trim();
        return if body.is_empty() { None } else { Some(body) };
    }
    if let Some(rest) = t.strip_prefix("* ").or_else(|| t.strip_prefix("*\t")) {
        let body = rest.trim();
        return if body.is_empty() { None } else { Some(body) };
    }
    // `-item` without space (common in sloppy model output)
    if t.starts_with('-') && t.len() > 1 && !t.as_bytes()[1].is_ascii_digit() {
        let body = t[1..].trim();
        if !body.is_empty() {
            return Some(body);
        }
    }
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            let body = r.trim();
            return if body.is_empty() { None } else { Some(body) };
        }
    }
    None
}

fn clip_step(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= MAX_STEP_CHARS {
        return s;
    }
    let kept: String = s.chars().take(MAX_STEP_CHARS.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix_plan::HardApproval;

    const OPEN_SETTINGS_FIXTURE: &str = r#"
**Steps:**

1. Open Settings
2) Click Display in the sidebar

- Choose the built-in display

3. Confirm the resolution looks right

"#;

    #[test]
    fn open_settings_fixture_parses_ordered_steps() {
        let steps = parse_steps(OPEN_SETTINGS_FIXTURE);
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].description, "Open Settings");
        assert_eq!(steps[1].description, "Click Display in the sidebar");
        assert_eq!(steps[2].description, "Choose the built-in display");
        assert_eq!(steps[3].description, "Confirm the resolution looks right");
        // Read-only open/settings wording stays soft (or whatever classify_step says).
        assert_eq!(
            steps[0].approval,
            classify_step("Open Settings", None, None)
        );
        assert_eq!(steps[0].approval, ApprovalNeed::None);
    }

    #[test]
    fn over_long_reply_caps_at_max_steps() {
        let mut raw = String::new();
        for i in 1..=20 {
            raw.push_str(&format!("{i}. Do thing number {i}\n"));
        }
        let steps = parse_steps(&raw);
        assert_eq!(steps.len(), MAX_STEPS);
        assert_eq!(steps[0].description, "Do thing number 1");
        assert_eq!(
            steps[MAX_STEPS - 1].description,
            format!("Do thing number {MAX_STEPS}")
        );
    }

    #[test]
    fn empty_or_garbage_gives_no_steps_and_no_draft() {
        assert!(parse_steps("").is_empty());
        assert!(parse_steps("   \n\n  ").is_empty());
        assert!(parse_steps("**Steps:**\n\n").is_empty());
        assert!(parse_steps(
            "asdf qwer zxcv with no list markers that is just prose noise and headers"
        )
        .is_empty());
        assert!(parse_steps("**Summary:**\nNothing useful.\n").is_empty());
        assert!(skill_draft("open Settings", &[]).is_none());
        let garbage = parse_steps("**Task:**\n");
        assert!(garbage.is_empty());
        assert!(skill_draft("open Settings", &garbage).is_none());
    }

    #[test]
    fn hard_wording_stays_hard_via_classify_step() {
        let delete = parse_steps("1. Delete the old profile folder\n");
        assert_eq!(delete.len(), 1);
        assert_eq!(delete[0].approval, ApprovalNeed::Hard(HardApproval::Delete));
        assert_eq!(
            delete[0].approval,
            classify_step(&delete[0].description, None, None)
        );

        let cred = parse_steps("1. Enter your password in the sign-in field\n");
        assert_eq!(cred.len(), 1);
        assert_eq!(
            cred[0].approval,
            ApprovalNeed::Hard(HardApproval::Credentials)
        );

        let send = parse_steps("1. Send email to the team with the report\n");
        assert_eq!(send.len(), 1);
        assert_eq!(send[0].approval, ApprovalNeed::Hard(HardApproval::Send));

        let money = parse_steps("1. Click buy now on the checkout page\n");
        assert_eq!(money.len(), 1);
        assert_eq!(money[0].approval, ApprovalNeed::Hard(HardApproval::Money));

        let os = parse_steps("1. Reboot the computer to finish\n");
        assert_eq!(os.len(), 1);
        assert_eq!(
            os[0].approval,
            ApprovalNeed::Hard(HardApproval::IrreversibleOs)
        );
    }

    #[test]
    fn read_only_step_not_downgraded_below_classify_step() {
        let steps = parse_steps("1. Open Settings\n");
        assert_eq!(steps.len(), 1);
        let expected = classify_step("Open Settings", None, None);
        assert_eq!(steps[0].approval, expected);
        // Soft for this wording today; never assert a weaker class than SC1.
        assert!(matches!(
            steps[0].approval,
            ApprovalNeed::None | ApprovalNeed::Hard(_)
        ));
        assert_eq!(expected, ApprovalNeed::None);
    }

    #[test]
    fn skill_draft_round_trips_and_lists_hard_pitfalls() {
        let steps =
            parse_steps("1. Open Settings\n2. Delete the old profile\n3. Enter your password\n");
        assert_eq!(steps.len(), 3);
        let draft = skill_draft("open Settings → Display", &steps).expect("draft");
        assert_eq!(draft.runs, 0);
        assert_eq!(draft.name, "open-settings-display");
        assert_eq!(draft.slash, "/open-settings-display");
        assert!(draft.instructions.contains("1. Open Settings"));
        assert!(draft.instructions.contains("2. Delete the old profile"));
        assert!(draft
            .pitfalls
            .contains("asks for approval on replay (delete)"));
        assert!(draft
            .pitfalls
            .contains("asks for approval on replay (credentials)"));
        assert!(draft.verify.contains("Enter your password"));

        let rendered = render_skill_md(&draft);
        let round = parse_skill_md(&rendered);
        assert_eq!(round.name, draft.name);
        assert_eq!(round.slash, draft.slash);
        assert_eq!(round.runs, 0);
        assert_eq!(round.instructions.trim(), draft.instructions.trim());
        assert!(round.pitfalls.contains("delete"));
        assert!(round.pitfalls.contains("credentials"));
    }

    #[test]
    fn steps_messages_has_system_user_and_handles_empty_note() {
        let msgs = steps_messages("open Settings → Display", 42, 6);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].0, "system");
        assert_eq!(msgs[1].0, "user");
        assert!(msgs[0].1.contains("watch-and-learn"));
        assert!(msgs[0].1.contains("enter your password"));
        assert!(msgs[1].1.contains("0:42"));
        assert!(msgs[1].1.contains("6 stills"));
        assert!(msgs[1].1.contains("open Settings → Display"));

        let empty = steps_messages("   ", 5, 3);
        assert_eq!(empty.len(), 2);
        assert!(empty[1].1.contains("no note"));
        assert!(empty[1].1.contains("3 stills"));
        assert!(empty[1].1.contains("0:05"));
    }

    #[test]
    fn module_has_no_network_or_filesystem_io() {
        let src = include_str!("watch_learn.rs");
        // Drop this test module's string literals that mention the banned tokens.
        let code = src
            .split("#[cfg(test)]")
            .next()
            .expect("production source before tests");
        for banned in [
            "std::fs",
            "std::net",
            "reqwest",
            "ureq",
            "tokio::fs",
            "grok_chat",
            "Command::new",
        ] {
            assert!(
                !code.contains(banned),
                "watch_learn.rs must stay pure; found `{banned}`"
            );
        }
    }

    #[test]
    fn hard_draft_is_still_produced_not_dropped() {
        let steps = parse_steps("1. Delete the backup folder\n");
        let draft = skill_draft("clean backups", &steps).expect("hard steps still draft");
        assert!(draft.pitfalls.contains("delete"));
        assert_eq!(draft.runs, 0);
    }
}
