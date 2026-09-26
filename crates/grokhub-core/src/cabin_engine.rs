//! Each cabin part learns on its own. This engine is the only place that
//! reads those notes together and tells a part what to change.
//!
//! No model call. A note has to show up twice before it becomes a directive.

use serde::{Deserialize, Serialize};

use crate::is_plain_text;
use crate::learning::LearningState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartNote {
    pub part: String,
    pub key: String,
    pub text: String,
    pub hits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CabinDirective {
    pub target: String,
    pub key: String,
    pub how: String,
}

pub fn note_part(state: &mut LearningState, part: &str, key: &str, text: &str) {
    let part = part.trim().to_ascii_lowercase();
    let key: String = key.trim().chars().take(80).collect();
    let text: String = text.trim().chars().take(160).collect();
    if part.is_empty() || key.is_empty() || text.len() < 4 || !is_plain_text(&text) {
        return;
    }
    if let Some(found) = state
        .part_notes
        .iter_mut()
        .find(|n| n.part == part && n.key == key)
    {
        found.text = text;
        found.hits = found.hits.saturating_add(1);
        return;
    }
    state.part_notes.insert(
        0,
        PartNote {
            part,
            key,
            text,
            hits: 1,
        },
    );
    if state.part_notes.len() > 80 {
        state.part_notes.truncate(80);
    }
}

/// Rebuild directives from what each part has seen. Old directives that the
/// notes no longer support are dropped.
pub fn absorb_cabin(state: &mut LearningState) {
    let mut next = Vec::new();
    for note in &state.part_notes {
        if note.hits < 2 {
            continue;
        }
        match note.part.as_str() {
            "ideas" if note.key.starts_with("opened:") => {
                push(
                    &mut next,
                    "ideas",
                    &format!("rank:{}", note.key),
                    &format!("Rank '{}' higher. They came back to it.", note.text),
                );
                if routine_title(&note.text) {
                    push(
                        &mut next,
                        "automations",
                        &format!("from-idea:{}", note.key),
                        &format!(
                            "They keep opening '{}'. When they describe that routine, offer to save it.",
                            note.text
                        ),
                    );
                }
            }
            "ideas" if note.key.starts_with("rejected:") => {
                push(
                    &mut next,
                    "ideas",
                    &format!("avoid:{}", note.key),
                    &format!("Do not suggest a card like {}.", note.text),
                );
            }
            "chat" if note.key == "style:short" => {
                push(
                    &mut next,
                    "chat",
                    "style:short",
                    "Keep replies short. They have asked for that.",
                );
            }
            "chat" if note.key.starts_with("pref:") => {
                push(
                    &mut next,
                    "chat",
                    &format!("use:{}", note.key),
                    &format!("Keep this in mind: {}", note.text),
                );
            }
            "imagine" if note.key == "generated" => {
                push(
                    &mut next,
                    "imagine",
                    "keep",
                    "They generate images. Remember the last style they used.",
                );
                push(
                    &mut next,
                    "chat",
                    "use-imagine",
                    "When they ask for a picture, use Imagine instead of only describing one.",
                );
            }
            "skills" if note.key.starts_with("ran:") => {
                push(
                    &mut next,
                    "skills",
                    &format!("offer:{}", note.key),
                    &format!("Offer {} when the ask matches.", note.text),
                );
                if note.hits >= 4 {
                    push(
                        &mut next,
                        "automations",
                        &format!("skill-schedule:{}", note.key),
                        &format!("They run {} often. A schedule may fit.", note.text),
                    );
                }
            }
            "workboard" if note.key.starts_with("done:") => {
                push(
                    &mut next,
                    "workboard",
                    &format!("trust:{}", note.key),
                    &format!("They finish '{}'. Mark that kind of card done when the work is done.", note.text),
                );
            }
            "workboard" if note.key.starts_with("paused:") => {
                push(
                    &mut next,
                    "workboard",
                    &format!("keep:{}", note.key),
                    "Leave paused cards visible. That is where they resume.",
                );
                push(
                    &mut next,
                    "chat",
                    &format!("resume:{}", note.key),
                    "A paused workboard card is still open. Resume it when they come back.",
                );
            }
            _ => {}
        }
    }
    absorb_automations(&state.part_notes, &mut next);
    next.truncate(12);
    state.directives = next;
}

fn absorb_automations(notes: &[PartNote], next: &mut Vec<CabinDirective>) {
    let mut names: Vec<String> = Vec::new();
    for note in notes {
        if note.part != "automations" {
            continue;
        }
        let Some(name) = note.key.split_once(':').map(|(_, name)| name.to_string()) else {
            continue;
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    for name in names {
        let ran = hits(notes, "automations", &format!("ran:{name}"));
        let skipped = hits(notes, "automations", &format!("skipped:{name}"));
        if skipped >= 2 && skipped >= ran {
            push(
                next,
                "automations",
                &format!("stop:{name}"),
                &format!("Stop offering {name}. They skip it."),
            );
            push(
                next,
                "ideas",
                &format!("avoid:auto:{name}"),
                &format!("Do not suggest a card like {name}."),
            );
        } else if ran >= 2 && ran > skipped {
            push(
                next,
                "automations",
                &format!("keep:{name}"),
                &format!("Keep {name}. It is part of their day."),
            );
        }
    }
}

fn hits(notes: &[PartNote], part: &str, key: &str) -> u32 {
    notes
        .iter()
        .find(|n| n.part == part && n.key == key)
        .map(|n| n.hits)
        .unwrap_or(0)
}

fn push(next: &mut Vec<CabinDirective>, target: &str, key: &str, how: &str) {
    if next.iter().any(|d| d.target == target && d.key == key) {
        return;
    }
    next.push(CabinDirective {
        target: target.to_string(),
        key: key.to_string(),
        how: how.chars().take(220).collect(),
    });
}

fn routine_title(title: &str) -> bool {
    let t = title.to_ascii_lowercase();
    t.contains("remind")
        || t.contains("morning")
        || t.contains("night")
        || t.contains("wrap")
        || t.contains("routine")
        || t.contains("schedule")
}

/// What one part should apply. Empty when the engine has nothing for it.
pub fn brief_for(state: &LearningState, target: &str) -> String {
    let target = target.trim().to_ascii_lowercase();
    let mut out = String::new();
    for directive in state
        .directives
        .iter()
        .filter(|d| d.target == target)
        .take(4)
    {
        let line = directive.how.trim();
        if line.is_empty() {
            continue;
        }
        if out.len() + line.len() + 1 > 360 {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

pub fn engine_slug(text: &str) -> String {
    let lower = text.trim().to_ascii_lowercase();
    let slug: String = lower
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slug.split('-')
        .filter(|w| w.len() >= 2)
        .take(4)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> LearningState {
        LearningState::default()
    }

    #[test]
    fn each_part_learns_and_the_engine_routes_it() {
        let mut s = state();
        note_part(&mut s, "ideas", "opened:morning-reminder", "Morning reminder");
        absorb_cabin(&mut s);
        assert!(s.directives.is_empty(), "one look is not a lesson");
        note_part(&mut s, "ideas", "opened:morning-reminder", "Morning reminder");
        absorb_cabin(&mut s);
        assert!(brief_for(&s, "ideas").contains("Rank 'Morning reminder'"));
        assert!(brief_for(&s, "automations").contains("offer to save"));
        assert!(brief_for(&s, "chat").is_empty());

        note_part(&mut s, "automations", "skipped:nightly", "nightly");
        note_part(&mut s, "automations", "skipped:nightly", "nightly");
        absorb_cabin(&mut s);
        assert!(brief_for(&s, "automations").contains("Stop offering nightly"));
        assert!(brief_for(&s, "ideas").contains("Do not suggest a card like nightly"));

        note_part(&mut s, "imagine", "generated", "They generate images.");
        note_part(&mut s, "imagine", "generated", "They generate images.");
        absorb_cabin(&mut s);
        assert!(brief_for(&s, "imagine").contains("Remember the last style"));
        assert!(brief_for(&s, "chat").contains("use Imagine"));

        note_part(&mut s, "workboard", "paused:harbor", "Harbor");
        note_part(&mut s, "workboard", "paused:harbor", "Harbor");
        absorb_cabin(&mut s);
        assert!(brief_for(&s, "workboard").contains("paused cards"));
        assert!(brief_for(&s, "chat").contains("paused workboard"));
    }

    #[test]
    fn a_later_skip_replaces_a_keep() {
        let mut s = state();
        note_part(&mut s, "automations", "ran:digest", "digest");
        note_part(&mut s, "automations", "ran:digest", "digest");
        absorb_cabin(&mut s);
        assert!(brief_for(&s, "automations").contains("Keep digest"));
        note_part(&mut s, "automations", "skipped:digest", "digest");
        note_part(&mut s, "automations", "skipped:digest", "digest");
        absorb_cabin(&mut s);
        let auto = brief_for(&s, "automations");
        assert!(auto.contains("Stop offering digest"), "{auto}");
        assert!(!auto.contains("Keep digest"), "{auto}");
    }
}
