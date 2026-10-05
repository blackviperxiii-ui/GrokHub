//! Command palette and nested file search.

use super::*;

impl Cabin {

    pub(super) fn open_palette(&mut self) {
        self.palette_open = true;
        self.palette_focus = true;
        self.palette_pick = 0;
        self.palette_q.clear();
        self.palette_files.clear();
        self.palette_files_q.clear();
        self.palette_files_root.clear();
        self.palette_file_rx = None;
        self.settings_menu_open = false;
    }

    pub(super) fn run_palette(&mut self, ctx: &egui::Context, action: &str) {
        self.palette_open = false;
        self.settings_menu_open = false;
        match action {
            "nav:chat" => {
                self.new_thread(false);
                self.nav = Nav::Chat;
            }
            "nav:night" => self.nav = Nav::Night,
            "nav:history" => self.nav = Nav::History,
            "nav:devices" => self.nav = Nav::Devices,
            "nav:connectors" => self.nav = Nav::Connectors,
            "nav:command" => self.nav = Nav::Command,
            "nav:agents" => self.nav = Nav::Agents,
            "nav:eyes" => {
                self.open_recent_chat();
                self.nav = Nav::Chat;
            }
            "nav:skills" => self.nav = Nav::Skills,
            "nav:board" => self.nav = Nav::Workboard,
            "nav:pulse" => self.nav = Nav::Pulse,
            "nav:imagine" => {
                self.imagine_want_focus = true;
                self.nav = Nav::Imagine;
            }
            "nav:memory" => self.nav = Nav::Memory,
            "nav:settings" => self.nav = Nav::Settings,
            "oauth" => self.start_oauth(),
            "diag" => self.copy_diagnostics(ctx),
            "shortcuts" => self.shortcuts_open = true,
            "voice" => self.listen_voice(),
            slash if slash.starts_with('/') => self.run_slash_line(slash),
            path if path.starts_with("file:") => {
                if let Some(shown) = palette_file_shown(path) {
                    match crate::desktop::open_path(shown) {
                        Ok(()) => self.status = shown.to_string(),
                        Err(e) => self.status = e,
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn ui_palette(&mut self, ctx: &egui::Context) {
        self.tick_palette_search(ctx);
        let mut close = false;
        // Picking a page anywhere else closes the palette: a nav change since it
        // opened, or a click outside it (not the click that opened it).
        let opening = self.palette_focus;
        let nav_key = egui::Id::new("palette-opened-on");
        let here = self.nav_id().to_string();
        if opening {
            ctx.data_mut(|d| d.insert_temp(nav_key, here.clone()));
        } else if ctx
            .data(|d| d.get_temp::<String>(nav_key))
            .is_some_and(|was| was != here)
        {
            close = true;
        }
        let mut picked: Option<String> = None;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }
        let cmds = filter_palette(&self.palette_q);
        let files = self.palette_files.clone();
        let root = self.palette_files_root.clone();
        let n = cmds.len() + files.len();
        let shown = egui::Window::new("Search")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_q)
                        .hint_text(crate::theme::hint("Search pages and commands"))
                        .desired_width(360.0),
                );
                if self.palette_focus {
                    edit.request_focus();
                    self.palette_focus = false;
                }
                self.palette_pick = slash_pick_step(self.palette_pick, n, 0);
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                    self.palette_pick = slash_pick_step(self.palette_pick, n, 1);
                } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
                {
                    self.palette_pick = slash_pick_step(self.palette_pick, n, -1);
                } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    picked = palette_row_action(&cmds, &files, &root, self.palette_pick);
                }
                egui::ScrollArea::vertical()
                    .max_height(PALETTE_LIST_H)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_width(360.0);
                        for i in 0..n {
                            let label = if i < cmds.len() {
                                cmds[i].0.to_string()
                            } else {
                                files[i - cmds.len()].clone()
                            };
                            // Every row takes the shortcut slot, so labels line up
                            // whether or not a key shows at the right.
                            let keys = cmds.get(i).and_then(|c| palette_shortcut(c.1));
                            let row = egui::Button::selectable(i == self.palette_pick, label)
                                .shortcut_text(
                                    egui::RichText::new(keys.unwrap_or_default())
                                        .size(crate::theme::FONT_TIP)
                                        .color(crate::theme::muted()),
                                );
                            if ui
                                .add_sized([ui.available_width(), 28.0], row)
                                .clicked()
                            {
                                picked = palette_row_action(&cmds, &files, &root, i);
                            }
                        }
                    });
                if crate::cards::ghost_pill(ui, "Close") {
                    close = true;
                }
            });
        if !opening {
            if let Some(win) = shown.map(|s| s.response.rect) {
                let outside = ctx.input(|i| {
                    i.pointer.any_click()
                        && i.pointer.interact_pos().is_some_and(|p| !win.contains(p))
                });
                if outside {
                    close = true;
                }
            }
        }
        if let Some(a) = picked {
            self.run_palette(ctx, &a);
        }
        if close {
            self.palette_open = false;
        }
    }

    /// File hits for the open palette. A saved empty result for this query is not walked again.
    pub(super) fn tick_palette_search(&mut self, ctx: &egui::Context) {
        let q = self.palette_q.trim().to_string();
        if q.is_empty() {
            self.palette_files.clear();
            self.palette_files_q.clear();
            self.palette_files_root.clear();
            return;
        }
        let root_now = self.palette_search_root();
        palette_forget_stale_walk(
            &mut self.palette_files,
            &mut self.palette_files_q,
            &mut self.palette_files_root,
            &q,
            &root_now,
        );
        if self.palette_file_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
            return;
        }
        if palette_search_is_saved(&self.palette_files_q, &self.palette_files_root, &q, &root_now) {
            return;
        }
        self.kick_palette_search();
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
    }

    pub(super) fn kick_palette_search(&mut self) {
        let q = self.palette_q.trim().to_string();
        if q.is_empty() {
            return;
        }
        let root = self.palette_search_root();
        let (tx, rx) = mpsc::channel();
        self.palette_file_rx = Some(rx);
        std::thread::spawn(move || {
            let hits = search_place(std::path::Path::new(&root), &q);
            let _ = tx.send((q, root, hits));
        });
    }

    pub(super) fn poll_palette_search(&mut self) {
        let Some(rx) = self.palette_file_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((q, root, hits)) => {
                if self.palette_open && q == self.palette_q.trim() {
                    self.palette_files = hits;
                    self.palette_files_q = q;
                    self.palette_files_root = root;
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.palette_file_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    /// Bound project, or ~/GrokHub-Work when nothing is bound. Never the process cwd.
    pub(super) fn palette_search_root(&self) -> String {
        let bound = crate::helpers::expand_home(self.cfg.project_dir.trim());
        if !bound.trim().is_empty() {
            return bound;
        }
        self.work_root()
    }
}

/// Contiguous scope runs, in sheet order. A scope that comes back later is a new group.
pub fn group_shortcut_scopes<'a>(scopes: impl IntoIterator<Item = &'a str>) -> Vec<(&'a str, usize)> {
    let mut out: Vec<(&str, usize)> = Vec::new();
    for scope in scopes {
        match out.last_mut() {
            Some((prev, n)) if *prev == scope => *n += 1,
            _ => out.push((scope, 1)),
        }
    }
    out
}

/// Two-column shortcut sheet. Keys are monospace chips; the action is muted.
pub(super) fn paint_shortcut_sheet(ui: &mut egui::Ui) {
    let mut last = "";
    egui::Grid::new("shortcut-sheet")
        .num_columns(2)
        .spacing(egui::vec2(12.0, 6.0))
        .show(ui, |ui| {
            for row in grokhub_core::SHORTCUTS {
                if row.scope != last {
                    ui.label(
                        egui::RichText::new(row.scope)
                            .size(crate::theme::FONT_CHROME)
                            .strong()
                            .color(crate::theme::fg()),
                    );
                    ui.end_row();
                    last = row.scope;
                }
                shortcut_key_chip(ui, row.keys);
                ui.label(
                    egui::RichText::new(row.action)
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::muted()),
                );
                ui.end_row();
            }
        });
}

fn shortcut_key_chip(ui: &mut egui::Ui, keys: &str) {
    let font = egui::FontId::monospace(crate::theme::FONT_TIP);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(keys.to_owned(), font, crate::theme::fg()));
    let pad = egui::vec2(8.0, 3.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        crate::theme::CHROME_RADIUS,
        crate::theme::surface(),
    );
    ui.painter().galley(
        egui::pos2(rect.min.x + pad.x, rect.center().y - galley.size().y * 0.5),
        galley,
        crate::theme::fg(),
    );
}

#[cfg(test)]
mod tests {
    use super::group_shortcut_scopes;

    #[test]
    fn shortcut_sheet_keeps_each_scope_together() {
        let scopes: Vec<&str> = grokhub_core::SHORTCUTS.iter().map(|s| s.scope).collect();
        let groups = group_shortcut_scopes(scopes.iter().copied());
        assert!(groups.len() >= 2, "{groups:?}");
        assert_eq!(
            groups.iter().map(|(_, n)| *n).sum::<usize>(),
            grokhub_core::SHORTCUTS.len()
        );
        let mut seen = Vec::new();
        for scope in &scopes {
            if seen.last() != Some(scope) {
                assert!(!seen.contains(scope), "{scope} is split across the sheet");
                seen.push(*scope);
            }
        }
        assert_eq!(groups[0].0, "Global");
        let src = include_str!("palette.rs");
        let sheet = src
            .split("fn paint_shortcut_sheet(")
            .nth(1)
            .and_then(|s| s.split("fn shortcut_key_chip(").next())
            .expect("sheet");
        assert!(
            sheet.contains("egui::Grid")
                && sheet.contains("SHORTCUTS")
                && sheet.contains("shortcut_key_chip")
                && sheet.contains("theme::muted()"),
            "{sheet}"
        );
        let chip = src
            .split("fn shortcut_key_chip(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("chip");
        assert!(
            chip.contains("FontId::monospace")
                && chip.contains("CHROME_RADIUS")
                && chip.contains("theme::surface()")
                && chip.contains("theme::fg()"),
            "{chip}"
        );
    }
}
