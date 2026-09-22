//! Settings page, cabin menu, and save.

use super::*;


pub(super) fn settings_group_home(group: SettingsGroup) -> SettingsSec {
    match group {
        SettingsGroup::General => SettingsSec::Account,
        SettingsGroup::About => SettingsSec::Update,
    }
}


pub(super) fn settings_sec_title(sec: SettingsSec) -> &'static str {
    match sec {
        SettingsSec::Account => "Account",
        SettingsSec::Appearance => "Appearance",
        SettingsSec::Behavior => "Behavior",
        SettingsSec::Update => "Update",
        SettingsSec::About => "About",
    }
}

impl Cabin {

    pub(super) fn ui_settings_menu(&mut self, ctx: &egui::Context) {
        if !self.settings_menu_open {
            return;
        }
        let mut pick: Option<&'static str> = None;
        let mut connect = false;
        let mut disconnect = false;
        let mut help = false;
        let authed = self.has_key();
        let chrome = self.avatar_chrome();
        let photo = self.photo_for_path(&chrome.picture_path);
        let shown = egui::Window::new("settings-menu")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -56.0])
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::panel())
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(8.0)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(220.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                Self::cabin_avatar(ui, &chrome.name, photo.as_ref());
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                for (id, label) in crate::theme::CABIN_MENU {
                    if crate::cards::felt_menu_row(ui, label) {
                        pick = Some(*id);
                    }
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                if crate::cards::felt_menu_row(ui, "Help") {
                    help = true;
                }
                let auth_label = if authed { "Sign out" } else { "Connect Grok" };
                if crate::cards::felt_menu_row(ui, auth_label) {
                    if authed {
                        disconnect = true;
                    } else {
                        connect = true;
                    }
                }
            });
        let menu_rect = shown.map(|r| r.response.rect);
        if let Some(id) = pick {
            self.set_nav_id(id);
            self.settings_menu_open = false;
        }
        if help {
            self.shortcuts_open = true;
            self.settings_menu_open = false;
        }
        if connect {
            self.start_oauth();
            self.settings_menu_open = false;
        }
        if disconnect {
            self.sign_out_oauth();
            self.settings_menu_open = false;
        }
        let outside = ctx.input(|i| i.pointer.any_click())
            && ctx.pointer_interact_pos().is_some_and(|pos| {
                menu_rect
                    .map(|r| !r.expand(8.0).contains(pos))
                    .unwrap_or(true)
            });
        if cabin_menu_should_dismiss(self.settings_menu_ignore, outside) {
            self.settings_menu_open = false;
        }
        self.settings_menu_ignore = false;
    }

    pub(super) fn ui_settings(&mut self, ctx: &egui::Context) {
        let mut save = false;
        let mut connect = false;
        let mut disconnect = false;
        let mut choose_picture = false;
        let mut clear_picture = false;
        let mut name_dirty = false;
        let mut update = false;
        let mut install_cli = false;
        let mut restart = false;
        let mut copy_diag = false;
        let cli_ready = grokhub_acp::find_grok().is_some() || grokhub_acp::grok_cli_known_good();
        let cli_installing = self.grok_install_rx.is_some();
        let show_cli_install =
            grokhub_core::should_show_manual_cli_install(cli_ready, cli_installing);
        let pending_update = self.update_pending_now();
        let update_label = settings_update_label(pending_update);
        let update_hint = settings_update_hint(pending_update);
        let cabin_notify = self.cabin_update_available();
        let cabin_notice = self
            .cabin_latest
            .as_deref()
            .filter(|_| cabin_notify)
            .map(|tag| cabin_update_notice(env!("CARGO_PKG_VERSION"), tag));
        let cli_notice = match (self.cli_installed.as_deref(), self.cli_alpha.as_deref()) {
            (Some(installed), Some(alpha))
                if should_update_cli_alpha(Some(installed), Some(alpha)) =>
            {
                Some(cli_update_notice(installed, alpha))
            }
            _ => None,
        };
        let cli_install_hint = if cli_installing {
            "Installing Grok Build CLI alpha (GROK_CHANNEL=alpha)…"
        } else if !self.grok_install_err.is_empty() {
            "Grok is missing or broken. Installs Grok Build CLI alpha from x.ai/cli."
        } else {
            "Installs Grok Build CLI alpha (GROK_CHANNEL=alpha / https://x.ai/cli/alpha) when grok is missing or broken."
        };
        let oauth_on = self.secrets.oauth.is_some();
        let picture_set = !self.cfg.profile_picture.trim().is_empty();
        let picture_hint = if picture_set {
            "Saved in cabin config."
        } else {
            "A local image, kept in cabin config."
        };
        let pending = self.oauth_pending.as_ref().map(|p| {
            format!("Approve {} at {}", p.user_code, p.verification_uri)
        });
        let doctor = self.doctor_text();
        let mut close = false;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }
        let mut next_sec: Option<SettingsSec> = None;
        let sec = self.settings_sec;
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("settings-overlay"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let modal = egui::Rect::from_center_size(
                    screen.center(),
                    egui::vec2(920.0, 620.0).min(screen.size() - egui::vec2(48.0, 48.0)),
                );
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(modal), |ui| {
                    egui::Frame::none()
                        .fill(crate::theme::bg())
                        .rounding(16.0)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::ZERO)
                        .show(ui, |ui| {
                            ui.set_min_size(modal.size());
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(220.0, modal.height()),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::Frame::none()
                                            .fill(crate::theme::surface())
                                            .inner_margin(egui::Margin::same(12.0))
                                            .show(ui, |ui| {
                                                ui.set_width(196.0);
                                                ui.set_min_height(modal.height() - 24.0);
                                                if crate::cards::section_label(ui, "General") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::General));
                                                }
                                                for (s, label) in [
                                                    (SettingsSec::Account, "Account"),
                                                    (SettingsSec::Appearance, "Appearance"),
                                                    (SettingsSec::Behavior, "Behavior"),
                                                ] {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                                ui.add_space(10.0);
                                                if crate::cards::section_label(ui, "About") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::About));
                                                }
                                                for (s, label) in [
                                                    (SettingsSec::Update, "Update"),
                                                    (SettingsSec::About, "About"),
                                                ] {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                            });
                                    },
                                );
                                ui.allocate_ui_with_layout(
                                    egui::vec2((modal.width() - 220.0).max(320.0), modal.height()),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.add_space(16.0);
                                        ui.horizontal(|ui| {
                                            ui.add_space(20.0);
                                            ui.label(
                                                RichText::new(settings_sec_title(sec))
                                                    .font(crate::theme::title_font(22.0))
                                                    .color(crate::theme::fg()),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.add_space(16.0);
                                                    if crate::theme::felt_label_button(
                                                        ui,
                                                        "×",
                                                        Color32::TRANSPARENT,
                                                        crate::theme::muted(),
                                                        6.0,
                                                        egui::vec2(28.0, 28.0),
                                                        None,
                                                        false,
                                                    )
                                                    .clicked()
                                                    {
                                                        close = true;
                                                    }
                                                    if crate::cards::ghost_pill(ui, "Save") {
                                                        save = true;
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(12.0);
                                        egui::ScrollArea::vertical()
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width((modal.width() - 260.0).max(280.0));
                                                ui.add_space(8.0);
                                                ui.indent("settings-body", |ui| {
                                                    match sec {
                                                        SettingsSec::Account => {
                                                            let name_before = self.cfg.display_name.clone();
                                                            crate::cards::settings_field(
                                                                ui,
                                                                "Name",
                                                                "Shown on the rail and the avatar menu. Leave this blank to use your Grok name.",
                                                                &mut self.cfg.display_name,
                                                                false,
                                                            );
                                                            if self.cfg.display_name != name_before {
                                                                name_dirty = true;
                                                            }
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                "Profile picture",
                                                                picture_hint,
                                                                "Choose",
                                                            ) {
                                                                choose_picture = true;
                                                            }
                                                            if picture_set
                                                                && crate::cards::settings_action(
                                                                    ui,
                                                                    "Remove picture",
                                                                    "The rail goes back to your Grok photo.",
                                                                    "Remove",
                                                                )
                                                            {
                                                                clear_picture = true;
                                                            }
                                                            let auth_title = if oauth_on {
                                                                "Connected"
                                                            } else {
                                                                "Connect Grok"
                                                            };
                                                            let auth_hint = if oauth_on {
                                                                "Signed in with Grok."
                                                            } else {
                                                                "Device-code OAuth. Also signs in the Grok Build CLI if it is not already connected."
                                                            };
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                auth_title,
                                                                auth_hint,
                                                                if oauth_on { "Sign out" } else { "Connect" },
                                                            ) {
                                                                if oauth_on {
                                                                    disconnect = true;
                                                                } else {
                                                                    connect = true;
                                                                }
                                                            }
                                                            if let Some(p) = &pending {
                                                                crate::cards::settings_note(ui, p);
                                                            }
                                                        }
                                                        SettingsSec::Appearance => {
                                                            crate::cards::settings_note(
                                                                ui,
                                                                appearance_hint(),
                                                            );
                                                            ui.horizontal(|ui| {
                                                                let current = parse_theme(&self.cfg.theme);
                                                                let os_dark = crate::theme::desktop_prefers_dark();
                                                                for choice in appearance_choices() {
                                                                    let on = current == *choice;
                                                                    let preview = if resolve_dark(*choice, os_dark)
                                                                    {
                                                                        crate::theme::BG
                                                                    } else {
                                                                        crate::theme::LIGHT_BG
                                                                    };
                                                                    if crate::cards::appearance_card(
                                                                        ui,
                                                                        theme_label(*choice),
                                                                        on,
                                                                        preview,
                                                                    ) {
                                                                        if let Some(next) =
                                                                            pick_theme(current, *choice)
                                                                        {
                                                                            self.cfg.theme = theme_id(next).into();
                                                                            self.persist_cfg();
                                                                            self.status = "Saved".into();
                                                                        }
                                                                    }
                                                                    ui.add_space(10.0);
                                                                }
                                                            });
                                                        }
                                                        SettingsSec::Behavior => {
                                                            if crate::cards::settings_toggle(ui, "Close to tray", "The cabin keeps working in the background.", &mut self.cfg.close_to_tray) {
                                                                self.persist_cfg();
                                                                self.status = "Saved".into();
                                                            }
                                                            if crate::cards::settings_toggle(
                                                                ui,
                                                                "Living wall",
                                                                "Every few hours the cabin paints a new cover. Twenty live. Oldest leaves first.",
                                                                &mut self.cfg.imagine_wall,
                                                            ) {
                                                                self.persist_cfg();
                                                                self.status = "Saved".into();
                                                            }
                                                            let quiet_menu = quiet_hours_menu(
                                                                &self.cfg.quiet_start,
                                                                &self.cfg.quiet_end,
                                                            );
                                                            let quiet_labels: Vec<String> =
                                                                quiet_menu.iter().map(|(l, _, _)| l.clone()).collect();
                                                            let quiet_selected = quiet_hours_choice_label(
                                                                &self.cfg.quiet_start,
                                                                &self.cfg.quiet_end,
                                                            );
                                                            if let Some(i) = crate::cards::settings_dropdown(
                                                                ui,
                                                                "Quiet hours",
                                                                "Inside quiet hours the cabin holds a destructive automation and stops anticipating. Off is start and end equal.",
                                                                &quiet_selected,
                                                                &quiet_labels,
                                                            ) {
                                                                if let Some((_, start, end)) = quiet_menu.get(i) {
                                                                    self.cfg.quiet_start = start.clone();
                                                                    self.cfg.quiet_end = end.clone();
                                                                    self.quiet_start_buf = start.clone();
                                                                    self.quiet_end_buf = end.clone();
                                                                    self.persist_cfg();
                                                                    self.status = "Saved".into();
                                                                }
                                                            }
                                                        }
                                                        SettingsSec::Update => {
                                                            if let Some(notice) = cli_notice.as_deref() {
                                                                crate::cards::settings_note(ui, notice);
                                                            }
                                                            if let Some(notice) = cabin_notice.as_deref() {
                                                                crate::cards::settings_note(ui, notice);
                                                            }
                                                            if let Some(note) = self.update_cabin_note.as_deref() {
                                                                crate::cards::settings_note(ui, note);
                                                            }
                                                            if show_cli_install
                                                                && crate::cards::settings_action(
                                                                    ui,
                                                                    "Install Grok Build CLI",
                                                                    cli_install_hint,
                                                                    if cli_installing { "Installing…" } else { "Install" },
                                                                )
                                                                && !cli_installing
                                                            {
                                                                install_cli = true;
                                                            }
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                update_label,
                                                                update_hint,
                                                                "Update",
                                                            ) {
                                                                update = true;
                                                            }
                                                            if let Some(pct) = self.update_pct {
                                                                let fill = if self.last_receipt_ok == Some(false) && !self.running {
                                                                    crate::theme::OFFLINE
                                                                } else {
                                                                    crate::theme::LIVE
                                                                };
                                                                crate::cards::settings_progress(ui, pct, fill);
                                                            }
                                                            if self.update_can_restart
                                                                && crate::cards::settings_action(
                                                                    ui,
                                                                    "Restart GrokHub",
                                                                    "Reload hub, then start a new cabin and exit this one.",
                                                                    "Restart",
                                                                )
                                                            {
                                                                restart = true;
                                                            }
                                                            if !self.status.is_empty() {
                                                                crate::cards::settings_note(ui, &self.status);
                                                            }
                                                        }
                                                        SettingsSec::About => {
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "GrokHub {}",
                                                                    env!("CARGO_PKG_VERSION")
                                                                ))
                                                                .size(crate::theme::FONT_HEADING)
                                                                .color(crate::theme::fg()),
                                                            );
                                                            ui.add_space(6.0);
                                                            crate::cards::settings_note(ui, "Native Grok Build cabin.");
                                                            crate::cards::settings_note(ui, &build_agent::grok_banner());
                                                            crate::cards::settings_note(ui, &doctor);
                                                            if crate::cards::settings_action(ui, "Diagnostics", "Copy a redacted bundle. No secrets.", "Copy") {
                                                                copy_diag = true;
                                                            }
                                                        }
                                                    }
                                                });
                                            });
                                    },
                                );
                            });
                        });
                });
            });
        if let Some(s) = next_sec {
            self.settings_sec = s;
        }
        if close {
            self.nav = self.settings_back;
        }
        if connect {
            self.start_oauth();
        }
        if disconnect {
            self.sign_out_oauth();
        }
        if name_dirty {
            if self.cfg.display_name.chars().count() > PROFILE_NAME_MAX {
                self.cfg.display_name = clip_profile_name(&self.cfg.display_name);
            }
            self.persist_cfg();
        }
        if choose_picture {
            self.pick_profile_picture();
        }
        if clear_picture {
            self.clear_profile_picture();
        }
        if update {
            self.queue_combined_update();
        }
        if install_cli {
            self.queue_grok_cli_install();
        }
        if restart {
            self.restart_after_update(ctx);
        }
        if copy_diag {
            let bundle = diagnostics_bundle(
                env!("CARGO_PKG_VERSION"),
                self.has_key(),
                HUB_KIND,
                self.skill_list.len(),
                self.last_receipt_ok,
                self.board.len(),
                &self.status,
            );
            ctx.output_mut(|o| o.copied_text = bundle);
            self.status = "Diagnostics copied".into();
        }
        if save {
            self.save_settings();
        }
    }

    pub(super) fn save_settings(&mut self) {
        self.cfg.api_key.clear();
        self.cfg.quiet_start = normalize_hm(&self.quiet_start_buf, &self.cfg.quiet_start);
        self.cfg.quiet_end = normalize_hm(&self.quiet_end_buf, &self.cfg.quiet_end);
        self.cfg.daily_auto_cap = cap_from_text(&self.cap_auto_buf, self.cfg.daily_auto_cap);
        self.cfg.host_hour_cap = cap_from_text(&self.cap_host_buf, self.cfg.host_hour_cap);
        self.quiet_start_buf = self.cfg.quiet_start.clone();
        self.quiet_end_buf = self.cfg.quiet_end.clone();
        self.cap_auto_buf = self.cfg.daily_auto_cap.to_string();
        self.cap_host_buf = self.cfg.host_hour_cap.to_string();
        if let Ok(mut st) = self.hub.lock() {
            if !self.cfg.device_name.trim().is_empty() {
                st.device_name = self.cfg.device_name.clone();
            }
        }
        let p = expand_home(&self.cfg.project_dir);
        let tree_changed = self
            .threads
            .get(self.thread_idx)
            .and_then(|t| t.grok_cwd.as_deref())
            .map(|cwd| cwd != p)
            .unwrap_or(false);
        self.cfg.project_dir = p.clone();
        if !p.trim().is_empty() {
            let dir = p.clone();
            std::thread::spawn(move || {
                let _ = std::fs::create_dir_all(&dir);
            });
        }
        self.project_sel = upsert_bound(&mut self.projects, &p);
        self.touch_projects();
        self.status = "Saved".into();
        self.sync_hub_voice();
        if tree_changed {
            if self.running {
                self.halt_in_flight();
            }
            self.acp = None;
            self.acp_spawn_rx = None;
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_cwd = None;
                t.grok_session = None;
            }
            self.persist();
        } else {
            self.flush_projects();
            self.persist_cfg();
            self.persist_hub();
            self.persist_secrets();
        }
    }
}
