//! `/learn <source>`: gather a URL, folder, PDF, or chat into a skill draft,
//! Confirm in chat, then save through the scanner and ChangeLedger.

use std::path::Path;
use std::sync::Mutex;

use grokhub_agent::harness::Origin;
use grokhub_core::learn_skill::{
    chat_body_from_pairs, draft_from_gathered, origin_for, parse_learn_rest, write_learned_skill,
    LearnDraft, LearnSource,
};
use grokhub_core::learn_skill_usage;

use crate::slash::Effect;

/// Drafts waiting for Confirm; older ones drop once KEEP is full.
const KEEP: usize = 4;

struct Drafts {
    next: u64,
    kept: Vec<(u64, LearnDraft, String)>,
}

static DRAFTS: Mutex<Drafts> = Mutex::new(Drafts {
    next: 1,
    kept: Vec::new(),
});

fn drafts() -> std::sync::MutexGuard<'static, Drafts> {
    DRAFTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn keep(draft: LearnDraft, reason: String) -> u64 {
    let mut d = drafts();
    let n = d.next;
    d.next += 1;
    d.kept.push((n, draft, reason));
    let extra = d.kept.len().saturating_sub(KEEP);
    d.kept.drain(..extra);
    n
}

const GONE: &str = "That skill draft is gone. Send /learn again.";

/// Run `/learn` skill sources (not reflect). Returns a Confirm or Notice.
pub fn run(
    config_dir: &Path,
    workspace: &Path,
    chat_id: Option<&str>,
    rest: &str,
) -> Result<Effect, String> {
    let source = parse_learn_rest(rest).map_err(|_| learn_skill_usage().to_string())?;
    match source {
        LearnSource::Reflect => Err("internal: reflect belongs to Op::Learn".into()),
        LearnSource::Save(n) => save(config_dir, n),
        other => offer(config_dir, workspace, chat_id, other),
    }
}

fn offer(
    config_dir: &Path,
    workspace: &Path,
    chat_id: Option<&str>,
    source: LearnSource,
) -> Result<Effect, String> {
    let origin = origin_for(&source).to_string();
    let (title, body) = gather(config_dir, workspace, chat_id, &source)?;
    let draft = draft_from_gathered(&title, &body, &origin)?;
    let slash = draft.skill.slash.clone();
    let name = draft.skill.name.clone();
    let refs = draft.references.len();
    let reason = match &source {
        LearnSource::Chat => "learned from this chat".to_string(),
        LearnSource::Url(u) => format!("learned from {u}"),
        LearnSource::Path(p) => format!("learned from {}", p.display()),
        LearnSource::Reflect | LearnSource::Save(_) => "learned".into(),
    };
    let preview = draft.skill.instructions.lines().take(8).collect::<Vec<_>>();
    let n = keep(draft, reason);
    let mut prompt = format!("Save these steps as the skill {slash} ({name})?");
    if refs > 0 {
        prompt.push_str(&format!(
            "\n\nLarge source: index plus {refs} reference file{}.",
            if refs == 1 { "" } else { "s" }
        ));
    }
    prompt.push_str("\n\n");
    prompt.push_str(&preview.join("\n"));
    prompt.push_str("\n\nGoes through the skill scanner; /skills undo takes it back.");
    Ok(Effect::Confirm {
        prompt,
        yes: format!("/learn --save {n}"),
    })
}

fn save(config_dir: &Path, n: u64) -> Result<Effect, String> {
    let (draft, reason) = {
        let mut d = drafts();
        let at = d.kept.iter().position(|(id, _, _)| *id == n).ok_or(GONE)?;
        let (_, draft, reason) = d.kept.remove(at);
        (draft, reason)
    };
    let skills_dir = grokhub_agent::skill_write::skills_dir(config_dir);
    grokhub_agent::harness::record_skill_change(
        config_dir,
        &skills_dir,
        &draft.skill.name,
        Origin::User,
        &reason,
        || write_learned_skill(&skills_dir, &draft).map(|_| ()),
    )?;
    grokhub_agent::invalidate_discover_cache();
    Ok(Effect::Notice {
        text: format!(
            "Saved the skill {}. /skills undo {} takes it back.",
            draft.skill.slash, draft.skill.name
        ),
    })
}

fn gather(
    config_dir: &Path,
    workspace: &Path,
    chat_id: Option<&str>,
    source: &LearnSource,
) -> Result<(String, String), String> {
    match source {
        LearnSource::Chat => {
            let id = chat_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                "Send /learn chat from a saved chat so there is a transcript to learn from."
                    .to_string()
            })?;
            let info = crate::history::saved(config_dir, id)
                .map_err(|_| "Send a message first. This chat isn't saved yet.".to_string())?;
            let pairs = grokhub_agent::transcript_pairs(&info);
            let body = chat_body_from_pairs(&pairs);
            if body.trim().is_empty() {
                return Err("Nothing to learn from — this chat has no turns yet.".into());
            }
            let title = if info.title.trim().is_empty() {
                "this chat".to_string()
            } else {
                info.title.clone()
            };
            Ok((title, body))
        }
        LearnSource::Url(url) => {
            let text = grokhub_agent::fetch_public_markdown(url)?;
            let title = url
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("docs")
                .to_string();
            Ok((title, text))
        }
        LearnSource::Path(raw) => {
            let confined = grokhub_agent::confine_workspace(workspace, &raw.to_string_lossy())?;
            let (title, body) =
                grokhub_core::gather_path_text(&confined).map_err(|e| e.to_string())?;
            Ok((title, body))
        }
        LearnSource::Reflect | LearnSource::Save(_) => Err(learn_skill_usage().into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::SkillMd;

    #[test]
    fn only_newest_drafts_wait_for_yes() {
        let first = keep(
            LearnDraft {
                skill: SkillMd::default(),
                meta: Default::default(),
                references: Vec::new(),
            },
            "t".into(),
        );
        for _ in 0..KEEP {
            keep(
                LearnDraft {
                    skill: SkillMd::default(),
                    meta: Default::default(),
                    references: Vec::new(),
                },
                "t".into(),
            );
        }
        let d = drafts();
        assert!(d.kept.len() <= KEEP);
        assert!(d.kept.iter().all(|(n, _, _)| *n != first));
    }
}
