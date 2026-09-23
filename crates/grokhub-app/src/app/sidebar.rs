//! Rail, titlebar, and avatar.

use super::*;

pub(super) fn fit_rail_label(ui: &egui::Ui, label: &str, max_w: f32) -> String {
    let font = egui::FontId::proportional(crate::theme::FONT_CHROME);
    let fits = |s: &str| {
        ui.fonts(|f| f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE))
            .size()
            .x
            <= max_w
    };
    if fits(label) {
        return label.to_string();
    }
    let mut t = label.to_string();
    while t.pop().is_some() {
        let candidate = format!("{}…", t.trim_end());
        if fits(&candidate) {
            return candidate;
        }
    }
    "…".into()
}

impl Cabin {
    pub(super) fn ui_titlebar(&mut self, ctx: &egui::Context) {
        let mut run_pending_update = false;
        let update_chip = update_chip_label(self.update_pending_now());
        egui::TopBottomPanel::top("titlebar")
            .exact_height(crate::theme::TITLEBAR_H)
            .frame(egui::Frame::none().fill(crate::theme::bg()))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("GrokHub")
                            .font(crate::theme::title_font(crate::theme::FONT_CHROME))
                            .color(crate::theme::fg()),
                    );
                    if let Some(label) = update_chip {
                        ui.add_space(8.0);
                        if crate::cards::titlebar_update_chip(ui, label) {
                            run_pending_update = true;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if titlebar_chrome_hit(&titlebar_chrome_btn(ui, ChromeBtn::Close)) {
                            let hide = crate::tray::should_hide_on_close(
                                self.cfg.close_to_tray,
                                self.tray.is_some(),
                            ) && !self.want_quit;
                            if hide {
                                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                                self.hide_to_tray(ctx);
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        if titlebar_chrome_hit(&titlebar_chrome_btn(
                            ui,
                            if self.win_max {
                                ChromeBtn::Restore
                            } else {
                                ChromeBtn::Maximize
                            },
                        )) {
                            let currently = ctx
                                .input(|i| i.viewport().maximized)
                                .unwrap_or(self.win_max);
                            self.win_max = next_maximized(currently);
                            self.cfg.window.maximized = self.win_max;
                            self.geom_dirty = true;
                            #[cfg(windows)]
                            {
                                let ppp = ctx.pixels_per_point().max(0.5);
                                if self.win_max {
                                    if let Some((x, y, w, h)) = crate::win_native::work_area() {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                            egui::vec2(w as f32 / ppp, h as f32 / ppp),
                                        ));
                                        ctx.send_viewport_cmd(
                                            egui::ViewportCommand::OuterPosition(egui::pos2(
                                                x as f32 / ppp,
                                                y as f32 / ppp,
                                            )),
                                        );
                                        let _ = crate::win_native::show_cabin(x, y, w, h, true);
                                    }
                                } else {
                                    let g = crate::window::clamp_geom(self.cfg.window);
                                    let x = (g.x.unwrap_or(100.0) * ppp).round() as i32;
                                    let y = (g.y.unwrap_or(100.0) * ppp).round() as i32;
                                    let w = (g.w * ppp).round() as i32;
                                    let h = (g.h * ppp).round() as i32;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                        egui::vec2(g.w, g.h),
                                    ));
                                    if let Some([lx, ly]) = crate::window::launch_pos(&g) {
                                        ctx.send_viewport_cmd(
                                            egui::ViewportCommand::OuterPosition(egui::pos2(
                                                lx, ly,
                                            )),
                                        );
                                    }
                                    let _ = crate::win_native::show_cabin(x, y, w, h, false);
                                }
                            }
                            #[cfg(not(windows))]
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                                    self.win_max,
                                ));
                            }
                        }
                        if titlebar_chrome_hit(&titlebar_chrome_btn(ui, ChromeBtn::Minimize)) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        let (_rect, drag) = ui.allocate_exact_size(
                            ui.available_size(),
                            egui::Sense::click_and_drag(),
                        );
                        if titlebar_should_start_drag(drag.drag_started()) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                    });
                });
                let hair = ui.max_rect();
                ui.painter().hline(
                    hair.x_range(),
                    hair.bottom() - 0.5,
                    egui::Stroke::new(1.0_f32, crate::theme::border()),
                );
            });
        if run_pending_update {
            self.open_update_overlay();
            self.queue_combined_update();
        }
    }

    pub(super) fn nav_row(
        ui: &mut egui::Ui,
        active: bool,
        icon: crate::icons::RailIcon,
        label: &str,
        outline: bool,
    ) -> egui::Response {
        let fill = if active {
            crate::theme::nav_active()
        } else {
            egui::Color32::TRANSPARENT
        };
        let color = if active {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        };
        let w = ui.available_width();
        let (_rect, resp) =
            ui.allocate_exact_size(egui::vec2(w, crate::theme::NAV_ROW_H), egui::Sense::click());
        let (resp, rect, fill) = crate::theme::feel_response(ui, resp, fill);
        ui.painter()
            .rect_filled(rect, crate::theme::CHROME_RADIUS, fill);
        if outline {
            ui.painter().rect_stroke(
                rect,
                crate::theme::CHROME_RADIUS,
                egui::Stroke::new(1.0_f32, crate::theme::border_strong()),
            );
        }
        let icon_c = egui::pos2(rect.left() + 20.0, rect.center().y);
        let icon_rect = egui::Rect::from_center_size(icon_c, egui::vec2(20.0, 20.0));
        crate::icons::paint_rail_icon_at(ui.painter(), icon_rect, icon, color);
        let text_left = rect.left() + 38.0;
        let text_right = rect.right() - 12.0;
        let painted = fit_rail_label(ui, label, (text_right - text_left).max(8.0));
        ui.painter().text(
            egui::pos2(text_left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            painted,
            egui::FontId::proportional(crate::theme::FONT_CHROME),
            color,
        );
        resp
    }

    pub(super) fn avatar_chrome(&self) -> AvatarMenu {
        avatar_menu(
            &self.cfg.display_name,
            &self.cfg.profile_picture,
            self.secrets.oauth.as_ref().and_then(|t| t.name.as_deref()),
            &self.greeting_user_md,
        )
    }

    pub(super) fn photo_for_path(&self, picture_path: &str) -> Option<TextureHandle> {
        if picture_path.trim().is_empty() {
            self.oauth_photo.clone()
        } else if self.profile_photo_key == picture_path {
            self.profile_photo.clone()
        } else {
            None
        }
    }

    pub(super) fn cabin_avatar(
        ui: &mut egui::Ui,
        name: &str,
        photo: Option<&TextureHandle>,
    ) -> egui::Response {
        let (_rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), RAIL_FOOTER_H),
            egui::Sense::click(),
        );
        let (resp, rect, wash) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
        if wash.a() > 0 {
            ui.painter().rect_filled(rect, 10.0, wash);
        }
        let c = egui::pos2(rect.left() + 20.0, rect.center().y);
        if let Some(tex) = photo {
            let size = egui::vec2(28.0, 28.0);
            egui::Image::from_texture(tex)
                .fit_to_exact_size(size)
                .rounding(14.0)
                .paint_at(ui, egui::Rect::from_center_size(c, size));
        } else {
            ui.painter().circle_filled(c, 14.0, crate::theme::panel());
        }
        ui.painter().circle_stroke(
            c,
            14.0,
            egui::Stroke::new(1.0_f32, crate::theme::border_strong()),
        );
        let text_left = rect.left() + 42.0;
        let text_right = rect.right() - 12.0;
        let painted = fit_rail_label(ui, name, (text_right - text_left).max(8.0));
        ui.painter().text(
            egui::pos2(text_left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            painted,
            egui::FontId::proportional(crate::theme::FONT_META),
            crate::theme::fg(),
        );
        resp
    }

    pub(super) fn ui_sidebar(&mut self, ctx: &egui::Context) {
        let chrome = self.avatar_chrome();
        let photo = self.photo_for_path(&chrome.picture_path);
        egui::SidePanel::left("rail")
            .exact_width(crate::theme::SIDEBAR_W)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(8.0)),
            )
            .show(ctx, |ui| {
                ui.add_space(4.0);
                if Self::nav_row(ui, false, crate::icons::RailIcon::Search, "Search", false)
                    .clicked()
                {
                    self.open_palette();
                }
                ui.add_space(6.0);
                let cur = self.nav_id();
                for (id, label) in crate::theme::GROK_NAV {
                    if Self::nav_row(
                        ui,
                        cur == *id,
                        crate::icons::rail_icon_for(id),
                        label,
                        false,
                    )
                    .clicked()
                    {
                        self.set_nav_id(id);
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Projects")
                            .size(12.0)
                            .color(crate::theme::subtle()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let plus =
                            crate::theme::felt_icon_hit(ui, "+", 22.0, crate::theme::muted(), 16.0)
                                .on_hover_text("New project or folder");
                        let plus_pos = plus.rect.left_bottom();
                        if plus.clicked() {
                            self.proj_plus_open = true;
                            self.proj_plus_pos = plus_pos;
                            self.proj_ignore_close = true;
                        }
                    });
                });
                let tree = visible_tree(&self.projects);
                let mut proj_act: Option<(String, ProjectMenuAct, egui::Pos2)> = None;
                for (depth, idx) in tree {
                    let kind = self.projects[idx].kind;
                    let open = self.projects[idx].open;
                    let indent = 20.0 * depth as f32;
                    if self.proj_rename.as_deref() == Some(self.projects[idx].id.as_str()) {
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut self.proj_rename_buf)
                                    .desired_width(ui.available_width() - 8.0)
                                    .hint_text("Name")
                                    .font(egui::FontId::proportional(13.0)),
                            );
                            if self.proj_rename_focus {
                                edit.request_focus();
                                if edit.has_focus() {
                                    self.proj_rename_focus = false;
                                }
                            }
                            if let Some(lock) = self.proj_rename_lock.clone() {
                                if self.proj_rename_buf == lock {
                                    select_all_edit(ui, edit.id, &self.proj_rename_buf);
                                } else {
                                    self.proj_rename_lock = None;
                                }
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.cancel_proj_rename();
                            } else if ui.input(|i| i.key_pressed(egui::Key::Enter))
                                || (edit.lost_focus() && !self.proj_rename_focus)
                            {
                                self.finish_proj_rename();
                            }
                        });
                        continue;
                    }
                    let icon = match kind {
                        ProjectKind::Folder => crate::icons::RailIcon::Folder,
                        ProjectKind::Project => crate::icons::RailIcon::Chat,
                    };
                    let active = project_row_active(
                        self.project_sel.as_deref() == Some(self.projects[idx].id.as_str()),
                        kind == ProjectKind::Project,
                        self.nav,
                    );
                    let row = ui
                        .horizontal(|ui| {
                            ui.add_space(indent);
                            if kind == ProjectKind::Folder {
                                crate::icons::paint_folder_caret(ui, open, crate::theme::subtle());
                            }
                            Self::nav_row(ui, active, icon, &self.projects[idx].name, false)
                        })
                        .inner;
                    if row.double_clicked() {
                        self.begin_proj_rename(
                            self.projects[idx].id.clone(),
                            self.projects[idx].name.clone(),
                        );
                    } else if row.clicked() {
                        let id = self.projects[idx].id.clone();
                        match kind {
                            ProjectKind::Folder => {
                                toggle_folder(&mut self.projects, &id);
                                self.touch_projects();
                                self.flush_projects();
                            }
                            ProjectKind::Project => self.bind_project_id(&id),
                        }
                    }
                    let nid = self.projects[idx].id.clone();
                    let row_pos = row.rect.left_bottom();
                    row.context_menu(|ui| {
                        for a in project_menu_acts(kind) {
                            if ui.button(project_menu_label(*a)).clicked() {
                                proj_act = Some((nid.clone(), *a, row_pos));
                                ui.close_menu();
                            }
                        }
                    });
                }
                if let Some((id, act, pos)) = proj_act {
                    self.proj_menu_pos = pos;
                    self.apply_project_menu(id, act);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("History")
                            .size(12.0)
                            .color(crate::theme::subtle()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::theme::felt_label_button(
                            ui,
                            "See all",
                            egui::Color32::TRANSPARENT,
                            crate::theme::subtle(),
                            6.0,
                            egui::vec2(0.0, 20.0),
                            None,
                            false,
                        )
                        .clicked()
                        {
                            self.nav = Nav::History;
                        }
                    });
                });
                crate::cards::search_bar(
                    ui,
                    &mut self.sidebar_q,
                    "Filter chats…",
                    (ui.available_width() - 8.0).max(80.0),
                );
                let hist_h = (ui.available_height() - RAIL_FOOTER_H).max(36.0);
                egui::ScrollArea::vertical()
                    .id_salt("rail-history")
                    .auto_shrink([false, true])
                    .max_height(hist_h)
                    .show(ui, |ui| {
                        if !self.grok_sessions_loaded {
                            self.reload_grok_sessions();
                        }
                        let q = self.sidebar_q.to_ascii_lowercase();
                        let current_sid = self
                            .threads
                            .get(self.thread_idx)
                            .and_then(|t| t.grok_session.clone());
                        let mut act: Option<TabAct> = None;
                        for s in &self.grok_sessions {
                            if self.pending_grok_deletes.contains(&s.id) {
                                continue;
                            }
                            let title = if s.title.is_empty() || s.title == s.id {
                                s.id.clone()
                            } else {
                                s.title.clone()
                            };
                            if !q.is_empty()
                                && !title.to_ascii_lowercase().contains(&q)
                                && !s.id.to_ascii_lowercase().contains(&q)
                            {
                                continue;
                            }
                            let on = current_sid.as_deref() == Some(s.id.as_str());
                            let resp = Self::nav_row(
                                ui,
                                on && self.nav == Nav::Chat,
                                crate::icons::RailIcon::Chat,
                                &display_tab_title(&title),
                                false,
                            );
                            if resp.clicked() {
                                act = Some(TabAct::OpenGrok(s.id.clone()));
                            }
                            let sid = s.id.clone();
                            resp.context_menu(|ui| {
                                if ui.button("Delete").clicked() {
                                    act = Some(TabAct::DeleteGrok(sid.clone()));
                                    ui.close_menu();
                                }
                            });
                        }
                        match act {
                            Some(TabAct::Switch(i)) => {
                                self.switch_thread(i);
                                self.nav = Nav::Chat;
                            }
                            Some(TabAct::Pin(i)) => self.pin_thread(i),
                            Some(TabAct::StartRename(i)) => self.begin_chat_rename(i),
                            Some(TabAct::CommitRename(i)) => {
                                let name = self.rename_buf.clone();
                                self.rename_thread(i, &name);
                            }
                            Some(TabAct::CancelRename) => {
                                self.rename_idx = None;
                                self.rename_focus = false;
                                self.rename_lock = None;
                            }
                            Some(TabAct::Delete(i)) => self.delete_thread_at(i),
                            Some(TabAct::OpenGrok(id)) => {
                                self.open_grok_session(&id);
                                self.composer_want_focus = true;
                            }
                            Some(TabAct::DeleteGrok(id)) => self.delete_grok_history(&id),
                            None => {}
                        }
                    });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if Self::cabin_avatar(ui, &chrome.name, photo.as_ref()).clicked() {
                        self.settings_menu_open = !self.settings_menu_open;
                        self.settings_menu_ignore = true;
                    }
                });
            });
    }

    pub(super) fn page_nav(&self) -> Nav {
        if self.nav != Nav::Settings {
            return self.nav;
        }
        if self.settings_back == Nav::Settings {
            Nav::Chat
        } else {
            self.settings_back
        }
    }

    pub(super) fn nav_id(&self) -> &'static str {
        match self.page_nav() {
            Nav::Chat => "chat",
            Nav::History => "history",
            Nav::Imagine => "imagine",
            Nav::Workboard => "workboard",
            Nav::Settings => "chat",
            Nav::Skills => "skills",
            Nav::Night => "automations",
            Nav::Command => "command",
            Nav::Agents => "queue",
            Nav::Devices => "devices",
            Nav::Memory => "memory",
            Nav::Connectors => "connectors",
        }
    }

    pub(super) fn set_nav_id(&mut self, id: &str) {
        self.nav = match id {
            "history" => Nav::History,
            "imagine" => {
                self.imagine_want_focus = true;
                Nav::Imagine
            }
            "workboard" => Nav::Workboard,
            "settings" => {
                if self.nav != Nav::Settings {
                    self.settings_back = self.nav;
                }
                self.settings_sec = SettingsSec::Account;
                Nav::Settings
            }
            "skills" => {
                self.skills_tab_connectors = false;
                Nav::Skills
            }
            "automations" => Nav::Night,
            "command" => Nav::Command,
            "queue" => Nav::Agents,
            "devices" => Nav::Devices,
            "memory" => Nav::Memory,
            "eyes" => {
                self.open_recent_chat();
                Nav::Chat
            }
            "connectors" => {
                self.skills_tab_connectors = true;
                Nav::Connectors
            }
            "chat" => {
                self.new_thread(false);
                Nav::Chat
            }
            _ => Nav::Chat,
        };
    }
}
