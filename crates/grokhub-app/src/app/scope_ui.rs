//! Spike-4b: Settings → Permissions, "What GrokHub can read". One row per
//! learning scope from Spike-4a, every one off until the user allows it.
//!
//! A scope grant is written only here, from a pointer click on Allow
//! (`cards::grant_pill`; Enter or Space on a focused Allow does nothing).
//! Revoke is a ghost and only takes access away; `/privacy` lists the same
//! grants with its own ghost Revoke. Nothing reads a scope yet (Spike-8), and
//! the screen stays the desktop switch.

use super::*;
use grokhub_agent::harness as hx;

/// The heading over the scope rows, in Settings and in `/privacy`.
pub(super) const SCOPES_HEAD: &str = "What GrokHub can read";
pub(super) const SCOPES_NOTE: &str = "Each is off until you allow it here. Nothing reads them yet: a later update will, and only what you allowed. The screen stays under Let Grok control the desktop.";
/// Browsers the history row offers: (scope id, name the user reads).
pub(super) const BROWSERS: &[(&str, &str)] = &[
    ("firefox", "Firefox"),
    ("chrome", "Chrome"),
    ("chromium", "Chromium"),
    ("edge", "Edge"),
    ("brave", "Brave"),
];

/// Placeholder in the folder field: one absolute folder.
const FOLDER_HINT: &str = if cfg!(windows) { "C:\\Users\\you\\Notes" } else { "/home/you/Notes" };

/// What one scope would read, for its row hint.
fn scope_reads(kind: &str) -> &'static str {
    match kind {
        "files" => "One folder you pick. Never your whole home folder, keys or logins.",
        "apps" => "Installed apps and how often you open them.",
        "browser_history" => "Pages you visited in one browser. Never cookies or saved logins.",
        "calendar" => "Your calendar events.",
        "mail" => "Your mail.",
        "system_state" => "Disk, services, logs and updates. Read only.",
        _ => "",
    }
}

/// The plain name of a scope grant (`/privacy`, Revoke rows, status lines).
pub(super) fn scope_label(source: &str) -> String {
    if let Some(dir) = source.strip_prefix("files:") {
        return format!("Files in {dir}");
    }
    if let Some(id) = source.strip_prefix("browser_history:") {
        let name = BROWSERS.iter().find(|(b, _)| *b == id).map(|(_, n)| *n).unwrap_or(id);
        return format!("Browser history ({name})");
    }
    hx::SCOPE_KINDS
        .iter()
        .find(|(kind, _)| *kind == source)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| source.to_string())
}

/// A typed folder as the scope stores it: trimmed, no trailing slash.
pub(super) fn folder_scope(text: &str) -> hx::Scope {
    let t = text.trim();
    let t = if t.len() > 1 { t.trim_end_matches(['/', '\\']) } else { t };
    hx::Scope::Files(t.to_string())
}

impl Cabin {
    /// Settings → Permissions, after "Leaving this computer".
    pub(super) fn ui_scope_rows(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_heading(ui, SCOPES_HEAD);
        crate::cards::settings_note(ui, SCOPES_NOTE);
        let ledger = self.consent().clone();
        // A refused click says why right here (the status line is behind the
        // modal). A lock is already said once at the top of the page.
        let locked = self.private_lock_for_paint().is_some();
        if let Some(note) = self.harness.scope_note.as_deref().filter(|_| !locked) {
            crate::cards::settings_note(ui, note);
        }
        let now = now_ms();
        let on_hint = |at: u64, kind: &str| {
            format!("On since {}. {}", grokhub_core::pulse::ago_label(at, now), scope_reads(kind))
        };
        let mut grant: Option<hx::Scope> = None;
        let mut revoke: Option<String> = None;
        for (kind, label) in hx::SCOPE_KINDS {
            ui.push_id(("scope-row", *kind), |ui| {
                let granted: Vec<&hx::Grant> = ledger
                    .active()
                    .filter(|g| g.destination.is_empty())
                    .filter(|g| g.source == *kind || g.source.starts_with(&format!("{kind}:")))
                    .collect();
                for g in &granted {
                    ui.push_id(g.id.as_str(), |ui| {
                        let title = scope_label(&g.source);
                        if crate::cards::settings_action_ghost(ui, &title, &on_hint(g.granted_at, kind), "Revoke") {
                            revoke = Some(g.id.clone());
                        }
                    });
                }
                let off = format!("Off. {}", scope_reads(kind));
                match *kind {
                    "files" => {
                        let folder = &mut self.harness.scope_folder;
                        let hint = if granted.is_empty() { off } else { "Add another folder.".to_string() };
                        let hit = crate::cards::settings_grant(ui, label, &hint, "Allow", |ui| {
                            ui.add(
                                egui::TextEdit::singleline(folder)
                                    .hint_text(FOLDER_HINT)
                                    .desired_width(180.0)
                                    .min_size(egui::vec2(0.0, 28.0))
                                    .vertical_align(egui::Align::Center),
                            );
                        });
                        if hit {
                            grant = Some(folder_scope(folder));
                        }
                    }
                    "browser_history" => {
                        let pick = &mut self.harness.scope_browser;
                        let hint = if granted.is_empty() { off } else { "Add another browser.".to_string() };
                        let hit = crate::cards::settings_grant(ui, label, &hint, "Allow", |ui| {
                            let shown = BROWSERS.get(*pick).map(|(_, n)| *n).unwrap_or("Firefox");
                            egui::ComboBox::from_id_salt("scope-browser")
                                .selected_text(shown)
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for (i, (_, name)) in BROWSERS.iter().enumerate() {
                                        ui.selectable_value(pick, i, *name);
                                    }
                                });
                        });
                        if hit {
                            let id = BROWSERS.get(*pick).map(|(b, _)| *b).unwrap_or("firefox");
                            grant = Some(hx::Scope::BrowserHistory(id.to_string()));
                        }
                    }
                    _ if granted.is_empty() && crate::cards::settings_grant(ui, label, &off, "Allow", |_| {}) => {
                        grant = hx::Scope::parse(kind);
                    }
                    _ => {}
                }
            });
        }
        ui.add_space(8.0);
        if let Some(scope) = grant {
            self.grant_scope_click(&scope);
        }
        if let Some(id) = revoke {
            self.revoke_other_grant(&id);
        }
    }

    /// The Allow click on a scope row. The only caller of `grant_scope` (rule 4).
    fn grant_scope_click(&mut self, scope: &hx::Scope) {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(std::path::PathBuf::from);
        let dir = crate::config::config_dir();
        let label = scope_label(&scope.key());
        if self.consent().scope_grant(scope).is_some() {
            self.status = format!("{label} is already allowed.");
            self.harness.scope_note = Some(self.status.clone());
            return;
        }
        match hx::grant_scope(&dir, scope, home.as_deref(), hx::UserClick::from_click()) {
            Ok(_) => {
                if matches!(scope, hx::Scope::Files(_)) {
                    self.harness.scope_folder.clear();
                }
                self.status = format!("{label} allowed. Nothing reads it yet. Revoke it here any time.");
                self.harness.scope_note = None;
            }
            Err(e) => {
                self.status = format!("Not allowed: {e}");
                self.harness.scope_note = Some(self.status.clone());
            }
        }
        self.harness.consent = None;
    }

    /// Revoke a grant that isn't the hub's (a scope or another destination),
    /// from Settings or the `/privacy` bubble. It only takes access away.
    pub(super) fn revoke_other_grant(&mut self, id: &str) {
        let label = self
            .consent()
            .active()
            .find(|g| g.id == id)
            .map(super::privacy_ui::grant_label)
            .unwrap_or_else(|| "That grant".into());
        self.status = match hx::revoke_grant(&crate::config::config_dir(), id) {
            Ok(_) => format!("{label} revoked. It's off until you allow it again."),
            Err(e) => format!("Could not revoke: {e}"),
        };
        self.harness.consent = None;
    }
}
