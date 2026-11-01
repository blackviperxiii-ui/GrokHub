//! The open mod's drawer: what it is, its description as plain text, its
//! files and what they need, a link to its page on its own site, and per
//! file the one way to get it: Install (a direct link), Download on the
//! site then "I downloaded it", or Subscribe in Steam.

use eframe::egui;
use grokhub_core::mods::install::Manifest;
use grokhub_core::mods::{Download, ModFile, ModSummary, Source};

use super::{install, jobs, ModGame};

pub(super) enum Act {
    None,
    Close,
    Failed(String),
}

/// How many files the drawer lists, newest first as the source sends them.
const FILES_SHOWN: usize = 6;

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(crate::theme::TYPE_CAPTION)
            .extra_letter_spacing(1.2)
            .strong()
            .color(crate::theme::muted()),
    );
}

/// "4.2 MB" or "880 KB".
pub(super) fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else {
        format!("{} KB", bytes.div_ceil(1_000))
    }
}

/// What getting a file would take, in a few words.
pub(super) fn file_line(f: &ModFile) -> String {
    let mut parts = Vec::new();
    if !f.version.is_empty() {
        parts.push(f.version.clone());
    }
    if f.size > 0 {
        parts.push(size_label(f.size));
    }
    match f.download {
        Download::Direct(_) => {}
        Download::Browser(_) => parts.push("download on the site".into()),
        Download::Steam(_) => parts.push("subscribe in Steam".into()),
    }
    parts.join(" · ")
}

pub(super) fn show(
    ui: &mut egui::Ui,
    game: &ModGame,
    source: Source,
    m: &ModSummary,
    offline: bool,
) -> Act {
    let game_id = game.entry.id;
    let mut act = Act::None;
    egui::Frame::NONE
        .fill(crate::theme::surface())
        .stroke(egui::Stroke::new(1.0, crate::theme::border()))
        .corner_radius(crate::theme::RADIUS_SM)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&m.name)
                        .size(crate::theme::TYPE_TITLE)
                        .strong()
                        .color(crate::theme::fg()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::cards::ghost_pill(ui, "Close") {
                        act = Act::Close;
                    }
                });
            });
            ui.label(
                egui::RichText::new(format!("by {} on {}", m.author, source.label()))
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
            );
            ui.add_space(8.0);
            let site = format!("Open on {}", source.label());
            if !m.page_url.is_empty() && crate::cards::ghost_pill(ui, &site) {
                if let Err(e) = super::open_link(&m.page_url) {
                    act = Act::Failed(e);
                }
            }
            ui.add_space(8.0);
            if offline {
                ui.label(
                    egui::RichText::new("Offline only is on.")
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                );
                return;
            }
            let detail = jobs::detail(ui.ctx(), game_id, source, &m.id);
            let files = jobs::files(ui.ctx(), game_id, source, &m.id);
            if detail.busy || files.busy {
                ui.spinner();
            }
            if detail.error.is_some() || files.error.is_some() {
                let err = detail.error.or(files.error).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(err)
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    );
                    if crate::cards::ghost_pill(ui, "Retry") {
                        jobs::retry_mod(game_id, source, &m.id);
                    }
                });
            }
            egui::ScrollArea::vertical()
                .id_salt("mods-drawer")
                .max_height((ui.available_height() - 40.0).max(160.0))
                .show(ui, |ui| {
                    if let Some(files) = &files.value {
                        if let Err(e) = files_list(ui, game, source, m, files) {
                            act = Act::Failed(e);
                        }
                    }
                    let text = detail
                        .value
                        .as_ref()
                        .map(|d| d.description.as_str())
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or(m.summary.as_str());
                    if !text.is_empty() {
                        ui.add_space(8.0);
                        caption(ui, "ABOUT");
                        ui.label(
                            egui::RichText::new(text)
                                .size(crate::theme::TYPE_BODY)
                                .color(crate::theme::fg()),
                        );
                    }
                });
        });
    act
}

/// What a file's row offers, given what is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Offer {
    Install,
    Update(String),
    Installed,
    Browser(String),
    Steam(String),
    /// Loadout won't write this game; the line says why.
    Blocked(String),
}

pub(super) fn offer(
    f: &ModFile,
    key: &str,
    installed: &[Manifest],
    blocked: Option<&str>,
) -> Offer {
    if let Download::Steam(id) = f.download {
        return Offer::Steam(grokhub_core::mods::workshop::item_url(&id.to_string()));
    }
    if let Some(why) = blocked {
        return Offer::Blocked(why.to_string());
    }
    let version = if f.version.is_empty() {
        &f.name
    } else {
        &f.version
    };
    match installed.iter().find(|i| i.mod_key == key) {
        Some(i) if &i.version == version => Offer::Installed,
        Some(i) => Offer::Update(i.version.clone()),
        None => match &f.download {
            Download::Direct(_) => Offer::Install,
            Download::Browser(url) => Offer::Browser(url.clone()),
            Download::Steam(_) => unreachable!("handled above"),
        },
    }
}

fn file_actions(
    ui: &mut egui::Ui,
    game: &ModGame,
    source: Source,
    m: &ModSummary,
    f: &ModFile,
    offer: Offer,
) -> Result<(), String> {
    let ctx = ui.ctx().clone();
    let busy = install::busy();
    let start = |url: &str| {
        install::start(
            &ctx,
            game.clone(),
            source,
            m.clone(),
            f.clone(),
            install::Origin::Net(url.to_string()),
        )
    };
    let url = match &f.download {
        Download::Direct(u) => u.as_str(),
        _ => "",
    };
    ui.horizontal(|ui| match offer {
        Offer::Install => {
            if !busy && super::super::change_undo::click_pill(ui, "Install") {
                start(url);
            }
            Ok(())
        }
        Offer::Update(was) => {
            if !busy && super::super::change_undo::click_pill(ui, "Update") {
                start(url);
            }
            muted(ui, &format!("{was} installed"));
            Ok(())
        }
        Offer::Installed => {
            muted(ui, "Installed");
            Ok(())
        }
        Offer::Steam(link) => {
            if crate::cards::ghost_pill(ui, "Subscribe in Steam") {
                return super::open_link(&link);
            }
            Ok(())
        }
        Offer::Browser(link) => {
            let site = format!("Download on {}", source.label());
            if crate::cards::ghost_pill(ui, &site) {
                install::wait_for_browser(source, m, f);
                return super::open_link(&link);
            }
            let waiting = install::pickup().is_some_and(|p| {
                p.key == install::mod_key(source, &m.id) && p.file_name == f.file_name
            });
            if waiting && !busy && super::super::change_undo::click_pill(ui, "I downloaded it") {
                install::picked_up(&ctx, game, source, m, f);
            }
            Ok(())
        }
        Offer::Blocked(_) => Ok(()),
    })
    .inner
}

fn muted(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(crate::theme::TYPE_CAPTION)
            .color(crate::theme::muted()),
    );
}

fn files_list(
    ui: &mut egui::Ui,
    game: &ModGame,
    source: Source,
    m: &ModSummary,
    files: &[ModFile],
) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    let key = install::mod_key(source, &m.id);
    let installed = install::installed_now(game.entry.id);
    let blocked = install::blocked(game);
    caption(ui, "FILES");
    if let Some(why) = &blocked {
        ui.label(
            egui::RichText::new(why)
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::amber()),
        );
    }
    let mut out = Ok(());
    for f in files.iter().take(FILES_SHOWN) {
        let name = if f.name.is_empty() {
            &f.file_name
        } else {
            &f.name
        };
        ui.label(
            egui::RichText::new(name)
                .size(crate::theme::TYPE_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.label(
            egui::RichText::new(file_line(f))
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
        let o = offer(f, &key, &installed, blocked.as_deref());
        if let Err(e) = file_actions(ui, game, source, m, f, o) {
            out = Err(e);
        }
    }
    let deps: Vec<&str> = files
        .iter()
        .find(|f| f.primary)
        .or(files.first())
        .map(|f| f.deps.iter().map(String::as_str).collect())
        .unwrap_or_default();
    if !deps.is_empty() {
        ui.add_space(6.0);
        caption(ui, "NEEDS");
        ui.label(
            egui::RichText::new(deps.join(", "))
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::fg()),
        );
    }
    out
}
