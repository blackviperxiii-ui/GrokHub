//! Slash-command dispatch.

use super::*;

impl Cabin {

    pub(super) fn run_slash(&mut self, slash: Slash) {
        if let Some(cmd) = home_slash_cmd(slash_kind(&slash)) {
            remember_home_slash(&mut self.chip_memory, cmd, now_ms());
        }
        match slash {
            Slash::Forget(topic) => {
                if self.scratch() {
                    self.status = "Scratch — no memory writes".into();
                    return;
                }
                match topic {
                    None => {
                        std::thread::spawn(|| {
                            let _ = config::write_memory("MEMORY.md", "");
                        });
                        if self.mem_name == "MEMORY.md" {
                            self.mem_body.clear();
                        }
                        self.status = "Forgot MEMORY.md".into();
                    }
                    Some(q) => {
                        let name = self.mem_name.clone();
                        let body = self.mem_body.clone();
                        std::thread::spawn(move || {
                            if name != "MEMORY.md" && config::read_memory(&name) != body {
                                let _ = config::write_memory(&name, &body);
                            }
                        });
                        if self.mem_name == "MEMORY.md" {
                            let next = forget_topic(&self.mem_body, &q);
                            let written = next.clone();
                            std::thread::spawn(move || {
                                let _ = config::write_memory("MEMORY.md", &written);
                            });
                            self.mem_body = next;
                            self.status = format!("Forgot {q}");
                        } else {
                            let topic = q.clone();
                            std::thread::spawn(move || {
                                let current = config::read_memory("MEMORY.md");
                                let next = forget_topic(&current, &topic);
                                let _ = config::write_memory("MEMORY.md", &next);
                            });
                            self.status = format!("Forgot {q}");
                        }
                    }
                }
            }
            Slash::MemoryShow => {
                self.nav = Nav::Memory;
                self.status = "Memory".into();
            }
            Slash::MemoryNote(note) => {
                if self.scratch() {
                    self.status = "Scratch — no memory writes".into();
                    return;
                }
                if !is_plain_text(&note) {
                    self.status = "Secrets never in markdown".into();
                    return;
                }
                let name = self.mem_name.clone();
                let body = self.mem_body.clone();
                if name != "MEMORY.md" {
                    std::thread::spawn(move || {
                        if config::read_memory(&name) != body {
                            let _ = config::write_memory(&name, &body);
                        }
                    });
                }
                if self.mem_name == "MEMORY.md" {
                    let mut next = self.mem_body.clone();
                    if !next.is_empty() && !next.ends_with('\n') {
                        next.push('\n');
                    }
                    next.push_str(note.trim());
                    next.push('\n');
                    let written = next.clone();
                    std::thread::spawn(move || {
                        let _ = config::write_memory("MEMORY.md", &written);
                    });
                    self.mem_body = next;
                    self.status = "Wrote MEMORY.md".into();
                } else {
                    let note = note.clone();
                    std::thread::spawn(move || {
                        let _ = config::append_memory("MEMORY.md", &note);
                    });
                    self.status = "Wrote MEMORY.md".into();
                }
            }
            Slash::Board => {
                self.nav = Nav::Workboard;
                self.status = format!("{} cards", self.board.len());
            }
            Slash::ImagineVideo(p) => {
                self.nav = Nav::Imagine;
                self.imagine_kind = grokhub_core::ImagineKind::Video;
                if !p.trim().is_empty() {
                    self.imagine_prompt = p;
                    self.kick_imagine();
                } else {
                    self.imagine_want_focus = true;
                }
            }
            Slash::Loop(seed) => {
                self.nav = Nav::Night;
                if seed.trim().is_empty() {
                    self.auto_compose = true;
                } else {
                    self.add_automation_seed(&seed);
                }
            }
            Slash::GrokSkills => {
                self.nav = Nav::Skills;
                self.skills_tab_connectors = false;
                self.reload_grok_catalog();
            }
            Slash::GrokConnectors => {
                self.nav = Nav::Connectors;
                self.skills_tab_connectors = true;
                self.reload_grok_catalog();
            }
            Slash::Model(name) => {
                let name = name.trim();
                let (id, effort) = name
                    .split_once(char::is_whitespace)
                    .map(|(a, b)| (a.trim(), b.trim()))
                    .unwrap_or((name, ""));
                if !id.is_empty() {
                    self.cfg.model = id.to_string();
                    self.persist_cfg();
                }
                if !effort.is_empty() {
                    self.run_slash(Slash::Effort(effort.to_string()));
                }
                self.status = format!("grok --model {}", self.cfg.model);
            }
            Slash::Goal(obj) => {
                let obj = obj.trim();
                if obj.is_empty() || obj.eq_ignore_ascii_case("status") {
                    self.status = if self.cfg.goal_pin.is_empty() {
                        "No goal pin".into()
                    } else {
                        format!("Goal: {}", self.cfg.goal_pin)
                    };
                } else if obj.eq_ignore_ascii_case("clear") {
                    self.cfg.goal_pin.clear();
                    self.persist_cfg();
                    self.status = "Goal cleared".into();
                } else {
                    self.cfg.goal_pin = obj.to_string();
                    self.persist_cfg();
                    self.status = format!("Goal: {}", self.cfg.goal_pin);
                }
            }
            Slash::Imagine(p) => {
                self.nav = Nav::Imagine;
                self.imagine_want_focus = true;
                if !p.trim().is_empty() {
                    self.imagine_prompt = p;
                    self.kick_imagine();
                }
            }
            Slash::Fork => {
                let sid = self
                    .threads
                    .get(self.thread_idx)
                    .and_then(|t| t.grok_session.clone());
                let cwd = self
                    .threads
                    .get(self.thread_idx)
                    .and_then(|t| t.grok_cwd.clone());
                let user_home = self
                    .threads
                    .get(self.thread_idx)
                    .map(|t| t.grok_user_home)
                    .unwrap_or(false);
                self.new_thread(false);
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = sid;
                    t.grok_cwd = cwd;
                    t.grok_user_home = user_home;
                    t.grok_fork = true;
                    t.title = "Fork".into();
                }
                self.acp = None;
                self.status =
                    "Forked — next send starts a new Grok session from this history".into();
            }
            Slash::Workflow(name) => {
                self.send_grok_slash(&format!("/workflow {name}"));
                self.status = format!("Workflow {name}");
            }
            Slash::Worktree => {
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_worktree = !t.grok_worktree;
                    self.status = if t.grok_worktree {
                        "Next chat uses --worktree".into()
                    } else {
                        "Worktree off".into()
                    };
                }
            }
            Slash::RewindFiles => self.rewind_project(),
            Slash::Compact => {
                let pin = self.cfg.goal_pin.trim().to_string();
                let start = compact_keep_start_from(
                    self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                    8,
                );
                if start > 0 {
                    self.live_mut().drain(..start);
                }
                if !pin.is_empty() {
                    let marked = format!("GOAL PIN: {pin}");
                    if !self
                        .messages
                        .iter()
                        .any(|m| m.1 == marked || m.1.starts_with(&format!("{marked}\n")))
                    {
                        self.live_mut().insert(0, ("system".into(), marked));
                    }
                }
                self.stamp_current_access();
                self.persist();
                self.send_grok_slash("/compact");
                self.status = "Compacting Grok context…".into();
            }
            Slash::Skill(name) => {
                if let Some(s) = self
                    .skill_list
                    .iter()
                    .find(|s| s.name == name || s.slash == name)
                {
                    self.nav = Nav::Chat;
                    self.send_chat(skill_use_in_chat_prompt(&s.slash, &s.name));
                } else {
                    self.status = format!("No skill {name}");
                }
            }
            Slash::LearnReflect => self.run_reflect(),
            Slash::Update => self.queue_update(),
            Slash::Help => {
                self.live_mut()
                    .push(("assistant".into(), mark_slash_result(&slash_help())));
                self.stamp_current_access();
                self.persist();
            }
            Slash::New => {
                self.new_thread(false);
            }
            Slash::Scratch => self.new_thread(true),
            Slash::Clear => {
                if self.running {
                    self.halt_in_flight();
                    self.finish_hub_dispatch("Cleared in-flight reply", false);
                }
                self.drop_leaving_thread_chrome();
                self.messages = Arc::new(Vec::new());
                self.followup_step = 0;
                self.active_skill_follow = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                    t.grok_cwd = None;
                    t.messages = self.messages.clone();
                }
                self.stamp_current_access();
                self.persist();
                self.status = "Cleared".into();
            }
            Slash::Undo => {
                if self.running {
                    self.halt_in_flight();
                    self.finish_hub_dispatch("Undid in-flight reply", false);
                    self.status = "Undid in-flight reply".into();
                } else if let Some(i) = self.messages.iter().rposition(|m| m.0 == "assistant") {
                    self.live_mut().remove(i);
                    self.followup_step = 0;
                    self.active_skill_follow = None;
                    self.stamp_current_access();
                    self.persist();
                    self.send_grok_slash("/rewind");
                    self.status = "Rewinding Grok conversation…".into();
                } else {
                    self.status = "Nothing to undo".into();
                }
            }
            Slash::Retry => {
                if let Some(m) = self
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.0 == "user" && !is_workload_user(&m.1))
                {
                    let t = m.1.clone();
                    self.kick_model_retry(t);
                } else {
                    self.status = "Nothing to retry".into();
                }
            }
            Slash::Stop => self.halt_work("Stopped"),
            Slash::Sh(cmd) => self.queue_sh(cmd),
            Slash::HostStatus => {
                self.status = build_agent::grok_banner();
            }
            Slash::Rename(title) => self.rename_thread(self.thread_idx, &title),
            Slash::Pin => self.pin_thread(self.thread_idx),
            Slash::Delete => self.delete_thread_at(self.thread_idx),
            Slash::Context => {
                let n = visible_turn_count_from(
                    self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                );
                if !self.grok_usage.is_empty() {
                    let grok = grok_context_line(&self.grok_usage);
                    self.status = format!(
                        "{n} turns · {grok} · pin {}",
                        if self.cfg.goal_pin.is_empty() {
                            "none"
                        } else {
                            &self.cfg.goal_pin
                        }
                    );
                } else {
                    let tokens = estimate_messages_from(
                        self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                    );
                    self.status = format!(
                        "{n} turns · {} tokens · {}% · pin {}",
                        tokens,
                        context_percent(tokens, CONTEXT_BUDGET_TOKENS),
                        if self.cfg.goal_pin.is_empty() {
                            "none"
                        } else {
                            &self.cfg.goal_pin
                        }
                    );
                }
            }
            Slash::Health => {
                self.nav = Nav::Settings;
                self.settings_sec = health_settings_sec();
                self.status = self.doctor_text();
            }
            Slash::Fix => {
                self.halt_work("Stopped");
                self.nav = Nav::Settings;
                self.settings_sec = health_settings_sec();
                self.status = self.doctor_text();
            }
            Slash::Remember(note) => self.run_slash(Slash::MemoryNote(note)),
            Slash::Mode(mode) => {
                self.cfg.mode = mode.clone();
                self.persist_cfg();
                self.status = mode_status_line(&mode, &self.cfg.model);
            }
            Slash::Dream => self.run_dream(),
            Slash::Import => self.import_openclaw(),
            Slash::Consult(q) => self.run_consult(q),
            Slash::Usage => {
                let cabin = usage_line(&self.usage);
                let grok = grok_usage_line(&self.grok_usage);
                self.status = if grok.is_empty() {
                    cabin
                } else {
                    format!("{cabin} · {grok}")
                };
                let sid = self
                    .threads
                    .get(self.thread_idx)
                    .and_then(|t| t.grok_session.clone())
                    .filter(|s| !s.trim().is_empty());
                if let (Some(bin), Some(id)) = (grokhub_acp::find_grok(), sid) {
                    if self.inspect_rx.is_none() {
                        let cwd = self.grok_cli_cwd();
                        let (tx, rx) = mpsc::channel();
                        self.inspect_rx = Some(rx);
                        self.status = format!("{} · grok usage…", self.status);
                        std::thread::spawn(move || {
                            let text =
                                grokhub_acp::session_usage(&bin, &cwd, &id).unwrap_or_else(|e| e);
                            let _ = tx.send(text);
                        });
                    }
                }
            }
            Slash::Models => {
                if let Some(bin) = grokhub_acp::find_grok() {
                    let cwd = self.grok_cwd();
                    if self.inspect_rx.is_none() {
                        let (tx, rx) = mpsc::channel();
                        self.inspect_rx = Some(rx);
                        self.status = "grok models".into();
                        std::thread::spawn(move || {
                            let text =
                                grokhub_acp::grok_user_stdout_timeout(&bin, &cwd, &["models"], 20)
                                    .unwrap_or_else(|e| e);
                            let ids = grokhub_acp::parse_models_list(&text);
                            let body = if ids.is_empty() {
                                text
                            } else {
                                format!("{}\n\n{}", ids.join("\n"), text)
                            };
                            let _ = tx.send(body);
                        });
                    }
                } else {
                    self.live_mut()
                        .push(("assistant".into(), mark_slash_result(&catalog_line())));
                    self.stamp_current_access();
                    self.persist();
                }
            }
            Slash::Palette => self.open_palette(),
            Slash::Plan => {
                if self.running {
                    self.halt_in_flight();
                }
                self.set_session_mode(SessionMode::Plan);
                self.acp = None;
                self.acp_spawn_rx = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                }
                self.persist_idle_key = self.persist_idle_now();
                self.status = "Plan mode — Grok Build will plan first".into();
            }
            Slash::AlwaysApprove => {
                let next = if self.permission_mode == PermissionMode::AlwaysApprove {
                    PermissionMode::Ask
                } else {
                    PermissionMode::AlwaysApprove
                };
                self.set_permission_mode(next);
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                }
                self.persist_idle_key = self.persist_idle_now();
                self.status = format!("Permission {}", self.permission_mode.as_str());
            }
            Slash::AutoPerm => {
                self.set_permission_mode(PermissionMode::Auto);
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                }
                self.persist_idle_key = self.persist_idle_now();
                self.status = "Permission auto".into();
            }
            Slash::Effort(level) => {
                if let Some(effort) = grokhub_core::parse_reasoning_effort(&level) {
                    if self.running {
                        self.halt_in_flight();
                    }
                    self.cfg.reasoning_effort = effort.to_string();
                    self.acp = None;
                    self.acp_spawn_rx = None;
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_session = None;
                    }
                    self.persist_cfg();
                    self.persist_idle_key = self.persist_idle_now();
                    self.status = format!("Effort {}", grokhub_core::effort_label(effort));
                } else {
                    self.status = "Effort: none | minimal | low | medium | high | xhigh".into();
                }
            }
            Slash::Sessions => {
                self.nav = Nav::History;
                self.grok_sessions_loaded = false;
                self.reload_grok_sessions();
                self.status = if grokhub_acp::find_grok().is_some() {
                    "Listing Grok sessions…".into()
                } else {
                    build_agent::grok_banner()
                };
            }
            Slash::Inspect => {
                self.nav = Nav::Connectors;
                if let Some(bin) = grokhub_acp::find_grok() {
                    let cwd = self.grok_cwd();
                    if self.inspect_rx.is_none() {
                        let (tx, rx) = mpsc::channel();
                        self.inspect_rx = Some(rx);
                        self.inspect_text = "Inspecting…".into();
                        self.status = "Grok inspect".into();
                        std::thread::spawn(move || {
                            let text = match grokhub_acp::inspect_json(&bin, &cwd) {
                                Ok(v) => {
                                    let note = inspect_advisory(&v);
                                    let pretty = serde_json::to_string_pretty(&v)
                                        .unwrap_or_else(|_| v.to_string());
                                    if note.is_empty() {
                                        pretty
                                    } else {
                                        format!("{note}\n\n{pretty}")
                                    }
                                }
                                Err(e) => e,
                            };
                            let _ = tx.send(text);
                        });
                    }
                } else {
                    self.inspect_text = build_agent::grok_banner();
                    self.status = self.inspect_text.clone();
                }
            }
            Slash::ProjectBind(path) => {
                let raw = path.unwrap_or_else(|| self.cfg.project_dir.clone());
                let home = std::env::var("HOME").ok();
                let p = resolve_bind_path(
                    &raw,
                    &self.cfg.project_dir,
                    &self.work_root(),
                    home.as_deref(),
                );
                let tree_changed = self.cfg.project_dir != p;
                self.cfg.project_dir = p.clone();
                let dir = p.clone();
                std::thread::spawn(move || {
                    let _ = std::fs::create_dir_all(&dir);
                });
                self.project_sel = upsert_bound(&mut self.projects, &p);
                self.touch_projects();
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if tree_changed {
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_cwd = None;
                        t.grok_session = None;
                    }
                    self.persist();
                } else {
                    self.flush_projects();
                    self.persist_cfg();
                }
                self.grok_sessions_loaded = false;
                self.status = format!("Bound {p}");
            }
            Slash::ProjectClear => {
                self.cfg.project_dir.clear();
                self.project_sel = None;
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_cwd = None;
                    t.grok_session = None;
                }
                self.grok_sessions_loaded = false;
                self.touch_projects();
                self.persist();
                self.status = "Unbound — full desktop".into();
            }
            Slash::ProjectShow => {
                self.status = if self.cfg.project_dir.trim().is_empty() {
                    "No bound project".into()
                } else {
                    format!("Project {}", self.cfg.project_dir)
                };
            }
            Slash::ProjectNew(name) => self.make_project(&name, None),
            Slash::ProjectFolder(name) => self.make_folder(&name),
            Slash::ProjectRename(name) => {
                let Some(id) = self.project_sel.clone() else {
                    self.status = "Select a project first".into();
                    return;
                };
                match rename_node(&mut self.projects, &id, &name) {
                    Ok(()) => {
                        self.status = format!("Renamed {name}");
                        self.touch_projects();
                        self.flush_projects();
                    }
                    Err(e) => self.status = e.into(),
                }
            }
            Slash::ProjectMove(folder) => self.move_sel_to_folder_name(&folder),
            Slash::ProjectDelete => {
                let Some(id) = self.project_sel.clone() else {
                    self.status = "Select a project first".into();
                    return;
                };
                self.remove_project_id(&id);
            }
            Slash::Send(task) => self.dispatch_send(task),
            Slash::Sync => self.sync_hub(),
            Slash::Hub => {
                self.nav = Nav::Devices;
                self.status = if self.hub_on {
                    "Hub sharing".into()
                } else {
                    "Start share on Devices".into()
                };
            }
            Slash::Inhabit(peer) => self.queue_inhabit(peer),
            Slash::Rewind => {
                if let Some(i) = self.messages.iter().rposition(|m| m.0 == "assistant") {
                    self.live_mut().remove(i);
                }
                self.send_grok_slash("/rewind");
                self.status = "Rewinding Grok conversation…".into();
            }
            Slash::Room(name) => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                let plan = plan_room(&name, &home);
                let p = format!("{home}/{}", plan.project_rel);
                let dir = p.clone();
                std::thread::spawn(move || {
                    let _ = std::fs::create_dir_all(&dir);
                });
                let tree_changed = self.cfg.project_dir != p;
                self.cfg.project_dir = p.clone();
                self.project_sel = upsert_bound(&mut self.projects, &p);
                self.touch_projects();
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if tree_changed {
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_cwd = None;
                        t.grok_session = None;
                    }
                    self.persist();
                } else {
                    self.flush_projects();
                    self.persist_cfg();
                }
                self.status = format!("Room {} → {p}", plan.slug);
                self.queue_sh(plan.host_script);
            }
            Slash::Export => {
                self.persist();
                if let Some(t) = self.threads.get(self.thread_idx) {
                    let t = t.clone();
                    let dest = if self.cfg.project_dir.trim().is_empty() {
                        config::config_dir().join("export.md")
                    } else {
                        std::path::PathBuf::from(expand_home(&self.cfg.project_dir))
                            .join("export.md")
                    };
                    let status_path = dest.display().to_string();
                    std::thread::spawn(move || {
                        let md = threads::export_markdown(&t);
                        let _ = std::fs::write(&dest, md);
                    });
                    self.status = format!("Wrote {status_path}");
                }
            }
            Slash::Recall(q) => {
                if self.recall_rx.is_some() {
                    self.status = "Recalling…".into();
                    return;
                }
                if !self.scratch() {
                    let name = self.mem_name.clone();
                    let body = self.mem_body.clone();
                    std::thread::spawn(move || {
                        if config::read_memory(&name) != body {
                            let _ = config::write_memory(&name, &body);
                        }
                    });
                }
                let q_owned = q.clone();
                let mem_name = self.mem_name.clone();
                let mem_body = self.mem_body.clone();
                // What the cabin learned by itself lives in learning.json, not the
                // markdown, and /recall used to miss all of it.
                let insights = self
                    .learning
                    .insights
                    .iter()
                    .map(|i| i.text.clone())
                    .collect::<Vec<_>>()
                    .join("\n");
                let vis = self.thread_idx;
                let mut thread_rows = Vec::new();
                for (i, t) in self.threads.iter().enumerate() {
                    let body = if i == vis {
                        search_thread_body(self.messages.iter().map(|m| m.1.as_str()))
                    } else {
                        search_thread_body(t.messages.iter().map(|(_, c)| c.as_str()))
                    };
                    thread_rows.push((t.title.clone(), body));
                }
                let (tx, rx) = mpsc::channel();
                self.recall_rx = Some(rx);
                self.status = "Recalling…".into();
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
                    let corpus = [
                        ("SOUL.md", soul),
                        ("USER.md", user),
                        ("MEMORY.md", memory),
                        ("learned", insights),
                    ];
                    let refs: Vec<(&str, &str)> =
                        corpus.iter().map(|(n, b)| (*n, b.as_str())).collect();
                    let mut hits = recall_hits(&q_owned, &refs);
                    let mut rows: Vec<(String, String)> = corpus
                        .iter()
                        .map(|(n, b)| ((*n).to_string(), b.clone()))
                        .collect();
                    rows.extend(thread_rows);
                    hits.extend(search_corpus(&q_owned, &rows));
                    let hits = dedupe_hits(hits);
                    let body = if hits.is_empty() {
                        format!("No recall for {q_owned}")
                    } else {
                        hits.join("\n")
                    };
                    let _ = tx.send(body);
                });
            }
        }
    }

    pub(super) fn run_slash_line(&mut self, line: &str) {
        if let Some(s) = parse_slash(line) {
            self.run_slash(s);
        }
    }
}
