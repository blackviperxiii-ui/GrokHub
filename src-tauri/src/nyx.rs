//! Nyx in the shell window: which of her pictures the chat's face and dock
//! show for the voice in use and what the chat is doing. The pictures are
//! the cabin's (`grokhub_core::companion::wardrobe`), bundled with the UI;
//! this only names them. Explicit art comes back only while Settings →
//! Personality → Adults-only personalities is on.

use std::collections::HashMap;
use std::path::Path;

use grokhub_core::companion::{wardrobe, Mood};
use grokhub_core::personality as presets;
use serde::Serialize;

use crate::personality;

/// One set of poses, each a path from the repo root
/// (`brand/nyx/actions/coding/2.jpg`). `set` names the set, so the UI moves
/// to the next pose only when the set or `mood`, what the chat is doing
/// under any outfit, changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NyxLook {
    pub set: String,
    pub poses: Vec<String>,
    pub explicit: bool,
    pub mood: Mood,
}

/// What the chat is doing, as a mood: a running tool's title picks Working,
/// Doctor, Reading or Writing; otherwise the mood the UI sent, when it is
/// one the chat can be in. The shell watches no games, so Gaming (and the
/// outfit-only moods, which come from the voice) read as Hanging out.
pub fn activity(mood: Option<Mood>, tool: Option<&str>) -> Mood {
    if let Some(title) = tool {
        return wardrobe::tool_mood(title);
    }
    match mood {
        Some(
            m @ (Mood::Working | Mood::Doctor | Mood::Reading | Mood::Writing | Mood::Thinking),
        ) => m,
        _ => Mood::HangingOut,
    }
}

/// The set for the stored voice doing `activity`. An explicit voice wears
/// its outfit only with `explicit_on`; with it off the voice reads as the
/// default one, so Nyx shows in her own mood. None for a voice with no art.
pub fn look(config_dir: &Path, stored: &str, explicit_on: bool, activity: Mood) -> Option<NyxLook> {
    let voice = presets::gated(
        presets::lookup(&personality::dir(config_dir), personality::shown_id(stored)),
        explicit_on,
    );
    let mood = wardrobe::voice_outfit(&voice.id, voice.explicit, explicit_on).unwrap_or(activity);
    wardrobe::dock_look(&voice.id, mood, explicit_on).map(|l| NyxLook {
        set: l.set.to_string(),
        poses: l.poses.iter().map(|p| p.to_string()).collect(),
        explicit: l.explicit,
        mood: activity,
    })
}

/// Settings → Personality's picture for each of `ids` that has one.
pub fn tiles(ids: &[String], explicit_on: bool) -> HashMap<String, String> {
    ids.iter()
        .filter_map(|id| {
            wardrobe::thumbnail(id, explicit_on).map(|path| (id.clone(), path.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-nyx-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn is_explicit_art(path: &str) -> bool {
        path == "brand/nyx-viper.jpg"
            || path == "brand/nyx-raunchy.jpg"
            || path.starts_with("brand/nyx/defaults/viper/")
            || path.starts_with("brand/nyx/defaults/whore/")
    }

    #[test]
    fn the_chat_picks_the_mood_and_never_gaming() {
        assert_eq!(
            activity(None, Some("web_fetch https://example.com")),
            Mood::Reading
        );
        assert_eq!(
            activity(Some(Mood::Thinking), Some("cleanup disk")),
            Mood::Doctor
        );
        assert_eq!(activity(Some(Mood::Thinking), None), Mood::Thinking);
        assert_eq!(activity(Some(Mood::Working), None), Mood::Working);
        assert_eq!(activity(None, None), Mood::HangingOut);
        assert_eq!(activity(Some(Mood::Gaming), None), Mood::HangingOut);
        assert_eq!(activity(Some(Mood::Raunchy), None), Mood::HangingOut);
        assert_eq!(activity(Some(Mood::Viper), None), Mood::HangingOut);
    }

    #[test]
    fn nyx_follows_the_chat_and_outfit_voices_keep_their_outfit() {
        let dir = temp_dir("follow");
        let nyx = look(&dir, "", false, Mood::Working).expect("Nyx has art");
        assert_eq!(nyx.set, "working");
        assert_eq!(nyx.poses.len(), 10);
        assert_eq!(nyx.poses[0], "brand/nyx-working.jpg");
        assert!(!nyx.explicit);
        assert_eq!(nyx.mood, Mood::Working);
        assert_eq!(
            look(&dir, "nyx", false, Mood::Thinking).map(|l| l.set),
            Some("thinking".to_string())
        );
        let pirate = look(&dir, "pirate", false, Mood::Reading).expect("pirate art");
        assert_eq!(pirate.set, "pirate");
        assert_eq!(pirate.poses[0], "brand/nyx/defaults/pirate/1.jpg");
        // A voice whose file is gone speaks as Nyx, so Nyx shows.
        assert_eq!(
            look(&dir, "no-such-voice", true, Mood::Working).map(|l| l.set),
            Some("working".to_string())
        );
        // A voice of the person's own has no art.
        let copy = personality::duplicate(&dir, "pirate").expect("copy");
        assert_eq!(copy, "pirate-copy");
        assert_eq!(look(&dir, &copy, true, Mood::Working), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nyx_look_hides_explicit_art_with_adults_only_off() {
        let dir = temp_dir("gate");
        for voice in ["whore", "viper"] {
            for mood in Mood::ALL.map(|m| activity(Some(m), None)) {
                let shown = look(&dir, voice, false, mood).expect("Nyx shows instead");
                assert!(!shown.explicit, "{voice} {mood:?}");
                for path in &shown.poses {
                    assert!(!is_explicit_art(path), "{voice} shows {path}");
                }
            }
        }
        assert_eq!(
            look(&dir, "whore", false, Mood::HangingOut).map(|l| l.set),
            Some("hanging-out".to_string())
        );
        // With the gate on, the outfit holds whatever the chat does.
        let on = look(&dir, "whore", true, Mood::Working).expect("explicit art");
        assert_eq!(on.set, "raunchy");
        assert!(on.explicit);
        assert_eq!(on.mood, Mood::Working, "the activity under the outfit");
        assert_eq!(
            look(&dir, "viper", true, Mood::Reading).map(|l| l.set),
            Some("viper".to_string())
        );
    }

    #[test]
    fn tiles_name_each_voices_picture_and_skip_explicit_ones_with_the_gate_off() {
        let ids: Vec<String> = ["nyx", "pirate", "whore", "viper", "pirate-copy"]
            .map(String::from)
            .to_vec();
        let off = tiles(&ids, false);
        assert_eq!(off.len(), 2);
        assert_eq!(off["nyx"], "brand/nyx-hanging-out.jpg");
        assert_eq!(off["pirate"], "brand/nyx/defaults/pirate/1.jpg");
        let on = tiles(&ids, true);
        assert_eq!(on.len(), 4);
        assert_eq!(on["whore"], "brand/nyx/defaults/whore/1.jpg");
        assert_eq!(on["viper"], "brand/nyx-viper.jpg");
    }
}
