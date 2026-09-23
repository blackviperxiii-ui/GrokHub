//! The other cabin pages.

use super::*;

impl Cabin {

    pub(super) fn ui_command(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                if crate::cards::page_header(ui, "Command", "Run") {
                    let line = self.cmd_line.trim().to_string();
                    if !line.is_empty() {
                        self.cmd_hist.push(line.clone());
                        self.cmd_line.clear();
                        self.queue_sh(line);
                    }
                }
                crate::cards::section_label(ui, "This box");
                if !self.host_live.is_empty() {
                    crate::cards::status_chip(ui, &self.host_live, crate::cards::ChipTone::Setup);
                    ui.add_space(8.0);
                }
                let mut run = false;
                egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        let enter = ui
                            .add(
                                egui::TextEdit::singleline(&mut self.cmd_line)
                                    .hint_text("$ ls — bound project is the working tree")
                                    .desired_width(f32::INFINITY)
                                    .frame(false),
                            )
                            .lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter {
                            run = true;
                        }
                    });
                if run {
                    let line = self.cmd_line.trim().to_string();
                    if !line.is_empty() {
                        self.cmd_hist.push(line.clone());
                        self.cmd_line.clear();
                        self.queue_sh(line);
                    }
                }
                ui.add_space(16.0);
                crate::cards::section_label(ui, "History");
                if self.cmd_hist.is_empty() && self.last_host.is_empty() {
                    let _ = crate::cards::empty_prompt_tile(
                        ui,
                        crate::icons::TileIcon::Host,
                        "Nothing run yet",
                        "Type a command above. The bound project is the working tree.",
                    );
                } else {
                    let hist: Vec<String> = self.cmd_hist.iter().rev().take(6).cloned().collect();
                    crate::cards::tile_row(ui, hist.len(), |ui, i| {
                        let cmd = &hist[i];
                        crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::Host,
                            cmd,
                            "Ran on this box",
                            None,
                            false,
                        );
                    });
                    if !self.last_host.is_empty() {
                        ui.add_space(12.0);
                        crate::cards::section_label(ui, "Last host");
                        let receipt: String = self.last_host.join(" ").chars().take(80).collect();
                        crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::Check,
                            "Last receipt",
                            &receipt,
                            None,
                            false,
                        );
                    }
                }
            });
    }

    pub(super) fn ui_connectors(&mut self, ctx: &egui::Context) {
        self.skills_tab_connectors = true;
        self.ui_skills(ctx);
    }

    pub(super) fn ui_agents(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                let _ = crate::cards::page_header(ui, "Queue", "");
                ui.label(
                    RichText::new("Background jobs for this thread.").color(crate::theme::muted()),
                );
                ui.add_space(12.0);
                if !self.cfg.goal_pin.is_empty() {
                    crate::cards::status_chip(
                        ui,
                        &format!("Pinned · step {}", self.goal_step),
                        crate::cards::ChipTone::Mute,
                    );
                    ui.add_space(8.0);
                }
                let mut run_at: Option<usize> = None;
                if !self.grok_tasks.is_empty() {
                    crate::cards::section_label(ui, "Grok tasks");
                    ui.add_space(8.0);
                    for (id, title, done) in &self.grok_tasks {
                        let failed = title.to_ascii_lowercase().starts_with("failed");
                        let st = if *done {
                            "done"
                        } else if failed {
                            "failed"
                        } else {
                            "running"
                        };
                        crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::Bolt,
                            title,
                            &format!("{st} · {id}"),
                            None,
                            *done,
                        );
                        ui.add_space(6.0);
                    }
                    ui.add_space(12.0);
                }
                if self.agents.is_empty() && self.grok_tasks.is_empty() {
                    let _ = crate::cards::empty_prompt_tile(
                        ui,
                        crate::icons::TileIcon::List,
                        "No jobs yet",
                        "Queued work from chat shows up here.",
                    );
                }
                for (i, a) in self.agents.iter().enumerate() {
                    crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Bolt,
                        &a.title,
                        &a.status,
                        None,
                        false,
                    );
                    ui.add_space(6.0);
                    if crate::cards::ghost_pill(ui, "Run") {
                        run_at = Some(i);
                    }
                }
                if let Some(i) = run_at {
                    if i < self.agents.len() {
                        if self.running {
                            self.status = "Busy — wait, then run".into();
                        } else {
                            self.agents[i].status = "running".into();
                            let p = self.agents[i].prompt.clone();
                            let tid = self.agents[i].thread_id.clone();
                            self.nav = Nav::Chat;
                            if !tid.is_empty() {
                                self.chat_job_thread = Some(tid);
                            }
                            self.push_bound_msg("user", p);
                            self.persist();
                            self.kick_model(false);
                        }
                    }
                }
            });
    }

    pub(super) fn ui_devices(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                if crate::cards::page_header(
                    ui,
                    "Devices",
                    if self.hub_on {
                        "Sharing"
                    } else {
                        "Start share"
                    },
                ) {
                    self.start_hub();
                }
                crate::cards::section_label(ui, "This computer");
                let mut rotated = false;
                let (name, sharing, pair_code) = if let Ok(mut st) = self.hub.lock() {
                    if self.hub_on
                        && st
                            .pair
                            .as_ref()
                            .is_some_and(|p| !pair_code_is_live(p.expires_at, now_ms()))
                    {
                        st.rotate_pair();
                        rotated = true;
                    }
                    (
                        st.device_name.clone(),
                        self.hub_on,
                        st.pair.as_ref().and_then(|p| {
                            devices_shows_pair_code(
                                self.hub_on,
                                pair_code_is_live(p.expires_at, now_ms()),
                            )
                            .then(|| p.code.clone())
                        }),
                    )
                } else {
                    (String::new(), false, None)
                };
                if rotated {
                    self.persist_hub();
                }
                let body = if sharing {
                    format!("Sharing on port {}", self.hub_port)
                } else {
                    "Not sharing. Start share to pair a phone or another computer.".into()
                };
                crate::cards::grok_tile(
                    ui,
                    crate::icons::TileIcon::Host,
                    if name.is_empty() { "This cabin" } else { &name },
                    &body,
                    None,
                    sharing,
                );
                ui.add_space(16.0);
                crate::cards::section_label(ui, "Pair");
                if let Some(code) = pair_code {
                    crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Connect,
                        &code,
                        &format!(
                            "Open {} on the other device.",
                            discover_hub_pair_url(self.hub_port)
                        ),
                        None,
                        false,
                    );
                } else if sharing {
                    ui.label(
                        RichText::new("Paired. Make a new code after another device joins.")
                            .size(13.0)
                            .color(crate::theme::muted()),
                    );
                    ui.add_space(8.0);
                    if crate::cards::ghost_pill(ui, "New code") {
                        if let Ok(mut s) = self.hub.lock() {
                            s.rotate_pair();
                        }
                        self.persist_hub();
                    }
                } else if crate::cards::empty_prompt_tile(
                    ui,
                    crate::icons::TileIcon::Connect,
                    "No pair code",
                    "Start share to mint a code for another device.",
                ) {
                    self.start_hub();
                }
                ui.add_space(16.0);
                crate::cards::section_label(ui, "Send a task");
                egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.task_prompt)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .frame(false)
                                .hint_text("What should this computer do?"),
                        );
                    });
                ui.add_space(8.0);
                if crate::cards::white_pill(ui, "Send a task home") {
                    let t = std::mem::take(&mut self.task_prompt);
                    self.nav = Nav::Chat;
                    self.send_chat(t);
                }
            });
    }

    pub(super) fn ui_memory(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                let _ = crate::cards::page_header(ui, "Memory", "");
                ui.horizontal(|ui| {
                    for name in ["SOUL.md", "USER.md", "MEMORY.md"] {
                        if crate::cards::tab_pill(ui, name, self.mem_name == name) {
                            self.open_memory_file(name);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::cards::ghost_pill(ui, "Restore") {
                            if self.scratch() {
                                self.status = "Scratch — no memory writes".into();
                            } else if self.mem_restore_rx.is_some() {
                                self.status = "Restoring…".into();
                            } else {
                                let name = self.mem_name.clone();
                                let (tx, rx) = mpsc::channel();
                                self.mem_restore_rx = Some(rx);
                                self.status = "Restoring…".into();
                                std::thread::spawn(move || {
                                    let _ = tx.send((name.clone(), config::restore_memory(&name)));
                                });
                            }
                        }
                        if crate::cards::ghost_pill(ui, "Reflect") {
                            self.run_reflect();
                        }
                        if crate::cards::white_pill(ui, "Save") {
                            if self.scratch() {
                                self.status = "Scratch — no memory writes".into();
                            } else {
                                let name = self.mem_name.clone();
                                let body = self.mem_body.clone();
                                std::thread::spawn(move || {
                                    if config::read_memory(&name) != body {
                                        let _ = config::write_memory(&name, &body);
                                    }
                                });
                                self.status = format!("Wrote {}", self.mem_name);
                            }
                        }
                    });
                });
                if !self.reflect_diff.is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Last reflect")
                            .size(12.0)
                            .color(crate::theme::subtle()),
                    );
                    ui.label(
                        RichText::new(&self.reflect_diff)
                            .monospace()
                            .size(12.0)
                            .color(crate::theme::muted()),
                    );
                }
                ui.add_space(12.0);
                egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.mem_body)
                                .desired_rows(24)
                                .desired_width(f32::INFINITY)
                                .frame(false)
                                .font(egui::TextStyle::Monospace),
                        );
                    });
            });
    }

    pub(super) fn ui_get_started(&mut self, ctx: &egui::Context) -> bool {
        let grok_present = grokhub_acp::find_grok().is_some();
        let cabin_oauth = self
            .secrets
            .oauth
            .as_ref()
            .is_some_and(|t| !t.access_token.trim().is_empty());
        let cli_connected = grokhub_acp::grok_cli_key().is_some();
        let installing = self.grok_install_rx.is_some();
        let show_oauth = grokhub_core::should_show_get_started_now(
            grok_present,
            cabin_oauth,
            self.cfg.get_started_done,
            cli_connected,
            self.official_cli_session,
        );
        let show_install = grokhub_core::should_show_cli_install_wait(
            self.grok_install_wait,
            !self.grok_install_err.is_empty(),
        );
        if !show_oauth && !show_install {
            return false;
        }
        if show_install {
            let body = if !installing && !self.grok_install_err.is_empty() {
                format!(
                    "{}\n{}",
                    self.grok_install_err,
                    grokhub_acp::grok_cli_install_cmd()
                )
            } else {
                "Installing Grok Build CLI (alpha)…".to_string()
            };
            let show_retry = grokhub_core::should_show_manual_cli_install(grok_present, installing)
                && !self.grok_install_err.is_empty();
            let mut retry = false;
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(crate::theme::bg()))
                .show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        egui::Frame::none()
                            .fill(crate::theme::panel())
                            .rounding(16.0)
                            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                            .inner_margin(egui::Margin::same(24.0))
                            .show(ui, |ui| {
                                ui.set_max_width(520.0);
                                ui.vertical_centered(|ui| {
                                    ui.add_space(24.0);
                                    ui.label(
                                        egui::RichText::new("Get Started")
                                            .font(crate::theme::title_font(
                                                crate::theme::GREET_HERO,
                                            ))
                                            .color(crate::theme::fg()),
                                    );
                                    ui.add_space(12.0);
                                    crate::cards::settings_note(ui, &body);
                                    if show_retry {
                                        ui.add_space(12.0);
                                        retry =
                                            crate::cards::white_pill(ui, "Install Grok Build CLI");
                                    }
                                });
                            });
                    });
                });
            if retry {
                self.queue_grok_cli_install();
            }
            return true;
        }
        let pending = self
            .oauth_pending
            .as_ref()
            .map(|p| format!("Approve {} at {}", p.user_code, p.verification_uri));
        let oauth_busy = self.oauth_pending.is_some()
            || self.oauth_start_rx.is_some()
            || self.oauth_poll_rx.is_some();
        let oauth_err = if pending.is_some() {
            None
        } else {
            grokhub_core::get_started_oauth_error(&self.status).map(str::to_string)
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()))
            .show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    egui::Frame::none()
                        .fill(crate::theme::panel())
                        .rounding(16.0)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::same(24.0))
                        .show(ui, |ui| {
                            ui.set_max_width(520.0);
                            if crate::cards::get_started_panel(
                                ui,
                                pending.as_deref(),
                                oauth_err.as_deref(),
                                !oauth_busy,
                            ) {
                                self.start_oauth();
                            }
                        });
                });
            });
        true
    }

    pub(super) fn ui_history(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "History", "Delete all") {
                self.delete_all_history();
            }
            let marks = session_markers(&self.threads, self.thread_idx);
            if !marks.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for mark in &marks {
                        if crate::cards::ghost_pill(ui, &mark.label) {
                            if let Some(i) = self.threads.iter().position(|t| t.id == mark.thread_id)
                            {
                                self.apply_switch_thread(i);
                                if mark.kind == SessionMarkKind::LastYou {
                                    self.jump_last_you = true;
                                }
                                self.nav = Nav::Chat;
                            }
                        }
                    }
                });
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                crate::cards::search_bar(ui, &mut self.history_q, "Search chats and memory", 320.0);
                if crate::cards::white_pill(ui, "Search") {
                    self.history_q_at = Some(Instant::now() - HISTORY_TYPE_DELAY);
                }
            });
            self.tick_history_search(ui.ctx());
            if self.history_hits.is_empty()
                && !self.history_q.trim().is_empty()
                && self.history_rx.is_none()
                && self.history_q_at.is_none()
            {
                ui.label(RichText::new("No matches.").size(13.0).color(crate::theme::muted()));
            }
            let mut open: Option<String> = None;
            for (target, line) in &self.history_hits {
                let hit = egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(10.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(line).size(13.0).color(crate::theme::fg()));
                    })
                    .response
                    .interact(egui::Sense::click());
                if hit.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if hit.clicked() {
                    open = Some(target.clone());
                }
                ui.add_space(6.0);
            }
            if let Some(target) = open {
                self.open_history_hit(&target);
            }
            ui.add_space(16.0);
            crate::cards::section_label(ui, "Grok Build sessions");
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Same list as `grok sessions list`. Delete here deletes it in Grok Build.")
                        .size(12.0)
                        .color(crate::theme::subtle()),
                );
                if crate::cards::ghost_pill(ui, "Refresh") {
                    self.grok_sessions_loaded = false;
                    self.reload_grok_sessions();
                    self.status = if grokhub_acp::find_grok().is_some() {
                        "Listing Grok sessions…".into()
                    } else {
                        build_agent::grok_banner()
                    };
                }
            });
            if !self.grok_sessions_loaded {
                self.reload_grok_sessions();
            }
            if self.grok_sessions_inflight > 0 && self.grok_sessions.is_empty() {
                ui.label(
                    RichText::new("Listing Grok sessions…")
                        .size(13.0)
                        .color(crate::theme::muted()),
                );
            } else if self.grok_sessions.is_empty() {
                ui.label(
                    RichText::new(if grokhub_acp::find_grok().is_some() {
                        "No grok sessions listed yet."
                    } else {
                        "Install Grok Build (x.ai/cli) to list sessions."
                    })
                    .size(13.0)
                    .color(crate::theme::muted()),
                );
            } else {
                let mut open: Option<String> = None;
                let mut del: Option<String> = None;
                for s in &self.grok_sessions {
                    if self.pending_grok_deletes.contains(&s.id) {
                        continue;
                    }
                    let kind = "Grok Build";
                    match crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Chat,
                        &s.title,
                        kind,
                        Some("Delete"),
                        false,
                    ) {
                        crate::cards::TileHit::Body => open = Some(s.id.clone()),
                        crate::cards::TileHit::Add => del = Some(s.id.clone()),
                        crate::cards::TileHit::None => {}
                    }
                    ui.add_space(6.0);
                }
                if let Some(id) = open {
                    self.open_grok_session(&id);
                }
                if let Some(id) = del {
                    self.delete_grok_history(&id);
                    self.nav = Nav::History;
                }
            }
        });
    }

    pub(super) fn ui_board(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                if crate::cards::page_header(ui, "Workboard", "New card") {
                    self.board_compose = true;
                }
                if self.board_compose {
                    egui::Frame::none()
                        .fill(crate::theme::elevated())
                        .rounding(16.0)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::same(14.0))
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.board_title)
                                    .hint_text("Card title")
                                    .desired_width(f32::INFINITY)
                                    .frame(false),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if crate::cards::white_pill(ui, "Add")
                                    && !self.board_title.trim().is_empty()
                                {
                                    self.board.push(BoardCard::new(
                                        &std::mem::take(&mut self.board_title),
                                        "",
                                        "",
                                    ));
                                    self.board_compose = false;
                                    self.flush_board();
                                }
                                if crate::cards::ghost_pill(ui, "Cancel") {
                                    self.board_compose = false;
                                    self.board_title.clear();
                                }
                            });
                        });
                    ui.add_space(16.0);
                }
                crate::cards::section_label(ui, "Open");
                let mut bump: Option<(usize, BoardStatus)> = None;
                if self.board.is_empty() {
                    let _ = crate::cards::empty_prompt_tile(
                        ui,
                        crate::icons::TileIcon::Board,
                        "No cards yet",
                        "Pin a task from chat, or add one here.",
                    );
                } else {
                    let n = self.board.len();
                    crate::cards::tile_row(ui, n, |ui, i| {
                        let c = &self.board[i];
                        let body = if c.detail.is_empty() {
                            c.status.as_str().to_string()
                        } else {
                            format!(
                                "{} · {}",
                                c.status.as_str(),
                                c.detail.chars().take(72).collect::<String>()
                            )
                        };
                        crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::Board,
                            &c.title,
                            &body,
                            None,
                            false,
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if crate::cards::ghost_pill(ui, "Open") {
                                bump = Some((i, BoardStatus::Approved));
                            }
                            if crate::cards::ghost_pill(ui, "Start") {
                                bump = Some((i, BoardStatus::InProgress));
                            }
                            if crate::cards::ghost_pill(ui, "Done") {
                                bump = Some((i, BoardStatus::Done));
                            }
                            if crate::cards::ghost_pill(ui, "Dismiss") {
                                bump = Some((i, BoardStatus::Dismissed));
                            }
                        });
                    });
                }
                if let Some((i, st)) = bump {
                    if let Some(c) = self.board.get_mut(i) {
                        c.status = st;
                    }
                    self.flush_board();
                }
            });
    }

    pub(super) fn ui_skills(&mut self, ctx: &egui::Context) {
        if !self.grok_catalog_loaded && self.grok_catalog_rx.is_none() {
            self.reload_grok_catalog();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Skills and Connectors", "Refresh") {
                self.reload_grok_catalog();
                self.skill_list = skills::list_skills();
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if crate::cards::tab_pill(ui, "Skills", !self.skills_tab_connectors) {
                    self.skills_tab_connectors = false;
                    self.nav = Nav::Skills;
                }
                if crate::cards::tab_pill(ui, "Connectors", self.skills_tab_connectors) {
                    self.skills_tab_connectors = true;
                    self.nav = Nav::Connectors;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::cards::search_field(ui, &mut self.skill_q);
                });
            });
            ui.add_space(16.0);
            let q = self.skill_q.to_ascii_lowercase();
            let mut use_skill: Option<String> = None;
            let mut use_cabin_skill: Option<(String, String)> = None;
            let mut mcp_toggle: Option<(String, bool)> = None;
            let mut mcp_remove: Option<String> = None;
            let mut plugin_toggle: Option<(String, bool)> = None;
            let mut plugin_install: Option<String> = None;
            let mut plugin_uninstall: Option<String> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
            if self.skills_tab_connectors {
                ui.horizontal(|ui| {
                    crate::cards::section_label(ui, "MCP servers");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::cards::ghost_pill(ui, "Doctor") {
                            self.run_grok_user_cmd(vec![
                                "mcp".into(),
                                "doctor".into(),
                                "--json".into(),
                            ]);
                        }
                        if crate::cards::white_pill(ui, "Add MCP") {
                            self.mcp_compose = true;
                        }
                    });
                });
                ui.label(
                    RichText::new("Grok Build `grok mcp` — add, enable, disable, or remove servers.")
                        .size(12.0)
                        .color(crate::theme::muted()),
                );
                if self.mcp_compose {
                    ui.add_space(8.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.mcp_nl)
                            .hint_text("name npx -y package   or   remove name")
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
                let mcp: Vec<_> = self
                    .grok_catalog
                    .mcp
                    .iter()
                    .filter(|s| {
                        q.is_empty()
                            || s.name.to_ascii_lowercase().contains(&q)
                            || s.target.to_ascii_lowercase().contains(&q)
                    })
                    .cloned()
                    .collect();
                if mcp.is_empty() {
                    ui.label(
                        RichText::new("No MCP servers in ~/.grok — add one with grok mcp add.")
                            .color(crate::theme::muted()),
                    );
                } else {
                    crate::cards::tile_row(ui, mcp.len(), |ui, i| {
                        let s = &mcp[i];
                        let add = if s.enabled { "Disable" } else { "Enable" };
                        let body = if s.target.is_empty() {
                            if s.enabled { "Enabled" } else { "Disabled" }.into()
                        } else {
                            s.target.clone()
                        };
                        let hit = crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::List,
                            &s.name,
                            &body,
                            Some(add),
                            s.enabled,
                        );
                        if hit == crate::cards::TileHit::Add {
                            mcp_toggle = Some((s.name.clone(), !s.enabled));
                        }
                        if crate::cards::ghost_pill(ui, "Remove") {
                            mcp_remove = Some(s.name.clone());
                        }
                    });
                }
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    crate::cards::section_label(ui, "Plugins");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::cards::ghost_pill(ui, "Update") {
                            self.run_grok_user_cmd(vec!["plugin".into(), "update".into()]);
                        }
                    });
                });
                ui.label(
                    RichText::new("Installed from the Grok Build marketplace (`grok plugin list`).")
                        .size(12.0)
                        .color(crate::theme::muted()),
                );
                ui.add_space(8.0);
                let installed: Vec<_> = self
                    .grok_catalog
                    .plugins
                    .iter()
                    .filter(|p| p.status != "available")
                    .filter(|p| {
                        q.is_empty()
                            || p.name.to_ascii_lowercase().contains(&q)
                            || p.marketplace.to_ascii_lowercase().contains(&q)
                    })
                    .cloned()
                    .collect();
                if installed.is_empty() {
                    ui.label(
                        RichText::new("No plugins installed yet — browse Marketplace below.")
                            .color(crate::theme::muted()),
                    );
                } else {
                    crate::cards::tile_row(ui, installed.len(), |ui, i| {
                        let p = &installed[i];
                        let add = if p.enabled { "Disable" } else { "Enable" };
                        let body = if p.marketplace.is_empty() {
                            p.source.clone()
                        } else {
                            p.marketplace.clone()
                        };
                        let hit = crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::Bolt,
                            &p.name,
                            &body,
                            Some(add),
                            p.enabled,
                        );
                        if hit == crate::cards::TileHit::Add {
                            plugin_toggle = Some((p.name.clone(), !p.enabled));
                        }
                        if crate::cards::ghost_pill(ui, "Uninstall") {
                            plugin_uninstall = Some(p.name.clone());
                        }
                    });
                }
                ui.add_space(20.0);
                crate::cards::section_label(ui, "Marketplace");
                ui.label(
                    RichText::new("xAI Official and other sources (`grok plugin marketplace`).")
                        .size(12.0)
                        .color(crate::theme::muted()),
                );
                ui.add_space(8.0);
                let market: Vec<_> = self
                    .grok_catalog
                    .plugins
                    .iter()
                    .filter(|p| p.status == "available")
                    .filter(|p| {
                        q.is_empty()
                            || p.name.to_ascii_lowercase().contains(&q)
                            || p.description.to_ascii_lowercase().contains(&q)
                            || p.marketplace.to_ascii_lowercase().contains(&q)
                    })
                    .cloned()
                    .collect();
                if market.is_empty() {
                    ui.label(
                        RichText::new("No marketplace plugins to install.")
                            .color(crate::theme::muted()),
                    );
                } else {
                    crate::cards::tile_row(ui, market.len(), |ui, i| {
                        let p = &market[i];
                        let body = if p.description.is_empty() {
                            p.marketplace.clone()
                        } else {
                            p.description.clone()
                        };
                        if crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::Bolt,
                            &p.name,
                            &body,
                            Some("Install"),
                            false,
                        ) == crate::cards::TileHit::Add
                        {
                            plugin_install = Some(p.name.clone());
                        }
                    });
                }
            } else {
            let workflows: Vec<_> = self
                .grok_catalog
                .workflows
                .iter()
                .filter(|w| {
                    q.is_empty()
                        || w.name.to_ascii_lowercase().contains(&q)
                        || w.description.to_ascii_lowercase().contains(&q)
                })
                .cloned()
                .collect();
            if !workflows.is_empty() {
                crate::cards::section_label(ui, "Workflows");
                ui.label(
                    RichText::new("Grok Build `/workflow` skills and `*.rhai` under ~/.grok/workflows.")
                        .size(12.0)
                        .color(crate::theme::muted()),
                );
                ui.add_space(8.0);
                crate::cards::tile_row(ui, workflows.len(), |ui, i| {
                    let w = &workflows[i];
                    if crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Bolt,
                        &w.name,
                        &format!("{} · {}", w.source, w.description),
                        Some("Use in chat"),
                        false,
                    ) == crate::cards::TileHit::Add
                    {
                        use_skill = Some(w.name.clone());
                    }
                });
                ui.add_space(16.0);
            }
            let cabin_skills: Vec<_> = self
                .skill_list
                .iter()
                .filter(|s| {
                    q.is_empty()
                        || s.name.to_ascii_lowercase().contains(&q)
                        || s.description.to_ascii_lowercase().contains(&q)
                        || s.trigger.to_ascii_lowercase().contains(&q)
                })
                .cloned()
                .collect();
            crate::cards::section_label(ui, "Cabin skills");
            ui.label(
                RichText::new("SKILL.md under ~/.config/GrokHub/skills. The cabin follows these on a matching ask and writes new ones after a hard host run.")
                    .size(12.0)
                    .color(crate::theme::muted()),
            );
            ui.add_space(8.0);
            if cabin_skills.is_empty() {
                ui.label(
                    RichText::new(if self.skill_list.is_empty() {
                        "None yet. The cabin saves one after it works something out, or /skill <name> runs one you wrote."
                    } else {
                        "None matched."
                    })
                    .color(crate::theme::muted()),
                );
            } else {
                crate::cards::tile_row(ui, cabin_skills.len(), |ui, i| {
                    let s = &cabin_skills[i];
                    let runs = match s.runs {
                        0 => "never run".to_string(),
                        1 => "1 run".to_string(),
                        n => format!("{n} runs"),
                    };
                    let body = if s.description.trim().is_empty() {
                        runs
                    } else {
                        format!("{runs} · {}", s.description)
                    };
                    if crate::cards::grok_tile(
                        ui,
                        crate::icons::icon_for_label(&s.name),
                        &s.name,
                        &body,
                        Some("Use in chat"),
                        false,
                    ) == crate::cards::TileHit::Add
                    {
                        use_cabin_skill = Some((s.slash.clone(), s.name.clone()));
                    }
                });
            }
            ui.add_space(16.0);
            crate::cards::section_label(ui, "Grok Build skills");
            ui.label(
                RichText::new("Bundled skills and plugin skills from `grok inspect`. Use in chat sends /name.")
                    .size(12.0)
                    .color(crate::theme::muted()),
            );
            ui.add_space(8.0);
            let skills: Vec<_> = self
                .grok_catalog
                .skills
                .iter()
                .filter(|s| {
                    q.is_empty()
                        || s.name.to_ascii_lowercase().contains(&q)
                        || s.description.to_ascii_lowercase().contains(&q)
                        || s.plugin.to_ascii_lowercase().contains(&q)
                })
                .cloned()
                .collect();
            if skills.is_empty() {
                ui.label(
                    RichText::new("Loading Grok Build skills… or none matched.")
                        .color(crate::theme::muted()),
                );
            } else {
                crate::cards::tile_row(ui, skills.len(), |ui, i| {
                    let s = &skills[i];
                    let src = grokhub_acp::skill_source_label(s);
                    let body = if s.description.is_empty() {
                        src
                    } else {
                        format!("{src} · {}", s.description)
                    };
                    let add = if s.user_invocable {
                        Some("Use in chat")
                    } else {
                        None
                    };
                    if crate::cards::grok_tile(
                        ui,
                        crate::icons::icon_for_label(&s.name),
                        &s.name,
                        &body,
                        add,
                        false,
                    ) == crate::cards::TileHit::Add
                    {
                        use_skill = Some(s.name.clone());
                    }
                });
            }
            }
            });
            if let Some(name) = use_skill {
                self.nav = Nav::Chat;
                self.send_chat(skill_use_in_chat_prompt(&format!("/{name}"), &name));
            }
            if let Some((slash, name)) = use_cabin_skill {
                self.nav = Nav::Chat;
                self.send_chat(skill_use_in_chat_prompt(&slash, &name));
            }
            if let Some((name, on)) = mcp_toggle {
                let cmd = if on { "enable" } else { "disable" };
                self.run_grok_user_cmd(vec!["mcp".into(), cmd.into(), name]);
            }
            if let Some(name) = mcp_remove {
                self.run_grok_user_cmd(vec!["mcp".into(), "remove".into(), name]);
            }
            if let Some((name, on)) = plugin_toggle {
                let cmd = if on { "enable" } else { "disable" };
                self.run_grok_user_cmd(vec!["plugin".into(), cmd.into(), name]);
            }
            if let Some(name) = plugin_uninstall {
                self.run_grok_user_cmd(vec![
                    "plugin".into(),
                    "uninstall".into(),
                    name,
                    "--confirm".into(),
                ]);
            }
            if let Some(name) = plugin_install {
                self.run_grok_user_cmd(vec![
                    "plugin".into(),
                    "install".into(),
                    name,
                    "--trust".into(),
                ]);
            }
        });
    }

    /// Search as the query settles. Every keystroke would spawn a walk of every thread,
    /// so a query waits `HISTORY_TYPE_DELAY` before it runs.
    pub(super) fn tick_history_search(&mut self, ctx: &egui::Context) {
        if self.history_q != self.history_q_seen {
            self.history_q_seen = self.history_q.clone();
            self.history_q_at = Some(Instant::now());
            self.history_hits.clear();
            if self.history_q.trim().is_empty() {
                self.history_q_at = None;
            }
        }
        let Some(at) = self.history_q_at else {
            return;
        };
        if at.elapsed() < HISTORY_TYPE_DELAY {
            ctx.request_repaint_after(HISTORY_TYPE_DELAY);
            return;
        }
        if self.history_rx.is_some() {
            // A search is still running. Come back for the newer query.
            ctx.request_repaint_after(HISTORY_TYPE_DELAY);
            return;
        }
        self.history_q_at = None;
        self.kick_history_search();
    }

    pub(super) fn kick_history_search(&mut self) {
        if !self.scratch() {
            let name = self.mem_name.clone();
            let body = self.mem_body.clone();
            std::thread::spawn(move || {
                if config::read_memory(&name) != body {
                    let _ = config::write_memory(&name, &body);
                }
            });
        }
        let q = self.history_q.clone();
        let mem_name = self.mem_name.clone();
        let mem_body = self.mem_body.clone();
        let vis = self.thread_idx;
        let mut thread_rows = Vec::new();
        for (i, t) in self.threads.iter().enumerate() {
            let body = if i == vis {
                search_thread_body(self.messages.iter().map(|m| m.1.as_str()))
            } else {
                search_thread_body(t.messages.iter().map(|(_, c)| c.as_str()))
            };
            thread_rows.push((format!("thread:{}", t.id), t.title.clone(), body));
        }
        let (tx, rx) = mpsc::channel();
        self.history_rx = Some(rx);
        self.status = "Searching…".into();
        std::thread::spawn(move || {
            let soul = if mem_name == "SOUL.md" {
                mem_body.clone()
            } else {
                config::read_memory("SOUL.md")
            };
            let user = if mem_name == "USER.md" {
                mem_body.clone()
            } else {
                config::read_memory("USER.md")
            };
            let memory = if mem_name == "MEMORY.md" {
                mem_body.clone()
            } else {
                config::read_memory("MEMORY.md")
            };
            let mut rows = vec![
                ("mem:SOUL.md".to_string(), "SOUL.md".to_string(), soul),
                ("mem:USER.md".to_string(), "USER.md".to_string(), user),
                ("mem:MEMORY.md".to_string(), "MEMORY.md".to_string(), memory),
            ];
            rows.extend(thread_rows);
            let hits = search_corpus_tagged(&q, &rows);
            let _ = tx.send((q, hits));
        });
    }

    /// A hit is a door: memory hits open that file in the editor, chat hits open the
    /// thread they came from.
    pub(super) fn open_history_hit(&mut self, target: &str) {
        if let Some(name) = target.strip_prefix("mem:") {
            let name = name.to_string();
            self.open_memory_file(&name);
            self.nav = Nav::Memory;
            self.status = name;
            return;
        }
        let Some(id) = target.strip_prefix("thread:") else {
            return;
        };
        let Some(idx) = self.threads.iter().position(|t| t.id == id) else {
            self.status = "That chat is gone".into();
            return;
        };
        // Also re-pins when the hit belongs to the chat already on screen.
        self.pin_chat_tail();
        self.switch_thread(idx);
        self.nav = Nav::Chat;
    }

    /// Show a memory file in the editor. The file being left is flushed to disk off the
    /// UI thread first, so switching tabs never drops an edit. Re-opening the file already
    /// in the editor keeps `mem_body` — the cache is a disk snapshot and would wipe typing.
    pub(super) fn open_memory_file(&mut self, name: &str) {
        if self.mem_name == name {
            return;
        }
        if !self.scratch() {
            let leaving = self.mem_name.clone();
            let body = self.mem_body.clone();
            if let Some(i) = Self::mem_file_idx(&leaving) {
                self.mem_cache_body[i] = body.clone();
                self.mem_cache_at[i] = config::memory_updated_at(&leaving);
            }
            std::thread::spawn(move || {
                if config::read_memory(&leaving) != body {
                    let _ = config::write_memory(&leaving, &body);
                }
            });
        }
        self.mem_name = name.into();
        let at = config::memory_updated_at(name);
        let Some(i) = Self::mem_file_idx(name) else {
            self.mem_body = config::read_memory(name);
            return;
        };
        if self.mem_cache_at[i] == 0 {
            self.mem_body = config::read_memory(name);
            self.mem_cache_body[i] = self.mem_body.clone();
            self.mem_cache_at[i] = at;
            return;
        }
        self.mem_body = self.mem_cache_body[i].clone();
        if self.mem_cache_at[i] != at && self.mem_file_rx.is_none() {
            let n = name.to_string();
            let (tx, rx) = mpsc::channel();
            self.mem_file_rx = Some((n.clone(), rx));
            std::thread::spawn(move || {
                let body = config::read_memory(&n);
                let at = config::memory_updated_at(&n);
                let _ = tx.send((at, body));
            });
        }
    }
}
