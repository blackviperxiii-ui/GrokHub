//! Nyx's wardrobe: every picture the corner box and the Personality picker
//! can show. Nyx changes with what she is doing (a set of poses per mood);
//! every other built-in voice is Nyx in that voice's outfit, the same set in
//! every mood. A set shows one pose at a time and moves on to the next pose
//! only when what the chat is doing changes, so nothing flickers. Explicit
//! outfits (Viper's onesie, the raunchy outfit) come only through
//! [`outfit_for`]'s adults-only gate, and are checked against it again here.
//!
//! The pictures are 448 x 350 crops of the originals in
//! `docs/design/loadout-baseline-v1/nyx-modes/`, kept under `brand/nyx/`.

use std::collections::HashMap;

use super::*;

/// The picker tile's picture is this many pixels square, enough for the
/// Selected card's 96 px icon on a 2x screen.
const THUMB_PX: u32 = 192;
/// ctx key for [`PoseCycle`].
const CYCLE: &str = "nyx-wardrobe-cycle";
/// ctx key for the dock's one decoded picture.
const DOCK_TEX: &str = "nyx-wardrobe-dock";
/// ctx key for this frame's explicit-art gate.
const EXPLICIT_OK: &str = "nyx-wardrobe-explicit-ok";
/// The default voice, who follows her mood instead of wearing an outfit.
/// Spelled out so chat-prompt code stays the only reader of personalities.
const NYX_ID: &str = "nyx";
/// The explicit built-in voices and the outfit [`outfit_for`] gives each.
const EXPLICIT_VOICES: &[(&str, Mood)] = &[("viper", Mood::Viper), ("whore", Mood::Raunchy)];

/// One picture, as the dock shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct Pose {
    /// Unique per picture; textures are cached under it.
    pub key: &'static str,
    pub jpg: &'static [u8],
}

/// `poses!("actions/coding": 2, 3; "actions/working": 1)`: those numbered
/// pictures under `brand/nyx/`, in that order.
macro_rules! poses {
    ($($dir:literal: $($n:literal),+);+ $(;)?) => {
        &[$($(Pose {
            key: concat!("nyx-", $dir, "/", $n),
            jpg: include_bytes!(concat!("../../../../../brand/nyx/", $dir, "/", $n, ".jpg")),
        }),+),+]
    };
}

/// Each mood's poses after its own picture ([`Mood::art`]). A mood with no
/// row here shows its own picture alone.
const MOOD_EXTRAS: &[(Mood, &[Pose])] = &[
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

/// A built-in voice's own outfit.
struct Outfit {
    id: &'static str,
    poses: &'static [Pose],
}

/// Every built-in voice but Nyx (who follows her mood) and the explicit ones
/// (whose outfit is a mood, [`EXPLICIT_VOICES`]).
const OUTFITS: &[Outfit] = &[
    Outfit {
        id: "bard",
        poses: poses!("defaults/bard": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "butler",
        poses: poses!("defaults/butler": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "chief-engineer",
        poses: poses!("defaults/chief-engineer": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "concise-pro",
        poses: poses!("defaults/concise-pro": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "drill-sergeant",
        poses: poses!("defaults/drill-sergeant": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "friendly-neighbor",
        poses: poses!("defaults/friendly-neighbor": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "genie",
        poses: poses!("defaults/genie": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "gentle-listener",
        poses: poses!("defaults/gentle-listener": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "hype-coach",
        poses: poses!("defaults/hype-coach": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "lawyer-precise",
        poses: poses!("defaults/lawyer-precise": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "mission-control",
        poses: poses!("defaults/mission-control": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "noir-detective",
        poses: poses!("defaults/noir-detective": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "pirate",
        poses: poses!("defaults/pirate": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "professor",
        poses: poses!("defaults/professor": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "retro-hacker",
        poses: poses!("defaults/retro-hacker": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "sarcastic-sidekick",
        poses: poses!("defaults/sarcastic-sidekick": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "southern-mechanic",
        poses: poses!("defaults/southern-mechanic": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "surfer",
        poses: poses!("defaults/surfer": 1, 2, 3, 4, 5),
    },
    Outfit {
        id: "zen-mentor",
        poses: poses!("defaults/zen-mentor": 1, 2, 3, 4, 5),
    },
];

/// A set of poses and what it is called. `set` keys the pose rotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct Look {
    pub set: &'static str,
    pub explicit: bool,
    pub poses: Vec<Pose>,
}

/// Nyx's poses for `mood`: its own picture first, then the extras.
pub(in crate::app) fn mood_look(mood: Mood) -> Look {
    let extras = MOOD_EXTRAS
        .iter()
        .find(|(m, _)| *m == mood)
        .map_or(&[][..], |(_, poses)| *poses);
    let mut poses = vec![Pose {
        key: mood.key(),
        jpg: mood.art(),
    }];
    poses.extend_from_slice(extras);
    Look {
        set: mood.key(),
        explicit: is_explicit(mood),
        poses,
    }
}

/// The moods that are outfits for explicit voices.
fn is_explicit(mood: Mood) -> bool {
    matches!(mood, Mood::Viper | Mood::Raunchy)
}

/// The outfit a non-explicit built-in voice wears in every mood. Nyx, the
/// explicit voices and custom voices have none.
fn outfit_look(id: &str) -> Option<Look> {
    OUTFITS.iter().find(|o| o.id == id).map(|o| Look {
        set: o.id,
        explicit: false,
        poses: o.poses.to_vec(),
    })
}

/// The voice is drawn as Nyx in its own outfit.
pub(in crate::app) fn has_outfit(id: &str) -> bool {
    EXPLICIT_VOICES.iter().any(|(v, _)| *v == id) || OUTFITS.iter().any(|o| o.id == id)
}

/// What the corner box shows for voice `id` in `mood`: an explicit outfit
/// mood's set ([`outfit_for`] already gated it), else the voice's own
/// outfit set, else Nyx's mood set. A deck pin (`pinned`) shows the mood's
/// set on any drawn voice, as the mode screenshots expect. None (the letter
/// tile) for voices with no art, and for explicit art while `explicit_ok`
/// is off.
pub(in crate::app) fn dock_look(
    id: &str,
    mood: Mood,
    pinned: bool,
    explicit_ok: bool,
) -> Option<Look> {
    let look = if is_explicit(mood) {
        mood_look(mood)
    } else if id != NYX_ID && !has_outfit(id) {
        return None;
    } else {
        match outfit_look(id) {
            Some(outfit) if !pinned => outfit,
            _ => mood_look(mood),
        }
    };
    (!look.explicit || explicit_ok).then_some(look)
}

/// The picker's picture for voice `id`: its `defaults/<id>/1` pose, else
/// the first pose of its set (Nyx's hanging-out picture, Viper's onesie).
/// None for voices with no art, and for explicit art while `explicit_ok` is
/// off.
pub(in crate::app) fn thumbnail(id: &str, explicit_ok: bool) -> Option<Pose> {
    let explicit = EXPLICIT_VOICES.iter().find(|(v, _)| *v == id);
    let look = if id == NYX_ID {
        mood_look(Mood::HangingOut)
    } else if let Some((_, mood)) = explicit {
        mood_look(*mood)
    } else {
        outfit_look(id)?
    };
    if look.explicit && !explicit_ok {
        return None;
    }
    let own = format!("nyx-defaults/{id}/1");
    look.poses
        .iter()
        .find(|p| p.key == own)
        .or(look.poses.first())
        .copied()
}

/// Which pose of a set shows. The first time a set comes on it shows its
/// first pose; each later time the set or the mood changes, that set moves
/// on one pose, wrapping at the end. While the set and mood hold, the pose
/// holds.
#[derive(Clone, Debug, Default)]
pub(in crate::app) struct PoseCycle {
    /// The set and mood on show and the pose picked for them.
    shown: Option<(&'static str, Mood, usize)>,
    /// Times each set has come on so far.
    turns: HashMap<&'static str, usize>,
}

impl PoseCycle {
    /// The index of the pose to show from a `len`-pose `set` in `mood`.
    pub(in crate::app) fn pick(&mut self, set: &'static str, mood: Mood, len: usize) -> usize {
        let len = len.max(1);
        if let Some((s, m, i)) = self.shown {
            if s == set && m == mood {
                return i % len;
            }
        }
        let turn = self.turns.entry(set).or_insert(0);
        let i = *turn % len;
        *turn += 1;
        self.shown = Some((set, mood, i));
        i
    }
}

/// The pose the dock shows now for `look` while the chat does `activity`
/// ([`Cabin::companion_activity`], the mood under any outfit).
pub(in crate::app) fn dock_pose(ctx: &egui::Context, look: &Look, activity: Mood) -> Option<Pose> {
    let i = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<PoseCycle>(egui::Id::new(CYCLE))
            .pick(look.set, activity, look.poses.len())
    });
    look.poses.get(i).copied()
}

/// The dock's texture for `pose`. Only the picture on show stays decoded,
/// so walking through the wardrobe never holds every picture in memory.
pub(in crate::app) fn dock_texture(ctx: &egui::Context, pose: Pose) -> Option<egui::TextureHandle> {
    type Cached = (&'static str, egui::TextureHandle);
    let id = egui::Id::new(DOCK_TEX);
    if let Some((key, tex)) = ctx.data(|d| d.get_temp::<Cached>(id)) {
        if key == pose.key {
            return Some(tex);
        }
    }
    let rgba = image::load_from_memory(pose.jpg).ok()?.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ctx.load_texture(
        pose.key,
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp::<Cached>(id, (pose.key, tex.clone())));
    Some(tex)
}

/// A [`THUMB_PX`] square from the middle of `pose`, for the picker's tiles.
pub(in crate::app) fn thumb_texture(
    ctx: &egui::Context,
    pose: Pose,
) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("nyx-wardrobe-thumb", pose.key));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(tex);
    }
    let img = image::load_from_memory(pose.jpg).ok()?;
    let side = img.width().min(img.height());
    let square = img.crop_imm((img.width() - side) / 2, 0, side, side);
    let rgba = square
        .resize_exact(THUMB_PX, THUMB_PX, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ctx.load_texture(
        format!("thumb-{}", pose.key),
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Hands this frame's explicit-art gate to painters that have no cabin.
pub(in crate::app) fn publish_explicit_ok(ctx: &egui::Context, ok: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(EXPLICIT_OK), ok));
}

/// This frame's explicit-art gate. Off until a frame publishes it.
pub(in crate::app) fn explicit_ok(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new(EXPLICIT_OK)))
        .unwrap_or(false)
}

impl Cabin {
    /// The one adults-only gate for explicit art: Settings → Adults-only
    /// personalities, the switch that shows explicit presets. It only turns
    /// on after the one-time 18+ check.
    pub(in crate::app) fn explicit_art_ok(&self) -> bool {
        self.cfg.explicit_personalities
    }

    /// What the corner box shows right now for the shown voice.
    pub(in crate::app) fn dock_look_now(&mut self) -> Option<Look> {
        let id = self.shown_personality().id;
        dock_look(
            &id,
            self.companion_mood(),
            pinned_mood().is_some(),
            self.explicit_art_ok(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::personality;

    const ALL_MOODS: [Mood; 9] = [
        Mood::Working,
        Mood::Gaming,
        Mood::Doctor,
        Mood::Reading,
        Mood::Thinking,
        Mood::Writing,
        Mood::HangingOut,
        Mood::Viper,
        Mood::Raunchy,
    ];

    fn every_look() -> Vec<Look> {
        let mut all: Vec<Look> = ALL_MOODS.iter().map(|m| mood_look(*m)).collect();
        all.extend(OUTFITS.iter().filter_map(|o| outfit_look(o.id)));
        all
    }

    fn is_explicit_art(key: &str) -> bool {
        key == "loadout-nyx-viper"
            || key == "loadout-nyx-raunchy"
            || key.starts_with("nyx-defaults/viper/")
            || key.starts_with("nyx-defaults/whore/")
    }

    #[test]
    fn each_mood_has_its_poses() {
        let counts: Vec<(Mood, usize)> = ALL_MOODS
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
                (Mood::Thinking, 5),
                (Mood::Writing, 5),
                (Mood::HangingOut, 3),
                (Mood::Viper, 5),
                (Mood::Raunchy, 6),
            ]
        );
        let working: Vec<&str> = mood_look(Mood::Working)
            .poses
            .iter()
            .map(|p| p.key)
            .collect();
        assert_eq!(working[0], "loadout-nyx-working");
        assert_eq!(working[1], "nyx-actions/working/1");
        assert_eq!(working[2], "nyx-actions/coding/2");
        assert_eq!(working[9], "nyx-actions/working/5");
        assert_eq!(
            mood_look(Mood::HangingOut).poses[2].key,
            "nyx-defaults/nyx/5"
        );
        assert_eq!(
            mood_look(Mood::Thinking).poses[0].key,
            "loadout-nyx-thinking"
        );
        assert_eq!(
            mood_look(Mood::Raunchy).poses[1].key,
            "nyx-defaults/whore/1"
        );
        assert!(mood_look(Mood::Viper).explicit);
        assert!(mood_look(Mood::Raunchy).explicit);
        assert!(!mood_look(Mood::Working).explicit);
    }

    #[test]
    fn every_picture_is_a_448_by_350_crop_and_shows_once() {
        let mut keys = Vec::new();
        let mut bytes = Vec::new();
        for look in every_look() {
            for pose in &look.poses {
                let img = image::load_from_memory(pose.jpg).expect("pose decodes");
                assert_eq!((img.width(), img.height()), (448, 350), "{}", pose.key);
                keys.push(pose.key);
                bytes.push(pose.jpg.as_ptr());
            }
        }
        // 49 mood poses (Viper's and the raunchy set among them) and 95
        // outfit poses.
        assert_eq!(keys.len(), 144);
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 144, "every pose has its own key");
        bytes.sort();
        bytes.dedup();
        assert_eq!(bytes.len(), 144, "every pose is its own picture");
    }

    #[test]
    fn every_built_in_voice_but_nyx_has_an_outfit_and_explicit_ones_wear_a_gated_mood() {
        assert_eq!(NYX_ID, personality::DEFAULT_ID);
        let builtins = personality::builtins();
        assert_eq!(builtins.len(), 22);
        for p in &builtins {
            if p.id == personality::DEFAULT_ID {
                assert!(!has_outfit(&p.id));
                continue;
            }
            assert!(has_outfit(&p.id), "{}", p.id);
            if p.explicit {
                assert_eq!(outfit_look(&p.id), None, "{}", p.id);
                let mood = outfit_for(&p.id, true, true).expect("explicit outfit");
                let (_, want) = EXPLICIT_VOICES
                    .iter()
                    .find(|(v, _)| *v == p.id)
                    .unwrap_or_else(|| panic!("{} is not listed as explicit", p.id));
                assert_eq!(mood, *want, "{}", p.id);
                assert!(mood_look(mood).explicit, "{}", p.id);
            } else {
                let look = outfit_look(&p.id).unwrap_or_else(|| panic!("{} has no outfit", p.id));
                assert!(!look.explicit, "{}", p.id);
                assert_eq!(look.poses.len(), 5, "{}", p.id);
            }
        }
        for (id, _) in EXPLICIT_VOICES {
            assert!(builtins.iter().any(|p| p.id == *id && p.explicit), "{id}");
        }
        for o in OUTFITS {
            assert!(
                builtins.iter().any(|p| p.id == o.id),
                "{} is a preset",
                o.id
            );
        }
        assert!(has_outfit("viper") && has_outfit("whore") && has_outfit("pirate"));
        assert!(!has_outfit("pirate-copy"));
        assert!(!has_outfit(""));
    }

    #[test]
    fn nyx_follows_her_mood_and_other_voices_wear_their_outfit() {
        let look = |id, mood| dock_look(id, mood, false, true).map(|l| l.set);
        assert_eq!(look("nyx", Mood::Working), Some("loadout-nyx-working"));
        assert_eq!(look("nyx", Mood::Thinking), Some("loadout-nyx-thinking"));
        assert_eq!(look("nyx", Mood::Writing), Some("loadout-nyx-writing"));
        assert_eq!(look("pirate", Mood::Working), Some("pirate"));
        assert_eq!(look("pirate", Mood::Gaming), Some("pirate"));
        assert_eq!(look("whore", Mood::Raunchy), Some("loadout-nyx-raunchy"));
        assert_eq!(
            look("whore-copy", Mood::Raunchy),
            Some("loadout-nyx-raunchy")
        );
        assert_eq!(look("viper", Mood::Viper), Some("loadout-nyx-viper"));
        // With no outfit from the gate, an explicit voice shows Nyx's mode.
        assert_eq!(look("viper", Mood::Working), Some("loadout-nyx-working"));
        assert_eq!(look("pirate-copy", Mood::Working), None, "custom voices");
        // A deck pin shows the mood's set on any drawn voice.
        let pinned = |id, mood| dock_look(id, mood, true, true).map(|l| l.set);
        assert_eq!(pinned("viper", Mood::Doctor), Some("loadout-nyx-doctor"));
        assert_eq!(pinned("pirate", Mood::Reading), Some("loadout-nyx-reading"));
        assert_eq!(pinned("pirate-copy", Mood::Reading), None);
    }

    #[test]
    fn explicit_art_never_shows_with_adults_only_off() {
        let mut ids: Vec<String> = personality::builtins().into_iter().map(|p| p.id).collect();
        ids.extend(["whore-copy".to_string(), String::new()]);
        for id in &ids {
            for mood in ALL_MOODS {
                for pinned in [false, true] {
                    if let Some(look) = dock_look(id, mood, pinned, false) {
                        assert!(!look.explicit, "{id} {mood:?} pinned={pinned}");
                        for pose in &look.poses {
                            assert!(!is_explicit_art(pose.key), "{id} shows {}", pose.key);
                        }
                    }
                }
            }
            if let Some(pose) = thumbnail(id, false) {
                assert!(!is_explicit_art(pose.key), "{id} thumbnail {}", pose.key);
            }
        }
        assert_eq!(dock_look("whore", Mood::Raunchy, false, false), None);
        assert_eq!(dock_look("viper", Mood::Viper, false, false), None);
        assert_eq!(dock_look("nyx", Mood::Viper, true, false), None);
        assert_eq!(dock_look("pirate", Mood::Raunchy, true, false), None);
        assert_eq!(
            dock_look("whore", Mood::HangingOut, false, false).map(|l| l.set),
            Some("loadout-nyx-hanging-out")
        );
        assert_eq!(thumbnail("whore", false), None);
        assert_eq!(thumbnail("viper", false), None);
        // With the gate open they do show.
        assert_eq!(
            thumbnail("whore", true).map(|p| p.key),
            Some("nyx-defaults/whore/1")
        );
        assert_eq!(
            thumbnail("viper", true).map(|p| p.key),
            Some("loadout-nyx-viper")
        );
    }

    #[test]
    fn the_picker_shows_each_voices_first_pose() {
        assert_eq!(
            thumbnail("pirate", false).map(|p| p.key),
            Some("nyx-defaults/pirate/1")
        );
        assert_eq!(
            thumbnail("nyx", false).map(|p| p.key),
            Some("loadout-nyx-hanging-out")
        );
        assert_eq!(thumbnail("pirate-copy", true), None);
        let ctx = egui::Context::default();
        let pose = thumbnail("zen-mentor", false).expect("zen mentor art");
        let tex = thumb_texture(&ctx, pose).expect("thumbnail decodes");
        assert_eq!(tex.size(), [192, 192]);
    }

    #[test]
    fn a_set_moves_on_one_pose_each_time_the_mood_changes() {
        let mut cycle = PoseCycle::default();
        // Same set, same mood: the pose holds.
        assert_eq!(cycle.pick("pirate", Mood::HangingOut, 5), 0);
        assert_eq!(cycle.pick("pirate", Mood::HangingOut, 5), 0);
        // The mood changes under an outfit: next pose.
        assert_eq!(cycle.pick("pirate", Mood::Thinking, 5), 1);
        assert_eq!(cycle.pick("pirate", Mood::Thinking, 5), 1);
        assert_eq!(cycle.pick("pirate", Mood::Working, 5), 2);
        // Another set starts at its own first pose.
        assert_eq!(cycle.pick("loadout-nyx-working", Mood::Working, 10), 0);
        assert_eq!(cycle.pick("loadout-nyx-thinking", Mood::Thinking, 5), 0);
        // Coming back moves that set on one.
        assert_eq!(cycle.pick("loadout-nyx-working", Mood::Working, 10), 1);
        assert_eq!(cycle.pick("pirate", Mood::Working, 5), 3);
        assert_eq!(cycle.pick("pirate", Mood::Gaming, 5), 4);
        // And wraps after the last.
        assert_eq!(cycle.pick("pirate", Mood::HangingOut, 5), 0);
        // A one-pose set always shows that pose.
        assert_eq!(cycle.pick("one", Mood::Gaming, 1), 0);
        assert_eq!(cycle.pick("one", Mood::Doctor, 1), 0);
    }

    #[test]
    fn the_dock_pose_holds_through_frames_and_changes_with_the_mood() {
        let ctx = egui::Context::default();
        let look = mood_look(Mood::Working);
        let first = dock_pose(&ctx, &look, Mood::Working).expect("pose");
        assert_eq!(first.key, "loadout-nyx-working");
        for _ in 0..5 {
            assert_eq!(dock_pose(&ctx, &look, Mood::Working), Some(first));
        }
        let thinking = mood_look(Mood::Thinking);
        assert_eq!(
            dock_pose(&ctx, &thinking, Mood::Thinking).map(|p| p.key),
            Some("loadout-nyx-thinking")
        );
        assert_eq!(
            dock_pose(&ctx, &look, Mood::Working).map(|p| p.key),
            Some("nyx-actions/working/1")
        );
        let tex = dock_texture(&ctx, first).expect("decodes");
        assert_eq!(tex.size(), [448, 350]);
    }

    #[test]
    fn explicit_voices_dress_up_only_with_adults_only_on() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
        cabin.thread_idx = 0;
        cabin.messages = Arc::new(Vec::new());
        for (id, set) in [
            ("whore", "loadout-nyx-raunchy"),
            ("viper", "loadout-nyx-viper"),
        ] {
            cabin.cfg.personality = id.into();
            cabin.cfg.explicit_personalities = false;
            cabin.cfg.adult_confirmed = true;
            assert!(
                !cabin.explicit_art_ok(),
                "the 18+ answer alone opens nothing"
            );
            let off = cabin.dock_look_now().expect("Nyx shows instead");
            assert_eq!(
                off.set, "loadout-nyx-hanging-out",
                "{id} with adults-only off"
            );
            assert!(!off.explicit);

            cabin.cfg.explicit_personalities = true;
            let on = cabin.dock_look_now().expect("explicit art");
            assert_eq!(on.set, set, "{id} with adults-only on");
            assert!(on.explicit);
        }
        // The outfit holds while the chat works; what it does moves the pose on.
        cabin.cfg.personality = "whore".into();
        cabin.messages = Arc::new(vec![("user".into(), "hi".into())]);
        cabin.running = true;
        cabin.chat_job_thread = Some(cabin.threads[0].id.clone());
        assert_eq!(cabin.companion_mood(), Mood::Raunchy);
        assert_eq!(cabin.companion_activity(), Mood::Thinking);
        cabin.running = false;
        cabin.chat_job_thread = None;
        assert_eq!(cabin.companion_activity(), Mood::HangingOut);

        cabin.cfg.explicit_personalities = false;
        cabin.cfg.personality = "pirate".into();
        assert_eq!(cabin.dock_look_now().map(|l| l.set), Some("pirate"));
    }
}
