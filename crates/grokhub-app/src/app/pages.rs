//! The other cabin pages.

use super::*;

/// Queue tile status. A finished task is done. A live task whose title starts
/// with "failed" is failed. Every other live task is running.
pub(super) fn queue_task_label(title: &str, done: bool) -> &'static str {
    let failed = title.to_ascii_lowercase().starts_with("failed");
    if done {
        "done"
    } else if failed {
        "failed"
    } else {
        "running"
    }
}

/// Shown when the catalog timed out and the list is empty.
pub(super) const CATALOG_TIMEOUT_EMPTY: &str =
    "Grok Build didn't answer. Refresh to try again.";

/// Shown above tiles when a timeout kept the previous list.
pub(super) const CATALOG_TIMEOUT_STALE: &str =
    "Showing the last list — Grok Build timed out.";

/// Empty catalog copy. Loading wins, then a search that filtered a non-empty list.
/// A timed-out empty list is not "none found".
pub(super) fn catalog_empty_line<'a>(
    loading: bool,
    query: &str,
    total: usize,
    empty_text: &'a str,
    status: &str,
) -> &'a str {
    if loading {
        "Loading…"
    } else if !query.is_empty() && total > 0 {
        "None matched."
    } else if status == super::acp::GROK_CATALOG_TIMEOUT && total == 0 {
        CATALOG_TIMEOUT_EMPTY
    } else {
        empty_text
    }
}

/// One muted line above tiles when the timeout kept a non-empty list.
pub(super) fn catalog_stale_line(status: &str, total: usize) -> Option<&'static str> {
    if status == super::acp::GROK_CATALOG_TIMEOUT && total > 0 {
        Some(CATALOG_TIMEOUT_STALE)
    } else {
        None
    }
}

fn paint_catalog_stale(ui: &mut egui::Ui, status: &str, total: usize) {
    let Some(line) = catalog_stale_line(status, total) else {
        return;
    };
    ui.label(
        RichText::new(line)
            .size(crate::theme::FONT_BODY)
            .color(crate::theme::muted()),
    );
    ui.add_space(6.0);
}

pub(super) enum BoardAct {
    Add,
    Save(String),
    Move { id: String, status: BoardStatus },
    Archive(String),
    Restore(String),
    /// Remove the card for good. Its chat stays in History.
    Delete(String),
    Link(String),
    Unlink(String),
    Open(String),
    Edit(String),
    /// Open the notes editor on this card.
    EditNotes(String),
    SaveNotes { id: String, notes: String },
    CancelNotes,
    /// Start a chat from this card, with its notes, and move it to Doing.
    Work(String),
    /// A click on a card: opens a small card in place, folds the open one.
    Expand(String),
    /// Send a line to the agent from the card's chat box.
    Say { id: String, text: String },
    /// Stop the turn running in a card's chat.
    Stop,
}

/// Drag payload for a workboard card: its id.
pub(super) struct BoardDrag(pub String);

/// Dropping a card on a column moves it there. The column it already sits in is a no-op.
pub(super) fn board_drop_move(from: BoardStatus, onto: KanbanColumn) -> Option<BoardStatus> {
    if from.column() == Some(onto) {
        None
    } else {
        Some(onto.status())
    }
}

/// While a card is dragged, its title follows the pointer.
pub(super) fn paint_drag_ghost(ctx: &egui::Context, title: &str) {
    let Some(pos) = ctx.pointer_interact_pos() else {
        return;
    };
    egui::Area::new(egui::Id::new("board-drag-ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(12.0, 8.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(crate::theme::panel())
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::link()))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(title.chars().take(60).collect::<String>())
                            .size(crate::theme::FONT_BODY)
                            .color(crate::theme::fg()),
                    );
                });
        });
}

/// A column takes drops below its last card, not only on the cards.
pub(super) const BOARD_DROP_MIN_H: f32 = 180.0;

impl Cabin {

    pub(super) fn ui_command(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
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
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .corner_radius(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        let enter = ui
                            .add(
                                egui::TextEdit::singleline(&mut self.cmd_line)
                                    .hint_text(crate::theme::hint("$ ls — bound project is the working tree"))
                                    .desired_width(f32::INFINITY)
                                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2))),
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

    pub(super) fn ui_connectors(&mut self, ui: &mut egui::Ui) {
        self.skills_tab_connectors = true;
        self.ui_skills(ui);
    }

    fn ui_connector_home_note(&self, ui: &mut egui::Ui) {
        let path = grokhub_acp::cabin_grok_home()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "the cabin Grok home".to_string());
        ui.label(
            RichText::new(format!(
                "MCP servers and hooks here come from your Grok Build home (~/.grok). Ask chats run in the cabin's own Grok home ({path}), so they don't see these yet."
            ))
            .size(12.0)
            .color(crate::theme::muted()),
        );
    }

    fn ui_hooks_section(&mut self, ui: &mut egui::Ui, q: &str) {
        if self.cfg.native_engine {
            self.ensure_native_listing();
        }
        let native = self.cfg.native_engine;
        let native_rows: Vec<grokhub_acp::GrokHookRow> = if native {
            self.native_hooks
                .iter()
                .map(|hook| grokhub_acp::GrokHookRow {
                    event: hook.event.clone(),
                    hook_type: hook.kind.clone(),
                    target: hook.command.clone(),
                    origin: match hook.origin {
                        grokhub_agent::HookOrigin::User => grokhub_acp::HookOrigin::User,
                        grokhub_agent::HookOrigin::Project => grokhub_acp::HookOrigin::Project,
                        grokhub_agent::HookOrigin::Plugin => grokhub_acp::HookOrigin::Other,
                    },
                    path: hook.path.display().to_string(),
                    matcher: if hook.matcher.is_empty() {
                        None
                    } else {
                        Some(hook.matcher.clone())
                    },
                })
                .collect()
        } else {
            Vec::new()
        };
        let hooks_section = ui
            .vertical(|ui| {
                crate::cards::section_label(ui, "Hooks");
                if !native && self.grok_catalog.project_trusted == Some(false) {
                    ui.label(
                        RichText::new(
                            "Project hooks stay hidden until this folder is trusted in Grok Build (/hooks-trust).",
                        )
                        .size(12.0)
                        .color(crate::theme::muted()),
                    );
                    ui.add_space(6.0);
                }
                let native_project_hooks = native
                    && self
                        .native_hooks
                        .iter()
                        .any(|hook| hook.origin == grokhub_agent::HookOrigin::Project);
                if native_project_hooks {
                    let trusted = self.native_hooks_trusted;
                    let note = if trusted {
                        "This folder is trusted: its project hooks run on native chats."
                    } else {
                        "Project hooks come from this repository and don't run on native chats until you trust this folder."
                    };
                    ui.label(RichText::new(note).size(12.0).color(crate::theme::muted()));
                    let label = if trusted {
                        "Stop trusting this folder"
                    } else {
                        "Trust this folder's hooks"
                    };
                    if ui.button(label).clicked() {
                        let workspace = self.grok_cwd();
                        match grokhub_agent::set_folder_trust(&workspace, !trusted) {
                            Ok(()) => self.native_listing_cwd.clear(),
                            Err(err) => self.status = err,
                        }
                    }
                    ui.add_space(6.0);
                }
                let hooks: Vec<grokhub_acp::GrokHookRow> = if native {
                    native_rows
                        .iter()
                        .filter(|h| {
                            q.is_empty()
                                || h.event.to_ascii_lowercase().contains(q)
                                || h.target.to_ascii_lowercase().contains(q)
                                || h.matcher
                                    .as_ref()
                                    .is_some_and(|m| m.to_ascii_lowercase().contains(q))
                        })
                        .cloned()
                        .collect()
                } else {
                    self.grok_catalog
                        .hooks
                        .iter()
                        .filter(|h| {
                            q.is_empty()
                                || h.event.to_ascii_lowercase().contains(q)
                                || h.target.to_ascii_lowercase().contains(q)
                                || h.matcher
                                    .as_ref()
                                    .is_some_and(|m| m.to_ascii_lowercase().contains(q))
                        })
                        .cloned()
                        .collect()
                };
                let none = if native {
                    self.native_hooks.is_empty()
                } else {
                    self.grok_catalog.hooks.is_empty()
                };
                if none {
                    ui.label(
                        RichText::new(
                            "No hooks in ~/.grok/hooks or this project's .grok/hooks.",
                        )
                        .color(crate::theme::muted()),
                    );
                } else if hooks.is_empty() {
                    ui.label(RichText::new("None matched.").color(crate::theme::muted()));
                } else {
                    for h in &hooks {
                        ui.add_space(8.0);
                        ui.label(RichText::new(&h.event).size(14.0).color(crate::theme::fg()));
                        let target_line = match (h.hook_type.is_empty(), h.target.is_empty()) {
                            (true, true) => String::new(),
                            (true, false) => h.target.clone(),
                            (false, true) => h.hook_type.clone(),
                            (false, false) => format!("{} {}", h.hook_type, h.target),
                        };
                        if !target_line.is_empty() {
                            ui.label(
                                RichText::new(target_line)
                                    .size(13.0)
                                    .color(crate::theme::fg()),
                            );
                        }
                        let origin = if h.path.is_empty() {
                            h.origin.as_str().to_string()
                        } else {
                            format!("{} · {}", h.origin.as_str(), h.path)
                        };
                        ui.label(
                            RichText::new(origin)
                                .size(12.0)
                                .color(crate::theme::muted()),
                        );
                        let matcher = h.matcher.as_deref().unwrap_or("any");
                        ui.label(
                            RichText::new(format!("matcher {matcher}"))
                                .size(12.0)
                                .color(crate::theme::muted()),
                        );
                    }
                }
            })
            .response;
        if self.scroll_to_hooks {
            hooks_section.scroll_to_me(Some(egui::Align::TOP));
            self.scroll_to_hooks = false;
        }
    }

    fn ui_mcp_row_status(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        status: &grokhub_acp::McpDoctorStatus,
    ) {
        let color = match status {
            grokhub_acp::McpDoctorStatus::Connected => crate::theme::live(),
            grokhub_acp::McpDoctorStatus::NeedsSignIn | grokhub_acp::McpDoctorStatus::Error(_) => {
                crate::theme::setup()
            }
        };
        ui.label(RichText::new(status.label()).size(12.0).color(color));
        if matches!(status, grokhub_acp::McpDoctorStatus::NeedsSignIn) {
            ui.label(
                RichText::new(format!(
                    "Sign in from Grok Build: run grok, open /mcps, press i on {name}"
                ))
                .size(12.0)
                .color(crate::theme::muted()),
            );
        }
    }

    pub(super) fn ui_agents(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
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
                let todo_session = self
                    .threads
                    .get(self.thread_idx)
                    .and_then(|thread| thread.grok_session.clone())
                    .unwrap_or_default();
                let todos = if self.cfg.native_engine {
                    grokhub_agent::todos_for(&todo_session)
                } else {
                    Vec::new()
                };
                if !self.grok_tasks.is_empty() {
                    crate::cards::section_label(ui, "Grok tasks");
                    ui.add_space(8.0);
                    for (id, title, done) in &self.grok_tasks {
                        let st = queue_task_label(title, *done);
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
                if !todos.is_empty() {
                    crate::cards::section_label(ui, "Todos");
                    ui.add_space(8.0);
                    for todo in &todos {
                        let done = todo.status == "completed" || todo.status == "cancelled";
                        crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::List,
                            &todo.content,
                            &format!("{} · {}", todo.status, todo.id),
                            None,
                            done,
                        );
                        ui.add_space(6.0);
                    }
                    ui.add_space(12.0);
                }
                if self.agents.is_empty() && self.grok_tasks.is_empty() && todos.is_empty() {
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
                    if self.start_queued_job(i) {
                        self.kick_model(false);
                    }
                }
            });
    }

    /// Queue Run state. True when the caller should kick the model.
    /// This call does not spawn grok.
    pub(super) fn start_queued_job(&mut self, i: usize) -> bool {
        if i >= self.agents.len() {
            return false;
        }
        if self.running {
            self.status = "Busy — wait, then run".into();
            return false;
        }
        self.agents[i].status = "running".into();
        let p = self.agents[i].prompt.clone();
        let tid = self.agents[i].thread_id.clone();
        self.nav = Nav::Chat;
        if !tid.is_empty() {
            self.chat_job_thread = Some(tid);
        }
        self.bg.steer_follow = None;
        self.bg.results_follow = None;
        self.push_bound_msg("user", p);
        self.persist();
        true
    }

    pub(super) fn ui_devices(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
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
                    "Not sharing. Start share to pair another computer.".into()
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
                            "Open {} on the other computer.",
                            discover_hub_pair_url(self.hub_port)
                        ),
                        None,
                        false,
                    );
                } else if sharing {
                    ui.label(
                        RichText::new("Paired. Make a new code after another computer joins.")
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
                    "Start share to mint a code for another computer.",
                ) {
                    self.start_hub();
                }
                ui.add_space(16.0);
                crate::cards::section_label(ui, "Send a task");
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .corner_radius(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.task_prompt)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2)))
                                .hint_text(crate::theme::hint("What should this computer do?")),
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

    pub(super) fn ui_memory(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
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
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .corner_radius(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.mem_body)
                                .desired_rows(24)
                                .desired_width(f32::INFINITY)
                                .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2)))
                                .font(egui::TextStyle::Monospace),
                        );
                    });
            });
    }

    pub(super) fn ui_get_started(&mut self, ui: &mut egui::Ui) -> bool {
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
                let cmd = grokhub_acp::grok_cli_install_cmd();
                // Some install errors already end with the command; do not print it twice.
                if self.grok_install_err.contains(cmd.trim()) {
                    self.grok_install_err.clone()
                } else {
                    format!("{}\n{}", self.grok_install_err, cmd)
                }
            } else {
                "Installing Grok Build CLI (alpha)…".to_string()
            };
            let show_retry = grokhub_core::should_show_manual_cli_install(grok_present, installing)
                && !self.grok_install_err.is_empty();
            let mut retry = false;
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(crate::theme::bg()))
                .show(ui, |ui| {
                    ui.centered_and_justified(|ui| {
                        egui::Frame::NONE
                            .fill(crate::theme::panel())
                            .corner_radius(crate::theme::SHEET_RADIUS)
                            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                            .inner_margin(egui::Margin::same(24))
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
            .frame(egui::Frame::NONE.fill(crate::theme::bg()))
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    egui::Frame::NONE
                        .fill(crate::theme::panel())
                        .corner_radius(crate::theme::SHEET_RADIUS)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::same(24))
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

    pub(super) fn ui_history(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::theme::bg()).inner_margin(egui::Margin::same(24)))
            .show(ui, |ui| {
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
                let hit = egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .corner_radius(10.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::symmetric(10, 6))
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
            crate::cards::section_label(ui, "Chats");
            ui.label(
                RichText::new("Cabin chats. Project chats stay in the project section. Background jobs stay off this list. Delete removes the chat.")
                    .size(12.0)
                    .color(crate::theme::subtle()),
            );
            let live_empty = self.messages.is_empty();
            let shown = threads::chat_section_indices(
                &self.threads,
                Some(self.thread_idx),
                live_empty,
            );
            if shown.is_empty() {
                ui.label(
                    RichText::new("No chats yet.")
                        .size(13.0)
                        .color(crate::theme::muted()),
                );
            } else {
                let mut open: Option<usize> = None;
                let mut del: Option<usize> = None;
                let keys: Vec<threads::SessionSortKey> = shown
                    .iter()
                    .map(|&i| {
                        let t = &self.threads[i];
                        threads::SessionSortKey {
                            pinned: t.pinned,
                            pinned_ms: t.pinned_ms,
                            accessed_ms: t.accessed_ms,
                            list_rank: 0,
                        }
                    })
                    .collect();
                let order = threads::session_list_order(&keys);
                for pos in order {
                    let i = shown[pos];
                    let title = self.thread_rail_title(i);
                    if threads::is_background_history_title(&title) {
                        continue;
                    }
                    let kind = if keys[pos].pinned { "Pinned" } else { "Chat" };
                    match crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Chat,
                        &title,
                        kind,
                        Some("Delete"),
                        false,
                    ) {
                        crate::cards::TileHit::Body => open = Some(i),
                        crate::cards::TileHit::Add => del = Some(i),
                        crate::cards::TileHit::None => {}
                    }
                    ui.add_space(6.0);
                }
                if let Some(i) = open {
                    self.switch_thread(i);
                    self.nav = Nav::Chat;
                    self.composer_want_focus = true;
                }
                if let Some(i) = del {
                    self.delete_thread_at(i);
                    self.nav = Nav::History;
                }
            }
            self.paint_native_history_merge(ui);
        });
    }

    pub(super) fn ui_board(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut act: Option<BoardAct> = None;
        // Esc belongs to a focused field, a menu, a sheet, the palette or a dialog
        // first. A field painted earlier this frame (the sidebar) has already
        // dropped its focus on this press, so last frame's focus counts too.
        let key_taken = self.board_view.keys_held
            || ctx.egui_wants_keyboard_input()
            || super::chat_ui::overlay_over_chat(&ctx)
            || self.palette_open
            || self.shortcuts_open
            || self.confirm.as_ref().is_some_and(|c| c.paints_overlay());
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
                if crate::cards::page_header(ui, "Workboards", "New card") {
                    self.board_edit = None;
                    self.board_title.clear();
                    self.board_notes.clear();
                    self.board_link = false;
                    self.board_compose = true;
                }
                crate::cards::help_text(
                    ui,
                    "Tasks from your chats and the ones you add, by status. Click a card to open it and talk to the agent, click its title or press Esc to fold it, and drag it to move it.",
                );
                ui.add_space(12.0);
                // The same needs-attention line and decision rows as Home (Spike-1b).
                if self.decisions_waiting() > 0 {
                    self.paint_inbox(ui);
                    ui.add_space(12.0);
                }
                if self.board_compose {
                    egui::Frame::NONE
                        .fill(crate::theme::elevated())
                        .corner_radius(crate::theme::CARD_RADIUS)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::same(14))
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.board_title)
                                    .hint_text(crate::theme::hint("Card title"))
                                    .desired_width(f32::INFINITY)
                                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2))),
                            );
                            ui.add_space(8.0);
                            ui.add(
                                egui::TextEdit::multiline(&mut self.board_notes)
                                    .hint_text(crate::theme::hint("Notes"))
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(3)
                                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2))),
                            );
                            ui.add_space(8.0);
                            ui.checkbox(&mut self.board_link, "Link current chat");
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                let save = if self.board_edit.is_some() {
                                    "Save"
                                } else {
                                    "Add"
                                };
                                if crate::cards::white_pill(ui, save)
                                    && !self.board_title.trim().is_empty()
                                {
                                    act = Some(if let Some(id) = self.board_edit.clone() {
                                        BoardAct::Save(id)
                                    } else {
                                        BoardAct::Add
                                    });
                                }
                                if crate::cards::ghost_pill(ui, "Cancel") {
                                    self.board_compose = false;
                                    self.board_edit = None;
                                    self.board_title.clear();
                                    self.board_notes.clear();
                                    self.board_link = false;
                                }
                            });
                        });
                    ui.add_space(16.0);
                }
                // With no live card, the empty tile is the whole board: no row of "—" columns.
                let board_empty = self.board.iter().all(|c| c.status.column().is_none());
                if board_empty {
                    if crate::cards::empty_prompt_tile(
                        ui,
                        crate::icons::TileIcon::Board,
                        "No cards yet",
                        "A chat run files a card here, or click to add one.",
                    ) && !self.board_compose
                    {
                        self.board_edit = None;
                        self.board_title.clear();
                        self.board_notes.clear();
                        self.board_link = false;
                        self.board_compose = true;
                    }
                    ui.add_space(12.0);
                }
                let dragging = egui::DragAndDrop::has_payload_of_type::<BoardDrag>(ui.ctx());
                let from = |board: &[BoardCard], drag: &BoardDrag| {
                    board.iter().find(|c| c.id == drag.0).map(|c| c.status)
                };
                egui::ScrollArea::vertical()
                    .id_salt("workboards")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Follow up: reports and questions from scheduled runs, full width,
                        // newest run first. Shown while it has cards, or as a drop target.
                        let mut follow: Vec<(String, u64)> = self
                            .board
                            .iter()
                            .filter(|c| c.status == BoardStatus::FollowUp)
                            .map(|c| (c.id.clone(), c.updated_ms))
                            .collect();
                        follow.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
                        if !board_empty && (!follow.is_empty() || dragging) {
                            let top = ui.cursor().min;
                            let w = ui.available_width();
                            let fresh = self
                                .board
                                .iter()
                                .filter(|c| c.status == BoardStatus::FollowUp && c.fresh)
                                .count();
                            let label = if fresh > 0 {
                                format!("{} · {fresh} new", KanbanColumn::FollowUp.label())
                            } else {
                                KanbanColumn::FollowUp.label().to_string()
                            };
                            crate::cards::section_label(ui, &label);
                            ui.label(
                                RichText::new("Reports and questions from your automations. Open one to read it and keep talking to the agent here.")
                                    .size(crate::theme::FONT_TIP)
                                    .color(crate::theme::muted()),
                            );
                            ui.add_space(6.0);
                            for (id, _) in &follow {
                                self.paint_board_card(ui, id, true, &mut act);
                                ui.add_space(8.0);
                            }
                            let bottom = ui.cursor().min.y.max(top.y + 64.0);
                            let zone_rect = egui::Rect::from_min_max(top, egui::pos2(top.x + w, bottom));
                            let zone = ui.interact(
                                zone_rect,
                                egui::Id::new("board-drop-follow"),
                                egui::Sense::hover(),
                            );
                            if let Some(drag) = zone.dnd_hover_payload::<BoardDrag>() {
                                if from(&self.board, &drag)
                                    .and_then(|st| board_drop_move(st, KanbanColumn::FollowUp))
                                    .is_some()
                                {
                                    ui.painter().rect_stroke(
                                        zone_rect.shrink(1.0),
                                        16.0,
                                        egui::Stroke::new(1.5_f32, crate::theme::link()),
                                        egui::StrokeKind::Middle,
                                    );
                                }
                            }
                            if let Some(drag) = zone.dnd_release_payload::<BoardDrag>() {
                                if let Some(status) = from(&self.board, &drag)
                                    .and_then(|st| board_drop_move(st, KanbanColumn::FollowUp))
                                {
                                    act = Some(BoardAct::Move {
                                        id: drag.0.clone(),
                                        status,
                                    });
                                }
                            }
                            ui.add_space(12.0);
                        }
                        if !board_empty {
                        ui.columns(KanbanColumn::ALL.len(), |cols| {
                            for (i, col) in KanbanColumn::ALL.iter().enumerate() {
                                let ui = &mut cols[i];
                                let col_top = ui.cursor().min;
                                let col_w = ui.available_width();
                                crate::cards::section_label(ui, col.label());
                                ui.add_space(6.0);
                                let ids: Vec<String> = self
                                    .board
                                    .iter()
                                    .filter(|c| c.status.column() == Some(*col))
                                    .map(|c| c.id.clone())
                                    .collect();
                                if ids.is_empty() {
                                    ui.label(
                                        RichText::new("—")
                                            .size(crate::theme::FONT_TIP)
                                            .color(crate::theme::subtle()),
                                    );
                                }
                                for id in ids {
                                    self.paint_board_card(ui, &id, false, &mut act);
                                    ui.add_space(8.0);
                                }
                                let bottom = ui.cursor().min.y.max(col_top.y + BOARD_DROP_MIN_H);
                                let zone_rect = egui::Rect::from_min_max(
                                    col_top,
                                    egui::pos2(col_top.x + col_w, bottom),
                                );
                                let zone = ui.interact(
                                    zone_rect,
                                    egui::Id::new(("board-drop", i)),
                                    egui::Sense::hover(),
                                );
                                if let Some(drag) = zone.dnd_hover_payload::<BoardDrag>() {
                                    if from(&self.board, &drag)
                                        .and_then(|st| board_drop_move(st, *col))
                                        .is_some()
                                    {
                                        ui.painter().rect_stroke(
                                            zone_rect.shrink(1.0),
                                            16.0,
                                            egui::Stroke::new(1.5_f32, crate::theme::link()),
                                            egui::StrokeKind::Middle,
                                        );
                                    }
                                }
                                if let Some(drag) = zone.dnd_release_payload::<BoardDrag>() {
                                    if let Some(status) =
                                        from(&self.board, &drag).and_then(|st| board_drop_move(st, *col))
                                    {
                                        act = Some(BoardAct::Move {
                                            id: drag.0.clone(),
                                            status,
                                        });
                                    }
                                }
                            }
                        });
                        }
                        let archived: Vec<(String, String)> = self
                            .board
                            .iter()
                            .filter(|c| c.status == BoardStatus::Dismissed)
                            .map(|c| (c.id.clone(), c.title.clone()))
                            .collect();
                        if !archived.is_empty() {
                            ui.add_space(12.0);
                            crate::cards::section_label(ui, "Archived");
                            for (id, title) in archived {
                                let width = ui.available_width().max(1.0);
                                let cache_id = egui::Id::new((
                                    "cabin-board-arch-h",
                                    id.as_str(),
                                    super::chat_ui::pane_width_bucket(width),
                                ));
                                let cached =
                                    ui.ctx().data(|d| d.get_temp::<f32>(cache_id)).unwrap_or(0.0);
                                // One `horizontal`: one parent auto-id. The cached
                                // height already includes the spacing under the row.
                                if super::chat_ui::reserve_offscreen_chat_row(ui, cached) {
                                    ui.skip_ahead_auto_ids(1);
                                    continue;
                                }
                                let y0 = ui.cursor().min.y;
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(title)
                                            .size(crate::theme::FONT_TIP)
                                            .color(crate::theme::muted()),
                                    );
                                    if crate::cards::ghost_pill(ui, "Restore") {
                                        act = Some(BoardAct::Restore(id.clone()));
                                    }
                                    if crate::cards::ghost_pill(ui, "Delete") {
                                        act = Some(BoardAct::Delete(id));
                                    }
                                });
                                let h = (ui.cursor().min.y - y0).max(0.0);
                                if h > 0.0 {
                                    ui.ctx().data_mut(|d| d.insert_temp(cache_id, h));
                                }
                            }
                        }
                    });
            });
        // Only a click opens a card; hover does nothing. A drag folds it at once.
        let dragging = egui::DragAndDrop::has_payload_of_type::<BoardDrag>(&ctx);
        super::board_ui::fold_board_on_drag(&mut self.board_view, dragging);
        let bare_esc = super::chat_ui::bare_press(ui, egui::Key::Escape);
        if super::board_ui::board_esc_folds(self.board_view.open.is_some(), bare_esc, key_taken) {
            super::chat_ui::drop_key(ui, egui::Key::Escape);
            self.board_view.open = None;
        }
        self.board_view.keys_held = ctx.egui_wants_keyboard_input();
        if self.apply_board_act(act) {
            self.flush_board();
        }
    }

    pub(super) fn apply_board_act(&mut self, act: Option<BoardAct>) -> bool {
        let Some(act) = act else {
            return false;
        };
        match act {
            BoardAct::Add => {
                let mut card = BoardCard::new(
                    &std::mem::take(&mut self.board_title),
                    &std::mem::take(&mut self.board_notes),
                    "",
                );
                card.status = BoardStatus::Todo;
                if self.board_link {
                    card.thread_id = Some(self.visible_thread_id());
                }
                self.board.push(card);
                self.board_compose = false;
                self.board_link = false;
                true
            }
            BoardAct::Save(id) => {
                let title = std::mem::take(&mut self.board_title);
                let notes = std::mem::take(&mut self.board_notes);
                let link = self.board_link;
                let thread = self.visible_thread_id();
                if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                    c.title = title.trim().chars().take(120).collect();
                    c.detail = notes.trim().chars().take(2000).collect();
                    if link {
                        c.thread_id = Some(thread);
                    }
                }
                self.board_compose = false;
                self.board_edit = None;
                self.board_link = false;
                true
            }
            BoardAct::Move { id, status } => {
                if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                    c.status = status;
                }
                true
            }
            BoardAct::Archive(id) => {
                if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                    c.status = BoardStatus::Dismissed;
                }
                true
            }
            BoardAct::Restore(id) => {
                if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                    c.status = BoardStatus::Todo;
                }
                true
            }
            BoardAct::Delete(id) => {
                let Some(thread) = self
                    .board
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.thread_id.clone())
                else {
                    return false;
                };
                if self.running && thread.is_some() && self.chat_job_thread == thread {
                    self.board_view.note = Some((id, "Stop the card's chat first".into()));
                    return false;
                }
                self.board.retain(|c| c.id != id);
                self.forget_board_card_view(&id);
                // A Follow up chat was hidden because its card was the way in.
                // With the card gone it goes back to History, as the status says.
                if self.unpark_follow_up_chat(thread.as_deref()) {
                    self.persist();
                }
                self.status = if thread.is_some() {
                    "Card deleted. Its chat stays in History.".into()
                } else {
                    "Card deleted".into()
                };
                true
            }
            BoardAct::Link(id) => {
                let thread = self.visible_thread_id();
                if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                    c.thread_id = Some(thread);
                }
                true
            }
            BoardAct::Unlink(id) => {
                if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                    c.thread_id = None;
                }
                true
            }
            BoardAct::EditNotes(id) => {
                let notes = self
                    .board
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.notes.clone())
                    .unwrap_or_default();
                self.board_notes_edit = Some((id, notes));
                false
            }
            BoardAct::CancelNotes => {
                self.board_notes_edit = None;
                false
            }
            BoardAct::SaveNotes { id, notes } => {
                self.board_notes_edit = None;
                if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                    c.notes = grokhub_core::clean_card_notes(&notes);
                }
                self.status = "Notes saved. The agent gets them with the next message on this card's chat.".into();
                true
            }
            BoardAct::Work(id) => {
                // Work happens on the card: open it and watch the chat there.
                self.board_view.open = Some(id.clone());
                self.say_on_card(&id, "");
                false
            }
            BoardAct::Expand(id) => {
                // A click on a small card opens it; a click on the open card's
                // title row folds it.
                super::board_ui::click_board_card(&mut self.board_view, &id);
                match self.board.iter_mut().find(|c| c.id == id && c.fresh) {
                    Some(c) => {
                        c.fresh = false;
                        true
                    }
                    None => false,
                }
            }
            BoardAct::Say { id, text } => {
                self.say_on_card(&id, &text);
                false
            }
            BoardAct::Stop => {
                self.halt_work("Stopped");
                false
            }
            BoardAct::Open(id) => {
                let thread = self
                    .board
                    .iter()
                    .find(|c| c.id == id)
                    .and_then(|c| c.thread_id.clone());
                if let Some(thread) = thread {
                    self.open_board_thread(&thread);
                }
                false
            }
            BoardAct::Edit(id) => {
                if let Some(c) = self.board.iter().find(|c| c.id == id) {
                    self.board_title = c.title.clone();
                    self.board_notes = c.detail.clone();
                    self.board_link = false;
                    self.board_edit = Some(c.id.clone());
                    self.board_compose = true;
                }
                false
            }
        }
    }

    pub(super) fn open_board_thread(&mut self, thread_id: &str) {
        let Some(idx) = self.threads.iter().position(|t| t.id == thread_id) else {
            self.status = "Linked chat is gone".into();
            return;
        };
        self.switch_thread(idx);
        self.nav = Nav::Chat;
    }

    pub(super) fn ui_skills(&mut self, ui: &mut egui::Ui) {
        if self.cfg.native_engine {
            self.ensure_native_listing();
        }
        if !self.grok_catalog_loaded && self.grok_catalog_rx.is_none() {
            self.reload_grok_catalog();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::theme::bg()).inner_margin(egui::Margin::same(24)))
            .show(ui, |ui| {
            if crate::cards::page_header(ui, "Skills and Connectors", "Refresh") {
                if self.cfg.native_engine {
                    self.native_listing_cwd.clear();
                    self.ensure_native_listing();
                }
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
            let mut workflow_verb: Option<WorkflowVerb> = None;
            let mut use_cabin_skill: Option<(String, String)> = None;
            let mut run_gh: Option<String> = None;
            let mut save_pat = false;
            let mut mcp_toggle: Option<(String, bool)> = None;
            let mut mcp_remove: Option<String> = None;
            let mut plugin_toggle: Option<(String, bool)> = None;
            let mut plugin_install: Option<String> = None;
            let mut plugin_uninstall: Option<String> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
            if self.skills_tab_connectors {
                if !self.connector_note.is_empty() {
                    ui.label(
                        RichText::new(&self.connector_note)
                            .size(12.0)
                            .color(crate::theme::muted()),
                    );
                    ui.add_space(12.0);
                }
                self.ui_connector_home_note(ui);
                ui.add_space(12.0);
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
                crate::cards::tile_row(ui, crate::cards::GITHUB_TILES.len(), |ui, i| {
                    let (title, body, tool) = crate::cards::GITHUB_TILES[i];
                    if matches!(
                        crate::cards::grok_tile(
                            ui,
                            crate::icons::TileIcon::Github,
                            title,
                            body,
                            Some("Run"),
                            false,
                        ),
                        crate::cards::TileHit::Add | crate::cards::TileHit::Body
                    ) {
                        run_gh = Some((*tool).to_string());
                    }
                });
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    crate::cards::section_label(ui, "MCP servers");
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
                crate::cards::help_text(ui, "Grok Build `grok mcp` — add, enable, disable, or remove servers.");
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
                let mcp: Vec<_> = self
                    .grok_catalog
                    .mcp
                    .iter()
                    .filter(|s| {
                        q.is_empty()
                            || s.name.to_ascii_lowercase().contains(&q)
                            || s.target.to_ascii_lowercase().contains(&q)
                    })
                    .map(|s| {
                        let status = self.mcp_status.get(&s.name).cloned();
                        (s.clone(), status)
                    })
                    .collect();
                if mcp.is_empty() {
                    ui.label(
                        RichText::new(catalog_empty_line(
                            self.grok_catalog_rx.is_some(),
                            &q,
                            self.grok_catalog.mcp.len(),
                            "No MCP servers in ~/.grok — add one with grok mcp add.",
                            &self.status,
                        ))
                        .color(crate::theme::muted()),
                    );
                } else {
                    paint_catalog_stale(ui, &self.status, self.grok_catalog.mcp.len());
                    crate::cards::tile_row(ui, mcp.len(), |ui, i| {
                        let (s, status) = &mcp[i];
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
                        if let Some(status) = status {
                            ui.add_space(4.0);
                            self.ui_mcp_row_status(ui, &s.name, status);
                        }
                    });
                }
                ui.add_space(20.0);
                self.ui_hooks_section(ui, &q);
                ui.add_space(20.0);
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
                    let installed_total = self
                        .grok_catalog
                        .plugins
                        .iter()
                        .filter(|p| p.status != "available")
                        .count();
                    ui.label(
                        RichText::new(catalog_empty_line(
                            self.grok_catalog_rx.is_some(),
                            &q,
                            installed_total,
                            "No plugins installed yet — browse Marketplace below.",
                            &self.status,
                        ))
                        .color(crate::theme::muted()),
                    );
                } else {
                    paint_catalog_stale(ui, &self.status, installed.len());
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
                crate::cards::help_text(ui, "xAI Official and other sources (`grok plugin marketplace`).");
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
                    let market_total = self
                        .grok_catalog
                        .plugins
                        .iter()
                        .filter(|p| p.status == "available")
                        .count();
                    ui.label(
                        RichText::new(catalog_empty_line(
                            self.grok_catalog_rx.is_some(),
                            &q,
                            market_total,
                            "No marketplace plugins to install.",
                            &self.status,
                        ))
                        .color(crate::theme::muted()),
                    );
                } else {
                    paint_catalog_stale(ui, &self.status, market.len());
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
            let workflows_section = ui
                .vertical(|ui| {
                    crate::cards::section_label(ui, "Workflows");
                    crate::cards::help_text(ui, "Grok Build `/workflow` skills and `*.rhai` under ~/.grok/workflows.");
                    ui.add_space(8.0);
                    if !workflows.is_empty() {
                        crate::cards::tile_row(ui, workflows.len(), |ui, i| {
                            let w = &workflows[i];
                            match crate::cards::grok_tile(
                                ui,
                                crate::icons::TileIcon::Bolt,
                                &w.name,
                                &format!("{} · {}", w.source, w.description),
                                Some("Use in chat"),
                                false,
                            ) {
                                crate::cards::TileHit::Add => use_skill = Some(w.name.clone()),
                                crate::cards::TileHit::Body => {
                                    self.workflow_target = w.name.clone();
                                }
                                crate::cards::TileHit::None => {}
                            }
                        });
                        ui.add_space(8.0);
                    }
                    ui.label(
                        RichText::new("Runs")
                            .size(13.0)
                            .strong()
                            .color(crate::theme::subtle()),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let field_w = (ui.available_width() - 248.0).clamp(180.0, 420.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.workflow_target)
                                .hint_text(crate::theme::hint("Workflow name or run id"))
                                .desired_width(field_w),
                        );
                        let ready = !self.workflow_target.trim().is_empty();
                        ui.add_enabled_ui(ready, |ui| {
                            if crate::cards::ghost_pill(ui, "Pause") {
                                workflow_verb = Some(WorkflowVerb::Pause);
                            }
                            if crate::cards::ghost_pill(ui, "Resume") {
                                workflow_verb = Some(WorkflowVerb::Resume);
                            }
                            if crate::cards::ghost_pill(ui, "Stop") {
                                workflow_verb = Some(WorkflowVerb::Stop);
                            }
                        });
                    });
                    if let Some(verb) = workflow_verb.take() {
                        let target = self.workflow_target.trim().to_string();
                        self.send_workflow_ctl(verb, &target);
                    }
                    ui.add_space(6.0);
                    if self.workflow_status_live && !self.status.is_empty() {
                        ui.label(
                            RichText::new(self.status.as_str())
                                .size(13.0)
                                .color(crate::theme::fg()),
                        );
                        ui.add_space(6.0);
                    }
                    ui.label(
                        RichText::new(
                            "Grok Build doesn't list live runs to the cabin yet — type a workflow name or run id.",
                        )
                        .size(12.0)
                        .color(crate::theme::muted()),
                    );
                    ui.add_space(16.0);
                })
                .response;
            if self.scroll_to_workflows {
                workflows_section.scroll_to_me(Some(egui::Align::TOP));
                self.scroll_to_workflows = false;
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
                    .size(crate::theme::FONT_BODY)
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
            if self.cfg.native_engine {
                crate::cards::help_text(
                    ui,
                    "Skills discovered for the native engine. Use in chat sends /name.",
                );
                ui.add_space(8.0);
                let skills: Vec<_> = self
                    .native_skills
                    .iter()
                    .filter(|s| {
                        q.is_empty()
                            || s.name.to_ascii_lowercase().contains(&q)
                            || s.description.to_ascii_lowercase().contains(&q)
                            || s.path.display().to_string().to_ascii_lowercase().contains(&q)
                    })
                    .cloned()
                    .collect();
                if skills.is_empty() {
                    ui.label(
                        RichText::new(if q.is_empty() {
                            "None found in this project or the user skills directory."
                        } else {
                            "None matched."
                        })
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::muted()),
                    );
                } else {
                    crate::cards::tile_row(ui, skills.len(), |ui, i| {
                        let s = &skills[i];
                        let src = format!("{} · {}", s.source.as_str(), s.path.display());
                        let body = if s.description.is_empty() {
                            src
                        } else {
                            format!("{src} · {}", s.description)
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
                            use_skill = Some(s.name.clone());
                        }
                    });
                }
            } else {
            crate::cards::help_text(ui, "Bundled skills and plugin skills from `grok inspect`. Use in chat sends /name.");
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
                    RichText::new(catalog_empty_line(
                        self.grok_catalog_rx.is_some(),
                        &q,
                        self.grok_catalog.skills.len(),
                        "None found. Refresh after installing a plugin.",
                        &self.status,
                    ))
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::muted()),
                );
            } else {
                paint_catalog_stale(ui, &self.status, self.grok_catalog.skills.len());
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
            // Capture the config directory now, like the other scheduled saves:
            // looking it up when the thread runs can land in another directory.
            let dir = config::config_dir();
            std::thread::spawn(move || {
                let _pin = pin_scheduled_dir(dir);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_empty_line_distinguishes_loading_search_and_empty() {
        let mcp = "No MCP servers in ~/.grok — add one with grok mcp add.";
        let plugins = "No plugins installed yet — browse Marketplace below.";
        let market = "No marketplace plugins to install.";
        let skills = "None found. Refresh after installing a plugin.";
        assert_eq!(catalog_empty_line(true, "", 0, mcp, ""), "Loading…");
        assert_eq!(catalog_empty_line(true, "grok", 4, plugins, ""), "Loading…");
        assert_eq!(catalog_empty_line(false, "zzz", 3, market, ""), "None matched.");
        assert_eq!(catalog_empty_line(false, "zzz", 0, skills, ""), skills);
        assert_eq!(catalog_empty_line(false, "", 0, mcp, ""), mcp);
        assert_eq!(catalog_empty_line(false, "", 2, plugins, ""), plugins);
        assert_eq!(catalog_empty_line(false, "  ", 2, skills, ""), "None matched.");
    }

    #[test]
    fn catalog_timeout_copy_names_a_missed_answer_and_a_stale_list() {
        let skills = "None found. Refresh after installing a plugin.";
        let timeout = crate::app::acp::GROK_CATALOG_TIMEOUT;
        assert_eq!(
            CATALOG_TIMEOUT_EMPTY,
            "Grok Build didn't answer. Refresh to try again."
        );
        assert_eq!(
            CATALOG_TIMEOUT_STALE,
            "Showing the last list — Grok Build timed out."
        );
        assert_eq!(
            catalog_empty_line(false, "", 0, skills, timeout),
            "Grok Build didn't answer. Refresh to try again."
        );
        assert_eq!(
            catalog_empty_line(false, "zzz", 0, skills, timeout),
            "Grok Build didn't answer. Refresh to try again."
        );
        assert_eq!(catalog_empty_line(true, "", 0, skills, timeout), "Loading…");
        assert_eq!(catalog_empty_line(false, "", 0, skills, "Harbor"), skills);
        assert_eq!(catalog_empty_line(false, "", 2, skills, timeout), skills);
        assert_eq!(
            catalog_stale_line(timeout, 3),
            Some("Showing the last list — Grok Build timed out.")
        );
        assert_eq!(catalog_stale_line(timeout, 0), None);
        assert_eq!(catalog_stale_line("Harbor", 4), None);
        let ui = include_str!("pages.rs");
        let skills_ui = ui
            .split("fn ui_skills(")
            .nth(1)
            .and_then(|s| s.split("fn open_history_hit(").next())
            .expect("ui_skills");
        assert!(
            skills_ui.matches("paint_catalog_stale(").count() >= 4,
            "skills and connectors tile lists paint the stale line: {skills_ui}"
        );
    }
}
