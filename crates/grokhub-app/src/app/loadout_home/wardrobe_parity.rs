//! The cabin embeds Nyx's pictures ([`super::nyx_wardrobe`]) and the shell
//! serves the same table from `grokhub_core::companion::wardrobe`. These
//! tests keep the two in step: same pictures, same order, same gate.

use grokhub_core::companion::wardrobe as core;
use grokhub_core::companion::Mood;

use super::nyx_wardrobe::{dock_look, mood_look, thumbnail, Look, Pose};
use super::MoodArt;

/// A cabin pose as a path from the repo root: `nyx-<dir>/<n>` is
/// `brand/nyx/<dir>/<n>.jpg`, and a mood's own picture is the companion
/// file's outfit for it.
fn path_of(pose: &Pose) -> String {
    if let Some(rest) = pose.key.strip_prefix("nyx-") {
        return format!("brand/nyx/{rest}.jpg");
    }
    let mood = Mood::ALL
        .into_iter()
        .find(|m| m.key() == pose.key)
        .unwrap_or_else(|| panic!("{} is neither a numbered pose nor a mood", pose.key));
    super::mood_outfit(mood).file.clone()
}

fn paths(look: &Look) -> Vec<String> {
    look.poses.iter().map(path_of).collect()
}

fn core_paths(look: &core::Look) -> Vec<String> {
    look.poses.iter().map(|p| p.to_string()).collect()
}

#[test]
fn every_mood_set_matches_the_core_table() {
    for mood in Mood::ALL {
        let cabin = mood_look(mood);
        let shell = core::mood_look(mood);
        assert_eq!(paths(&cabin), core_paths(&shell), "{mood:?}");
        assert_eq!(cabin.explicit, shell.explicit, "{mood:?}");
    }
    assert_eq!(
        paths(&mood_look(Mood::Working))[..2],
        [
            "brand/nyx-working.jpg".to_string(),
            "brand/nyx/actions/working/1.jpg".to_string()
        ]
    );
}

#[test]
fn every_voice_wears_the_same_outfit_in_both() {
    let mut ids: Vec<String> = grokhub_core::personality::builtins()
        .into_iter()
        .map(|p| p.id)
        .collect();
    ids.extend(["pirate-copy".to_string(), "whore-copy".to_string()]);
    let mut outfits = 0;
    for id in &ids {
        for gate in [false, true] {
            for mood in Mood::ALL {
                let cabin = dock_look(id, mood, false, gate);
                let shell = core::dock_look(id, mood, gate);
                assert_eq!(
                    cabin.as_ref().map(paths),
                    shell.as_ref().map(core_paths),
                    "{id} {mood:?} gate={gate}"
                );
            }
            assert_eq!(
                thumbnail(id, gate).as_ref().map(path_of),
                core::thumbnail(id, gate).map(str::to_string),
                "{id} thumbnail gate={gate}"
            );
        }
        if core::outfit_look(id).is_some() {
            outfits += 1;
        }
    }
    assert_eq!(outfits, 19, "every non-explicit built-in voice but Nyx");
}
