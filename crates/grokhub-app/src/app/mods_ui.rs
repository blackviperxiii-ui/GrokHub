//! Mods: the user's moddable games and, for the picked one, search and
//! browse over the sources the allowlist names for it. Installing waits for
//! the user's click and then a second click on a sheet that lists every
//! file ([`install`]); nothing is downloaded or written before the first.
//!
//! The rail is the Library's games joined with the allowlist, most recently
//! played first. A game the allowlist blocks, or hasn't checked, sits under
//! a collapsed "Can't mod" with its reason and never gets a search box.
//! Every lookup runs off the UI thread ([`jobs`]); the cached page shows at
//! once and the fresh one replaces it when it comes.

pub(super) mod agent;
mod drawer;
mod install;
mod jobs;
mod suggest;

use std::path::{Path, PathBuf};

use grokhub_core::battlenet::BnetGame;
use grokhub_core::mods::allowlist::{self, Check, Entry, UNKNOWN_REASON};
use grokhub_core::mods::{Browse, ModSummary, Page, Query, Source};
use grokhub_core::steam::SteamGame;

use super::*;

const VIEW_ID: &str = "mods-view";
/// How long typing must pause before a search goes out.
const QUERY_WAIT_MS: u64 = 400;
const RAIL_W: f32 = 230.0;
const PAGE_PAD: f32 = 24.0;
const DRAWER_W: f32 = 340.0;

/// A game Loadout may mod, as this PC has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ModGame {
    pub entry: &'static Entry,
    pub last_played: u64,
    pub playtime_min: u64,
    /// Battle.net install folders (WoW's AddOns live under them).
    pub installs: Vec<PathBuf>,
}

/// A game Loadout won't mod, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CantMod {
    pub key: String,
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Rail {
    pub games: Vec<ModGame>,
    pub cant: Vec<CantMod>,
}

impl Rail {
    fn add(
        &mut self,
        check: Check,
        key: String,
        name: String,
        played: (u64, u64),
        install: Option<PathBuf>,
    ) {
        match check {
            Check::Allowed(entry) => {
                if let Some(g) = self.games.iter_mut().find(|g| g.entry.id == entry.id) {
                    g.last_played = g.last_played.max(played.0);
                    g.playtime_min += played.1;
                    g.installs.extend(install);
                } else {
                    self.games.push(ModGame {
                        entry,
                        last_played: played.0,
                        playtime_min: played.1,
                        installs: install.into_iter().collect(),
                    });
                }
            }
            Check::Blocked(reason) => {
                let (key, name) = match allowlist::by_id(&key) {
                    Some(e) => (e.id.to_string(), e.name.to_string()),
                    None => (key, name),
                };
                if !self.cant.iter().any(|c| c.key == key) {
                    self.cant.push(CantMod { key, name, reason });
                }
            }
        }
    }
}

/// The rail: Library games the allowlist allows, most recently played then
/// most played, and the rest under "Can't mod" by name.
pub(super) fn mod_rail(steam: &[SteamGame], bnet: &[BnetGame], minecraft: bool) -> Rail {
    let mut rail = Rail::default();
    for g in steam {
        let check = allowlist::check_steam(g.app_id);
        let key = check
            .entry()
            .map(|e| e.id.to_string())
            .or_else(|| allowlist::by_steam_app(g.app_id).map(|e| e.id.to_string()))
            .unwrap_or_else(|| format!("steam:{}", g.app_id));
        rail.add(
            check,
            key,
            g.name.clone(),
            (g.last_played, g.playtime_min),
            None,
        );
    }
    for g in bnet {
        let check = allowlist::check_bnet(&g.product);
        let key = allowlist::by_bnet_product(&g.product)
            .map(|e| e.id.to_string())
            .unwrap_or_else(|| format!("bnet:{}", g.product));
        rail.add(check, key, g.name(), (0, 0), Some(g.install.clone()));
    }
    if minecraft {
        rail.add(
            allowlist::check("minecraft"),
            "minecraft".into(),
            "Minecraft".into(),
            (0, 0),
            None,
        );
    }
    rail.games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then(b.playtime_min.cmp(&a.playtime_min))
            .then(
                a.entry
                    .name
                    .to_lowercase()
                    .cmp(&b.entry.name.to_lowercase()),
            )
    });
    rail.cant.sort_by_key(|c| c.name.to_lowercase());
    rail
}

/// The Minecraft launcher's folder, when this PC has one.
fn minecraft_dir() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(".minecraft")),
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".minecraft")),
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support/minecraft")),
    ];
    candidates.into_iter().flatten().find(|d| d.is_dir())
}

/// Results the page may show: adult mods only with 18+ on.
pub(super) fn visible_mods(page: &Page, adult_ok: bool) -> Vec<&ModSummary> {
    page.mods.iter().filter(|m| adult_ok || !m.adult).collect()
}

/// What the page lists under a game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Popular,
    Trending,
    Updated,
    Installed,
}

impl Tab {
    const ORDER: [Tab; 4] = [Tab::Popular, Tab::Trending, Tab::Updated, Tab::Installed];

    fn label(self) -> &'static str {
        match self.browse() {
            Some(b) => b.label(),
            None => "Installed",
        }
    }

    fn browse(self) -> Option<Browse> {
        match self {
            Tab::Popular => Some(Browse::Popular),
            Tab::Trending => Some(Browse::Trending),
            Tab::Updated => Some(Browse::Updated),
            Tab::Installed => None,
        }
    }
}

/// One mod already on this PC, read from the game's own folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Installed {
    pub name: String,
    pub detail: String,
    pub link: Option<String>,
}

/// What a game has installed that Loadout didn't put there, read-only:
/// Steam Workshop items and WoW addons.
pub(super) fn installed_mods(game: &ModGame, steam_root: Option<&Path>) -> Vec<Installed> {
    let mut out = Vec::new();
    if let (Some(root), Some(app_id)) = (steam_root, game.entry.steam_app_id) {
        if game.entry.sources.contains(&Source::Workshop) {
            for item in grokhub_core::steam::workshop_items(root, app_id) {
                out.push(Installed {
                    name: format!("Workshop item {}", item.id),
                    detail: format!("{:.1} MB, from Steam Workshop", item.size as f64 / 1e6),
                    link: Some(grokhub_core::mods::workshop::item_url(&item.id.to_string())),
                });
            }
        }
    }
    for install in &game.installs {
        for flavor in grokhub_core::battlenet::wow_flavors(install) {
            let addons = flavor.dir(install).join("Interface").join("AddOns");
            let Ok(dir) = std::fs::read_dir(&addons) else {
                continue;
            };
            let mut names: Vec<String> = dir
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| !n.starts_with("Blizzard_") && !n.starts_with('.'))
                .collect();
            names.sort_by_key(|n| n.to_lowercase());
            out.extend(names.into_iter().map(|name| Installed {
                name,
                detail: format!("Addon, {}", flavor.label()),
                link: None,
            }));
        }
    }
    out
}

/// Open a mod's page: its site over https, or the Steam client.
pub(super) fn open_link(url: &str) -> Result<(), String> {
    if url.starts_with("steam://url/") {
        crate::desktop::open_path(url)
    } else {
        crate::desktop::open_url(url)
    }
}

/// What the page keeps between frames.
#[derive(Clone, Debug, Default)]
struct ModsView {
    /// An allowlist id, or a "Can't mod" key.
    picked: Option<String>,
    source: Option<Source>,
    tab: Tab,
    query: String,
    typed_at: u64,
    /// The query in use, once typing paused.
    searched: String,
    page: u32,
    /// The mod whose drawer is open.
    open: Option<String>,
    show_cant: bool,
    /// What the Installed tab read, for which game.
    installed: Option<(&'static str, Vec<Installed>)>,
    key_input: String,
}

impl ModsView {
    fn query(&self) -> Query {
        let browse = self.tab.browse().unwrap_or(Browse::Popular);
        let mut q = if self.searched.trim().is_empty() {
            Query::browse(browse)
        } else {
            let mut q = Query::search(&self.searched);
            q.browse = browse;
            q
        };
        q.page = self.page;
        q
    }

    /// Pick a game: its first source, Popular, no search.
    fn pick(&mut self, key: String) {
        *self = ModsView {
            picked: Some(key),
            show_cant: self.show_cant,
            ..Default::default()
        };
    }
}

impl Cabin {
    /// Opens Mods on one Library game.
    pub(super) fn show_in_mods(&mut self, ctx: &egui::Context, app_id: u32) {
        let key = allowlist::by_steam_app(app_id)
            .map(|e| e.id.to_string())
            .unwrap_or_else(|| format!("steam:{app_id}"));
        let mut view = ModsView::default();
        view.pick(key);
        view.show_cant = true;
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(VIEW_ID), view));
        self.set_nav_id("mods");
    }

    fn adult_ok(&self) -> bool {
        self.cfg.explicit_personalities && self.cfg.adult_confirmed
    }

    pub(super) fn ui_mods(&mut self, ui: &mut egui::Ui) {
        let id = egui::Id::new(VIEW_ID);
        let mut view: ModsView = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        let (_, steam) = self.steam_library(now_ms());
        let steam = steam.to_vec();
        let bnet = self.bnet_games();
        let rail = mod_rail(&steam, &bnet, minecraft_dir().is_some());
        egui::Frame::NONE
            .fill(crate::theme::bg())
            .inner_margin(egui::Margin::same(PAGE_PAD as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                install::recover_once(ui.ctx());
                ui.label(
                    egui::RichText::new("Mods")
                        .font(crate::theme::title_font(28.0))
                        .color(crate::theme::fg()),
                );
                ui.add_space(12.0);
                if let Err(e) = requested_card(ui) {
                    self.status = e;
                }
                let offline = grokhub_core::offline::enabled();
                let ranked = suggest::suggestions(ui.ctx(), &rail, self.adult_ok(), offline);
                suggest::row(ui, &rail, &mut view, &ranked);
                if view.picked.is_none() {
                    view.picked = rail.games.first().map(|g| g.entry.id.to_string());
                }
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(RAIL_W, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(RAIL_W);
                            mods_rail(ui, &rail, &mut view);
                        },
                    );
                    ui.vertical(|ui| self.mods_pane(ui, &rail, &mut view));
                });
            });
        ui.ctx().data_mut(|d| d.insert_temp(id, view));
        install::sheet(ui.ctx());
    }

    /// The right side for the picked game.
    fn mods_pane(&mut self, ui: &mut egui::Ui, rail: &Rail, view: &mut ModsView) {
        let picked = view.picked.clone().unwrap_or_default();
        if let Some(game) = rail.games.iter().find(|g| g.entry.id == picked) {
            let game = game.clone();
            self.mods_game(ui, &game, view);
        } else if let Some(cant) = rail.cant.iter().find(|c| c.key == picked) {
            cant_mod_pane(ui, cant);
        } else if picked.is_empty() {
            crate::ui_kit::empty_state(
                ui,
                "No moddable games found",
                "Mods lists the Steam and Battle.net games on this PC that Loadout can mod safely, and Minecraft when its launcher folder is here.",
                None,
            );
        } else {
            let reason = allowlist::by_id(&picked)
                .map(|e| e.blocked.to_string())
                .unwrap_or_else(|| UNKNOWN_REASON.to_string());
            cant_mod_pane(
                ui,
                &CantMod {
                    key: picked.clone(),
                    name: "This game".into(),
                    reason,
                },
            );
        }
    }

    fn mods_game(&mut self, ui: &mut egui::Ui, game: &ModGame, view: &mut ModsView) {
        let entry = game.entry;
        let source = view
            .source
            .filter(|s| entry.sources.contains(s))
            .or_else(|| entry.sources.first().copied());
        ui.label(
            egui::RichText::new(entry.name)
                .size(crate::theme::TYPE_HEADLINE)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.label(
            egui::RichText::new(entry.mode_note())
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::amber()),
        );
        install::status(ui);
        if game.last_played > 0 {
            suggest::toggle(ui, game);
        }
        ui.add_space(10.0);
        let offline = grokhub_core::offline::enabled();
        search_row(ui, view, offline);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for tab in Tab::ORDER {
                if crate::cards::felt_segment(ui, &tab.label().to_uppercase(), view.tab == tab)
                    .clicked()
                {
                    view.tab = tab;
                    view.page = 0;
                    view.open = None;
                }
            }
            if entry.sources.len() > 1 {
                ui.add_space(12.0);
                for s in entry.sources {
                    if crate::cards::felt_segment(ui, s.label(), source == Some(*s)).clicked() {
                        view.source = Some(*s);
                        view.page = 0;
                        view.open = None;
                    }
                }
            }
        });
        ui.add_space(12.0);
        let Some(source) = source else {
            return;
        };
        if view.tab == Tab::Installed {
            self.installed_tab(ui, game, view);
        } else {
            self.results(ui, game, source, view, offline);
        }
        ui.add_space(16.0);
        self.key_rows(ui, entry, view);
    }

    fn results(
        &mut self,
        ui: &mut egui::Ui,
        game: &ModGame,
        source: Source,
        view: &mut ModsView,
        offline: bool,
    ) {
        let game_id = game.entry.id;
        let q = view.query();
        let slot = if offline {
            jobs::Slot {
                value: jobs::cached_page(game_id, source, &q),
                busy: false,
                error: Some("Offline only is on. Installed mods still show.".into()),
            }
        } else {
            jobs::page(ui.ctx(), game_id, source, &q)
        };
        if let Some(err) = &slot.error {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(err)
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::muted()),
                );
                if !offline && crate::cards::ghost_pill(ui, "Retry") {
                    jobs::retry_page(game_id, source, &q);
                }
            });
            if let Some(url) = err
                .split_whitespace()
                .find(|w| w.starts_with("steam://url/"))
            {
                if crate::cards::ghost_pill(ui, "Open the Workshop in Steam") {
                    let url = url.trim_end_matches(['.', ',']);
                    if let Err(e) = open_link(url) {
                        self.status = e;
                    }
                }
            }
        }
        if slot.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new(if slot.value.is_some() {
                        "Refreshing…"
                    } else {
                        "Looking…"
                    })
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
                );
            });
        }
        let Some(page) = slot.value else {
            return;
        };
        let shown = visible_mods(&page, self.adult_ok());
        if shown.is_empty() && !slot.busy {
            ui.label(
                egui::RichText::new("Nothing here yet.")
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
            );
            return;
        }
        let open = view
            .open
            .as_ref()
            .and_then(|id| shown.iter().find(|m| &m.id == id).map(|m| (*m).clone()));
        let full = ui.available_width();
        let list_w = if open.is_some() {
            (full - DRAWER_W - 16.0).max(240.0)
        } else {
            full
        };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.allocate_ui_with_layout(
                egui::vec2(list_w, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(list_w);
                    egui::ScrollArea::vertical()
                        .id_salt("mods-results")
                        .max_height((ui.available_height() - 140.0).max(200.0))
                        .show(ui, |ui| {
                            for m in &shown {
                                if mod_card(ui, m, view.open.as_deref() == Some(&m.id)) {
                                    view.open = Some(m.id.clone());
                                }
                            }
                            pager(ui, view, &page, shown.len());
                        });
                },
            );
            if let Some(m) = open {
                ui.allocate_ui_with_layout(
                    egui::vec2(DRAWER_W, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(DRAWER_W);
                        match drawer::show(ui, game, source, &m, offline) {
                            drawer::Act::None => {}
                            drawer::Act::Close => view.open = None,
                            drawer::Act::Failed(e) => self.status = e,
                        }
                    },
                );
            }
        });
    }

    fn installed_tab(&mut self, ui: &mut egui::Ui, game: &ModGame, view: &mut ModsView) {
        let fresh = view
            .installed
            .as_ref()
            .is_some_and(|(id, _)| *id == game.entry.id);
        if !fresh {
            let root = grokhub_core::steam::detect_root();
            view.installed = Some((game.entry.id, installed_mods(game, root.as_deref())));
        }
        let ours = install::installed_now(game.entry.id);
        let rows: Vec<Installed> = view
            .installed
            .as_ref()
            .map(|(_, rows)| rows.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| {
                !ours
                    .iter()
                    .any(|m| m.dirs.iter().any(|d| d.ends_with(&format!("/{}", r.name))))
            })
            .collect();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} on this PC", rows.len() + ours.len()))
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
            );
            if crate::cards::ghost_pill(ui, "Refresh") {
                view.installed = None;
            }
        });
        for m in &ours {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&m.name)
                        .size(crate::theme::TYPE_BODY)
                        .strong()
                        .color(crate::theme::fg()),
                );
                ui.label(
                    egui::RichText::new(format!("{}, installed by Loadout", m.version))
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                );
                if !install::busy() && change_undo::click_pill(ui, "Uninstall") {
                    install::start_remove(ui.ctx(), game, m);
                }
            });
        }
        if rows.is_empty() && ours.is_empty() {
            ui.label(
                egui::RichText::new(
                    "No mods found for this game yet. Mods Loadout installs will list here too.",
                )
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::muted()),
            );
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("mods-installed")
            .max_height((ui.available_height() - 140.0).max(200.0))
            .show(ui, |ui| {
                for row in &rows {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&row.name)
                                .size(crate::theme::TYPE_BODY)
                                .strong()
                                .color(crate::theme::fg()),
                        );
                        ui.label(
                            egui::RichText::new(&row.detail)
                                .size(crate::theme::TYPE_CAPTION)
                                .color(crate::theme::muted()),
                        );
                        if let Some(link) = &row.link {
                            if crate::cards::ghost_pill(ui, "Open") {
                                if let Err(e) = open_link(link) {
                                    self.status = e;
                                }
                            }
                        }
                    });
                }
            });
    }

    /// Optional keys for the sources this game uses: a Nexus Mods personal
    /// key (Trending) and a Steam Web API key (Workshop search). Saving one
    /// asks for approval first, then seals it in the keyring.
    fn key_rows(&mut self, ui: &mut egui::Ui, entry: &Entry, view: &mut ModsView) {
        let rows: Vec<(Source, &str)> = entry
            .sources
            .iter()
            .filter_map(|s| match s {
                Source::Nexus => Some((
                    *s,
                    "Nexus Mods personal key: optional, for Trending. For your own use only.",
                )),
                Source::Workshop => Some((
                    *s,
                    "Steam Web API key: optional, to search the Workshop here.",
                )),
                _ => None,
            })
            .collect();
        if rows.is_empty() {
            return;
        }
        egui::CollapsingHeader::new(
            egui::RichText::new("Source keys")
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        )
        .id_salt("mods-keys")
        .show(ui, |ui| {
            for (source, line) in rows {
                let saved = jobs::has_key(source);
                ui.label(
                    egui::RichText::new(line)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::fg()),
                );
                ui.horizontal(|ui| {
                    if saved {
                        ui.label(
                            egui::RichText::new("Saved")
                                .size(crate::theme::TYPE_CAPTION)
                                .color(crate::theme::amber()),
                        );
                        if crate::cards::ghost_pill(ui, "Forget") {
                            grokhub_agent::harness::forget_mod_key(
                                &crate::config::config_dir(),
                                source.id(),
                            );
                            jobs::reset();
                            jobs::set_key_note(format!("Forgot the {} key.", source.label()));
                        }
                    } else {
                        ui.add(
                            egui::TextEdit::singleline(&mut view.key_input)
                                .password(true)
                                .hint_text("Paste the key")
                                .desired_width(220.0),
                        );
                        if crate::cards::ghost_pill(ui, "Save") {
                            save_key(source, std::mem::take(&mut view.key_input));
                        }
                    }
                });
            }
            if let Some(note) = jobs::key_note() {
                ui.label(
                    egui::RichText::new(note)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                );
            }
        });
    }
}

/// The mod the chat asked to set up: one card, the same one way to get the
/// file as in the drawer, and Dismiss. Nothing has been fetched yet.
fn requested_card(ui: &mut egui::Ui) -> Result<(), String> {
    let Some(r) = install::requested() else {
        return Ok(());
    };
    let key = install::mod_key(r.source, &r.m.id);
    let installed = install::installed_now(r.game.entry.id);
    let blocked = install::blocked(&r.game);
    let offer = drawer::offer(&r.f, &key, &installed, blocked.as_deref());
    let mut out = Ok(());
    egui::Frame::NONE
        .fill(crate::theme::surface())
        .stroke(egui::Stroke::new(1.0, crate::theme::amber()))
        .corner_radius(crate::theme::RADIUS_SM)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(720.0));
            ui.label(
                egui::RichText::new(format!("Install {} for {}?", r.m.name, r.game.entry.name))
                    .size(crate::theme::TYPE_LABEL)
                    .strong()
                    .color(crate::theme::fg()),
            );
            ui.label(
                egui::RichText::new(format!(
                    "{} · {} · from {} · asked for in chat",
                    r.f.file_name,
                    drawer::file_line(&r.f),
                    r.source.label()
                ))
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
            );
            ui.label(
                egui::RichText::new(r.game.entry.risk)
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::amber()),
            );
            if let drawer::Offer::Blocked(why) = &offer {
                ui.label(
                    egui::RichText::new(why)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::amber()),
                );
            }
            ui.horizontal(|ui| {
                out = drawer::file_actions(ui, &r.game, r.source, &r.m, &r.f, offer);
                if crate::cards::ghost_pill(ui, "Dismiss") {
                    install::drop_request();
                }
            });
        });
    ui.add_space(12.0);
    out
}

/// Approve, then seal the key, off the UI thread.
fn save_key(source: Source, key: String) {
    if key.trim().is_empty() {
        jobs::set_key_note("Type the key first.".into());
        return;
    }
    jobs::set_key_note("Approve the card to save the key.".into());
    std::thread::spawn(move || {
        let name = format!("mods-{}", source.id());
        let note = if !crate::secrets::wait_connector_credentials(&name, "save its API key") {
            format!("Nothing saved. The {} key was denied.", source.label())
        } else {
            match grokhub_agent::harness::seal_mod_key(
                &crate::config::config_dir(),
                source.id(),
                &key,
            ) {
                Ok(()) => {
                    jobs::reset();
                    format!("Saved the {} key.", source.label())
                }
                Err(e) => format!("Couldn't save the key: {e}"),
            }
        };
        jobs::set_key_note(note);
    });
}

/// The search field, debounced; greyed while Offline only is on.
fn search_row(ui: &mut egui::Ui, view: &mut ModsView, offline: bool) {
    let now = now_ms();
    let w = ui.available_width().clamp(160.0, 420.0);
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0, crate::theme::border()))
        .corner_radius(crate::theme::RADIUS_SM)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            let hint = if offline {
                "Offline only is on"
            } else {
                "Search mods"
            };
            let edit = ui.add_enabled(
                !offline,
                egui::TextEdit::singleline(&mut view.query)
                    .hint_text(hint)
                    .frame(egui::Frame::NONE)
                    .desired_width(w),
            );
            if edit.changed() {
                view.typed_at = now;
            }
        });
    if view.query != view.searched {
        let waited = now.saturating_sub(view.typed_at);
        if waited >= QUERY_WAIT_MS {
            view.searched = view.query.clone();
            view.page = 0;
            view.open = None;
            if view.tab == Tab::Installed {
                view.tab = Tab::Popular;
            }
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(QUERY_WAIT_MS - waited));
        }
    }
}

/// The left rail: moddable games, then "Can't mod" collapsed.
fn mods_rail(ui: &mut egui::Ui, rail: &Rail, view: &mut ModsView) {
    egui::ScrollArea::vertical()
        .id_salt("mods-rail")
        .show(ui, |ui| {
            for g in &rail.games {
                let on = view.picked.as_deref() == Some(g.entry.id);
                if rail_row(ui, g.entry.name, &g.entry.mode_note(), on) {
                    view.pick(g.entry.id.to_string());
                }
            }
            if rail.cant.is_empty() {
                return;
            }
            ui.add_space(12.0);
            let label = format!(
                "{} CAN'T MOD ({})",
                if view.show_cant { "HIDE" } else { "SHOW" },
                rail.cant.len()
            );
            if ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(crate::theme::TYPE_CAPTION)
                            .extra_letter_spacing(1.2)
                            .color(crate::theme::muted()),
                    )
                    .sense(egui::Sense::click()),
                )
                .clicked()
            {
                view.show_cant = !view.show_cant;
            }
            if view.show_cant {
                for c in &rail.cant {
                    let on = view.picked.as_deref() == Some(c.key.as_str());
                    if rail_row(ui, &c.name, &c.reason, on) {
                        let keep = view.show_cant;
                        view.pick(c.key.clone());
                        view.show_cant = keep;
                    }
                }
            }
        });
}

fn rail_row(ui: &mut egui::Ui, name: &str, note: &str, on: bool) -> bool {
    let fill = if on {
        crate::theme::raised()
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(crate::theme::RADIUS_SM)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(name)
                    .size(crate::theme::TYPE_BODY)
                    .strong()
                    .color(crate::theme::fg()),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(note)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                )
                .truncate(),
            );
        })
        .response
        .interact(egui::Sense::click())
        .clicked()
}

/// A game Loadout won't mod: its reason, and nothing to search.
fn cant_mod_pane(ui: &mut egui::Ui, cant: &CantMod) {
    ui.label(
        egui::RichText::new(&cant.name)
            .size(crate::theme::TYPE_HEADLINE)
            .strong()
            .color(crate::theme::fg()),
    );
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(&cant.reason)
            .size(crate::theme::TYPE_BODY)
            .color(crate::theme::signal_red()),
    );
    ui.label(
        egui::RichText::new(
            "Loadout only mods games and modes where mods can't affect anyone else's play.",
        )
        .size(crate::theme::TYPE_CAPTION)
        .color(crate::theme::muted()),
    );
}

/// "1.2M", "34K", "980".
pub(super) fn short_count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}K", n / 1_000),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// One result: name, author, counts, when it changed. True when clicked.
fn mod_card(ui: &mut egui::Ui, m: &ModSummary, open: bool) -> bool {
    let stroke = if open {
        crate::theme::amber()
    } else {
        crate::theme::border()
    };
    let resp = egui::Frame::NONE
        .fill(crate::theme::surface())
        .stroke(egui::Stroke::new(1.0, stroke))
        .corner_radius(crate::theme::RADIUS_SM)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&m.name)
                        .size(crate::theme::TYPE_LABEL)
                        .strong()
                        .color(crate::theme::fg()),
                );
                if m.adult {
                    ui.label(
                        egui::RichText::new("18+")
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::signal_red()),
                    );
                }
            });
            let mut facts = vec![format!("by {}", m.author)];
            facts.push(format!("{} downloads", short_count(m.downloads)));
            if m.likes > 0 {
                facts.push(format!("{} likes", short_count(m.likes)));
            }
            if m.updated > 0 {
                facts.push(format!(
                    "updated {}",
                    super::library_ui::last_played_label(m.updated, now_ms() / 1000).to_lowercase()
                ));
            }
            ui.label(
                egui::RichText::new(facts.join(" · "))
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
            );
            if !m.summary.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&m.summary)
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::fg()),
                    )
                    .truncate(),
                );
            }
        })
        .response
        .interact(egui::Sense::click());
    ui.add_space(6.0);
    resp.clicked()
}

fn pager(ui: &mut egui::Ui, view: &mut ModsView, page: &Page, shown: usize) {
    let more = match page.total {
        Some(total) => {
            u64::from(view.page + 1) * u64::from(grokhub_core::mods::source::PAGE_SIZE) < total
        }
        None => page.mods.len() >= grokhub_core::mods::source::PAGE_SIZE as usize,
    };
    ui.horizontal(|ui| {
        if view.page > 0 && crate::cards::ghost_pill(ui, "Previous") {
            view.page -= 1;
            view.open = None;
        }
        ui.label(
            egui::RichText::new(format!("Page {} · {shown} shown", view.page + 1))
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
        if more && crate::cards::ghost_pill(ui, "Next") {
            view.page += 1;
            view.open = None;
        }
    });
}

#[cfg(test)]
#[path = "mods_ui_tests.rs"]
mod tests;
