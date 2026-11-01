//! Nyx's wardrobe as plain data: every picture a window can show for a
//! voice, as a path from the repo root. Nyx changes with what she is doing
//! (a set of poses per mood); every other built-in voice is Nyx in that
//! voice's outfit, the same set in every mood. Explicit outfits (Viper's
//! onesie, the raunchy outfit) show only with adults-only personalities on.
//!
//! The cabin embeds the same pictures (`loadout_home/nyx_wardrobe.rs`, kept
//! in step by a test there); the shell serves them from its bundled UI.

use std::collections::HashMap;

use super::{nyx, Mood};
use crate::personality::DEFAULT_ID;

/// The explicit built-in voices and the outfit each wears over every mood.
pub const EXPLICIT_VOICES: &[(&str, Mood)] = &[("viper", Mood::Viper), ("whore", Mood::Raunchy)];

/// `poses!("actions/coding": 2, 3; "actions/working": 1)`: those numbered
/// pictures under `brand/nyx/`, in that order.
macro_rules! poses {
    ($($dir:literal: $($n:literal),+);+ $(;)?) => {
        &[$($(concat!("brand/nyx/", $dir, "/", $n, ".jpg")),+),+]
    };
}

/// Each mood's poses after its own picture (the companion file's outfit for
/// that mood). A mood with no row here shows its own picture alone.
const MOOD_EXTRAS: &[(Mood, &[&str])] = &[
    (
        Mood::Working,
        poses!(
            "actions/working": 1;
            "actions/coding": 2;
            "actions/working": 2;
            "actions/coding": 3;
            "actions/working": 3;
            "actions/coding": 4;
            "actions/working": 4;
            "actions/coding": 5;
            "actions/working": 5;
        ),
    ),
    (Mood::Doctor, poses!("actions/fixing": 2, 3, 4, 5)),
    (Mood::Gaming, poses!("actions/gaming": 2, 3, 4, 5)),
    (Mood::Reading, poses!("actions/reading": 2, 3, 4, 5)),
    (Mood::Thinking, poses!("actions/thinking": 2, 3, 4, 5)),
    (Mood::Writing, poses!("actions/writing": 2, 3, 4, 5)),
    (Mood::HangingOut, poses!("defaults/nyx": 4, 5)),
    (Mood::Viper, poses!("defaults/viper": 2, 3, 4, 5)),
    (Mood::Raunchy, poses!("defaults/whore": 1, 2, 3, 4, 5)),
];

/// Every built-in voice but Nyx (who follows her mood) and the explicit
/// ones (whose outfit is a mood, [`EXPLICIT_VOICES`]), with its poses.
const OUTFITS: &[(&str, &[&str])] = &[
    ("bard", poses!("defaults/bard": 1, 2, 3, 4, 5)),
    ("butler", poses!("defaults/butler": 1, 2, 3, 4, 5)),
    (
        "chief-engineer",
        poses!("defaults/chief-engineer": 1, 2, 3, 4, 5),
    ),
    ("concise-pro", poses!("defaults/concise-pro": 1, 2, 3, 4, 5)),
    (
        "drill-sergeant",
        poses!("defaults/drill-sergeant": 1, 2, 3, 4, 5),
    ),
    (
        "friendly-neighbor",
        poses!("defaults/friendly-neighbor": 1, 2, 3, 4, 5),
    ),
    ("genie", poses!("defaults/genie": 1, 2, 3, 4, 5)),
    (
        "gentle-listener",
        poses!("defaults/gentle-listener": 1, 2, 3, 4, 5),
    ),
    ("hype-coach", poses!("defaults/hype-coach": 1, 2, 3, 4, 5)),
    (
        "lawyer-precise",
        poses!("defaults/lawyer-precise": 1, 2, 3, 4, 5),
    ),
    (
        "mission-control",
        poses!("defaults/mission-control": 1, 2, 3, 4, 5),
    ),
    (
        "noir-detective",
        poses!("defaults/noir-detective": 1, 2, 3, 4, 5),
    ),
    ("pirate", poses!("defaults/pirate": 1, 2, 3, 4, 5)),
    ("professor", poses!("defaults/professor": 1, 2, 3, 4, 5)),
    (
        "retro-hacker",
        poses!("defaults/retro-hacker": 1, 2, 3, 4, 5),
    ),
    (
        "sarcastic-sidekick",
        poses!("defaults/sarcastic-sidekick": 1, 2, 3, 4, 5),
    ),
    (
        "southern-mechanic",
        poses!("defaults/southern-mechanic": 1, 2, 3, 4, 5),
    ),
    ("surfer", poses!("defaults/surfer": 1, 2, 3, 4, 5)),
    ("zen-mentor", poses!("defaults/zen-mentor": 1, 2, 3, 4, 5)),
];

/// A set of poses and what it is called. `set` keys the pose rotation: a
/// mood's slug for Nyx's sets, the voice id for an outfit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub set: &'static str,
    pub explicit: bool,
    pub poses: Vec<&'static str>,
}

/// The moods that are outfits for explicit voices.
fn is_explicit(mood: Mood) -> bool {
    matches!(mood, Mood::Viper | Mood::Raunchy)
}

/// Nyx's poses for `mood`: its own picture from the companion file first,
/// then the extras.
pub fn mood_look(mood: Mood) -> Look {
    let extras = MOOD_EXTRAS
        .iter()
        .find(|(m, _)| *m == mood)
        .map_or(&[][..], |(_, poses)| *poses);
    let own: &'static str = nyx().outfit_for(mood, true).file.as_str();
    let mut poses = vec![own];
    poses.extend_from_slice(extras);
    Look {
        set: mood.slug(),
        explicit: is_explicit(mood),
        poses,
    }
}

/// The outfit a non-explicit built-in voice wears in every mood. Nyx, the
/// explicit voices and custom voices have none.
pub fn outfit_look(id: &str) -> Option<Look> {
    OUTFITS
        .iter()
        .find(|(o, _)| *o == id)
        .map(|(o, poses)| Look {
            set: o,
            explicit: false,
            poses: poses.to_vec(),
        })
}

/// The voice is drawn as Nyx in its own outfit.
pub fn has_outfit(id: &str) -> bool {
    EXPLICIT_VOICES.iter().any(|(v, _)| *v == id) || OUTFITS.iter().any(|(o, _)| *o == id)
}

/// The outfit a voice wears over every mood. Only explicit voices have one,
/// and only while adults-only personalities are on: Viper's snake onesie,
/// else the raunchy outfit (custom explicit copies included).
pub fn voice_outfit(id: &str, explicit: bool, adults_only: bool) -> Option<Mood> {
    if !(explicit && adults_only) {
        None
    } else if id == "viper" {
        Some(Mood::Viper)
    } else {
        Some(Mood::Raunchy)
    }
}

/// What a dock shows for voice `id` in `mood`: an explicit outfit mood's
/// set, else the voice's own outfit set, else Nyx's mood set. None for
/// voices with no art, and for explicit art while `explicit_ok` is off.
pub fn dock_look(id: &str, mood: Mood, explicit_ok: bool) -> Option<Look> {
    let look = if is_explicit(mood) {
        mood_look(mood)
    } else if id != DEFAULT_ID && !has_outfit(id) {
        return None;
    } else {
        outfit_look(id).unwrap_or_else(|| mood_look(mood))
    };
    (!look.explicit || explicit_ok).then_some(look)
}

/// The picker's picture for voice `id`: its `defaults/<id>/1` pose, else
/// the first pose of its set (Nyx's hanging-out picture, Viper's onesie).
/// None for voices with no art, and for explicit art while `explicit_ok` is
/// off.
pub fn thumbnail(id: &str, explicit_ok: bool) -> Option<&'static str> {
    let explicit = EXPLICIT_VOICES.iter().find(|(v, _)| *v == id);
    let look = if id == DEFAULT_ID {
        mood_look(Mood::HangingOut)
    } else if let Some((_, mood)) = explicit {
        mood_look(*mood)
    } else {
        outfit_look(id)?
    };
    if look.explicit && !explicit_ok {
        return None;
    }
    let own = format!("brand/nyx/defaults/{id}/1.jpg");
    look.poses
        .iter()
        .find(|p| **p == own)
        .or(look.poses.first())
        .copied()
}

/// Words in a tool title that mean looking after the PC itself.
const DOCTOR_WORDS: &[&str] = &[
    "bsod",
    "chkdsk",
    "cleanup",
    "crash",
    "diagnose",
    "diagnostics",
    "dism",
    "doctor",
    "driver",
    "drivers",
    "fan",
    "fans",
    "repair",
    "sfc",
    "temperature",
    "temperatures",
    "temps",
    "thermal",
    "troubleshoot",
];

/// Words in a tool title that mean reading or looking something up.
const READING_WORDS: &[&str] = &[
    "browse",
    "docs",
    "documentation",
    "fetch",
    "lookup",
    "news",
    "research",
    "search",
    "url",
    "web",
    "wiki",
];

/// Words in a tool title that mean writing prose. Plain file writes and
/// edits stay Working.
const WRITING_WORDS: &[&str] = &[
    "compose", "draft", "essay", "journal", "letter", "memo", "notes", "poem", "prose", "story",
];

/// The mood for a running tool, by the words in its title. PC care wins
/// over looking things up, then writing prose; anything else is work at
/// the desk.
pub fn tool_mood(title: &str) -> Mood {
    let lower = title.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
    if has(DOCTOR_WORDS) {
        Mood::Doctor
    } else if has(READING_WORDS) || words.iter().any(|w| w.starts_with("http")) {
        Mood::Reading
    } else if has(WRITING_WORDS) {
        Mood::Writing
    } else {
        Mood::Working
    }
}

/// Which pose of a set shows. The first time a set comes on it shows its
/// first pose; each later time the set or the mood changes, that set moves
/// on one pose, wrapping at the end. While the set and mood hold, the pose
/// holds.
#[derive(Clone, Debug, Default)]
pub struct PoseCycle {
    /// The set and mood on show and the pose picked for them.
    shown: Option<(String, Mood, usize)>,
    /// Times each set has come on so far.
    turns: HashMap<String, usize>,
}

impl PoseCycle {
    /// The index of the pose to show from a `len`-pose `set` in `mood`.
    pub fn pick(&mut self, set: &str, mood: Mood, len: usize) -> usize {
        let len = len.max(1);
        if let Some((s, m, i)) = &self.shown {
            if s == set && *m == mood {
                return i % len;
            }
        }
        let turn = self.turns.entry(set.to_string()).or_insert(0);
        let i = *turn % len;
        *turn += 1;
        self.shown = Some((set.to_string(), mood, i));
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::personality;
    use std::path::Path;

    fn every_look() -> Vec<Look> {
        let mut all: Vec<Look> = Mood::ALL.iter().map(|m| mood_look(*m)).collect();
        all.extend(OUTFITS.iter().filter_map(|(o, _)| outfit_look(o)));
        all
    }

    fn is_explicit_art(path: &str) -> bool {
        path == "brand/nyx-viper.jpg"
            || path == "brand/nyx-raunchy.jpg"
            || path.starts_with("brand/nyx/defaults/viper/")
            || path.starts_with("brand/nyx/defaults/whore/")
    }

    #[test]
    fn every_table_path_is_a_picture_in_the_repo_and_shows_once() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut paths = Vec::new();
        for look in every_look() {
            for path in &look.poses {
                assert!(path.starts_with("brand/nyx"), "{path}");
                assert!(path.ends_with(".jpg"), "{path}");
                assert!(root.join(path).is_file(), "{path} is missing");
                paths.push(*path);
            }
        }
        // 49 mood poses (Viper's and the raunchy set among them) and 95
        // outfit poses, the cabin's count.
        assert_eq!(paths.len(), 144);
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), 144, "every pose is its own picture");
    }

    #[test]
    fn each_mood_starts_with_the_companion_files_picture() {
        let counts: Vec<(Mood, usize)> = Mood::ALL
            .iter()
            .map(|m| (*m, mood_look(*m).poses.len()))
            .collect();
        assert_eq!(
            counts,
            vec![
                (Mood::Working, 10),
                (Mood::Gaming, 5),
                (Mood::Doctor, 5),
                (Mood::Reading, 5),
                (Mood::HangingOut, 3),
                (Mood::Viper, 5),
                (Mood::Thinking, 5),
                (Mood::Writing, 5),
                (Mood::Raunchy, 6),
            ]
        );
        let working = mood_look(Mood::Working);
        assert_eq!(working.set, "working");
        assert_eq!(working.poses[0], "brand/nyx-working.jpg");
        assert_eq!(working.poses[1], "brand/nyx/actions/working/1.jpg");
        assert_eq!(working.poses[2], "brand/nyx/actions/coding/2.jpg");
        assert_eq!(working.poses[9], "brand/nyx/actions/working/5.jpg");
        assert_eq!(
            mood_look(Mood::Thinking).poses[0],
            "brand/nyx/actions/thinking/1.jpg"
        );
        assert_eq!(
            mood_look(Mood::HangingOut).poses,
            vec![
                "brand/nyx-hanging-out.jpg",
                "brand/nyx/defaults/nyx/4.jpg",
                "brand/nyx/defaults/nyx/5.jpg"
            ]
        );
        assert!(mood_look(Mood::Viper).explicit);
        assert!(mood_look(Mood::Raunchy).explicit);
        assert!(!mood_look(Mood::Working).explicit);
    }

    #[test]
    fn tool_titles_pick_the_mood() {
        assert_eq!(tool_mood("web_fetch https://example.com/a"), Mood::Reading);
        assert_eq!(tool_mood("fetch https://example.com"), Mood::Reading);
        assert_eq!(tool_mood("open https://example.com"), Mood::Reading);
        assert_eq!(tool_mood("cleanup disk"), Mood::Doctor);
        assert_eq!(tool_mood("Check fan temps"), Mood::Doctor);
        assert_eq!(tool_mood("search the web for drivers"), Mood::Doctor);
        assert_eq!(tool_mood("draft_letter"), Mood::Writing);
        assert_eq!(tool_mood("write_file src/main.rs"), Mood::Working);
        assert_eq!(tool_mood("bash cargo test"), Mood::Working);
        assert_eq!(tool_mood(""), Mood::Working);
    }

    #[test]
    fn nyx_follows_her_mood_and_outfit_voices_keep_their_outfit() {
        let set = |id, mood| dock_look(id, mood, true).map(|l| l.set);
        assert_eq!(set("nyx", Mood::Working), Some("working"));
        assert_eq!(set("nyx", Mood::Thinking), Some("thinking"));
        assert_eq!(set("nyx", Mood::Reading), Some("reading"));
        assert_eq!(set("nyx", Mood::HangingOut), Some("hanging-out"));
        assert_eq!(set("pirate", Mood::Working), Some("pirate"));
        assert_eq!(set("pirate", Mood::Doctor), Some("pirate"));
        assert_eq!(set("zen-mentor", Mood::Thinking), Some("zen-mentor"));
        assert_eq!(set("whore", Mood::Raunchy), Some("raunchy"));
        assert_eq!(set("whore-copy", Mood::Raunchy), Some("raunchy"));
        assert_eq!(set("viper", Mood::Viper), Some("viper"));
        // With no outfit from the gate, an explicit voice shows Nyx's mood.
        assert_eq!(set("viper", Mood::Working), Some("working"));
        assert_eq!(set("pirate-copy", Mood::Working), None, "custom voices");
        assert_eq!(set("", Mood::Working), None);
    }

    #[test]
    fn explicit_voices_give_nothing_explicit_without_the_gate() {
        assert_eq!(voice_outfit("viper", true, false), None);
        assert_eq!(voice_outfit("whore", true, false), None);
        assert_eq!(voice_outfit("viper", true, true), Some(Mood::Viper));
        assert_eq!(voice_outfit("whore", true, true), Some(Mood::Raunchy));
        assert_eq!(voice_outfit("whore-copy", true, true), Some(Mood::Raunchy));
        assert_eq!(voice_outfit("pirate", false, true), None);

        assert_eq!(dock_look("whore", Mood::Raunchy, false), None);
        assert_eq!(dock_look("viper", Mood::Viper, false), None);
        assert_eq!(dock_look("nyx", Mood::Viper, false), None);
        assert_eq!(dock_look("pirate", Mood::Raunchy, false), None);
        assert_eq!(thumbnail("whore", false), None);
        assert_eq!(thumbnail("viper", false), None);

        let mut ids: Vec<String> = personality::builtins().into_iter().map(|p| p.id).collect();
        ids.extend(["whore-copy".to_string(), String::new()]);
        for id in &ids {
            for mood in Mood::ALL {
                if let Some(look) = dock_look(id, mood, false) {
                    assert!(!look.explicit, "{id} {mood:?}");
                    for path in &look.poses {
                        assert!(!is_explicit_art(path), "{id} shows {path}");
                    }
                }
            }
            if let Some(path) = thumbnail(id, false) {
                assert!(!is_explicit_art(path), "{id} thumbnail {path}");
            }
        }
        // With the gate open they do show.
        assert_eq!(
            thumbnail("whore", true),
            Some("brand/nyx/defaults/whore/1.jpg")
        );
        assert_eq!(thumbnail("viper", true), Some("brand/nyx-viper.jpg"));
    }

    #[test]
    fn every_built_in_voice_but_nyx_has_an_outfit() {
        let builtins = personality::builtins();
        assert_eq!(builtins.len(), 22);
        for p in &builtins {
            if p.id == DEFAULT_ID {
                assert!(!has_outfit(&p.id));
                continue;
            }
            assert!(has_outfit(&p.id), "{}", p.id);
            if p.explicit {
                assert_eq!(outfit_look(&p.id), None, "{}", p.id);
                assert!(
                    EXPLICIT_VOICES.iter().any(|(v, _)| *v == p.id),
                    "{} is not listed as explicit",
                    p.id
                );
            } else {
                let look = outfit_look(&p.id).unwrap_or_else(|| panic!("{} has no outfit", p.id));
                assert_eq!(look.poses.len(), 5, "{}", p.id);
            }
        }
        for (id, _) in OUTFITS {
            assert!(builtins.iter().any(|p| p.id == *id), "{id} is a preset");
        }
    }

    #[test]
    fn the_picker_shows_each_voices_first_pose() {
        assert_eq!(
            thumbnail("pirate", false),
            Some("brand/nyx/defaults/pirate/1.jpg")
        );
        assert_eq!(thumbnail("nyx", false), Some("brand/nyx-hanging-out.jpg"));
        assert_eq!(thumbnail("pirate-copy", true), None);
        assert_eq!(thumbnail("", true), None);
    }

    #[test]
    fn a_set_moves_on_one_pose_each_time_the_mood_changes() {
        let mut cycle = PoseCycle::default();
        assert_eq!(cycle.pick("pirate", Mood::HangingOut, 5), 0);
        assert_eq!(cycle.pick("pirate", Mood::HangingOut, 5), 0);
        assert_eq!(cycle.pick("pirate", Mood::Thinking, 5), 1);
        assert_eq!(cycle.pick("pirate", Mood::Thinking, 5), 1);
        assert_eq!(cycle.pick("pirate", Mood::Working, 5), 2);
        assert_eq!(cycle.pick("working", Mood::Working, 10), 0);
        assert_eq!(cycle.pick("thinking", Mood::Thinking, 5), 0);
        assert_eq!(cycle.pick("working", Mood::Working, 10), 1);
        assert_eq!(cycle.pick("pirate", Mood::Working, 5), 3);
        assert_eq!(cycle.pick("pirate", Mood::Doctor, 5), 4);
        assert_eq!(cycle.pick("pirate", Mood::HangingOut, 5), 0);
        assert_eq!(cycle.pick("one", Mood::Reading, 1), 0);
        assert_eq!(cycle.pick("one", Mood::Doctor, 1), 0);
    }
}
