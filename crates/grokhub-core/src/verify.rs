use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyResult {
    pub ok: bool,
    pub detail: String,
}

pub fn verify_script_path(skill_dir: impl AsRef<Path>) -> PathBuf {
    skill_dir.as_ref().join("scripts").join("verify.sh")
}

pub fn interpret_verify(code: Option<i32>, stdout: &str) -> VerifyResult {
    VerifyResult {
        ok: code == Some(0),
        detail: stdout.trim().to_string(),
    }
}

pub fn has_verify_ok(text: &str) -> bool {
    text.lines().any(|l| {
        let t = l.trim();
        t == "VERIFY_OK" || t.starts_with("VERIFY_OK:")
    })
}

pub fn has_goal_complete(text: &str) -> bool {
    text.lines().any(|l| l.trim().starts_with("GOAL_COMPLETE"))
}

pub fn can_mark_done(verify_passed: bool, saw_verify_ok: bool) -> bool {
    verify_passed || saw_verify_ok
}

/// A new user turn must re-verify. One VERIFY_OK must not unlock Done for the session.
pub fn verify_ok_after_user_turn(prev: bool, new_user_turn: bool) -> bool {
    if new_user_turn {
        false
    } else {
        prev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_and_gate() {
        let ok = interpret_verify(Some(0), "ok\n");
        assert!(ok.ok);
        assert_eq!(ok.detail, "ok");
        let bad = interpret_verify(Some(1), "missing");
        assert!(!bad.ok);
        assert_eq!(bad.detail, "missing");
        assert!(!interpret_verify(None, "").ok);
        assert_eq!(
            verify_script_path("/tmp/skills/flash-pi"),
            PathBuf::from("/tmp/skills/flash-pi/scripts/verify.sh")
        );
        assert!(has_verify_ok("done\nVERIFY_OK\n"));
        assert!(has_goal_complete("GOAL_COMPLETE flash"));
        assert!(can_mark_done(true, false));
        assert!(can_mark_done(false, true));
        assert!(!can_mark_done(false, false));
        assert!(!verify_ok_after_user_turn(true, true));
        assert!(verify_ok_after_user_turn(true, false));
        assert!(!verify_ok_after_user_turn(false, false));
    }
}
