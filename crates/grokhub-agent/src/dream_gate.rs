//! Round 3 item 39: lessons the nightly dream writes stay out of the model's
//! reach until a paired eval shows they help.
//!
//! The dream appends lessons to the cabin skill `dream-lessons`. Before this,
//! that skill reached every chat three ways (the skills reminder, the `skill`
//! tool and the app's auto-follow) with nothing proving it did any good. Now
//! the skill only shows while `PASSED` beside its `SKILL.md` holds the
//! SHA-256 of the current `SKILL.md`. The live eval's `--ab dream_lessons`
//! arm writes that marker, and only when its verdict passes; any new lesson
//! changes the hash, so the skill hides again until the next pass.

use std::path::Path;

use sha2::{Digest, Sha256};

/// The cabin skill the dream writes.
pub const LESSON_SKILL: &str = "dream-lessons";
/// The marker beside its `SKILL.md`.
pub const PASSED: &str = "PASSED";

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// True when the lessons in `skill_dir` passed the eval as they stand now.
pub fn proven(skill_dir: &Path) -> bool {
    let Ok(body) = std::fs::read_to_string(skill_dir.join("SKILL.md")) else {
        return false;
    };
    std::fs::read_to_string(skill_dir.join(PASSED)).is_ok_and(|mark| mark.trim() == digest(&body))
}

/// Record that the lessons in `skill_dir`, as they stand now, passed.
pub fn mark_passed(skill_dir: &Path) -> Result<(), String> {
    let body = std::fs::read_to_string(skill_dir.join("SKILL.md")).map_err(|e| e.to_string())?;
    std::fs::write(skill_dir.join(PASSED), format!("{}\n", digest(&body)))
        .map_err(|e| e.to_string())
}

/// The lessons skill's folder under `config_dir`.
pub fn skill_dir(config_dir: &Path) -> std::path::PathBuf {
    config_dir
        .join("skills")
        .join(grokhub_core::skill_dir_name(LESSON_SKILL))
}

/// May the app auto-follow (or the model see) the cabin skill `name`?
/// Everything but unproven dream lessons may.
pub fn may_follow(name: &str, config_dir: &Path) -> bool {
    !name.eq_ignore_ascii_case(LESSON_SKILL) || proven(&skill_dir(config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lessons_show_only_while_the_pass_matches_them() {
        let cfg = std::env::temp_dir().join(format!("gh-dream-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cfg);
        let dir = skill_dir(&cfg);
        assert!(!may_follow("dream-lessons", &cfg));
        assert!(may_follow("deploy", &cfg));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: dream-lessons\n---\n\n- one\n",
        )
        .unwrap();
        assert!(!proven(&dir), "no pass yet");
        mark_passed(&dir).unwrap();
        assert!(proven(&dir));
        assert!(may_follow("Dream-Lessons", &cfg));
        // A new lesson needs a new pass.
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: dream-lessons\n---\n\n- one\n- two\n",
        )
        .unwrap();
        assert!(!proven(&dir));
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
