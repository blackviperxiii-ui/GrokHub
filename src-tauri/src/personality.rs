//! Settings → Personality (shell parity): the cabin's presets and the
//! person's own in `{config}/personalities/`, the same files the cabin reads.
//! A personality sets tone only; the safety rules in the stable prompt win.

use std::path::{Path, PathBuf};

use grokhub_core::personality::{self, Emoji, Personality, Verbosity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub description: String,
    /// A reply to "my build failed" in this voice.
    pub sample: String,
    pub tone_rules: String,
    pub emoji: Emoji,
    pub verbosity: Verbosity,
    /// Profanity allowed: the card says Explicit.
    pub explicit: bool,
    /// The person's own, so it can be edited and deleted.
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub presets: Vec<Preset>,
    /// The default for every chat.
    pub current: String,
    /// Custom files that could not be used, each naming the file and why.
    pub problems: Vec<String>,
}

/// The editor's fields for one custom preset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tone_rules: String,
    pub emoji: Emoji,
    pub verbosity: Verbosity,
}

pub fn dir(config_dir: &Path) -> PathBuf {
    config_dir.join("personalities")
}

/// The id a stored field stands for: empty is the default voice.
pub fn shown_id(stored: &str) -> &str {
    match stored.trim() {
        "" => personality::DEFAULT_ID,
        id => id,
    }
}

/// How an id is stored, so the default voice stays an empty field like the cabin's.
pub fn stored_id(id: &str) -> String {
    match id.trim() {
        personality::DEFAULT_ID => String::new(),
        id => id.to_string(),
    }
}

fn preset(p: Personality) -> Preset {
    Preset {
        id: p.id,
        name: p.name,
        description: p.description,
        sample: p.sample,
        tone_rules: p.tone_rules,
        emoji: p.emoji,
        verbosity: p.verbosity,
        explicit: p.explicit,
        custom: p.custom,
    }
}

pub fn load(config_dir: &Path, stored: &str) -> Page {
    let (all, problems) = personality::load_all(&dir(config_dir));
    let current = shown_id(stored);
    // A default whose file was deleted in the cabin falls back to the default voice.
    let current = if all.iter().any(|p| p.id == current) {
        current.to_string()
    } else {
        personality::DEFAULT_ID.to_string()
    };
    Page {
        presets: all.into_iter().map(preset).collect(),
        current,
        problems,
    }
}

/// The preset for `id`, refused when no preset has it.
pub fn pick(config_dir: &Path, id: &str) -> Result<Personality, String> {
    let (all, _) = personality::load_all(&dir(config_dir));
    all.into_iter()
        .find(|p| p.id == id.trim())
        .ok_or_else(|| format!("No personality called {id}."))
}

/// The block a chat's system prompt carries for the stored id, with the
/// closer that puts the safety rules back on top. Empty for the default voice.
pub fn voice(config_dir: &Path, stored: &str) -> String {
    let block = personality::lookup(&dir(config_dir), shown_id(stored)).prompt_block();
    if block.is_empty() {
        block
    } else {
        format!("{block}\n\n{}", grokhub_core::talk::VOLATILE_SAFETY_CLOSER)
    }
}

/// A new custom copy of `id`; returns its id.
pub fn duplicate(config_dir: &Path, id: &str) -> Result<String, String> {
    let base = pick(config_dir, id)?;
    let (all, _) = personality::load_all(&dir(config_dir));
    let copy = personality::duplicate(&base, &all);
    personality::save_custom(&dir(config_dir), &copy)?;
    Ok(copy.id)
}

/// Saves the editor's fields over a custom preset, keeping what the editor
/// doesn't show (sample, address, voice hint, explicit).
pub fn save(config_dir: &Path, edit: Edit) -> Result<(), String> {
    let mut p = pick(config_dir, &edit.id)?;
    if !p.custom {
        return Err(format!(
            "{} is a built-in personality; duplicate it to make your own.",
            p.name
        ));
    }
    p.name = edit.name.trim().to_string();
    p.description = edit.description.trim().to_string();
    p.tone_rules = edit.tone_rules.trim().to_string();
    p.emoji = edit.emoji;
    p.verbosity = edit.verbosity;
    personality::save_custom(&dir(config_dir), &p).map(|_| ())
}

pub fn delete(config_dir: &Path, id: &str) -> Result<(), String> {
    let p = pick(config_dir, id)?;
    if !p.custom {
        return Err("Built-in personalities can't be deleted.".into());
    }
    personality::delete_custom(&dir(config_dir), &p.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gh-shell-persona-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_page_lists_the_builtins_with_the_default_first_and_current() {
        let cfg = scratch("page");
        let page = load(&cfg, "");
        assert_eq!(page.current, "default");
        assert_eq!(page.presets[0].id, "default");
        assert_eq!(page.presets[0].name, "Default");
        assert!(page.presets.iter().any(|p| p.id == "pirate" && !p.custom));
        assert_eq!(page.problems, Vec::<String>::new());
        assert_eq!(load(&cfg, "pirate").current, "pirate");
        assert_eq!(load(&cfg, "gone-voice").current, "default");
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn the_default_voice_adds_nothing_and_a_preset_adds_its_block_and_the_closer() {
        let cfg = scratch("voice");
        assert_eq!(voice(&cfg, ""), "");
        assert_eq!(voice(&cfg, "default"), "");
        let pirate = voice(&cfg, "pirate");
        assert_eq!(
            pirate,
            format!(
                "{}\n\n{}",
                personality::builtin("pirate").prompt_block(),
                grokhub_core::talk::VOLATILE_SAFETY_CLOSER
            )
        );
        assert!(pirate.starts_with("Personality: "), "{pirate}");
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn duplicate_edit_and_delete_a_custom_preset() {
        let cfg = scratch("custom");
        let id = duplicate(&cfg, "pirate").expect("duplicate");
        assert_eq!(id, "pirate-copy");
        assert!(dir(&cfg).join("pirate-copy.toml").exists());
        let page = load(&cfg, "");
        let copy = page.presets.iter().find(|p| p.id == id).expect("listed");
        assert!(copy.custom);
        assert_eq!(
            copy.name,
            format!("{} copy", personality::builtin("pirate").name)
        );

        save(
            &cfg,
            Edit {
                id: id.clone(),
                name: "Deckhand".into(),
                description: "Salty but brief.".into(),
                tone_rules: "Talk like a deckhand.".into(),
                emoji: Emoji::Light,
                verbosity: Verbosity::Terse,
            },
        )
        .expect("save");
        let edited = pick(&cfg, &id).unwrap();
        assert_eq!(
            (
                edited.name.as_str(),
                edited.tone_rules.as_str(),
                edited.emoji,
                edited.verbosity
            ),
            (
                "Deckhand",
                "Talk like a deckhand.",
                Emoji::Light,
                Verbosity::Terse
            )
        );
        assert!(voice(&cfg, &id).starts_with("Personality: Deckhand. Talk like a deckhand."));

        delete(&cfg, &id).expect("delete");
        assert!(!dir(&cfg).join("pirate-copy.toml").exists());
        assert_eq!(load(&cfg, &id).current, "default");
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn builtins_refuse_edits_and_deletes_and_bad_fields_are_refused() {
        let cfg = scratch("refuse");
        let edit = |id: &str, name: &str| Edit {
            id: id.into(),
            name: name.into(),
            description: "One line.".into(),
            tone_rules: "Plain.".into(),
            emoji: Emoji::None,
            verbosity: Verbosity::Normal,
        };
        let pirate = personality::builtin("pirate").name;
        assert_eq!(
            save(&cfg, edit("pirate", "Mine")),
            Err(format!(
                "{pirate} is a built-in personality; duplicate it to make your own."
            ))
        );
        assert_eq!(
            delete(&cfg, "pirate"),
            Err("Built-in personalities can't be deleted.".to_string())
        );
        assert_eq!(
            save(&cfg, edit("nobody", "Mine")),
            Err("No personality called nobody.".to_string())
        );
        let id = duplicate(&cfg, "default").unwrap();
        assert_eq!(id, "my-voice");
        assert_eq!(
            save(&cfg, edit(&id, "")),
            Err("Personality my-voice: the name must be 1 to 32 characters.".to_string())
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn stored_ids_keep_the_default_empty() {
        assert_eq!(stored_id("default"), "");
        assert_eq!(stored_id(" pirate "), "pirate");
        assert_eq!(shown_id(""), "default");
    }

    #[test]
    fn the_json_is_camel_case_with_lowercase_enums() {
        let p = Preset {
            id: "a".into(),
            name: "A".into(),
            description: "d".into(),
            sample: String::new(),
            tone_rules: "t".into(),
            emoji: Emoji::Light,
            verbosity: Verbosity::Chatty,
            explicit: false,
            custom: true,
        };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({
                "id": "a", "name": "A", "description": "d", "sample": "", "toneRules": "t",
                "emoji": "light", "verbosity": "chatty", "explicit": false, "custom": true,
            })
        );
        let e: Edit = serde_json::from_value(serde_json::json!({
            "id": "a", "name": "A", "description": "d", "toneRules": "t",
            "emoji": "none", "verbosity": "terse",
        }))
        .unwrap();
        assert_eq!((e.emoji, e.verbosity), (Emoji::None, Verbosity::Terse));
    }
}
