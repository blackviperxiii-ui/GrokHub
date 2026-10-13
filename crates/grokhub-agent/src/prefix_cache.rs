//! Provider prompt prefix cache: keep the start of each model `input` byte-identical
//! across turns when only per-turn state (talk style, playbook, memory snippets) changes.
//!
//! Off by default (`{config_dir}/prefix_cache.toml`). When enabled, the first system
//! message stays the published cabin prompt for the session; volatile blocks ride on
//! the user turn inside `<cabin-volatile>` instead of mutating that system message or
//! inserting extra system rows mid-history.

use std::path::Path;
use std::sync::Mutex;

use crate::client::{ContentPart, InputItem};

pub const WRAPPER_START: &str = "<cabin-volatile>\n";
pub const WRAPPER_END: &str = "\n</cabin-volatile>";

const CONFIG_NAME: &str = "prefix_cache.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    pub enabled: bool,
}

static TEST_OVERRIDE: Mutex<Option<bool>> = Mutex::new(None);

/// Test hook: `Some(true|false)` overrides the file; `None` reads the file again.
#[cfg(test)]
pub fn set_enabled_for_tests(on: Option<bool>) {
    *TEST_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()) = on;
}

pub fn config_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(CONFIG_NAME)
}

pub fn settings(config_dir: &Path) -> Settings {
    if let Some(on) = *TEST_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()) {
        return Settings { enabled: on };
    }
    let Ok(text) = std::fs::read_to_string(config_path(config_dir)) else {
        return Settings::default();
    };
    parse_file(&text)
}

fn parse_file(text: &str) -> Settings {
    let mut enabled = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("enabled") && value.trim().eq_ignore_ascii_case("true") {
            enabled = true;
        }
    }
    Settings { enabled }
}

pub fn enabled(config_dir: &Path) -> bool {
    settings(config_dir).enabled
}

/// Per-turn volatile system state kept out of the stable system prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnVolatile {
    pub style: Option<String>,
    pub playbook: Option<String>,
    pub memory: Option<String>,
}

impl TurnVolatile {
    pub fn assemble(
        workspace: &Path,
        user_text: &str,
        first_turn: bool,
        style: Option<grokhub_core::talk::TalkStyle>,
        playbook: Option<&str>,
    ) -> Self {
        let style = style.map(|s| s.block()).filter(|s| !s.is_empty());
        let playbook = playbook
            .map(|block| block.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let memory = if first_turn {
            crate::memory::first_turn_injection(workspace, user_text)
        } else {
            crate::memory::followup_injection(user_text)
        };
        Self {
            style,
            playbook,
            memory,
        }
    }

    pub fn format(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = &self.style {
            parts.push(s.clone());
        }
        if let Some(p) = &self.playbook {
            parts.push(p.clone());
        }
        if let Some(m) = &self.memory {
            parts.push(m.clone());
        }
        parts.join("\n\n")
    }
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

/// Split a stored user turn into volatile block and visible user text.
#[cfg(test)]
fn split_user_turn(text: &str) -> Option<(&str, &str)> {
    let trimmed = text.strip_prefix(WRAPPER_START)?;
    let (volatile, rest) = trimmed.split_once(WRAPPER_END)?;
    let user = rest.strip_prefix("\n\n").unwrap_or(rest);
    Some((volatile, user))
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
    fn temp_config(enabled: bool) -> (std::path::PathBuf, Settings) {
        let dir = crate::harness::test_dir("prefix-cache-settings");
        let path = config_path(&dir);
        std::fs::write(&path, format!("enabled = {enabled}\n")).unwrap();
        let s = settings(&dir);
        (dir, s)
    }

    #[test]
    fn settings_default_off_and_file_enables() {
        set_enabled_for_tests(None);
        let dir = crate::harness::test_dir("prefix-cache-default");
        let path = config_path(&dir);
        let _ = std::fs::remove_file(&path);
        assert!(!enabled(&dir));
        let (_dir, on) = temp_config(true);
        assert!(on.enabled);
        set_enabled_for_tests(Some(false));
        assert!(!enabled(&_dir));
        set_enabled_for_tests(None);
    }

    #[test]
    fn stable_system_prefix_unchanged_when_only_volatile_changes() {
        let v_a = TurnVolatile {
            style: Some("Talk style for this turn: short.".into()),
            playbook: None,
            memory: None,
        };
        let v_b = TurnVolatile {
            style: Some("Talk style for this turn: concise.".into()),
            playbook: Some("Playbook for this turn: files.".into()),
            memory: None,
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
