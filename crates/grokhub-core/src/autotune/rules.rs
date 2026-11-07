//! What auto setup would set, and how sure it is, from the PC, the setup
//! answers and how the app has been used.
//!
//! Each rule says one thing in plain words ("Text is bigger because Windows
//! text size is 150%"). [`super::plan`] decides whether a proposal is
//! applied, offered as a suggestion or dropped.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::device::{DeviceProfile, FormFactor, Tier};
use super::questions::{Answers, Qid};
use super::settings::{text_step_for, Key, Value};

/// How the app has been used, gathered by each app from what it already
/// learns (the rhythm, chip ranking, reactions).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    /// The rhythm's suggested quiet hours, once it has seen enough days.
    pub rhythm_quiet: Option<(u8, u8)>,
    /// Notifications dismissed without opening in the last week.
    pub dismissed_week: u32,
    /// Thinking blocks the user closed in a row.
    pub thoughts_closed_streak: u32,
    /// Rows of suggestion chips shown and never tapped, in a row.
    pub chips_ignored_streak: u32,
}

/// Everything the rules read.
#[derive(Debug, Clone, Default)]
pub struct Signals {
    pub device: DeviceProfile,
    pub answers: Answers,
    pub usage: Usage,
    /// Settings as they stand now (for rules that build on another setting).
    pub current: BTreeMap<Key, Value>,
}

/// One rule's view of one setting.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub key: Key,
    pub value: Value,
    /// 0 to 1. An answer is 1.
    pub confidence: f32,
    /// A setup answer, not a guess.
    pub asked: bool,
    /// Stable id, so a rule the user undoes twice can be turned off.
    pub rule: &'static str,
    /// Why, in a sentence the user reads.
    pub reason: String,
}

fn p(key: Key, value: Value, confidence: f32, rule: &'static str, reason: String) -> Proposal {
    Proposal {
        key,
        value,
        confidence,
        asked: false,
        rule,
        reason,
    }
}

fn answer(key: Key, value: Value, rule: &'static str, reason: &str) -> Proposal {
    Proposal {
        key,
        value,
        confidence: 1.0,
        asked: true,
        rule,
        reason: reason.to_string(),
    }
}

/// `Windows`, `macOS`, or "your desktop" on Linux.
pub fn os_name(os: &str) -> &'static str {
    match os {
        "windows" => "Windows",
        "macos" => "macOS",
        _ => "your desktop",
    }
}

/// Every rule id and the settings it moves.
const RULES: &[(&str, &[Key])] = &[
    ("os_text_size", &[Key::TextSize]),
    ("os_reduce_motion", &[Key::ReduceMotion]),
    ("light_pc_motion", &[Key::ReduceMotion]),
    ("os_high_contrast", &[Key::Theme]),
    ("os_light_theme", &[Key::Theme]),
    ("light_pc_effects", &[Key::Effects]),
    ("pc_runs_local_ai", &[Key::LocalModel]),
    ("local_prompt_cleanup", &[Key::PromptCleanup]),
    ("steam_found", &[Key::GameCompanion]),
    ("steam_found_nudges", &[Key::GameNudges]),
    ("code_folders_found", &[Key::ResumeCode]),
    ("wide_screen", &[Key::ChatObjectives]),
    ("two_screens", &[Key::DockPanels]),
    ("portable_quits", &[Key::CloseToTray]),
    ("rhythm_quiet", &[Key::QuietHours]),
    ("many_dismissed", &[Key::Notifications]),
    ("thoughts_closed", &[Key::CollapseThoughts]),
    ("chips_ignored", &[Key::Suggestions]),
    ("answer_talk", &[Key::Personality]),
    ("answer_approvals", &[Key::Approvals]),
    ("answer_notifications", &[Key::Notifications]),
    ("answer_games", &[Key::GameCompanion, Key::GameNudges]),
    ("answer_coding", &[Key::ResumeCode]),
    ("answer_background", &[Key::CloseToTray]),
    ("answer_local_ai", &[Key::LocalModel]),
    ("answer_sleep", &[Key::QuietHours, Key::DreamHour]),
];

/// The settings a rule moves (empty for an unknown id).
pub fn rule_keys(rule: &str) -> &'static [Key] {
    RULES
        .iter()
        .find(|(id, _)| *id == rule)
        .map(|(_, k)| *k)
        .unwrap_or(&[])
}

/// Every proposal the rules make, in no particular order.
pub fn propose(s: &Signals) -> Vec<Proposal> {
    let d = &s.device;
    let os = os_name(&d.os);
    let app = crate::brand::product_name();
    let tier = d.tier();
    let mut out = Vec::new();

    // Looks: the OS settings first, they are the user's own choice elsewhere.
    if let Some(pct) = d.look.text_scale_pct.filter(|p| *p >= 110) {
        let step = text_step_for(pct);
        if step > 100 {
            out.push(p(
                Key::TextSize,
                Value::Pct(step),
                0.9,
                "os_text_size",
                format!("Text is bigger because {os} text size is {pct}%."),
            ));
        }
    }
    if d.look.reduce_motion == Some(true) {
        out.push(p(
            Key::ReduceMotion,
            Value::Bool(true),
            0.95,
            "os_reduce_motion",
            format!("Motion is reduced because {os} has animations turned off."),
        ));
    }
    if tier == Tier::T0 {
        out.push(p(
            Key::ReduceMotion,
            Value::Bool(true),
            0.8,
            "light_pc_motion",
            "Animations are lighter to keep this PC quick.".into(),
        ));
    }
    if d.look.high_contrast == Some(true) {
        out.push(p(
            Key::Theme,
            Value::pick("light"),
            0.85,
            "os_high_contrast",
            format!("The light theme is on because {os} uses high contrast."),
        ));
    } else if d.look.prefers_dark == Some(false) {
        out.push(p(
            Key::Theme,
            Value::pick("system"),
            0.6,
            "os_light_theme",
            format!("{os} uses a light theme, so {app} can follow it."),
        ));
    }
    if tier == Tier::T0 || d.low_end() {
        out.push(p(
            Key::Effects,
            Value::Bool(false),
            0.85,
            "light_pc_effects",
            "Background effects are off to keep this PC quick.".into(),
        ));
    }

    // What this PC can do.
    if tier >= Tier::T2 && d.free_disk_mb >= 20_000 {
        out.push(p(
            Key::LocalModel,
            Value::Bool(true),
            0.65,
            "pc_runs_local_ai",
            format!("This PC can run an AI model on its own ({}).", tier.plain()),
        ));
    }
    let local_on = s.current.get(&Key::LocalModel) == Some(&Value::Bool(true));
    if local_on && tier >= Tier::T2 {
        out.push(p(
            Key::PromptCleanup,
            Value::Bool(true),
            0.6,
            "local_prompt_cleanup",
            "The AI on this PC can tidy a prompt before it goes out.".into(),
        ));
    }
    if d.steam && d.has_dgpu() && !s.answers.contains_key(&Qid::Games) {
        out.push(p(
            Key::GameCompanion,
            Value::Bool(true),
            0.6,
            "steam_found",
            "Steam is installed, so the game companion can help in game.".into(),
        ));
        out.push(p(
            Key::GameNudges,
            Value::Bool(true),
            0.6,
            "steam_found_nudges",
            "Steam is installed, so I can tell you about game patches and driver updates.".into(),
        ));
    }
    if d.dev_folders && !s.answers.contains_key(&Qid::Coding) {
        out.push(p(
            Key::ResumeCode,
            Value::Bool(true),
            0.6,
            "code_folders_found",
            "You have code projects here, so I can pick up where you left off.".into(),
        ));
    }
    if let Some(disp) = d.display {
        if disp.width_px >= 2_560 {
            out.push(p(
                Key::ChatObjectives,
                Value::Bool(true),
                0.6,
                "wide_screen",
                "Your screen is wide enough to show objectives beside the chat.".into(),
            ));
        }
        if disp.monitors >= 2 {
            out.push(p(
                Key::DockPanels,
                Value::Bool(true),
                0.55,
                "two_screens",
                "With two screens, panels can sit beside the chat.".into(),
            ));
        }
    }
    if matches!(d.form_factor(), FormFactor::Tablet | FormFactor::Handheld)
        && !s.answers.contains_key(&Qid::Background)
    {
        out.push(p(
            Key::CloseToTray,
            Value::Bool(false),
            0.55,
            "portable_quits",
            format!("On a tablet, closing {app} can quit it to save battery."),
        ));
    }

    // How it's used.
    if let Some((a, b)) = s.usage.rhythm_quiet {
        if !s.answers.contains_key(&Qid::Sleep) {
            out.push(p(
                Key::QuietHours,
                Value::Hours(a, b),
                0.7,
                "rhythm_quiet",
                "You're usually away at these hours, so pings can wait.".into(),
            ));
        }
    }
    if s.usage.dismissed_week >= 10 {
        out.push(p(
            Key::Notifications,
            Value::pick("important"),
            0.65,
            "many_dismissed",
            "You closed most notifications this week, so only important ones could show.".into(),
        ));
    }
    if s.usage.thoughts_closed_streak >= 5 {
        out.push(p(
            Key::CollapseThoughts,
            Value::Bool(true),
            0.85,
            "thoughts_closed",
            "Thinking now starts collapsed, because you close it every time.".into(),
        ));
    }
    if s.usage.chips_ignored_streak >= 30 {
        out.push(p(
            Key::Suggestions,
            Value::Bool(false),
            0.6,
            "chips_ignored",
            "You haven't used the suggestion chips in a while.".into(),
        ));
    }

    out.extend(from_answers(&s.answers));
    out
}

/// What the setup answers set.
fn from_answers(a: &Answers) -> Vec<Proposal> {
    let mut out = Vec::new();
    for (q, v) in a {
        let v = v.as_str();
        match (q, v) {
            (Qid::Talk, "friendly" | "direct" | "playful") => out.push(answer(
                Key::Personality,
                Value::pick(v),
                "answer_talk",
                "You picked how I talk.",
            )),
            (Qid::Approvals, "ask" | "risky") => out.push(answer(
                Key::Approvals,
                Value::pick(v),
                "answer_approvals",
                "You picked when I ask first.",
            )),
            (Qid::Notifications, "all" | "important" | "quiet") => out.push(answer(
                Key::Notifications,
                Value::pick(v),
                "answer_notifications",
                "You picked how many notifications you get.",
            )),
            (Qid::Games, yes_no) => {
                let on = yes_no == "yes";
                out.push(answer(
                    Key::GameCompanion,
                    Value::Bool(on),
                    "answer_games",
                    "You told me whether you play games here.",
                ));
                out.push(answer(
                    Key::GameNudges,
                    Value::Bool(on),
                    "answer_games",
                    "You told me whether you play games here.",
                ));
            }
            (Qid::Coding, yes_no) => out.push(answer(
                Key::ResumeCode,
                Value::Bool(yes_no == "yes"),
                "answer_coding",
                "You told me whether you code here.",
            )),
            (Qid::Background, yes_no) => out.push(answer(
                Key::CloseToTray,
                Value::Bool(yes_no == "yes"),
                "answer_background",
                "You told me whether to keep running when the window closes.",
            )),
            (Qid::LocalAi, yes_no) => {
                // An answer can turn it on. "No" leaves it as it is, so a
                // model already running in the background keeps running.
                if yes_no == "yes" {
                    out.push(answer(
                        Key::LocalModel,
                        Value::Bool(true),
                        "answer_local_ai",
                        "You asked for AI that runs on this PC.",
                    ));
                }
            }
            (Qid::Sleep, hour) => {
                if let Some(h) = hour.parse::<u8>().ok().filter(|h| *h < 24) {
                    let wake = (h + 9) % 24;
                    out.push(answer(
                        Key::QuietHours,
                        Value::Hours(h, wake),
                        "answer_sleep",
                        "You told me when you usually sleep.",
                    ));
                    // Tidy-up runs an hour before bed, while the PC is still on.
                    out.push(answer(
                        Key::DreamHour,
                        Value::Hour((h + 23) % 24),
                        "answer_sleep",
                        "You told me when you usually sleep.",
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::device::tests::{beast, surface_pro_7};
    use super::super::device::{Display, OsLook};
    use super::*;

    fn keys(ps: &[Proposal]) -> Vec<(Key, &'static str)> {
        let mut v: Vec<_> = ps.iter().map(|p| (p.key, p.rule)).collect();
        v.sort();
        v
    }

    #[test]
    fn a_surface_at_150_percent_gets_big_text_light_motion_and_no_effects() {
        let mut device = surface_pro_7();
        device.look = OsLook {
            text_scale_pct: Some(150),
            ..OsLook::default()
        };
        let ps = propose(&Signals {
            device,
            ..Signals::default()
        });
        let text = ps.iter().find(|p| p.key == Key::TextSize).expect("text");
        assert_eq!(text.value, Value::Pct(150));
        assert_eq!(
            text.reason,
            "Text is bigger because Windows text size is 150%."
        );
        assert_eq!(
            keys(&ps),
            vec![
                (Key::TextSize, "os_text_size"),
                (Key::ReduceMotion, "light_pc_motion"),
                (Key::Effects, "light_pc_effects"),
                (Key::CloseToTray, "portable_quits"),
            ]
        );
        assert!(
            ps.iter().all(|p| p.key != Key::LocalModel),
            "no local AI on 8 GB"
        );
    }

    #[test]
    fn the_beast_is_offered_local_ai_and_game_extras_and_keeps_its_looks() {
        let mut device = beast();
        device.free_disk_mb = 400_000;
        device.display = Some(Display {
            scale_pct: 100,
            width_px: 3_440,
            monitors: 2,
        });
        let ps = propose(&Signals {
            device,
            ..Signals::default()
        });
        assert_eq!(
            keys(&ps),
            vec![
                (Key::LocalModel, "pc_runs_local_ai"),
                (Key::GameCompanion, "steam_found"),
                (Key::GameNudges, "steam_found_nudges"),
                (Key::ChatObjectives, "wide_screen"),
                (Key::DockPanels, "two_screens"),
            ]
        );
        let local = ps.iter().find(|p| p.key == Key::LocalModel).expect("local");
        assert_eq!(local.confidence, 0.65);
    }

    #[test]
    fn answers_set_what_they_say_and_no_never_turns_local_ai_off() {
        let mut answers = Answers::new();
        answers.insert(Qid::Sleep, "23".into());
        answers.insert(Qid::LocalAi, "no".into());
        answers.insert(Qid::Talk, "direct".into());
        answers.insert(Qid::Games, "no".into());
        let ps = propose(&Signals {
            device: beast(),
            answers,
            ..Signals::default()
        });
        let quiet = ps.iter().find(|p| p.key == Key::QuietHours).expect("quiet");
        assert_eq!(quiet.value, Value::Hours(23, 8));
        assert!(quiet.asked);
        let dream = ps.iter().find(|p| p.key == Key::DreamHour).expect("dream");
        assert_eq!(dream.value, Value::Hour(22));
        assert!(ps.iter().all(|p| p.key != Key::LocalModel));
        assert!(ps
            .iter()
            .any(|p| p.key == Key::GameCompanion && p.value == Value::Bool(false) && p.asked));
        assert!(
            ps.iter().all(|p| p.rule != "steam_found"),
            "an answer about games replaces the Steam guess"
        );
        assert_eq!(
            ps.iter()
                .find(|p| p.key == Key::Personality)
                .map(|p| &p.value),
            Some(&Value::pick("direct"))
        );
    }

    #[test]
    fn usage_rules_wait_for_enough_signal() {
        let quiet = Signals {
            device: beast(),
            usage: Usage {
                thoughts_closed_streak: 4,
                dismissed_week: 9,
                chips_ignored_streak: 29,
                rhythm_quiet: None,
            },
            ..Signals::default()
        };
        let ps = propose(&quiet);
        assert!(ps.iter().all(|p| !matches!(
            p.key,
            Key::CollapseThoughts | Key::Notifications | Key::Suggestions | Key::QuietHours
        )));
        let busy = Signals {
            usage: Usage {
                thoughts_closed_streak: 5,
                dismissed_week: 10,
                chips_ignored_streak: 30,
                rhythm_quiet: Some((23, 7)),
            },
            ..quiet
        };
        let ks: Vec<Key> = propose(&busy).iter().map(|p| p.key).collect();
        for k in [
            Key::CollapseThoughts,
            Key::Notifications,
            Key::Suggestions,
            Key::QuietHours,
        ] {
            assert!(ks.contains(&k), "{k:?}");
        }
    }

    #[test]
    fn prompt_cleanup_is_offered_only_once_local_ai_is_on() {
        let mut s = Signals {
            device: beast(),
            ..Signals::default()
        };
        assert!(propose(&s).iter().all(|p| p.key != Key::PromptCleanup));
        s.current.insert(Key::LocalModel, Value::Bool(true));
        assert!(propose(&s).iter().any(|p| p.key == Key::PromptCleanup));
    }

    #[test]
    fn every_rule_that_fires_is_listed_with_its_settings() {
        let mut device = beast();
        device.free_disk_mb = 400_000;
        device.dev_folders = true;
        device.display = Some(Display {
            scale_pct: 100,
            width_px: 3_440,
            monitors: 2,
        });
        let mut light = surface_pro_7();
        light.look = OsLook {
            text_scale_pct: Some(150),
            reduce_motion: Some(true),
            high_contrast: Some(true),
            prefers_dark: None,
        };
        let mut answers = Answers::new();
        for (q, v) in [
            (Qid::Talk, "playful"),
            (Qid::Approvals, "risky"),
            (Qid::Notifications, "quiet"),
            (Qid::Games, "yes"),
            (Qid::Coding, "yes"),
            (Qid::Background, "yes"),
            (Qid::LocalAi, "yes"),
            (Qid::Sleep, "21"),
        ] {
            answers.insert(q, v.into());
        }
        let busy = Usage {
            rhythm_quiet: Some((23, 7)),
            dismissed_week: 12,
            thoughts_closed_streak: 6,
            chips_ignored_streak: 40,
        };
        let mut current = BTreeMap::new();
        current.insert(Key::LocalModel, Value::Bool(true));
        let mut seen = std::collections::BTreeSet::new();
        for s in [
            Signals {
                device: device.clone(),
                usage: busy.clone(),
                current: current.clone(),
                ..Signals::default()
            },
            Signals {
                device: light,
                ..Signals::default()
            },
            Signals {
                device,
                answers,
                usage: busy,
                current,
            },
            Signals {
                device: DeviceProfile {
                    look: OsLook {
                        prefers_dark: Some(false),
                        ..OsLook::default()
                    },
                    ..beast()
                },
                ..Signals::default()
            },
        ] {
            for p in propose(&s) {
                assert!(rule_keys(p.rule).contains(&p.key), "{} {:?}", p.rule, p.key);
                seen.insert(p.rule);
            }
        }
        let listed: std::collections::BTreeSet<_> = RULES.iter().map(|(id, _)| *id).collect();
        assert_eq!(seen, listed, "a listed rule never fires in these fixtures");
        assert!(rule_keys("nope").is_empty());
    }

    #[test]
    fn high_contrast_wins_over_a_light_theme_hint() {
        let mut device = beast();
        device.look = OsLook {
            high_contrast: Some(true),
            prefers_dark: Some(false),
            ..OsLook::default()
        };
        let ps = propose(&Signals {
            device,
            ..Signals::default()
        });
        let themes: Vec<_> = ps.iter().filter(|p| p.key == Key::Theme).collect();
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].value, Value::pick("light"));
        assert_eq!(
            themes[0].reason,
            "The light theme is on because your desktop uses high contrast."
        );
    }
}
