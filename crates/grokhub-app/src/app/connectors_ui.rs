//! Settings → Connectors: every MCP server the cabin can use, one row each
//! (initial, name, status chip, target, account, actions), then the GitHub
//! token, hooks, and Grok Build plugins that used to share the Skills page.
//!
//! Two sources. The cabin's own servers (`mcp.json` in the cabin home) are
//! what native chats use, so they show while the native engine is on. Grok
//! Build's servers (`~/.grok`, through `grok mcp`) are what Grok Build chats
//! use. Disconnect removes a server and its sign-in, so it asks on the
//! confirm sheet first; Cancel leaves the server as it was.

use super::pages::{catalog_empty_line, paint_catalog_stale};
use super::*;
use crate::cards::ChipTone;

/// Where a connector's entry lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectorSource {
    /// `mcp.json` in the cabin home (native engine).
    Cabin,
    /// `~/.grok`, managed through `grok mcp`.
    GrokBuild,
}

/// The status chip on a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ConnectorState {
    Connected,
    NeedsSignIn,
    /// The last error, as the server or Grok Build named it. Empty when none was given.
    Error(String),
    Off,
    NotChecked,
}

impl ConnectorState {
    pub(super) fn chip(&self) -> (&'static str, ChipTone) {
        match self {
            Self::Connected => ("Connected", ChipTone::Live),
            Self::NeedsSignIn => ("Needs sign-in", ChipTone::Setup),
            Self::Error(_) => ("Error", ChipTone::Offline),
            Self::Off => ("Off", ChipTone::Mute),
            Self::NotChecked => ("Not checked", ChipTone::Mute),
        }
    }
}

/// A button on a row, right-aligned in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectorAct {
    SignIn,
    Reconnect,
    SignOut,
    TurnOn,
    TurnOff,
    Disconnect,
}

impl ConnectorAct {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::SignIn => "Connect",
            Self::Reconnect => "Reconnect",
            Self::SignOut => "Sign out",
            Self::TurnOn => "Turn on",
            Self::TurnOff => "Turn off",
            Self::Disconnect => "Disconnect",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConnectorRow {
    pub name: String,
    pub source: ConnectorSource,
    pub state: ConnectorState,
    /// The command or URL the server runs at.
    pub detail: String,
    /// "Signed in to Linear as ada@example.com", when signed in.
    pub account: Option<String>,
    /// What to do next when the cabin can't do it for you.
    pub note: Option<String>,
    pub acts: Vec<ConnectorAct>,
}

impl ConnectorRow {
    /// The error line under the row, naming the error.
    pub(super) fn error_line(&self) -> Option<String> {
        match &self.state {
            ConnectorState::Error(why) if !why.trim().is_empty() => Some(format!("Error: {}", why.trim())),
            ConnectorState::Error(_) => Some("Error: the server stopped without saying why.".into()),
            _ => None,
        }
    }
}

/// Rows for the cabin's own servers, from the last configured/doctor read.
pub(super) fn cabin_connector_rows(rows: &[grokhub_agent::mcp::DoctorRow]) -> Vec<ConnectorRow> {
    use grokhub_agent::mcp::SignIn;
    rows.iter()
        .map(|r| {
            let signed_in = matches!(r.sign_in, SignIn::SignedIn(_));
            let state = if r.status == "disabled" {
                ConnectorState::Off
            } else if r.sign_in == SignIn::SignedOut {
                ConnectorState::NeedsSignIn
            } else if r.status == "error" {
                ConnectorState::Error(r.last_error.clone())
            } else if r.status == "connected" {
                ConnectorState::Connected
            } else {
                ConnectorState::NotChecked
            };
            let mut acts = match state {
                ConnectorState::Off => Vec::new(),
                ConnectorState::NeedsSignIn => vec![ConnectorAct::SignIn],
                _ => vec![ConnectorAct::Reconnect],
            };
            if signed_in {
                acts.push(ConnectorAct::SignOut);
            }
            if !grokhub_agent::mcp::is_desktop_server(&r.name) {
                acts.push(ConnectorAct::Disconnect);
            }
            ConnectorRow {
                name: r.name.clone(),
                source: ConnectorSource::Cabin,
                state,
                detail: r.detail.clone(),
                account: match &r.sign_in {
                    SignIn::SignedIn(line) => Some(line.clone()),
                    _ => None,
                },
                note: None,
                acts,
            }
        })
        .collect()
}

/// Rows for Grok Build's servers, with the last `grok mcp doctor` status.
pub(super) fn grok_connector_rows(
    servers: &[grokhub_acp::GrokMcpRow],
    status: &acp::McpStatusMap,
) -> Vec<ConnectorRow> {
    servers
        .iter()
        .map(|s| {
            let state = if !s.enabled {
                ConnectorState::Off
            } else {
                match status.get(&s.name) {
                    Some(grokhub_acp::McpDoctorStatus::Connected) => ConnectorState::Connected,
                    Some(grokhub_acp::McpDoctorStatus::NeedsSignIn) => ConnectorState::NeedsSignIn,
                    Some(grokhub_acp::McpDoctorStatus::Error(why)) => ConnectorState::Error(why.clone()),
                    None => ConnectorState::NotChecked,
                }
            };
            let note = (state == ConnectorState::NeedsSignIn)
                .then(|| format!("Sign in from Grok Build: run grok, open /mcps, press i on {}", s.name));
            let toggle = if s.enabled { ConnectorAct::TurnOff } else { ConnectorAct::TurnOn };
            ConnectorRow {
                name: s.name.clone(),
                source: ConnectorSource::GrokBuild,
                state,
                detail: s.target.clone(),
                account: None,
                note,
                acts: vec![toggle, ConnectorAct::Disconnect],
            }
        })
        .collect()
}

/// Rows whose name, target, or account holds `q` (already lowercase). Empty matches all.
pub(super) fn filter_connector_rows(rows: Vec<ConnectorRow>, q: &str) -> Vec<ConnectorRow> {
    if q.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|r| {
            r.name.to_ascii_lowercase().contains(q)
                || r.detail.to_ascii_lowercase().contains(q)
                || r.account.as_deref().is_some_and(|a| a.to_ascii_lowercase().contains(q))
        })
        .collect()
}

/// The confirm sheet's title for a Disconnect.
pub(super) fn disconnect_question(name: &str) -> String {
    format!("Disconnect {name}?")
}

/// The confirm sheet's detail line: what goes, and where it came from.
pub(super) fn disconnect_detail(source: ConnectorSource) -> &'static str {
    match source {
        ConnectorSource::Cabin => {
            "Its entry leaves this cabin's connections and its sign-in and saved token are deleted. /connections can bring the entry back; you'd sign in again."
        }
        ConnectorSource::GrokBuild => "Grok Build removes it from ~/.grok (grok mcp remove).",
    }
}

/// One connector row: initial, name and chip, target, account or note, and
/// the buttons on the right. Returns the button clicked.
fn paint_connector_row(ui: &mut egui::Ui, row: &ConnectorRow) -> Option<ConnectorAct> {
    let mut hit = None;
    egui::Frame::NONE
        .fill(crate::theme::elevated())
        .corner_radius(12.0)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 8.0, crate::theme::surface());
                let initial: String = row.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    initial,
                    egui::FontId::proportional(15.0),
                    crate::theme::fg(),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&row.name).size(15.0).color(crate::theme::fg()));
                        let (label, tone) = row.state.chip();
                        crate::cards::status_chip(ui, label, tone);
                    });
                    if !row.detail.is_empty() {
                        ui.label(RichText::new(&row.detail).size(12.0).color(crate::theme::muted()));
                    }
                    if let Some(line) = row.error_line() {
                        ui.label(RichText::new(line).size(12.0).color(crate::theme::setup()));
                    }
                    if let Some(account) = &row.account {
                        ui.label(RichText::new(account).size(12.0).color(crate::theme::fg()));
                    }
                    if let Some(note) = &row.note {
                        ui.label(RichText::new(note).size(12.0).color(crate::theme::muted()));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for act in row.acts.iter().rev() {
                        let clicked = match act {
                            ConnectorAct::Disconnect => crate::cards::ghost_pill(ui, act.label()),
                            ConnectorAct::SignIn => crate::cards::white_pill(ui, act.label()),
                            _ => crate::cards::ghost_pill(ui, act.label()),
                        };
                        if clicked {
                            hit = Some(*act);
                        }
                    }
                });
            });
        });
    ui.add_space(8.0);
    hit
}

impl Cabin {
    /// Open Settings on Connectors (sidebar, palette, `/connectors`, `/mcps`, …).
    pub(super) fn open_connectors(&mut self) {
        if self.nav != Nav::Settings {
            self.settings_back = self.nav;
        }
        self.settings_sec = SettingsSec::Connectors;
        self.nav = Nav::Settings;
    }

    pub(super) fn arm_disconnect(&mut self, name: &str, source: ConnectorSource) {
        self.confirm = Some(confirm::ConfirmKind::Disconnect {
            name: name.to_string(),
            source,
        });
    }

    /// The confirm sheet said yes.
    pub(super) fn disconnect_confirmed(&mut self, name: &str, source: ConnectorSource) {
        match source {
            ConnectorSource::Cabin => {
                crate::native_mcp::spawn(crate::native_mcp::Job::Disconnect(name.to_string()));
                self.status = format!("Disconnecting {name}");
            }
            ConnectorSource::GrokBuild => {
                self.run_grok_user_cmd(vec!["mcp".into(), "remove".into(), name.to_string()]);
            }
        }
    }

    fn take_connector_act(&mut self, row: &ConnectorRow, act: ConnectorAct) {
        use crate::native_mcp::{spawn, Job};
        let name = row.name.clone();
        match (row.source, act) {
            (source, ConnectorAct::Disconnect) => self.arm_disconnect(&name, source),
            (ConnectorSource::Cabin, ConnectorAct::SignIn) => spawn(Job::SignIn(name)),
            (ConnectorSource::Cabin, ConnectorAct::Reconnect) => spawn(Job::Restart(name)),
            (ConnectorSource::Cabin, ConnectorAct::SignOut) => spawn(Job::SignOut(name)),
            (ConnectorSource::GrokBuild, ConnectorAct::TurnOn) => {
                self.run_grok_user_cmd(vec!["mcp".into(), "enable".into(), name]);
            }
            (ConnectorSource::GrokBuild, ConnectorAct::TurnOff) => {
                self.run_grok_user_cmd(vec!["mcp".into(), "disable".into(), name]);
            }
            _ => {}
        }
    }

    /// The Connectors section of Settings. Settings owns the scroll area.
    pub(super) fn ui_settings_connectors(&mut self, ui: &mut egui::Ui) {
        if self.cfg.native_engine {
            self.ensure_native_listing();
        }
        if !self.grok_catalog_loaded && self.grok_catalog_rx.is_none() {
            self.reload_grok_catalog();
        }
        ui.horizontal(|ui| {
            crate::cards::search_field(ui, &mut self.skill_q);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::cards::ghost_pill(ui, "Refresh") {
                    if self.cfg.native_engine {
                        self.native_listing_cwd.clear();
                        self.ensure_native_listing();
                        crate::native_mcp::spawn(crate::native_mcp::Job::Doctor);
                    }
                    self.reload_grok_catalog();
                }
            });
        });
        ui.add_space(12.0);
        let q = self.skill_q.to_ascii_lowercase();
        let mut picked: Option<(ConnectorRow, ConnectorAct)> = None;
        if self.cfg.native_engine {
            let snap = crate::native_mcp::snapshot();
            ui.horizontal(|ui| {
                crate::cards::section_label(ui, "This cabin");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::cards::ghost_pill(ui, "Check status") {
                        crate::native_mcp::spawn(crate::native_mcp::Job::Doctor);
                    }
                    if crate::cards::ghost_pill(ui, "Import") {
                        crate::native_mcp::spawn(crate::native_mcp::Job::Import);
                    }
                });
            });
            crate::cards::help_text(ui, "Servers native chats use, kept in this cabin's own home. Import copies definitions from the cabin's Grok home.");
            ui.add_space(8.0);
            if !snap.note.is_empty() {
                crate::cards::settings_note(ui, &snap.note);
            }
            let rows = filter_connector_rows(cabin_connector_rows(&snap.rows), &q);
            if snap.rows.is_empty() {
                crate::cards::settings_note(ui, "No connectors in this cabin yet. Import copies them from the cabin's Grok home.");
            } else if rows.is_empty() {
                crate::cards::settings_note(ui, "None matched.");
            }
            for row in &rows {
                if let Some(act) = paint_connector_row(ui, row) {
                    picked = Some((row.clone(), act));
                }
            }
            if snap.busy {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
            ui.add_space(16.0);
        }
        ui.horizontal(|ui| {
            crate::cards::section_label(ui, "Grok Build");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::cards::ghost_pill(ui, "Doctor") {
                    self.run_mcp_doctor();
                    ui.ctx().request_repaint();
                }
                if crate::cards::white_pill(ui, "Add MCP") {
                    self.mcp_compose = true;
                }
            });
        });
        self.ui_connector_home_note(ui);
        if !self.connector_note.is_empty() {
            ui.add_space(4.0);
            crate::cards::settings_note(ui, &self.connector_note.clone());
        }
        if self.mcp_compose {
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.mcp_nl)
                    .hint_text(crate::theme::hint("name npx -y package   or   remove name"))
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if crate::cards::white_pill(ui, "Run") {
                    let line = std::mem::take(&mut self.mcp_nl);
                    self.submit_mcp_line(&line);
                    self.mcp_compose = false;
                }
                if crate::cards::ghost_pill(ui, "Cancel") {
                    self.mcp_compose = false;
                }
            });
        }
        ui.add_space(8.0);
        let grok_rows = filter_connector_rows(grok_connector_rows(&self.grok_catalog.mcp, &self.mcp_status), &q);
        if grok_rows.is_empty() {
            ui.label(
                RichText::new(catalog_empty_line(
                    self.grok_catalog_rx.is_some(),
                    &q,
                    self.grok_catalog.mcp.len(),
                    "No MCP servers in ~/.grok — add one with Add MCP.",
                    &self.status,
                ))
                .color(crate::theme::muted()),
            );
        } else {
            paint_catalog_stale(ui, &self.status, self.grok_catalog.mcp.len());
            for row in &grok_rows {
                if let Some(act) = paint_connector_row(ui, row) {
                    picked = Some((row.clone(), act));
                }
            }
        }
        if let Some((row, act)) = picked {
            self.take_connector_act(&row, act);
        }
        ui.add_space(20.0);
        self.ui_github_connector(ui);
        ui.add_space(20.0);
        self.ui_hooks_section(ui, &q);
        ui.add_space(20.0);
        self.ui_grok_plugins(ui, &q);
    }

    fn ui_github_connector(&mut self, ui: &mut egui::Ui) {
        let mut save_pat = false;
        let mut run_gh: Option<String> = None;
        crate::cards::section_label(ui, "GitHub");
        ui.label(
            RichText::new("Read-only. Who am I and List repos use the PAT via run_connector. No writes. No other websites.")
                .size(12.0)
                .color(crate::theme::muted()),
        );
        ui.add_space(8.0);
        crate::cards::settings_field(
            ui,
            "Personal access token",
            "Classic or fine-grained PAT with repo read. Stored in secrets.json.",
            &mut self.secrets.github_token,
            true,
        );
        if crate::cards::white_pill(ui, "Save PAT") {
            save_pat = true;
        }
        ui.add_space(8.0);
        for (title, body, tool) in crate::cards::GITHUB_TILES {
            if crate::cards::settings_action(ui, title, body, "Run") {
                run_gh = Some((*tool).to_string());
            }
        }
        if save_pat {
            self.persist_secrets();
            self.status = if self.secrets.github_token.trim().is_empty() {
                "GitHub PAT cleared".into()
            } else {
                "GitHub PAT saved".into()
            };
        }
        if let Some(tool) = run_gh {
            self.nav = Nav::Chat;
            self.run_connector("github", &tool, "");
        }
    }

    /// Grok Build plugins: the installed ones, then its plugin marketplace.
    fn ui_grok_plugins(&mut self, ui: &mut egui::Ui, q: &str) {
        let mut plugin_toggle: Option<(String, bool)> = None;
        let mut plugin_install: Option<String> = None;
        let mut plugin_uninstall: Option<String> = None;
        ui.horizontal(|ui| {
            crate::cards::section_label(ui, "Plugins");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::cards::ghost_pill(ui, "Update") {
                    self.run_grok_user_cmd(vec!["plugin".into(), "update".into()]);
                }
            });
        });
        crate::cards::help_text(ui, "Installed from the Grok Build marketplace (`grok plugin list`).");
        ui.add_space(8.0);
        let matches = |p: &&grokhub_acp::GrokPluginRow| {
            q.is_empty()
                || p.name.to_ascii_lowercase().contains(q)
                || p.description.to_ascii_lowercase().contains(q)
                || p.marketplace.to_ascii_lowercase().contains(q)
        };
        let installed: Vec<_> = self
            .grok_catalog
            .plugins
            .iter()
            .filter(|p| p.status != "available")
            .filter(matches)
            .cloned()
            .collect();
        let installed_total = self.grok_catalog.plugins.iter().filter(|p| p.status != "available").count();
        if installed.is_empty() {
            ui.label(
                RichText::new(catalog_empty_line(
                    self.grok_catalog_rx.is_some(),
                    q,
                    installed_total,
                    "No plugins installed yet — browse Marketplace below.",
                    &self.status,
                ))
                .color(crate::theme::muted()),
            );
        } else {
            paint_catalog_stale(ui, &self.status, installed.len());
            for p in &installed {
                let body = if p.marketplace.is_empty() { p.source.clone() } else { p.marketplace.clone() };
                let toggle = if p.enabled { "Disable" } else { "Enable" };
                if crate::cards::settings_action(ui, &p.name, &body, toggle) {
                    plugin_toggle = Some((p.name.clone(), !p.enabled));
                }
                if crate::cards::ghost_pill(ui, "Uninstall") {
                    plugin_uninstall = Some(p.name.clone());
                }
            }
        }
        ui.add_space(20.0);
        crate::cards::section_label(ui, "Plugin marketplace");
        crate::cards::help_text(ui, "xAI Official and other sources (`grok plugin marketplace`).");
        ui.add_space(8.0);
        let market: Vec<_> = self
            .grok_catalog
            .plugins
            .iter()
            .filter(|p| p.status == "available")
            .filter(matches)
            .cloned()
            .collect();
        let market_total = self.grok_catalog.plugins.iter().filter(|p| p.status == "available").count();
        if market.is_empty() {
            ui.label(
                RichText::new(catalog_empty_line(
                    self.grok_catalog_rx.is_some(),
                    q,
                    market_total,
                    "No marketplace plugins to install.",
                    &self.status,
                ))
                .color(crate::theme::muted()),
            );
        } else {
            paint_catalog_stale(ui, &self.status, market.len());
            for p in &market {
                let body = if p.description.is_empty() { p.marketplace.clone() } else { p.description.clone() };
                if crate::cards::settings_action(ui, &p.name, &body, "Install") {
                    plugin_install = Some(p.name.clone());
                }
            }
        }
        if let Some((name, on)) = plugin_toggle {
            let cmd = if on { "enable" } else { "disable" };
            self.run_grok_user_cmd(vec!["plugin".into(), cmd.into(), name]);
        }
        if let Some(name) = plugin_uninstall {
            self.run_grok_user_cmd(vec!["plugin".into(), "uninstall".into(), name, "--confirm".into()]);
        }
        if let Some(name) = plugin_install {
            self.run_grok_user_cmd(vec!["plugin".into(), "install".into(), name, "--trust".into()]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::mcp::{DoctorRow, SignIn};

    fn doctor(name: &str, status: &str, err: &str, sign_in: SignIn) -> DoctorRow {
        DoctorRow {
            name: name.into(),
            status: status.into(),
            tool_count: 0,
            last_error: err.into(),
            detail: format!("http https://{name}.example/mcp"),
            sign_in,
        }
    }

    #[test]
    fn cabin_rows_name_each_state_and_offer_the_next_step() {
        let rows = cabin_connector_rows(&[
            doctor("linear", "connected", "", SignIn::SignedIn("Signed in to linear as ada@example.com".into())),
            doctor("notion", "not checked", "", SignIn::SignedOut),
            doctor("github", "error", "timed out after 5s", SignIn::NotOffered),
            doctor("old", "disabled", "", SignIn::NotOffered),
            doctor("files", "stopped", "", SignIn::NotOffered),
        ]);
        let got: Vec<(&str, &str, Vec<&str>)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.state.chip().0, r.acts.iter().map(|a| a.label()).collect()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("linear", "Connected", vec!["Reconnect", "Sign out", "Disconnect"]),
                ("notion", "Needs sign-in", vec!["Connect", "Disconnect"]),
                ("github", "Error", vec!["Reconnect", "Disconnect"]),
                ("old", "Off", vec!["Disconnect"]),
                ("files", "Not checked", vec!["Reconnect", "Disconnect"]),
            ]
        );
        assert_eq!(rows[0].account.as_deref(), Some("Signed in to linear as ada@example.com"));
        assert_eq!(rows[1].account, None);
        assert_eq!(rows[2].error_line().as_deref(), Some("Error: timed out after 5s"));
        assert_eq!(rows[0].error_line(), None);
        assert!(rows.iter().all(|r| r.source == ConnectorSource::Cabin));
    }

    #[test]
    fn the_cabin_desktop_server_has_no_disconnect() {
        let name = "grokhub-desktop";
        assert!(grokhub_agent::mcp::is_desktop_server(name));
        let rows = cabin_connector_rows(&[doctor(name, "connected", "", SignIn::NotOffered)]);
        assert_eq!(rows[0].acts, vec![ConnectorAct::Reconnect]);
    }

    #[test]
    fn grok_rows_read_enabled_and_the_doctor_status() {
        let row = |name: &str, enabled: bool| grokhub_acp::GrokMcpRow {
            name: name.into(),
            enabled,
            target: format!("npx -y {name}-mcp"),
        };
        let mut status = acp::McpStatusMap::new();
        status.insert("ctx7".into(), grokhub_acp::McpDoctorStatus::Connected);
        status.insert("gmail".into(), grokhub_acp::McpDoctorStatus::NeedsSignIn);
        status.insert("broken".into(), grokhub_acp::McpDoctorStatus::Error("spawn failed: npx".into()));
        status.insert("off".into(), grokhub_acp::McpDoctorStatus::Connected);
        let rows = grok_connector_rows(
            &[row("ctx7", true), row("gmail", true), row("broken", true), row("off", false), row("new", true)],
            &status,
        );
        let got: Vec<(&str, &str, Vec<&str>)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.state.chip().0, r.acts.iter().map(|a| a.label()).collect()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("ctx7", "Connected", vec!["Turn off", "Disconnect"]),
                ("gmail", "Needs sign-in", vec!["Turn off", "Disconnect"]),
                ("broken", "Error", vec!["Turn off", "Disconnect"]),
                ("off", "Off", vec!["Turn on", "Disconnect"]),
                ("new", "Not checked", vec!["Turn off", "Disconnect"]),
            ]
        );
        assert_eq!(rows[1].note.as_deref(), Some("Sign in from Grok Build: run grok, open /mcps, press i on gmail"));
        assert_eq!(rows[0].note, None);
        assert_eq!(rows[2].error_line().as_deref(), Some("Error: spawn failed: npx"));
        assert_eq!(rows[0].detail, "npx -y ctx7-mcp");
    }

    #[test]
    fn search_matches_name_target_and_account() {
        let rows = cabin_connector_rows(&[
            doctor("linear", "connected", "", SignIn::SignedIn("Signed in to linear as ada@example.com".into())),
            doctor("notion", "connected", "", SignIn::NotOffered),
        ]);
        let names = |q: &str| -> Vec<String> {
            filter_connector_rows(rows.clone(), q).into_iter().map(|r| r.name).collect()
        };
        assert_eq!(names(""), ["linear", "notion"]);
        assert_eq!(names("ada@"), ["linear"]);
        assert_eq!(names("notion.example"), ["notion"]);
        assert!(names("zzz").is_empty());
    }

    #[test]
    fn disconnect_asks_first_and_cancel_keeps_the_server() {
        let _hold = crate::config::hold_test_config();
        let root = crate::config::test_config_root("connectors-disconnect");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("GROKHUB_CONFIG", &root);
        std::env::set_var("GROKHUB_GROK", root.join("missing-grok"));
        let mut cabin = Cabin::quiet_for_test();
        let row = grok_connector_rows(
            &[grokhub_acp::GrokMcpRow { name: "ctx7".into(), enabled: true, target: "npx".into() }],
            &acp::McpStatusMap::new(),
        )
        .remove(0);
        cabin.take_connector_act(&row, ConnectorAct::Disconnect);
        assert_eq!(
            cabin.confirm,
            Some(confirm::ConfirmKind::Disconnect { name: "ctx7".into(), source: ConnectorSource::GrokBuild })
        );
        assert_eq!(disconnect_question("ctx7"), "Disconnect ctx7?");
        // Cancel is the sheet dropping `confirm`: nothing ran.
        cabin.confirm = None;
        assert!(cabin.connector_note.is_empty());
        std::env::remove_var("GROKHUB_GROK");
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn connectors_open_in_settings_and_remember_the_page_behind() {
        let _hold = crate::config::hold_test_config();
        let root = crate::config::test_config_root("connectors-open");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("GROKHUB_CONFIG", &root);
        std::env::set_var("GROKHUB_GROK", root.join("missing-grok"));
        let mut cabin = Cabin::quiet_for_test();
        cabin.nav = Nav::Skills;
        cabin.open_connectors();
        assert_eq!(cabin.nav, Nav::Settings);
        assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
        assert_eq!(cabin.settings_back, Nav::Skills);
        cabin.open_connectors();
        assert_eq!(cabin.settings_back, Nav::Skills, "a second open keeps the page behind Settings");
        std::env::remove_var("GROKHUB_GROK");
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
