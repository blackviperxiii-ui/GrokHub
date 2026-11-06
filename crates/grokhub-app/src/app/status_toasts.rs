//! Which cabin status lines are worth keeping in the bell (U7a).
//!
//! Saved / run-finished / error / offline lines land in the Notifications
//! menu. Nothing pops over the window: the bell is the one place they show.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StatusToastKind {
    Success,
    Error,
}

/// Which in-app toast (if any) a cabin status line should raise.
pub(super) fn status_toast_kind(status: &str) -> Option<StatusToastKind> {
    let t = status.trim();
    if t.is_empty() {
        return None;
    }
    // The internet drop log (`net_log_ui`): the line shows on every page.
    if t.starts_with("Offline since ") {
        return Some(StatusToastKind::Error);
    }
    if t.starts_with("Back online.") {
        return Some(StatusToastKind::Success);
    }
    if is_saved(t) {
        return Some(StatusToastKind::Success);
    }
    if is_run_finished(t) {
        return Some(StatusToastKind::Success);
    }
    if is_status_error(t) {
        return Some(StatusToastKind::Error);
    }
    None
}

fn is_saved(t: &str) -> bool {
    t == "Saved" || t.starts_with("Saved ")
}

fn is_run_finished(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    lower.contains("finished") && !is_status_error(t)
}

fn is_status_error(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    lower.contains("failed")
        || lower.contains("failure")
        || lower.contains(" error")
        || lower.starts_with("error")
        || lower.contains("couldn't")
        || lower.contains("could not")
        || lower.starts_with("refusing")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_internet_drop_and_its_return_raise_toasts_on_every_page() {
        assert_eq!(
            status_toast_kind("Offline since 02:01: neither public resolver answered."),
            Some(StatusToastKind::Error)
        );
        assert_eq!(
            status_toast_kind(
                "Back online. The internet dropped at 02:01 for 5 min; it's in the drop log."
            ),
            Some(StatusToastKind::Success)
        );
    }

    #[test]
    fn saved_and_saved_path_are_success() {
        assert_eq!(status_toast_kind("Saved"), Some(StatusToastKind::Success));
        assert_eq!(
            status_toast_kind("Saved /tmp/harbor.png"),
            Some(StatusToastKind::Success)
        );
        assert_eq!(
            status_toast_kind("Saved playbook Any job"),
            Some(StatusToastKind::Success)
        );
    }

    #[test]
    fn run_finished_is_success() {
        assert_eq!(
            status_toast_kind("Host job finished"),
            Some(StatusToastKind::Success)
        );
        assert_eq!(
            status_toast_kind("Channel switch finished — restart Loadout"),
            Some(StatusToastKind::Success)
        );
    }

    #[test]
    fn errors_toast_as_error() {
        assert_eq!(
            status_toast_kind("OpenClaw import failed"),
            Some(StatusToastKind::Error)
        );
        assert_eq!(
            status_toast_kind("Couldn't finish checking your computer"),
            Some(StatusToastKind::Error)
        );
        assert_eq!(
            status_toast_kind("Could not save the grant: disk full"),
            Some(StatusToastKind::Error)
        );
        assert_eq!(
            status_toast_kind("refusing an update that would wipe config"),
            Some(StatusToastKind::Error)
        );
        assert_eq!(
            status_toast_kind("Error: network down"),
            Some(StatusToastKind::Error)
        );
    }

    #[test]
    fn progress_and_idle_do_not_toast() {
        assert_eq!(status_toast_kind(""), None);
        assert_eq!(status_toast_kind("Thinking…"), None);
        assert_eq!(status_toast_kind("Host…"), None);
        assert_eq!(status_toast_kind("Syncing…"), None);
        assert_eq!(status_toast_kind("New chat"), None);
        assert_eq!(status_toast_kind("Diagnostics copied"), None);
    }
}
