//! Shared explicit-yes confirm sheet (cabin C / R1).
//!
//! Reuses the B2 Always beat: elevated frame, 2px intent stroke, title + one
//! consequence line, primary or danger + Cancel.
//!
//! Keyboard (locked by tests):
//! - Composer draft nonempty: never consume Enter/Esc (send stays on the draft).
//! - Palette or Settings open: never consume.
//! - Ask-card Always confirm (inline): Enter = Allow, Esc = Deny. Sheet buttons
//!   are pointer-only so the second beat does not steal those keys.
//! - Session Always escalate and destructive host (overlay): Enter = Confirm,
//!   Esc = Cancel when the composer is empty.
//!
//! Focus: overlay requests focus on the primary when it opens. Tab stays on
//! Confirm / Cancel. Cancel and Esc drop the sheet with no side effect.

use super::*;

/// Ask-card Always second beat. Named so tests can lock the inherit line.
pub(super) const ALWAYS_CONFIRM_LINE1: &str = "Skip every tool prompt this launch.";
pub(super) const ALWAYS_CONFIRM_LINE2: &str =
    "Night, loops, and phone inherit --always-approve until quit.";

pub(super) const ALWAYS_SESSION_TITLE: &str = "Always this launch";
pub(super) const HOST_CONFIRM_TITLE: &str = "Destructive host";
pub(super) const HOST_CONFIRM_PRIMARY: &str = "Run";

/// Confirm sheet stays up only while Ask still shows this `rpc_id`.
pub(super) fn always_confirm_matches_rpc(
    armed: Option<&serde_json::Value>,
    current: &serde_json::Value,
) -> bool {
    armed == Some(current)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ConfirmKind {
    AlwaysSession,
    DestructiveHost { cmd: String },
}

impl ConfirmKind {
    pub(super) fn paints_overlay(&self) -> bool {
        match self {
            Self::AlwaysSession | Self::DestructiveHost { .. } => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfirmAct {
    Confirm,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ConfirmSpec {
    pub title: &'static str,
    pub consequence: &'static str,
    pub primary: &'static str,
    pub danger: bool,
}

pub(super) fn always_session_spec() -> ConfirmSpec {
    ConfirmSpec {
        title: ALWAYS_SESSION_TITLE,
        consequence: ALWAYS_CONFIRM_LINE1,
        primary: "Confirm",
        danger: false,
    }
}

pub(super) fn destructive_host_spec() -> ConfirmSpec {
    ConfirmSpec {
        title: HOST_CONFIRM_TITLE,
        consequence: "This command can destroy data on this machine.",
        primary: HOST_CONFIRM_PRIMARY,
        danger: true,
    }
}

/// Overlay Enter/Esc. Ask-card Always passes `steal_keys = false`.
pub(super) fn confirm_key(
    enter: bool,
    esc: bool,
    composer_has_text: bool,
    overlay_open: bool,
    steal_keys: bool,
) -> Option<ConfirmAct> {
    if !steal_keys || overlay_open || composer_has_text {
        return None;
    }
    if esc {
        return Some(ConfirmAct::Cancel);
    }
    if enter {
        return Some(ConfirmAct::Confirm);
    }
    None
}

pub(super) fn should_confirm_destructive_host(cmd: &str) -> bool {
    host_risk(cmd) == HostRisk::Destructive && !is_rewind_copy_cmd(cmd)
}

pub(super) fn paint_confirm_sheet(ui: &mut egui::Ui, spec: ConfirmSpec, detail: &str) -> Option<ConfirmAct> {
    let stroke = if spec.danger {
        crate::theme::offline()
    } else {
        crate::theme::always_amber()
    };
    let mut act = None;
    egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(crate::theme::CHROME_RADIUS)
        .stroke(egui::Stroke::new(2.0_f32, stroke))
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(spec.title)
                    .size(14.0)
                    .color(crate::theme::fg()),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(spec.consequence)
                    .size(13.0)
                    .color(crate::theme::muted()),
            );
            let extra = detail.trim();
            if !extra.is_empty() && extra != spec.consequence {
                ui.label(
                    RichText::new(extra)
                        .size(13.0)
                        .color(crate::theme::muted()),
                );
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let primary = if spec.danger {
                    crate::cards::danger_pill(ui, spec.primary)
                } else {
                    crate::cards::white_pill(ui, spec.primary)
                };
                if primary {
                    act = Some(ConfirmAct::Confirm);
                }
                if crate::cards::ghost_pill(ui, "Cancel") {
                    act = Some(ConfirmAct::Cancel);
                }
            });
        });
    act
}

impl Cabin {
    pub(super) fn clear_confirm(&mut self) {
        self.perm_always_confirm = None;
        self.confirm = None;
    }

    pub(super) fn paint_confirm_overlay(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.confirm.clone() else {
            return;
        };
        if !kind.paints_overlay() {
            return;
        }
        let (spec, detail) = match &kind {
            ConfirmKind::AlwaysSession => (always_session_spec(), ALWAYS_CONFIRM_LINE2),
            ConfirmKind::DestructiveHost { cmd } => {
                (destructive_host_spec(), cmd.as_str())
            }
        };
        let overlay_open = self.palette_open || self.nav == Nav::Settings;
        let steal = confirm_key(
            ctx.input(|i| i.key_pressed(egui::Key::Enter)),
            ctx.input(|i| i.key_pressed(egui::Key::Escape)),
            !self.composer.trim().is_empty(),
            overlay_open,
            true,
        );
        let mut act = steal;
        let id = egui::Id::new("cabin-confirm-sheet");
        egui::Area::new(id)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                if ui.memory(|m| m.focused().is_none()) {
                    ui.memory_mut(|m| m.request_focus(id));
                }
                if let Some(hit) = paint_confirm_sheet(ui, spec, detail) {
                    act = Some(hit);
                }
            });
        match act {
            Some(ConfirmAct::Confirm) => self.take_confirm(kind),
            Some(ConfirmAct::Cancel) => self.confirm = None,
            None => {}
        }
    }

    fn take_confirm(&mut self, kind: ConfirmKind) {
        self.confirm = None;
        match kind {
            ConfirmKind::AlwaysSession => self.apply_session_always(),
            ConfirmKind::DestructiveHost { cmd } => {
                self.run_cmds(vec![cmd]);
            }
        }
    }

    pub(super) fn apply_session_always(&mut self) {
        self.set_permission_mode(PermissionMode::AlwaysApprove);
        if self.running {
            self.halt_in_flight();
        }
        self.acp = None;
        self.acp_spawn_rx = None;
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.grok_session = None;
        }
        self.persist_idle_key = self.persist_idle_now();
        self.status = "Permission always-approve".into();
    }

    pub(super) fn arm_session_always(&mut self) {
        self.confirm = Some(ConfirmKind::AlwaysSession);
        if self.running {
            self.halt_in_flight();
        }
        self.acp = None;
        self.acp_spawn_rx = None;
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.grok_session = None;
        }
        self.persist_idle_key = self.persist_idle_now();
        self.status = "Confirm Always…".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_confirm_names_session_skip_and_scheduled_inherit() {
        assert!(ALWAYS_CONFIRM_LINE1
            .to_ascii_lowercase()
            .contains("skip every tool prompt this launch"));
        assert!(
            ALWAYS_CONFIRM_LINE2.contains("--always") && ALWAYS_CONFIRM_LINE2.contains("approve")
        );
        assert!(ALWAYS_CONFIRM_LINE2.contains("Night"));
        assert!(ALWAYS_CONFIRM_LINE2.contains("loop"));
        assert!(ALWAYS_CONFIRM_LINE2.contains("phone"));
        let a = serde_json::json!(1);
        let b = serde_json::json!(2);
        assert!(always_confirm_matches_rpc(Some(&a), &a));
        assert!(!always_confirm_matches_rpc(Some(&a), &b));
        assert!(!always_confirm_matches_rpc(None, &a));
    }

    #[test]
    fn confirm_keys_leave_a_composer_draft_alone() {
        assert_eq!(
            confirm_key(true, false, true, false, true),
            None,
            "Enter must send a typed follow-up"
        );
        assert_eq!(confirm_key(false, true, true, false, true), None);
        assert_eq!(confirm_key(true, false, false, true, true), None);
        assert_eq!(
            confirm_key(true, false, false, false, false),
            None,
            "Ask Always confirm does not steal Enter"
        );
        assert_eq!(
            confirm_key(true, false, false, false, true),
            Some(ConfirmAct::Confirm)
        );
        assert_eq!(
            confirm_key(false, true, false, false, true),
            Some(ConfirmAct::Cancel)
        );
    }

    #[test]
    fn destructive_host_skips_rewind_copies() {
        assert!(should_confirm_destructive_host("rm foo.txt"));
        assert!(should_confirm_destructive_host("sudo reboot"));
        let copy = rewind_copy_cmd("/home/j/.config/GrokHub/rewind/rw1", "/home/j/proj");
        assert!(!should_confirm_destructive_host(&copy));
        assert!(!should_confirm_destructive_host("git status"));
    }

    #[test]
    fn sheet_copy_is_title_consequence_primary() {
        let always = always_session_spec();
        assert_eq!(always.title, ALWAYS_SESSION_TITLE);
        assert_eq!(always.primary, "Confirm");
        assert!(!always.danger);
        let host = destructive_host_spec();
        assert_eq!(host.title, HOST_CONFIRM_TITLE);
        assert_eq!(host.primary, HOST_CONFIRM_PRIMARY);
        assert!(host.danger);
    }
}
