//! Spike-4b: Settings → Permissions, "What GrokHub can read". One row per
//! learning scope from Spike-4a, every one off until the user allows it.
//!
//! A scope grant is written only here, from a pointer click on Allow
//! (`cards::settings_grant_row`; Enter or Space on a focused Allow does nothing).
//! Revoke is a ghost and only takes access away; `/privacy` lists the same
//! grants with its own ghost Revoke. Nothing reads a scope yet (Spike-8), and
//! the screen stays the desktop switch. While private data is locked every
//! pill here is disabled, with the lock on hover (SB-01).

use super::*;
use grokhub_agent::harness as hx;

/// The heading over the scope rows, in Settings and in `/privacy`.
pub(super) const SCOPES_HEAD: &str = "What GrokHub can read";
/// SB-09: the screen's setting is named in quotes, with where it lives.
pub(super) const SCOPES_NOTE: &str = "Each is off until you allow it here. Nothing reads them yet: a later update will, and only what you allowed. Screen access is \"Let Grok control the desktop\" in Settings → Cabin defaults.";
/// Browsers the history row offers: (scope id, name the user reads).
pub(super) const BROWSERS: &[(&str, &str)] = &[
    ("firefox", "Firefox"),
    ("chrome", "Chrome"),
    ("chromium", "Chromium"),
    ("edge", "Edge"),
    ("brave", "Brave"),
];

/// SB-04: the native folder dialog's button on the files row.
pub(super) const CHOOSE_FOLDER: &str = "Choose folder…";
/// SB-06: the files scope takes any number of folders, one grant each. The
/// row reads "Files in a folder" until one is allowed, then "Add a folder".
pub(super) const ADD_FOLDER: &str = "Add a folder";
const ADD_BROWSER: &str = "Add a browser";
/// How many characters of a folder path a row hint shows (SB-03).
const PATH_HINT_CHARS: usize = 48;

/// Placeholder in the folder field: one absolute folder, in this OS's shape (SB-04).
pub(super) const FOLDER_HINT: &str = folder_hint_for(hx::KeyringOs::current());

pub(super) const fn folder_hint_for(os: hx::KeyringOs) -> &'static str {
    match os {
        hx::KeyringOs::Windows => "C:\\Users\\you\\Notes",
        hx::KeyringOs::MacOs => "/Users/you/Notes",
        hx::KeyringOs::Linux => "/home/you/Notes",
    }
}

/// What one scope would read, for its row hint.
fn scope_reads(kind: &str) -> &'static str {
    match kind {
        "files" => "A folder you pick. Never your whole home folder, keys or logins.",
        "apps" => "Installed apps and how often you open them.",
        "browser_history" => "Pages you visited in one browser. Never cookies or saved logins.",
        // SB-10 TODO: name the calendar source and the mail account once the
        // Spike-8 readers exist; until then there is nothing to name.
        "calendar" => "Your calendar events.",
        "mail" => "Your mail.",
        "system_state" => "Disk, services, logs and updates. Read only.",
        _ => "",
    }
}

/// The last part of a folder path: "notes" for `/home/you/Documents/notes` or
/// `C:\Users\you\notes`. A root (`/`, `C:\`) keeps its whole path (SB-03).
pub(super) fn folder_name(dir: &str) -> &str {
    let trimmed = dir.trim_end_matches(['/', '\\']);
    match trimmed.rsplit(['/', '\\']).next() {
        Some(name) if !name.is_empty() && !name.ends_with(':') => name,
        _ => dir,
    }
}

/// `text` cut to at most `max` characters with "…" in the middle, keeping more
/// of the end (the folder name) than the start (SB-03).
pub(super) fn middle_ellipsis(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max || max < 3 {
        return text.to_string();
    }
    let keep = max - 1;
    let head = keep * 2 / 5;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// The plain name of a scope grant (`/privacy`, Revoke rows, status lines).
/// A folder is named by its last part ("Files in notes"); the whole path is
/// in [`scope_detail`].
pub(super) fn scope_label(source: &str) -> String {
    if let Some(dir) = source.strip_prefix("files:") {
        return format!("Files in {}", folder_name(dir));
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

/// The whole folder path behind a files grant, for hover (SB-03).
pub(super) fn scope_detail(source: &str) -> Option<&str> {
    source.strip_prefix("files:")
}

/// A typed folder as the scope stores it: trimmed, no trailing slash.
pub(super) fn folder_scope(text: &str) -> hx::Scope {
    let t = text.trim();
    let t = if t.len() > 1 { t.trim_end_matches(['/', '\\']) } else { t };
    hx::Scope::Files(t.to_string())
}

/// What the folder dialog said: the folder picked, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FolderPick {
    Picked(String),
    /// Closed without a folder.
    Cancelled,
    /// Came back with nothing faster than anyone can cancel: no dialog
    /// opened (no desktop portal or zenity on this Linux, for example).
    NoDialog,
}

/// Shown under the heading when no folder dialog could open.
pub(super) const NO_FOLDER_DIALOG: &str = "No folder dialog opened on this computer. Type the folder's path instead.";

/// A `None` from the dialog this fast means it never opened.
const NO_DIALOG_WITHIN: std::time::Duration = std::time::Duration::from_millis(400);

impl FolderPick {
    pub(super) fn from_answer(got: Option<String>, took: std::time::Duration) -> Self {
        match got {
            Some(dir) => Self::Picked(dir),
            None if took < NO_DIALOG_WITHIN => Self::NoDialog,
            None => Self::Cancelled,
        }
    }
}

/// Open the OS folder dialog (SB-04) and hand back what it said. Picking
/// only fills the field: the grant is still the Allow click.
fn pick_folder() -> mpsc::Receiver<FolderPick> {
    let (tx, rx) = mpsc::channel();
    let pick = move || {
        let start = std::time::Instant::now();
        let got = rfd::FileDialog::new()
            .set_title("Choose a folder GrokHub may read")
            .pick_folder()
            .map(|p| p.display().to_string());
        let _ = tx.send(FolderPick::from_answer(got, start.elapsed()));
    };
    // AppKit dialogs belong to the main thread; the others run off the UI thread.
    if cfg!(target_os = "macos") {
        pick();
    } else {
        std::thread::spawn(pick);
    }
    rx
}

impl Cabin {
    /// Settings → Permissions, after "Leaving this computer".
    pub(super) fn ui_scope_rows(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_heading(ui, SCOPES_HEAD);
        crate::cards::settings_note(ui, SCOPES_NOTE);
        let ledger = self.consent().clone();
        // A refused click says why right here (the status line is behind the
        // modal). A lock is already said once at the top of the page, and
        // every pill below is disabled with the lock on hover (SB-01).
        let lock = self.private_lock_for_paint();
        let locked = lock.as_ref().map(super::privacy_ui::lock_hover);
        self.poll_folder_pick(ui.ctx());
        if let Some(note) = self.harness.scope_note.as_deref().filter(|_| locked.is_none()) {
            crate::cards::settings_note(ui, note);
        }
        let now = now_ms();
        let on_hint = |at: u64, rest: &str| format!("On since {}. {rest}", grokhub_core::pulse::ago_label(at, now));
        let mut grant: Option<hx::Scope> = None;
        let mut revoke: Option<String> = None;
        let mut choose = false;
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
                        // SB-03: the folder name is the title; the path is in
                        // the hint, cut in the middle, and whole on hover.
                        let (hint, hover) = match scope_detail(&g.source) {
                            Some(dir) => (on_hint(g.granted_at, &middle_ellipsis(dir, PATH_HINT_CHARS)), Some(dir)),
                            None => (on_hint(g.granted_at, scope_reads(kind)), None),
                        };
                        let pill = crate::cards::GrantPill::Revoke;
                        if crate::cards::settings_grant_row(ui, &title, &hint, hover, pill, locked, false, |_| {}) {
                            revoke = Some(g.id.clone());
                        }
                    });
                }
                let off = format!("Off. {}", scope_reads(kind));
                let allow = crate::cards::GrantPill::Allow;
                match *kind {
                    "files" => {
                        let (title, hint) = if granted.is_empty() {
                            (*label, off)
                        } else {
                            (ADD_FOLDER, "Pick or type another folder. Never your whole home folder, keys or logins.".to_string())
                        };
                        let picking = self.harness.scope_pick_rx.is_some();
                        let folder = &mut self.harness.scope_folder;
                        let hit = crate::cards::settings_grant_row(ui, title, &hint, None, allow, locked, true, |ui| {
                            ui.add_enabled_ui(!picking, |ui| {
                                if crate::cards::ghost_pill(ui, CHOOSE_FOLDER) {
                                    choose = true;
                                }
                            });
                            ui.add(
                                egui::TextEdit::singleline(folder)
                                    .hint_text(crate::theme::hint(FOLDER_HINT))
                                    .desired_width(f32::INFINITY)
                                    .min_size(egui::vec2(0.0, 28.0))
                                    .vertical_align(egui::Align::Center),
                            );
                        });
                        if hit {
                            grant = Some(folder_scope(folder));
                        }
                    }
                    "browser_history" => {
                        let (title, hint) = if granted.is_empty() {
                            (*label, off)
                        } else {
                            (ADD_BROWSER, "Pick another browser. Never cookies or saved logins.".to_string())
                        };
                        let pick = &mut self.harness.scope_browser;
                        let hit = crate::cards::settings_grant_row(ui, title, &hint, None, allow, locked, false, |ui| {
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
                    _ if granted.is_empty()
                        && crate::cards::settings_grant_row(ui, label, &off, None, allow, locked, false, |_| {}) =>
                    {
                        grant = hx::Scope::parse(kind);
                    }
                    _ => {}
                }
            });
        }
        ui.add_space(8.0);
        if choose && locked.is_none() {
            self.harness.scope_pick_rx = Some(pick_folder());
        }
        // Belt and braces: a locked page never grants or revokes, whatever
        // the paint returned (SB-01).
        if locked.is_some() {
            return;
        }
        if let Some(scope) = grant {
            self.grant_scope_click(&scope);
        }
        if let Some(id) = revoke {
            self.revoke_other_grant(&id);
        }
    }

    /// The folder dialog's answer, once it has one. A cancel leaves the field as is.
    fn poll_folder_pick(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.harness.scope_pick_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(FolderPick::Picked(dir)) => {
                self.harness.scope_folder = dir;
                self.harness.scope_note = None;
            }
            Ok(FolderPick::NoDialog) => self.harness.scope_note = Some(NO_FOLDER_DIALOG.to_string()),
            Ok(FolderPick::Cancelled) | Err(mpsc::TryRecvError::Disconnected) => {}
            Err(mpsc::TryRecvError::Empty) => {
                self.harness.scope_pick_rx = Some(rx);
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// SB-03: a folder grant is titled by its folder name, on any OS's path.
    #[test]
    fn a_folder_is_named_by_its_last_part() {
        assert_eq!(folder_name("/home/you/Documents/notes"), "notes");
        assert_eq!(folder_name("/home/you/Documents/notes/"), "notes");
        assert_eq!(folder_name("C:\\Users\\you\\Notes"), "Notes");
        assert_eq!(folder_name("/"), "/");
        assert_eq!(folder_name("C:\\"), "C:\\");
        assert_eq!(scope_label("files:/tmp/gh-4b-shots/home/Documents/notes"), "Files in notes");
        assert_eq!(scope_label("files:D:\\Work\\Q3 plans"), "Files in Q3 plans");
        assert_eq!(scope_detail("files:/srv/notes"), Some("/srv/notes"));
        assert_eq!(scope_detail("calendar"), None);
        assert_eq!(scope_label("calendar"), "Calendar");
        assert_eq!(scope_label("browser_history:brave"), "Browser history (Brave)");
    }

    /// SB-03: a long path keeps its start and, mostly, its end.
    #[test]
    fn middle_ellipsis_keeps_both_ends() {
        let long = "/tmp/gh-scopes-shots/home/Documents/Projects/2026/clients/hale/notes";
        let cut = middle_ellipsis(long, 32);
        assert_eq!(cut.chars().count(), 32, "{cut}");
        assert_eq!(cut, "/tmp/gh-scop…/clients/hale/notes");
        assert!(cut.starts_with("/tmp/") && cut.ends_with("/hale/notes"), "{cut}");
        assert_eq!(middle_ellipsis("/srv/notes", 32), "/srv/notes", "short paths stay whole");
        assert_eq!(middle_ellipsis("abcdef", 2), "abcdef", "too small a cap leaves it whole");
        // Characters, not bytes: never splits a multi-byte letter.
        let wide = "/home/jörg/Документы/заметки/очень-длинное-имя";
        let cut = middle_ellipsis(wide, 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.contains('…'));
    }

    /// SB-07: "On" / "Off" leads the hint in the foreground colour.
    #[test]
    fn hints_lead_with_their_state() {
        let job = crate::cards::state_hint_job("Off. Your mail.");
        assert_eq!(job.text, "Off. Your mail.");
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[0].format.color, crate::theme::fg());
        assert_eq!(job.sections[1].format.color, crate::theme::muted());
        let on = crate::cards::state_hint_job("On since 1m ago. /srv/notes");
        assert_eq!(on.sections.len(), 2);
        assert_eq!(on.sections[0].format.color, crate::theme::fg());
        let plain = crate::cards::state_hint_job("Pick another browser. Never cookies or saved logins.");
        assert_eq!(plain.sections.len(), 1);
        assert_eq!(plain.sections[0].format.color, crate::theme::muted());
        assert_eq!(crate::cards::state_hint_job("Online now").sections.len(), 1, "a word that only starts with On");
    }

    /// SB-04: a dialog that answers "nothing" faster than anyone can cancel
    /// never opened, so the row says to type the path instead.
    #[test]
    fn a_folder_dialog_that_never_opened_is_told_apart_from_a_cancel() {
        use std::time::Duration;
        assert_eq!(FolderPick::from_answer(Some("/srv/notes".into()), Duration::from_millis(5)), FolderPick::Picked("/srv/notes".into()));
        assert_eq!(FolderPick::from_answer(None, Duration::from_millis(20)), FolderPick::NoDialog);
        assert_eq!(FolderPick::from_answer(None, Duration::from_secs(3)), FolderPick::Cancelled);
        assert!(NO_FOLDER_DIALOG.contains("Type the folder's path"));
    }

    /// SB-09: the screen's setting is quoted and says where it lives.
    #[test]
    fn the_screen_setting_is_quoted_with_its_place() {
        assert!(SCOPES_NOTE.ends_with("Screen access is \"Let Grok control the desktop\" in Settings → Cabin defaults."));
        let settings = include_str!("settings.rs");
        assert!(settings.contains("SettingsSec::Defaults => \"Cabin defaults\""));
        let defaults = settings.split("SettingsSec::Defaults => {").nth(1).expect("Cabin defaults section");
        assert!(defaults.contains("\"Let Grok control the desktop\""), "the switch lives in Cabin defaults");
    }
}
