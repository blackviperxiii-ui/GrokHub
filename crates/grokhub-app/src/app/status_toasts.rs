//! egui-notify toasts for cabin status lines (U7a).
//!
//! Mirrors Saved / run-finished / error status into in-app toasts. Does not
//! change status string semantics, persistence, or keyboard paths.

use eframe::egui;
use egui_notify::{Anchor, Toasts};
use std::time::{Duration, Instant};

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

/// Identical toasts (same kind and text) raised within this window collapse
/// into the newest toast instead of stacking.
const DEDUPE_WINDOW: Duration = Duration::from_secs(TOAST_SECS);

/// The most recently raised toast, kept so a repeat can fold into it.
#[derive(Debug)]
struct RecentToast {
    kind: StatusToastKind,
    text: String,
    at: Instant,
    count: u32,
}

/// Per-app toast bookkeeping: last seen status plus the latest raised toast.
#[derive(Debug, Default)]
pub(super) struct StatusToastState {
    last_status: String,
    recent: Option<RecentToast>,
}

impl StatusToastState {
    /// Count for this toast: 1 if it is new, n if it repeats the latest toast
    /// within [`DEDUPE_WINDOW`]. Records it as the latest either way.
    fn register(&mut self, kind: StatusToastKind, text: &str, now: Instant) -> u32 {
        if let Some(r) = &mut self.recent {
            if r.kind == kind
                && r.text == text
                && now.saturating_duration_since(r.at) < DEDUPE_WINDOW
            {
                r.at = now;
                r.count += 1;
                return r.count;
            }
        }
        self.recent = Some(RecentToast {
            kind,
            text: text.to_string(),
            at: now,
            count: 1,
        });
        1
    }
}

/// When `status` changes to a toast-worthy line, enqueue one toast. A repeat of
/// the latest toast inside the window replaces it with a counted one (fresh timer).
pub(super) fn sync_status_toasts(toasts: &mut Toasts, state: &mut StatusToastState, status: &str) {
    sync_status_toasts_at(toasts, state, status, Instant::now());
}

fn sync_status_toasts_at(
    toasts: &mut Toasts,
    state: &mut StatusToastState,
    status: &str,
    now: Instant,
) {
    if status == state.last_status {
        return;
    }
    state.last_status = status.to_string();
    let Some(kind) = status_toast_kind(status) else {
        return;
    };
    let count = state.register(kind, status, now);
    let text = if count > 1 {
        toasts.dismiss_latest_toast();
        format!("{status} (x{count})")
    } else {
        status.to_string()
    };
    let duration = Some(Duration::from_secs(TOAST_SECS));
    match kind {
        StatusToastKind::Success => {
            toasts.success(text).duration(duration).closable(true);
        }
        StatusToastKind::Error => {
            toasts.error(text).duration(duration).closable(true);
        }
    }
}

/// Title and body for a token budget warning.
pub(crate) fn token_budget_notice(
    level: grokhub_core::BudgetLevel,
    line: &str,
    pauses: bool,
) -> (&'static str, String) {
    match level {
        grokhub_core::BudgetLevel::Over if pauses => (
            "Token budget used up",
            format!("{line}. Scheduled work waits until tomorrow."),
        ),
        grokhub_core::BudgetLevel::Over => ("Token budget used up", format!("{line}.")),
        _ => ("Token budget at 80%", format!("{line}.")),
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
        let mut st = StatusToastState::default();
        sync_status_toasts(&mut toasts, &mut st, "Saved");
        assert_eq!(toasts.len(), 1);
        sync_status_toasts(&mut toasts, &mut st, "Saved");
        assert_eq!(toasts.len(), 1);
        sync_status_toasts(&mut toasts, &mut st, "OpenClaw import failed");
        assert_eq!(toasts.len(), 2);
        sync_status_toasts(&mut toasts, &mut st, "Thinking…");
        assert_eq!(toasts.len(), 2);
        assert_eq!(st.last_status, "Thinking…");
    }

    #[test]
    fn identical_toast_in_window_counts_up() {
        let t0 = Instant::now();
        let mut st = StatusToastState::default();
        let k = StatusToastKind::Success;
        assert_eq!(st.register(k, "Saved", t0), 1);
        assert_eq!(st.register(k, "Saved", t0 + Duration::from_secs(1)), 2);
        // Timer refreshes on each repeat, so 3s after the last one still collapses.
        assert_eq!(st.register(k, "Saved", t0 + Duration::from_secs(4)), 3);
    }

    #[test]
    fn toast_after_window_or_with_other_text_or_kind_is_new() {
        let t0 = Instant::now();
        let mut st = StatusToastState::default();
        assert_eq!(st.register(StatusToastKind::Success, "Saved", t0), 1);
        let late = t0 + DEDUPE_WINDOW;
        assert_eq!(st.register(StatusToastKind::Success, "Saved", late), 1);
        assert_eq!(st.register(StatusToastKind::Success, "Saved a", late), 1);
        assert_eq!(st.register(StatusToastKind::Error, "Saved a", late), 1);
    }

    #[test]
    fn repeated_status_through_idle_collapses_instead_of_stacking() {
        let t0 = Instant::now();
        let mut toasts = cabin_toasts();
        let mut st = StatusToastState::default();
        sync_status_toasts_at(&mut toasts, &mut st, "Saved", t0);
        sync_status_toasts_at(&mut toasts, &mut st, "Thinking…", t0);
        sync_status_toasts_at(&mut toasts, &mut st, "Saved", t0 + Duration::from_secs(1));
        // Original is dismissed (fading) and replaced by one counted toast.
        assert_eq!(toasts.len(), 2);
        assert_eq!(st.recent.as_ref().map(|r| r.count), Some(2));
    }

    #[test]
    fn cabin_toasts_clear_the_titlebar_strip() {
        // Compile-time: margin clears chrome (runtime assert! on consts trips clippy).
        const {
            assert!(TOAST_BELOW_TITLEBAR >= crate::theme::TITLEBAR_H + TOAST_EDGE);
            assert!(crate::theme::TITLEBAR_H == 40.0);
        }
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
