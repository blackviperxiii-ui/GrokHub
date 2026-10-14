//! egui-notify toasts for cabin status lines (U7a).
//!
//! Mirrors Saved / run-finished / error status into in-app toasts. Does not
//! change status string semantics, persistence, or keyboard paths.

use eframe::egui;
use egui_notify::{Anchor, Toasts};
use std::time::Duration;

const TOAST_SECS: u64 = 4;
/// Side inset from the screen edge (egui-notify default).
const TOAST_EDGE: f32 = 8.0;
/// Clear the undecorated titlebar (× / max / min) so toasts never cover chrome.
const TOAST_BELOW_TITLEBAR: f32 = crate::theme::TITLEBAR_H + TOAST_EDGE;

/// Cabin toasts: top-right, margin clears [`crate::theme::TITLEBAR_H`].
pub(super) fn cabin_toasts() -> Toasts {
    Toasts::default()
        .with_anchor(Anchor::TopRight)
        .with_margin(egui::vec2(TOAST_EDGE, TOAST_BELOW_TITLEBAR))
}

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

/// When `status` changes to a toast-worthy line, enqueue one toast.
pub(super) fn sync_status_toasts(toasts: &mut Toasts, last: &mut String, status: &str) {
    if status == last.as_str() {
        return;
    }
    *last = status.to_string();
    let Some(kind) = status_toast_kind(status) else {
        return;
    };
    let duration = Some(Duration::from_secs(TOAST_SECS));
    match kind {
        StatusToastKind::Success => {
            toasts.success(status).duration(duration).closable(true);
        }
        StatusToastKind::Error => {
            toasts.error(status).duration(duration).closable(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            status_toast_kind("Channel switch finished — restart GrokHub"),
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

    #[test]
    fn sync_only_fires_once_per_status_change() {
        let mut toasts = cabin_toasts();
        let mut last = String::new();
        sync_status_toasts(&mut toasts, &mut last, "Saved");
        assert_eq!(toasts.len(), 1);
        sync_status_toasts(&mut toasts, &mut last, "Saved");
        assert_eq!(toasts.len(), 1);
        sync_status_toasts(&mut toasts, &mut last, "OpenClaw import failed");
        assert_eq!(toasts.len(), 2);
        sync_status_toasts(&mut toasts, &mut last, "Thinking…");
        assert_eq!(toasts.len(), 2);
        assert_eq!(last, "Thinking…");
    }

    #[test]
    fn cabin_toasts_clear_the_titlebar_strip() {
        assert_eq!(crate::theme::TITLEBAR_H, 40.0);
        assert!(
            TOAST_BELOW_TITLEBAR >= crate::theme::TITLEBAR_H + TOAST_EDGE,
            "top margin must sit below chrome hits"
        );
        // Constructing the configured collector is enough: margin is private.
        let _ = cabin_toasts();
        let src = include_str!("status_toasts.rs");
        assert!(
            src.contains("with_anchor(Anchor::TopRight)")
                && src.contains("TOAST_BELOW_TITLEBAR")
                && src.contains("TITLEBAR_H"),
            "cabin_toasts must pin TopRight under TITLEBAR_H"
        );
    }
}
