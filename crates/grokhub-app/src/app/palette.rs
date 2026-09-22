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

    pub(super) fn run_palette(&mut self, action: &str) {
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
            "nav:imagine" => {
                self.imagine_want_focus = true;
                self.nav = Nav::Imagine;
            }
            "nav:memory" => self.nav = Nav::Memory,
            "nav:settings" => self.nav = Nav::Settings,
            "oauth" => self.start_oauth(),
            "diag" => {
                self.status = diagnostics_bundle(
                    env!("CARGO_PKG_VERSION"),
                    self.has_key(),
                    HUB_KIND,
                    self.skill_list.len(),
                    self.last_receipt_ok,
                    self.board.len(),
                    &self.status,
                );
            }
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
        let mut picked: Option<String> = None;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }
        let cmds = filter_palette(&self.palette_q);
        let files = self.palette_files.clone();
        let root = self.palette_files_root.clone();
        let n = cmds.len() + files.len();
        egui::Window::new("Palette")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_q)
                        .hint_text("Go to…")
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
                            if ui
                                .add_sized(
                                    [ui.available_width(), 28.0],
                                    egui::SelectableLabel::new(i == self.palette_pick, label),
                                )
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
        if let Some(a) = picked {
            self.run_palette(&a);
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
