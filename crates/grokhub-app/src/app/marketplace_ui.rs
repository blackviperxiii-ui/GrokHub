//! Settings → Connectors → Marketplace: the curated catalog
//! (`grokhub_core::mcp_catalog`) with search, category pills, a detail view
//! and one-click Install into the cabin's own `mcp.json`.
//!
//! Installing is reversible (`/connections` undo, or Disconnect), so it asks
//! nothing. A browser sign-in opens right after Install. A typed API key is a
//! credential: Save key goes through the confirm sheet first, and Cancel
//! clears the field.

use grokhub_core::mcp_catalog::{self, CatalogAuth, CatalogEntry};

use super::*;
use crate::cards::ChipTone;

/// Which tab of the Connectors page is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ConnectorsTab {
    #[default]
    Installed,
    Marketplace,
}

#[derive(Default)]
pub(super) struct MarketState {
    pub tab: ConnectorsTab,
    pub q: String,
    /// `None` is All.
    pub category: Option<String>,
    /// The entry whose detail view is open.
    pub detail: Option<String>,
    /// The key typed on a detail view, held until Save key is approved or cancelled.
    pub key: String,
}

/// The sign-in line on an entry's detail view.
pub(super) fn sign_in_line(e: &CatalogEntry) -> String {
    match e.auth {
        CatalogAuth::None => "No sign-in needed. It works as soon as it's installed.".into(),
        CatalogAuth::Oauth => format!("Signs in to {} in your browser right after Install.", e.publisher),
        CatalogAuth::Key => format!("Needs an API key. {}", e.key_hint),
        CatalogAuth::Google => "Uses your Google account, set up through Google Cloud.".into(),
        CatalogAuth::Own => format!("Signs in with your {} account by itself the first time a chat uses it.", e.publisher),
    }
}

/// What a Google entry shows until it can connect for you: the manual
/// Google Cloud steps.
pub(super) const GOOGLE_MANUAL_STEPS: &[&str] = &[
    "In Google Cloud console, pick or create a project and enable this service's API.",
    "Create an OAuth client (Desktop app) under APIs & Services, Credentials.",
    "Add your account as a test user on the OAuth consent screen.",
    "Install here, then press Connect on its row in the Installed tab.",
];

/// The badge chip on a Marketplace row: installed or not.
pub(super) fn market_chip(installed: bool) -> Option<(&'static str, ChipTone)> {
    installed.then_some(("Installed", ChipTone::Live))
}

fn paint_badge(ui: &mut egui::Ui, logo: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().rect_filled(rect, size / 4.0, crate::theme::surface());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        logo,
        egui::FontId::proportional(size * 0.45),
        crate::theme::fg(),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowHit {
    Open,
    Install,
}

fn paint_market_row(ui: &mut egui::Ui, e: &CatalogEntry, installed: bool) -> Option<RowHit> {
    let mut hit = None;
    egui::Frame::NONE
        .fill(crate::theme::elevated())
        .corner_radius(12.0)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                paint_badge(ui, &e.logo, 32.0);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&e.name).size(15.0).color(crate::theme::fg()));
                        if let Some((label, tone)) = market_chip(installed) {
                            crate::cards::status_chip(ui, label, tone);
                        }
                    });
                    ui.label(
                        RichText::new(format!("{} · {}", e.publisher, e.category))
                            .size(12.0)
                            .color(crate::theme::muted()),
                    );
                    ui.label(RichText::new(&e.description).size(12.0).color(crate::theme::fg()));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::cards::ghost_pill(ui, "Details") {
                        hit = Some(RowHit::Open);
                    }
                    if !installed && crate::cards::white_pill(ui, "Install") {
                        hit = Some(RowHit::Install);
                    }
                });
            });
        });
    ui.add_space(8.0);
    hit
}

impl Cabin {
    /// Names in the cabin's `mcp.json`, from the last snapshot.
    fn installed_names(&self) -> Vec<String> {
        crate::native_mcp::snapshot().rows.into_iter().map(|r| r.name).collect()
    }

    /// Install one catalog entry. Nothing to approve: it only writes an
    /// entry the user can disconnect or undo. A browser sign-in follows.
    pub(super) fn market_install(&mut self, e: &CatalogEntry) {
        crate::native_mcp::spawn(crate::native_mcp::Job::Install {
            name: e.id.clone(),
            label: e.name.clone(),
            entry: e.config_entry(),
            sign_in: e.auth == CatalogAuth::Oauth,
        });
        self.status = format!("Installing {}", e.name);
    }

    /// Save key: ask on the confirm sheet first.
    pub(super) fn arm_save_key(&mut self, e: &CatalogEntry) {
        if self.market.key.trim().is_empty() {
            self.status = format!("Type the {} key first", e.name);
            return;
        }
        self.confirm = Some(confirm::ConfirmKind::SaveKey {
            name: e.id.clone(),
            label: e.name.clone(),
        });
    }

    /// The confirm sheet said yes: seal the typed key, then clear the field.
    pub(super) fn save_key_confirmed(&mut self, name: &str) {
        let key = std::mem::take(&mut self.market.key);
        crate::native_mcp::spawn(crate::native_mcp::Job::SaveKey {
            name: name.to_string(),
            key,
        });
    }

    /// The tab pills at the top of Settings → Connectors.
    pub(super) fn ui_connectors_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (label, tab) in [("Installed", ConnectorsTab::Installed), ("Marketplace", ConnectorsTab::Marketplace)] {
                if crate::cards::tab_pill(ui, label, self.market.tab == tab) {
                    self.market.tab = tab;
                }
            }
        });
        ui.add_space(12.0);
    }

    pub(super) fn ui_marketplace(&mut self, ui: &mut egui::Ui) {
        let catalog = mcp_catalog::bundled();
        let installed = self.installed_names();
        let snap = crate::native_mcp::snapshot();
        if snap.busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        if !self.cfg.native_engine {
            crate::cards::settings_note(
                ui,
                "Installed connectors are used by native chats. Turn on the native engine in Settings, Labs to use them.",
            );
        }
        if !snap.note.is_empty() {
            crate::cards::settings_note(ui, &snap.note);
        }
        if let Some(id) = self.market.detail.clone() {
            match catalog.get(&id) {
                Some(e) => self.ui_market_detail(ui, e, installed.contains(&e.id)),
                None => self.market.detail = None,
            }
            return;
        }
        crate::cards::search_field(ui, &mut self.market.q);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if crate::cards::tab_pill(ui, "All", self.market.category.is_none()) {
                self.market.category = None;
            }
            for cat in catalog.categories() {
                if crate::cards::tab_pill(ui, cat, self.market.category.as_deref() == Some(cat)) {
                    self.market.category = Some(cat.to_string());
                }
            }
        });
        ui.add_space(12.0);
        let shown = catalog.search(&self.market.q, self.market.category.as_deref());
        if shown.is_empty() {
            crate::cards::settings_note(ui, "No connector matched. Try another word, or pick All.");
        }
        let mut install = None;
        for e in shown {
            match paint_market_row(ui, e, installed.contains(&e.id)) {
                Some(RowHit::Open) => self.market.detail = Some(e.id.clone()),
                Some(RowHit::Install) => install = Some(e.clone()),
                None => {}
            }
        }
        if let Some(e) = install {
            self.market_install(&e);
        }
    }

    fn ui_market_detail(&mut self, ui: &mut egui::Ui, e: &CatalogEntry, installed: bool) {
        if crate::cards::ghost_pill(ui, "Back") {
            self.market.detail = None;
            self.market.key.clear();
            return;
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            paint_badge(ui, &e.logo, 44.0);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&e.name).size(18.0).color(crate::theme::fg()));
                    if let Some((label, tone)) = market_chip(installed) {
                        crate::cards::status_chip(ui, label, tone);
                    }
                });
                ui.label(
                    RichText::new(format!("{} · {}", e.publisher, e.category))
                        .size(12.0)
                        .color(crate::theme::muted()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !installed && crate::cards::white_pill(ui, "Install") {
                    self.market_install(e);
                }
            });
        });
        ui.add_space(8.0);
        ui.label(RichText::new(&e.description).size(13.0).color(crate::theme::fg()));
        ui.add_space(12.0);
        crate::cards::section_label(ui, "What it can do");
        for cap in &e.capabilities {
            ui.label(RichText::new(format!("•  {cap}")).size(13.0).color(crate::theme::fg()));
        }
        ui.add_space(12.0);
        crate::cards::section_label(ui, "Permissions");
        crate::cards::help_text(ui, &e.permissions);
        ui.add_space(12.0);
        crate::cards::section_label(ui, "Sign-in");
        crate::cards::help_text(ui, &sign_in_line(e));
        if installed {
            self.ui_market_sign_in(ui, e);
        }
        ui.add_space(12.0);
        crate::cards::section_label(ui, "Runs");
        crate::cards::help_text(ui, &format!("`{}`", e.target()));
        ui.add_space(12.0);
        crate::cards::section_label(ui, "Source");
        if ui.link(RichText::new(&e.source).size(13.0)).clicked() {
            let _ = crate::desktop::open_url(&e.source);
        }
    }

    /// The next step after Install, by how the entry signs in.
    fn ui_market_sign_in(&mut self, ui: &mut egui::Ui, e: &CatalogEntry) {
        ui.add_space(6.0);
        match e.auth {
            CatalogAuth::None | CatalogAuth::Own => {}
            CatalogAuth::Oauth => {
                if crate::cards::white_pill(ui, "Sign in") {
                    crate::native_mcp::spawn(crate::native_mcp::Job::SignIn(e.id.clone()));
                }
            }
            CatalogAuth::Key => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.market.key)
                            .password(true)
                            .hint_text(crate::theme::hint("Paste the key"))
                            .desired_width(280.0),
                    );
                    if crate::cards::white_pill(ui, "Save key") {
                        self.arm_save_key(e);
                    }
                });
            }
            CatalogAuth::Google => {
                for (i, step) in GOOGLE_MANUAL_STEPS.iter().enumerate() {
                    crate::cards::help_text(ui, &format!("{}. {step}", i + 1));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> CatalogEntry {
        mcp_catalog::bundled().get(id).cloned().unwrap()
    }

    #[test]
    fn the_sign_in_line_follows_the_auth_type() {
        assert_eq!(sign_in_line(&entry("playwright")), "No sign-in needed. It works as soon as it's installed.");
        assert_eq!(sign_in_line(&entry("linear")), "Signs in to Linear in your browser right after Install.");
        assert!(sign_in_line(&entry("brave-search")).starts_with("Needs an API key. "));
        assert_eq!(sign_in_line(&entry("gmail")), "Uses your Google account, set up through Google Cloud.");
        assert_eq!(
            sign_in_line(&entry("microsoft-365")),
            "Signs in with your Microsoft account by itself the first time a chat uses it."
        );
        assert_eq!(market_chip(true), Some(("Installed", ChipTone::Live)));
        assert_eq!(market_chip(false), None);
    }

    #[test]
    fn save_key_asks_first_and_cancel_clears_the_key() {
        let mut cabin = Cabin::quiet_for_test();
        let brave = entry("brave-search");
        cabin.arm_save_key(&brave);
        assert!(cabin.confirm.is_none(), "an empty field asks nothing");
        assert_eq!(cabin.status, "Type the Brave Search key first");

        cabin.market.key = "sk-abcdefghijklmnopqrstuv".into();
        cabin.arm_save_key(&brave);
        assert!(matches!(
            &cabin.confirm,
            Some(confirm::ConfirmKind::SaveKey { name, label }) if name == "brave-search" && label == "Brave Search"
        ));
        let src = include_str!("confirm.rs");
        let cancel = src.split("Some(ConfirmAct::Cancel) => {").nth(1).unwrap();
        let cancel = &cancel[..cancel.find("None => {}").unwrap()];
        assert!(cancel.contains("ConfirmKind::SaveKey") && cancel.contains("self.market.key.clear()"), "{cancel}");
        assert!(!format!("{:?}", cabin.confirm).contains("sk-abc"), "the key never rides on the card");
    }

    #[test]
    fn installing_asks_nothing() {
        let src = include_str!("marketplace_ui.rs").replace("\r\n", "\n");
        let body = src.split("fn market_install(").nth(1).unwrap();
        let body = &body[..body.find("\n    }\n").unwrap()];
        assert!(body.contains("Job::Install") && !body.contains("confirm"), "{body}");
    }
}
