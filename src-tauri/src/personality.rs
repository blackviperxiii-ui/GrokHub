//! Settings → Personality (shell parity): the cabin's presets and the
//! person's own in `{config}/personalities/`, the same files the cabin reads.
//! A personality sets tone only; the safety rules in the stable prompt win.

use std::path::{Path, PathBuf};

use grokhub_agent::voice_session::VoiceConfig;
use grokhub_core::personality::{self, Emoji, Heat, Personality, Verbosity};
use serde::{Deserialize, Serialize};

use crate::settings::ShellSettings;

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
    /// Adults-only personalities: off leaves the explicit presets out of `presets`.
    /// On only once the 18+ check was answered yes.
    pub explicit: bool,
    /// The switch was turned on before the 18+ check: nothing changed, and the
    /// page asks the check first.
    pub age_check: bool,
    /// Settings → Personality → Heat, shown while `explicit` is on.
    pub heat: Heat,
    /// Settings → Personality → Dynamic chat.
    pub dynamic_chat: bool,
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

/// The id a stored field stands for: empty, or the retired "default" preset,
/// is the default voice.
pub fn shown_id(stored: &str) -> &str {
    match stored.trim() {
        "" | personality::LEGACY_DEFAULT_ID => personality::DEFAULT_ID,
        id => id,
    }
}

/// How an id is stored, so the default voice stays an empty field like the cabin's.
pub fn stored_id(id: &str) -> String {
    match id.trim() {
        personality::DEFAULT_ID | personality::LEGACY_DEFAULT_ID => String::new(),
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

pub fn load(config_dir: &Path, settings: &ShellSettings) -> Page {
    let explicit_on = settings.adults_on();
    let (all, problems) = personality::load_all(&dir(config_dir));
    let shown = personality::visible(&all, explicit_on);
    let current = shown_id(&settings.personality);
    // A default whose file was deleted in the cabin, or an explicit one while
    // adults-only personalities are off, falls back to the default voice.
    let current = if shown.iter().any(|p| p.id == current) {
        current.to_string()
    } else {
        personality::DEFAULT_ID.to_string()
    };
    Page {
        presets: shown.into_iter().map(preset).collect(),
        current,
        problems,
        explicit: explicit_on,
        age_check: false,
        heat: settings.quip_heat,
        dynamic_chat: settings.dynamic_chat,
    }
}

/// Refuses an explicit preset while adults-only personalities are off.
pub fn allowed(p: Personality, explicit_on: bool) -> Result<Personality, String> {
    if p.explicit && !explicit_on {
        Err(format!(
            "Turn on Adults-only personalities to use {}.",
            p.name
        ))
    } else {
        Ok(p)
    }
}

/// The preset for `id`, refused when no preset has it.
pub fn pick(config_dir: &Path, id: &str) -> Result<Personality, String> {
    let (all, _) = personality::load_all(&dir(config_dir));
    let want = shown_id(id);
    all.into_iter()
        .find(|p| p.id == want)
        .ok_or_else(|| format!("No personality called {id}."))
}

/// The voice every chat speaks in: the stored default, or the default voice
/// while it is an explicit preset and adults-only is off.
fn home(config_dir: &Path, settings: &ShellSettings) -> Personality {
    let p = personality::lookup(&dir(config_dir), shown_id(&settings.personality));
    personality::gated(p, settings.adults_on())
}

/// The xAI voice spoken replies use: the stored default's voice hint when it
/// names a voice xAI has, else the default voice (as the cabin picks it).
pub fn speech_voice(config_dir: &Path, settings: &ShellSettings) -> &'static str {
    grokhub_core::tts_voice(home(config_dir, settings).voice_hint.as_deref())
}

/// The block a chat's system prompt carries for the stored default. With 18+
/// on, every voice jokes at the chosen heat. Empty only when that preset's
/// tone rules are empty. The safety closer comes after the whole volatile
/// tail (`agent::volatile_tail`), not here.
pub fn voice(config_dir: &Path, settings: &ShellSettings) -> String {
    home(config_dir, settings).prompt_block_for(settings.adult_heat())
}

/// A chat's voice settings for the agent's per-turn voice flow: the default
/// voice as home, every preset but the explicit ones as the voices dynamic
/// chat may hand to, and the heat. The shell has no per-chat pick, so no chat
/// is pinned.
pub fn voice_config(
    config_dir: &Path,
    settings: &ShellSettings,
    utc_offset_min: i32,
) -> VoiceConfig {
    let (all, _) = personality::load_all(&dir(config_dir));
    VoiceConfig {
        home: home(config_dir, settings),
        // Adults-only voices join only with adults-only on, and then speak
        // only for 18+ turns.
        roster: all
            .into_iter()
            .filter(|p| settings.adult_heat().is_some() || !p.explicit)
            .collect(),
        dynamic: settings.dynamic_chat,
        pinned: false,
        adult: settings.adult_heat(),
        utc_offset_min,
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

    /// Settings with `stored` as the default voice, adults-only on (and the
    /// 18+ check answered) or off.
    fn with(stored: &str, adults: bool) -> ShellSettings {
        ShellSettings {
            personality: stored.to_string(),
            explicit_personalities: adults,
            adult_confirmed: adults,
            ..ShellSettings::default()
        }
    }

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
        let page = load(&cfg, &with("", false));
        assert_eq!(page.current, "nyx");
        assert_eq!(page.presets[0].id, "nyx");
        assert_eq!(page.presets[0].name, grokhub_core::brand::companion_name());
        assert!(page.presets.iter().all(|p| p.id != "default"));
        // A profile saved on the retired "default" preset shows Nyx in use.
        assert_eq!(load(&cfg, &with("default", false)).current, "nyx");
        assert!(page.presets.iter().any(|p| p.id == "pirate" && !p.custom));
        assert_eq!(page.problems, Vec::<String>::new());
        assert_eq!(load(&cfg, &with("pirate", false)).current, "pirate");
        assert_eq!(load(&cfg, &with("gone-voice", false)).current, "nyx");
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn adults_only_off_leaves_explicit_presets_out_and_mutes_a_stored_pick() {
        let cfg = scratch("adults");
        let explicit = |page: &Page| {
            page.presets
                .iter()
                .filter(|p| p.explicit)
                .map(|p| p.id.clone())
                .collect::<Vec<_>>()
        };
        let off = load(&cfg, &with("whore", false));
        assert!(!off.explicit);
        assert_eq!(explicit(&off), Vec::<String>::new());
        assert_eq!(off.current, "nyx");
        let on = load(&cfg, &with("whore", true));
        assert!(on.explicit);
        assert_eq!(explicit(&on), ["viper", "whore"]);
        assert_eq!(on.current, "whore");
        assert_eq!(
            voice(&cfg, &with("whore", false)),
            voice(&cfg, &with("", false))
        );
        assert!(voice(&cfg, &with("whore", true)).starts_with("Personality: Whore. Whore mode:"));
        let whore = pick(&cfg, "whore").unwrap();
        assert_eq!(
            allowed(whore.clone(), false).unwrap_err(),
            "Turn on Adults-only personalities to use Whore."
        );
        assert_eq!(allowed(whore, true).unwrap().id, "whore");
        assert_eq!(
            allowed(pick(&cfg, "pirate").unwrap(), false).unwrap().id,
            "pirate"
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn the_default_voice_adds_its_values_and_a_preset_adds_its_block_without_the_closer() {
        let cfg = scratch("voice");
        let default_voice = voice(&cfg, &with("", false));
        assert!(default_voice.starts_with("Personality: Nyx. "));
        assert!(default_voice.contains("not sycophantic"));
        // The closer rides once, at the end of the whole tail (agent.rs).
        assert!(!default_voice.contains(grokhub_core::talk::VOLATILE_SAFETY_CLOSER));
        assert_eq!(voice(&cfg, &with("default", false)), default_voice);
        let pirate = voice(&cfg, &with("pirate", false));
        assert_eq!(pirate, personality::builtin("pirate").prompt_block());
        assert!(pirate.starts_with("Personality: "), "{pirate}");
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn every_voice_jokes_at_the_heat_only_with_18_plus_on() {
        let cfg = scratch("heat");
        let at = |heat: Heat, adults: bool, confirmed: bool| {
            voice(
                &cfg,
                &ShellSettings {
                    personality: "pirate".into(),
                    explicit_personalities: adults,
                    adult_confirmed: confirmed,
                    quip_heat: heat,
                    ..ShellSettings::default()
                },
            )
        };
        let plain = personality::builtin("pirate").prompt_block();
        // Off, or on without the 18+ check, is the plain block at any heat.
        assert_eq!(at(Heat::Filthy, false, false), plain);
        assert_eq!(at(Heat::Filthy, false, true), plain);
        assert_eq!(at(Heat::Filthy, true, false), plain);
        assert!(!plain.contains(personality::ADULT_HUMOR));
        let spicy = at(Heat::Spicy, true, true);
        assert!(spicy.contains(personality::ADULT_HUMOR), "{spicy}");
        assert!(spicy.contains(Heat::Spicy.line()), "{spicy}");
        assert!(!spicy.contains(Heat::Filthy.line()), "{spicy}");
        let cheeky = at(Heat::Cheeky, true, true);
        assert!(cheeky.contains(personality::ADULT_HUMOR));
        assert!(cheeky.contains(Heat::Cheeky.line()));
        assert!(!cheeky.contains(Heat::Spicy.line()));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn the_voice_config_brings_adults_only_voices_only_with_adults_only_on() {
        let cfg = scratch("session");
        let settings = ShellSettings {
            personality: "viper".into(),
            explicit_personalities: true,
            adult_confirmed: true,
            quip_heat: Heat::Spicy,
            ..ShellSettings::default()
        };
        let on = voice_config(&cfg, &settings, 120);
        assert_eq!(on.home.id, "viper");
        assert!(!on.roster.is_empty());
        // Adults-only on: the adults-only voice can take an 18+ turn.
        assert!(on.roster.iter().any(|p| p.id == "whore" && p.explicit));
        assert!(on.roster.iter().any(|p| p.id == "pirate"));
        assert!(on.dynamic && !on.pinned);
        assert_eq!(on.adult, Some(Heat::Spicy));
        assert_eq!(on.utc_offset_min, 120);
        // Adults-only off: the explicit default speaks as the default voice.
        let off = voice_config(
            &cfg,
            &ShellSettings {
                explicit_personalities: false,
                dynamic_chat: false,
                ..settings
            },
            0,
        );
        assert_eq!(off.home.id, "nyx");
        assert_eq!(off.adult, None);
        assert!(!off.dynamic);
        assert!(
            off.roster.iter().all(|p| !p.explicit),
            "no 18+ voice with it off"
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn the_page_carries_the_heat_and_dynamic_chat() {
        let cfg = scratch("page-flow");
        let page = load(&cfg, &with("", false));
        assert!(!page.age_check);
        assert_eq!(page.heat, Heat::Cheeky);
        assert!(page.dynamic_chat);
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["ageCheck"], false);
        assert_eq!(json["heat"], "cheeky");
        assert_eq!(json["dynamicChat"], true);
        // On without the 18+ check stays off on the page.
        let unconfirmed = ShellSettings {
            explicit_personalities: true,
            ..ShellSettings::default()
        };
        assert!(!load(&cfg, &unconfirmed).explicit);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn duplicate_edit_and_delete_a_custom_preset() {
        let cfg = scratch("custom");
        let id = duplicate(&cfg, "pirate").expect("duplicate");
        assert_eq!(id, "pirate-copy");
        assert!(dir(&cfg).join("pirate-copy.toml").exists());
        let page = load(&cfg, &with("", false));
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
        assert!(voice(&cfg, &with(&id, false))
            .starts_with("Personality: Deckhand. Talk like a deckhand."));

        delete(&cfg, &id).expect("delete");
        assert!(!dir(&cfg).join("pirate-copy.toml").exists());
        assert_eq!(load(&cfg, &with(&id, false)).current, "nyx");
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
        assert_eq!(id, "nyx-copy");
        assert_eq!(
            save(&cfg, edit(&id, "")),
            Err("Personality nyx-copy: the name must be 1 to 32 characters.".to_string())
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn stored_ids_keep_the_default_empty() {
        assert_eq!(stored_id("nyx"), "");
        assert_eq!(stored_id("default"), "");
        assert_eq!(stored_id(" pirate "), "pirate");
        assert_eq!(shown_id(""), "nyx");
        assert_eq!(shown_id("default"), "nyx");
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
