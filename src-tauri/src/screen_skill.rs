//! Save as skill, as the cabin offered it after a recording: `/record
//! --skill` sends the newest recording's stills to Grok once more to read
//! the steps you showed, and the draft comes back in the chat as a question.
//! Nothing is saved until Yes, which sends `/record --save-skill <n>`; the
//! skill is then written through the change ledger, so `/skills undo`
//! takes it back. Steps that need an approval keep asking on every replay.

use std::path::Path;
use std::sync::Mutex;

use grokhub_agent::CancelToken;
use grokhub_core::screen_record as sr;
use grokhub_core::SkillMd;

use crate::screen_record::Reporter;

/// The line a diagnosed report ends with.
pub const OFFER: &str = "To keep the steps you showed as a skill, send /record --skill.";
/// Read the newest recording's steps.
const LEARN: &str = "--skill";
/// Save draft `n` (the draft's Yes).
const SAVE: &str = "--save-skill";
/// Drafts kept for a Yes; older ones go.
const KEEP: usize = 4;

/// What a `/record` line asks of Save as skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillAsk {
    Learn,
    Save(u64),
    /// `--save-skill` with no usable number.
    Unknown,
}

/// The skill part of a `/record` line, or `None` for a recording.
pub fn parse(rest: &str) -> Option<SkillAsk> {
    let rest = rest.trim();
    if rest == LEARN {
        return Some(SkillAsk::Learn);
    }
    let n = rest.strip_prefix(SAVE)?;
    if !n.is_empty() && !n.starts_with(' ') {
        return None;
    }
    Some(n.trim().parse().map_or(SkillAsk::Unknown, SkillAsk::Save))
}

struct Drafts {
    next: u64,
    kept: Vec<(u64, SkillMd)>,
    /// A read is under way; one at a time.
    reading: bool,
}

static DRAFTS: Mutex<Drafts> = Mutex::new(Drafts {
    next: 1,
    kept: Vec::new(),
    reading: false,
});

fn drafts() -> std::sync::MutexGuard<'static, Drafts> {
    DRAFTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn keep(draft: SkillMd) -> u64 {
    let mut d = drafts();
    let n = d.next;
    d.next += 1;
    d.kept.push((n, draft));
    let extra = d.kept.len().saturating_sub(KEEP);
    d.kept.drain(..extra);
    n
}

/// `/record --skill` or `/record --save-skill <n>` typed in `chat`.
pub fn run(
    config_dir: &Path,
    ask: SkillAsk,
    remote: bool,
    chat: &str,
    reporter: impl FnOnce() -> Reporter,
) -> Result<String, String> {
    if remote {
        return Err("Save as skill works on your computer, not from a browser.".into());
    }
    match ask {
        SkillAsk::Learn => learn(config_dir, chat, reporter()),
        SkillAsk::Save(n) => save(config_dir, n),
        SkillAsk::Unknown => Err(GONE.into()),
    }
}

const GONE: &str = "That skill draft is gone. Send /record --skill to read the steps again.";

fn learn(config_dir: &Path, chat: &str, reporter: Reporter) -> Result<String, String> {
    if chat.trim().is_empty() {
        return Err("Send /record --skill from a chat, so the draft has somewhere to go.".into());
    }
    if !crate::screen_record::consented(config_dir) {
        return Err("Screen recording is off. Record the steps with /record first.".into());
    }
    let Some(rec) = crate::screen_record::last_recorded() else {
        return Err("No recording to learn from yet. Show the steps with /record first.".into());
    };
    let frames = crate::screen_report::frames_in(&rec.dir);
    if frames.is_empty() {
        return Err("No stills left in that recording's folder.".into());
    }
    {
        let mut d = drafts();
        if d.reading {
            return Err("Already reading the steps from a recording.".into());
        }
        d.reading = true;
    }
    let stills = sr::sample_keyframes(frames.len(), sr::KEYFRAMES).len();
    let cancel = CancelToken::new();
    let call = crate::screen_record::begin_call(&cancel, stills, "Reading the steps from");
    let chat = chat.trim().to_string();
    let config_dir = config_dir.to_path_buf();
    std::thread::spawn(move || {
        let _config = grokhub_agent::perm::ConfigGuard::set(&config_dir);
        let steps = crate::screen_report::read_steps(
            &rec.note,
            rec.secs,
            &frames,
            reporter.client,
            &cancel,
        );
        let title = sr::recording_title(rec.secs, &rec.note);
        match steps.map(|steps| grokhub_core::watch_learn::skill_draft(&rec.note, &steps)) {
            Ok(Some(draft)) => {
                let prompt = draft_prompt(&draft);
                let n = keep(draft);
                (reporter.confirm)(&chat, &prompt, &format!("/record {SAVE} {n}"));
            }
            Ok(None) => (reporter.deliver)(
                &chat,
                &format!("{title}\n\nNo clear steps found in the recording. Nothing was saved."),
            ),
            Err(why) => (reporter.deliver)(
                &chat,
                &format!("{title}\n\nCould not turn this recording into a skill: {why}. Nothing was saved."),
            ),
        }
        crate::screen_record::end_call(call);
        drafts().reading = false;
    });
    Ok(match stills {
        1 => "Reading the steps from 1 still…".into(),
        n => format!("Reading the steps from {n} stills…"),
    })
}

/// The draft's question: its name, the steps, and whether some still ask.
fn draft_prompt(draft: &SkillMd) -> String {
    let asks = if draft.pitfalls.contains("asks for approval") {
        "\n\nSteps that need your approval still ask every time it runs."
    } else {
        ""
    };
    format!(
        "Save these steps as the skill {}?\n\n{}{asks}",
        draft.slash, draft.instructions
    )
}

fn save(config_dir: &Path, n: u64) -> Result<String, String> {
    let draft = {
        let mut d = drafts();
        let at = d.kept.iter().position(|(id, _)| *id == n).ok_or(GONE)?;
        d.kept.remove(at).1
    };
    let file = grokhub_agent::skill_write::skill_folder(config_dir, &draft.name).join("SKILL.md");
    if file.exists() {
        return Err(format!(
            "There's already a skill called {}. Rename or delete it on the Skills tab, then send /record --skill again.",
            draft.name
        ));
    }
    grokhub_agent::skill_write::save_skill_logged(
        config_dir,
        &draft,
        grokhub_agent::harness::Origin::User,
        "saved from a screen recording",
    )?;
    let asks = if draft.pitfalls.contains("asks for approval") {
        " Steps that need your approval still ask every time it runs."
    } else {
        ""
    };
    Ok(format!(
        "Saved the skill {}. /skills undo {} takes it back.{asks}",
        draft.slash, draft.name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_flags_are_skill_lines() {
        assert_eq!(parse("--skill"), Some(SkillAsk::Learn));
        assert_eq!(parse(" --skill "), Some(SkillAsk::Learn));
        assert_eq!(parse("--save-skill 3"), Some(SkillAsk::Save(3)));
        assert_eq!(parse("--save-skill"), Some(SkillAsk::Unknown));
        assert_eq!(parse("--save-skill x"), Some(SkillAsk::Unknown));
        // A note that only starts like a flag is a recording's note.
        assert_eq!(parse("--skill tree lags"), None);
        assert_eq!(parse("--save-skills"), None);
        assert_eq!(parse("skill tree lags"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_draft_says_when_steps_still_ask() {
        let mut draft = SkillMd {
            slash: "/clean-temp".into(),
            instructions: "1. Open the folder.\n2. Delete the temp files.".into(),
            pitfalls: "Step 2 (Delete the temp files.) asks for approval on replay (delete)."
                .into(),
            ..SkillMd::default()
        };
        assert_eq!(
            draft_prompt(&draft),
            "Save these steps as the skill /clean-temp?\n\n1. Open the folder.\n2. Delete the temp files.\n\nSteps that need your approval still ask every time it runs."
        );
        draft.pitfalls =
            "No hard approval steps; soft path still goes through the existing gates on replay."
                .into();
        assert!(!draft_prompt(&draft).contains("still ask"));
    }

    #[test]
    fn only_the_newest_drafts_wait_for_a_yes() {
        let first = keep(SkillMd::default());
        for _ in 0..KEEP {
            keep(SkillMd::default());
        }
        let d = drafts();
        assert!(d.kept.len() <= KEEP);
        assert!(d.kept.iter().all(|(n, _)| *n != first), "the oldest went");
    }
}
