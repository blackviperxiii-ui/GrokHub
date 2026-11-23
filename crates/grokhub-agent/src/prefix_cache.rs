//! Provider prompt prefix cache: keep the start of each model `input` byte-identical
//! across turns when only per-turn state (talk style, playbook, memory snippets) changes.
//!
//! Always on: the first system message stays the published cabin prompt for
//! the session; volatile blocks ride on the user turn inside
//! `<cabin-volatile>` instead of mutating that system message or inserting
//! extra system rows mid-history. `{config_dir}/prefix_cache.toml` only says
//! when old notes leave (`old_notes`); an `enabled` line there is ignored.

use std::path::Path;

use crate::client::{ContentPart, InputItem};

pub const WRAPPER_START: &str = "<cabin-volatile>\n";
pub const WRAPPER_END: &str = "\n</cabin-volatile>";

const CONFIG_NAME: &str = "prefix_cache.toml";

/// When an earlier turn's `<cabin-volatile>` notes (its talk style, playbook,
/// memory) leave the history, so only the current turn's notes steer the reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OldNotes {
    /// At compaction, which rebuilds the prompt anyway, so no cache is lost.
    #[default]
    AtCompaction,
    /// Before every request (`old_notes = "every_turn"`): old notes never
    /// linger, at the price of the last turn's cached tail each turn.
    EveryTurn,
}

pub fn config_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(CONFIG_NAME)
}

/// When earlier turns lose their notes, from `prefix_cache.toml`.
pub fn old_notes(config_dir: &Path) -> OldNotes {
    std::fs::read_to_string(config_path(config_dir))
        .map(|text| parse_old_notes(&text))
        .unwrap_or_default()
}

fn parse_old_notes(text: &str) -> OldNotes {
    // `enabled` is a switch older versions read; it is still taken, and ignored.
    let file = crate::config_file::parse(CONFIG_NAME, text, &["old_notes", "enabled"]);
    match file.one_of(
        "old_notes",
        &["every_turn", "at_compaction"],
        "at_compaction",
    ) {
        Some("every_turn") => OldNotes::EveryTurn,
        _ => OldNotes::AtCompaction,
    }
}

/// Per-turn volatile system state kept out of the stable system prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnVolatile {
    pub style: Option<String>,
    pub playbook: Option<String>,
    pub memory: Option<String>,
    pub intentions: Option<String>,
    /// The turn's voice note: a guest voice, the depth note, a quip slot
    /// ([`crate::voice_session`]).
    pub voice: Option<String>,
    /// This turn steers differently from the last one (LO-51): its notes open
    /// with a line that earlier turns' notes are old. Old notes stay in the
    /// history so the cached prefix stays append-only.
    pub supersedes: bool,
}

impl TurnVolatile {
    pub fn assemble(
        workspace: &Path,
        user_text: &str,
        first_turn: bool,
        style: Option<grokhub_core::talk::TalkStyle>,
        soft_caps: bool,
        playbook: Option<&str>,
        already: &[String],
        intentions: Option<String>,
        recall: bool,
    ) -> Self {
        let style = style
            .map(|s| s.prompt_block(soft_caps))
            .filter(|s| !s.is_empty());
        let playbook = playbook
            .map(|block| block.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        // A scratch chat reads no memory.
        let memory = if !recall {
            None
        } else if first_turn {
            crate::memory::first_turn_injection(workspace, user_text)
        } else {
            crate::memory::followup_injection(workspace, user_text, already)
        };
        let intentions = intentions.filter(|block| !block.trim().is_empty());
        Self {
            style,
            playbook,
            memory,
            intentions,
            voice: None,
            supersedes: false,
        }
    }

    /// The same notes plus the turn's voice note, if there is one.
    pub fn with_voice(mut self, voice: Option<String>) -> Self {
        self.voice = voice.filter(|v| !v.trim().is_empty());
        self
    }

    /// Mark the notes as replacing the last turn's when its style or playbook
    /// is not among them.
    pub fn superseding(mut self, history: &[InputItem]) -> Self {
        self.supersedes = steering_changed(history, &self);
        self
    }

    pub fn format(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.supersedes {
            parts.push(SUPERSEDES_NOTE.to_string());
        }
        if let Some(v) = &self.voice {
            parts.push(v.clone());
        }
        if let Some(s) = &self.style {
            parts.push(s.clone());
        }
        if let Some(p) = &self.playbook {
            parts.push(p.clone());
        }
        if let Some(m) = &self.memory {
            parts.push(m.clone());
        }
        if let Some(block) = &self.intentions {
            parts.push(block.clone());
        }
        parts.join("\n\n")
    }
}

/// Lead line of a turn whose notes replace the last turn's.
pub const SUPERSEDES_NOTE: &str =
    "Notes from earlier turns are old. Follow only these, for this message.";

/// This turn's talk style or playbook is not in the newest stored user turn's
/// notes, so the two would steer differently.
pub fn steering_changed(items: &[InputItem], volatile: &TurnVolatile) -> bool {
    let Some(notes) = items.iter().rev().find_map(|item| match item {
        InputItem::Message { role, content } if role == "user" => Some(
            content
                .iter()
                .find_map(|part| match part {
                    ContentPart::InputText(text) => split_user_turn(text).map(|(n, _)| n),
                    _ => None,
                })
                .unwrap_or(""),
        ),
        _ => None,
    }) else {
        return false;
    };
    // The research recipe steers only its own turn: a turn without it says
    // the earlier notes are old.
    let research_ended = notes.contains(crate::research::RECIPE)
        && !volatile
            .intentions
            .as_deref()
            .is_some_and(|block| block.contains(crate::research::RECIPE));
    !notes.is_empty()
        && (research_ended
            || [&volatile.style, &volatile.playbook]
                .into_iter()
                .flatten()
                .any(|block| !notes.contains(block.as_str())))
}

pub fn push_stable_system(history: &mut Vec<InputItem>, stable_system: &str) {
    let stable = stable_system.trim();
    if stable.is_empty() {
        return;
    }
    history.push(InputItem::Message {
        role: "system".into(),
        content: vec![ContentPart::InputText(stable.to_string())],
    });
}

pub fn push_user_turn(
    history: &mut Vec<InputItem>,
    volatile: &TurnVolatile,
    user_text: &str,
    image: Option<&str>,
) {
    let body = user_message_text(&volatile.format(), user_text);
    let mut content = vec![ContentPart::InputText(body)];
    if let Some(url) = image.map(str::trim).filter(|s| !s.is_empty()) {
        content.push(ContentPart::InputImage(url.to_string()));
    }
    history.push(InputItem::Message {
        role: "user".into(),
        content,
    });
}

pub fn user_message_text(volatile: &str, user_text: &str) -> String {
    let volatile = volatile.trim();
    if volatile.is_empty() {
        return user_text.to_string();
    }
    format!("{WRAPPER_START}{volatile}{WRAPPER_END}\n\n{user_text}")
}

/// What the person typed in a stored user turn, without the `<cabin-volatile>`
/// block the cabin put in front of it. Text with no block comes back as is.
pub fn visible_user_text(text: &str) -> &str {
    split_user_turn(text).map_or(text, |(_, user)| user)
}

/// Split a stored user turn into volatile block and visible user text.
pub(crate) fn split_user_turn(text: &str) -> Option<(&str, &str)> {
    let trimmed = text.strip_prefix(WRAPPER_START)?;
    let (volatile, rest) = trimmed.split_once(WRAPPER_END)?;
    let user = rest.strip_prefix("\n\n").unwrap_or(rest);
    Some((volatile, user))
}

/// Drop the `<cabin-volatile>` notes from every user turn but the newest, so
/// an earlier turn's playbook or talk style can't steer this one. Returns
/// whether anything changed.
pub fn drop_older_notes(items: &mut [InputItem]) -> bool {
    let Some(last) = items
        .iter()
        .rposition(|item| matches!(item, InputItem::Message { role, .. } if role == "user"))
    else {
        return false;
    };
    let mut changed = false;
    for item in &mut items[..last] {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role != "user" {
            continue;
        }
        for part in content.iter_mut() {
            if let ContentPart::InputText(text) = part {
                if let Some((_, user)) = split_user_turn(text) {
                    *text = user.to_string();
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Desktop episode worker input: stable system + growing view + volatile step tail.
pub fn episode_worker_input(
    system_base: &str,
    view_text: &str,
    step_text: &str,
    observation_image: Option<&str>,
) -> Vec<InputItem> {
    let system = if system_base.trim().is_empty() {
        crate::episode::EPISODE_RULES.to_string()
    } else {
        format!("{system_base}\n\n{}", crate::episode::EPISODE_RULES)
    };
    let mut tail = vec![ContentPart::InputText(step_text.to_string())];
    if let Some(img) = observation_image.map(str::trim).filter(|s| !s.is_empty()) {
        tail.push(ContentPart::InputImage(img.to_string()));
    }
    vec![
        InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText(system)],
        },
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(view_text.to_string())],
        },
        InputItem::Message {
            role: "user".into(),
            content: tail,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::input_wire_value;
    #[test]
    fn a_turn_that_steers_differently_says_older_notes_are_old() {
        let turn = |playbook: &str| TurnVolatile {
            playbook: Some(playbook.to_string()),
            ..Default::default()
        };
        let mut history = Vec::new();
        let first = turn("research playbook").superseding(&history);
        assert!(!first.supersedes, "nothing earlier to replace");
        push_user_turn(&mut history, &first, "where do i turn these in", None);
        let same = turn("research playbook").superseding(&history);
        assert!(!same.supersedes);
        push_user_turn(&mut history, &same, "and the vendor?", None);
        let hangout = turn("hangout playbook").superseding(&history);
        assert!(hangout.supersedes);
        assert!(hangout.format().starts_with(SUPERSEDES_NOTE));
        let before: Vec<_> = history.clone();
        push_user_turn(&mut history, &hangout, "yeah", None);
        // Append-only: earlier turns go out byte for byte.
        assert_eq!(history[..2], before[..]);
    }

    #[test]
    fn an_old_enabled_line_is_ignored() {
        let dir = crate::harness::test_dir("prefix-cache-settings");
        let path = config_path(&dir);
        let _ = std::fs::remove_file(&path);
        assert_eq!(old_notes(&dir), OldNotes::AtCompaction);
        std::fs::write(&path, "enabled = false\nold_notes = every_turn\n").unwrap();
        assert_eq!(old_notes(&dir), OldNotes::EveryTurn);
    }

    #[test]
    fn old_notes_leave_at_compaction_unless_set_to_every_turn() {
        assert_eq!(parse_old_notes(""), OldNotes::AtCompaction);
        assert_eq!(
            parse_old_notes("old_notes = \"every_turn\"\n"),
            OldNotes::EveryTurn
        );
        assert_eq!(parse_old_notes("old_notes = soon"), OldNotes::AtCompaction);
    }

    #[test]
    fn only_the_newest_turn_keeps_its_notes() {
        let mut items = Vec::new();
        push_stable_system(&mut items, "rules");
        let hangout = TurnVolatile {
            playbook: Some("Playbook for this turn: hanging out.".into()),
            ..Default::default()
        };
        push_user_turn(&mut items, &hangout, "lol same", None);
        items.push(InputItem::Message {
            role: "assistant".into(),
            content: vec![ContentPart::InputText("haha".into())],
        });
        let tuneup = TurnVolatile {
            playbook: Some("Playbook for this turn: check up this PC.".into()),
            ..Default::default()
        };
        push_user_turn(&mut items, &tuneup, "check my pc", None);
        assert!(drop_older_notes(&mut items));
        let texts: Vec<String> = items
            .iter()
            .map(|i| match i {
                InputItem::Message { content, .. } => match &content[0] {
                    ContentPart::InputText(t) => t.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            })
            .collect();
        assert_eq!(texts[0], "rules");
        assert_eq!(texts[1], "lol same");
        assert_eq!(texts[2], "haha");
        assert!(texts[3].contains("check up this PC") && texts[3].ends_with("check my pc"));
        // Nothing left to drop the second time.
        assert!(!drop_older_notes(&mut items));
    }

    #[test]
    fn visible_user_text_drops_only_the_cabin_notes() {
        let stored = user_message_text("Talk style for this turn: short.", "hi there");
        assert_ne!(stored, "hi there");
        assert_eq!(visible_user_text(&stored), "hi there");
        assert_eq!(visible_user_text("plain words"), "plain words");
        // A person who types the tag themselves keeps their words.
        assert_eq!(
            visible_user_text("<cabin-volatile>\nno closing tag"),
            "<cabin-volatile>\nno closing tag"
        );
    }

    #[test]
    fn stable_system_prefix_unchanged_when_only_volatile_changes() {
        let v_a = TurnVolatile {
            style: Some("Talk style for this turn: short.".into()),
            playbook: None,
            memory: None,
            intentions: None,
            voice: None,
            supersedes: false,
        };
        let v_b = TurnVolatile {
            style: Some("Talk style for this turn: concise.".into()),
            playbook: Some("Playbook for this turn: files.".into()),
            memory: None,
            intentions: None,
            voice: None,
            supersedes: false,
        };
        let mut history_a: Vec<InputItem> = Vec::new();
        push_stable_system(&mut history_a, "STABLE_RULES");
        push_user_turn(&mut history_a, &v_a, "hello", None);
        let mut history_b: Vec<InputItem> = Vec::new();
        push_stable_system(&mut history_b, "STABLE_RULES");
        push_user_turn(&mut history_b, &v_b, "hello", None);
        let sys_a = serde_json::to_string(&input_wire_value(&history_a[..1])).unwrap();
        let sys_b = serde_json::to_string(&input_wire_value(&history_b[..1])).unwrap();
        assert_eq!(sys_a, sys_b, "stable system must stay byte-identical");
        let msg_a = message_text(&history_a, 1);
        let msg_b = message_text(&history_b, 1);
        let (_, user_a) = split_user_turn(&msg_a).unwrap();
        let (_, user_b) = split_user_turn(&msg_b).unwrap();
        assert_eq!(user_a, "hello");
        assert_eq!(user_b, "hello");
        assert_ne!(
            message_text(&history_a, 1),
            message_text(&history_b, 1),
            "volatile wrapper should differ"
        );
    }

    #[test]
    fn second_user_turn_appends_without_mutating_first() {
        let mut history: Vec<InputItem> = Vec::new();
        push_stable_system(&mut history, "STABLE");
        push_user_turn(&mut history, &TurnVolatile::default(), "turn one", None);
        let frozen = serde_json::to_string(&input_wire_value(&history)).unwrap();
        push_user_turn(
            &mut history,
            &TurnVolatile {
                style: Some("Talk style for this turn: quiet.".into()),
                playbook: None,
                memory: None,
                intentions: None,
                voice: None,
                supersedes: false,
            },
            "turn two",
            None,
        );
        let prefix = serde_json::to_string(&input_wire_value(&history[..2])).unwrap();
        assert_eq!(prefix, frozen, "first turn wire must not change");
    }

    #[test]
    fn episode_input_keeps_dynamic_state_in_the_last_user_message() {
        let stable_view = "Past lessons: none.\nEpisode so far:\n";
        let step_a = "Step 1 of this episode.\nGoal step: Wi-Fi\nLatest observation:\noff";
        let step_b = "Step 2 of this episode.\nGoal step: Wi-Fi\nLatest observation:\non";
        let a = episode_worker_input("CABIN", stable_view, step_a, None);
        let b = episode_worker_input("CABIN", stable_view, step_b, None);
        let wire =
            |items: &[InputItem]| serde_json::to_string(&input_wire_value(&items[..2])).unwrap();
        assert_eq!(wire(&a), wire(&b), "system + view prefix must match");
        assert_ne!(message_text(&a, 2), message_text(&b, 2));
        assert!(message_text(&a, 2).contains("Step 1"));
        assert!(message_text(&b, 2).contains("Step 2"));
    }

    fn message_text(items: &[InputItem], idx: usize) -> String {
        match &items[idx] {
            InputItem::Message { content, .. } => content
                .iter()
                .filter_map(|p| match p {
                    ContentPart::InputText(t) => Some(t.as_str()),
                    ContentPart::InputImage(_) => None,
                })
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    }
}
