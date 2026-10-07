//! Slash-command dispatch.

use super::*;

/// Memory lines for `/recall` when `memory_backend` is `amr`. The cabin
/// opens the store (and runs the one-time import) before this thread starts.
/// Private notes (`nodes/<id>.sealed`) open with the learned-tier key; when
/// they can't, one line says so instead (Spike-4b, fail closed).
fn recall_amr_lines(store: &grokhub_core::amr::AmrStore, query: &str) -> Vec<String> {
    let report = store.recall_report(query);
    let mut lines: Vec<String> = report.hits.iter().map(|hit| hit.display()).collect();
    if let Some(line) = locked_recall_line(report.locked, report.why.as_deref()) {
        lines.push(line);
    }
    lines
}

/// The `/recall` line for private notes that stayed shut.
pub(super) fn locked_recall_line(locked: usize, why: Option<&str>) -> Option<String> {
    if locked == 0 {
        return None;
    }
    let notes = if locked == 1 { "1 private note".to_string() } else { format!("{locked} private notes") };
    Some(format!("{notes} not searched. {}", why.unwrap_or("Private memory is locked.")))
}

impl Cabin {

    pub(super) fn run_slash(&mut self, slash: Slash) {
        if let Some(cmd) = home_slash_cmd(slash_kind(&slash)) {
            remember_home_slash(&mut self.chip_memory, cmd, now_ms());
        }
        if self.dispatch_native_slash(&slash) {
            return;
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
                        // AMR: tombstone matching nodes; MEMORY.md still drops the topic below.
                        let amr_line = if self.amr_on() { Some(self.forget_amr(&q)) } else { None };
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
                        if let Some(line) = amr_line {
                            self.status = line;
                        }
                    }
                }
            }
            Slash::MemoryShow => {
                self.nav = Nav::Memory;
                self.status = "Memory".into();
            }
            Slash::MemoryDream => {
                let text = amr_memory::memory_dream_text(&config::config_dir());
                self.live_mut().push(("assistant".into(), mark_slash_result(&text)));
                self.stamp_current_access();
                self.persist();
                self.status = "Memory dream".into();
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
                if self.amr_on() {
                    // Single write: the note is a node, not a MEMORY.md line.
                    self.status = self.amr_remember_now(&note, "note");
                    return;
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
            Slash::SkillChanges => self.run_skill_changes(),
            Slash::SkillUndo(name) => self.run_skill_undo(&name),
            Slash::SkillRestore(name) => self.run_skill_restore(&name),
            Slash::GrokWorkflows => {
                self.nav = Nav::Skills;
                self.skills_tab_connectors = false;
                self.scroll_to_workflows = true;
                self.reload_grok_catalog();
            }
            Slash::GrokConnectors => {
                self.nav = Nav::Connectors;
                self.skills_tab_connectors = true;
                self.reload_grok_catalog();
            }
            Slash::GrokHooks => {
                self.nav = Nav::Connectors;
                self.skills_tab_connectors = true;
                self.scroll_to_hooks = true;
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
            Slash::WorkflowCtl { verb, target } => {
                self.send_workflow_ctl(verb, &target);
            }
            Slash::WorkflowUsage => {
                self.workflow_status_live = true;
                self.status = "Usage: /workflow <verb> <name-or-run-id>".into();
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
            Slash::Btw => {
                self.set_session_mode(SessionMode::Ask);
                self.status = if self.running {
                    "btw — side ask, main run continues".into()
                } else {
                    "btw — look-safe side ask".into()
                };
            }
            Slash::ViewPlan => {
                let has = self
                    .threads
                    .get(self.thread_idx)
                    .is_some_and(|t| !t.plan_body.trim().is_empty());
                if has {
                    self.plan_open = true;
                    self.status = "View plan".into();
                } else {
                    self.status = "No plan yet — use Plan mode".into();
                }
            }
            Slash::RewindFiles => self.rewind_project(),
            Slash::Compact => {
                if self.native_compact_if_current() {
                    self.stamp_current_access();
                    self.persist();
                    return;
                }
                self.send_grok_slash("/compact");
                if !self.running {
                    return;
                }
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
            Slash::Background(arg) => self.run_bg_slash(&arg),
            Slash::Queue(text) => self.queue_or_send(text),
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
                let mut cabin = usage_line(&self.usage);
                if self.cfg.daily_token_budget > 0 {
                    cabin = format!(
                        "{cabin} · budget {}",
                        grokhub_core::budget_line(&self.usage, self.cfg.daily_token_budget)
                    );
                }
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
                self.hold_chat_name_for_plan();
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                }
                self.persist_idle_key = self.persist_idle_now();
                self.status = "Plan mode — Grok Build will plan first".into();
            }
            Slash::AlwaysApprove => {
                if self.permission_mode == PermissionMode::AlwaysApprove {
                    self.confirm = None;
                    self.set_permission_mode(PermissionMode::Ask);
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
                } else {
                    self.arm_session_always();
                }
            }
            Slash::AutoPerm => {
                self.confirm = None;
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
                    self.status = "Effort: none | low | medium | high | xhigh".into();
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
            Slash::Privacy => self.run_privacy(),
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
                self.send_grok_slash("/rewind");
                if !self.running {
                    return;
                }
                if let Some(i) = self.messages.iter().rposition(|m| m.0 == "assistant") {
                    self.live_mut().remove(i);
                }
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
            Slash::ExportAs(fmt) => match grokhub_core::ChatExport::parse(&fmt) {
                Some(grokhub_core::ChatExport::Markdown) => self.run_slash(Slash::Export),
                Some(format) => self.export_thread_as(format),
                None => self.status = grokhub_core::EXPORT_FORMATS_HINT.into(),
            },
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
                // AMR imported the other insights; chip habits are rebuilt each night
                // and stay in learning.json, so only they are read from there.
                let amr = self.amr_on();
                let insights = self
                    .learning
                    .insights
                    .iter()
                    .filter(|i| !amr || i.key.starts_with("habit:") || i.key.starts_with("skip:"))
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
                let amr_store = if amr {
                    let scratch = self.scratch();
                    Some(self.amr_store(scratch))
                } else {
                    None
                };
                let (tx, rx) = mpsc::channel();
                self.recall_rx = Some(rx);
                self.status = "Recalling…".into();
                std::thread::spawn(move || {
                    // Legacy reads SOUL/USER/MEMORY/learned. AMR reads amr/nodes and the
                    // same files (dual-read), identical lines once. Threads on both paths.
                    let amr_hits = amr_store.as_ref().map(|store| recall_amr_lines(store, &q_owned));
                    let (mut hits, mut rows) = {
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
                        let legacy = grokhub_core::amr::LegacyMemory::new(
                            corpus
                                .iter()
                                .map(|(name, body)| ((*name).to_string(), body.clone()))
                                .collect(),
                        );
                        let hits = grokhub_core::amr::MemoryEngine::recall(&legacy, &q_owned);
                        let rows: Vec<(String, String)> = corpus
                            .into_iter()
                            .map(|(name, body)| (name.to_string(), body))
                            .collect();
                        match amr_hits {
                            Some(amr_hits) => (amr_memory::dual_read_hits(amr_hits, hits), rows),
                            None => (hits, rows),
                        }
                    };
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

    /// Forward `/workflow <verb> <target>` through `send_grok_slash`.
    /// A live turn queues it. Ask with no agent waits out the ACP handshake.
    pub(super) fn send_workflow_ctl(&mut self, verb: WorkflowVerb, target: &str) {
        let target = target.trim();
        self.workflow_status_live = true;
        if target.is_empty() {
            self.status = "Usage: /workflow <verb> <name-or-run-id>".into();
            return;
        }
        let cmd = format!("/workflow {} {}", verb.as_str(), target);
        if self.running {
            self.workflow_ctl_queue.push(cmd);
            self.status = format!(
                "Workflow {} {} — queued until the current turn ends",
                verb.as_str(),
                target
            );
            return;
        }
        if self.permission_mode.uses_acp() && self.acp.is_none() {
            if self.acp_spawn_rx.is_none() {
                if let Err(e) = self.ensure_acp() {
                    self.status = format!("Workflow {} {} — sent", verb.as_str(), target);
                    self.fail_ask_without_acp(&e);
                    return;
                }
            }
            if self.acp.is_none() {
                self.workflow_ctl_queue.push(cmd);
                self.workflow_ctl_await_acp = true;
                self.status = format!(
                    "Workflow {} {} — queued until Grok Build connects",
                    verb.as_str(),
                    target
                );
                return;
            }
        }
        self.status = format!("Workflow {} {} — sent", verb.as_str(), target);
        self.send_grok_slash(&cmd);
    }

    pub(super) fn run_slash_line(&mut self, line: &str) {
        if let Some(s) = parse_slash(line) {
            self.run_slash(s);
        }
    }
}

impl Cabin {
    /// `/export html` and `/export json` land next to `export.md`.
    pub(super) fn export_dest(&self, file: &str) -> std::path::PathBuf {
        if self.cfg.project_dir.trim().is_empty() {
            config::config_dir().join(file)
        } else {
            std::path::PathBuf::from(expand_home(&self.cfg.project_dir)).join(file)
        }
    }

    /// Flush the pane, then format and write off the UI thread.
    pub(super) fn export_thread_as(&mut self, format: grokhub_core::ChatExport) {
        self.persist();
        let Some(t) = self.threads.get(self.thread_idx).cloned() else {
            self.status = "Nothing to export".into();
            return;
        };
        let dest = self.export_dest(format.file_name());
        let status_path = dest.display().to_string();
        let footer = format!("Exported from GrokHub {}", env!("CARGO_PKG_VERSION"));
        let now = now_ms();
        std::thread::spawn(move || {
            let msgs = t.messages.iter().map(|m| (m.0.as_str(), m.1.as_str()));
            let body = match format {
                grokhub_core::ChatExport::Html => {
                    grokhub_core::chat_export_html(&t.title, msgs, &footer)
                }
                grokhub_core::ChatExport::Json => {
                    grokhub_core::chat_export_json(&t.title, &t.id, msgs, now)
                }
                grokhub_core::ChatExport::Markdown => threads::export_markdown(&t),
            };
            let _ = std::fs::write(&dest, body);
        });
        self.status = format!("Wrote {status_path}");
    }
}

impl Cabin {
    /// Native-thread handlers. Returns false so the existing match still runs
    /// when the lab flag is off or the thread is a CLI thread.
    fn dispatch_native_slash(&mut self, slash: &Slash) -> bool {
        let thread_native = self
            .threads
            .get(self.thread_idx)
            .is_some_and(|thread| thread.native);
        if !grokhub_agent::manual_compact_targets_native(self.cfg.native_engine, thread_native) {
            return false;
        }
        match slash {
            Slash::Remember(note) => {
                if self.scratch() {
                    self.status = "Scratch — no memory writes".into();
                    return true;
                }
                if self.amr_on() {
                    let note = grokhub_agent::remember_note_text(note).to_string();
                    // Like the legacy native path: secrets are redacted, not refused.
                    self.status = if note.trim().is_empty() {
                        "Nothing to remember".into()
                    } else if !is_plain_text(&note) {
                        format!("{} (secrets redacted)", self.amr_remember_now(&note, "native"))
                    } else {
                        self.amr_remember_now(&note, "native")
                    };
                    return true;
                }
                let workspace = self.native_slash_workspace();
                self.status = match grokhub_agent::remember(&workspace, note) {
                    Ok(msg) => msg,
                    Err(err) => err,
                };
                true
            }
            Slash::Dream => {
                if self.scratch() {
                    self.status = "Scratch — no memory writes".into();
                    return true;
                }
                self.prompt_native_memory("/dream", "Dreaming…");
                true
            }
            Slash::Inspect => {
                self.show_native_session_info();
                true
            }
            Slash::Fork => {
                self.fork_native_thread();
                true
            }
            Slash::Rewind => {
                // Native sessions are append-only JSONL. Dropping only the bubble would
                // leave the reply in the model's history, so this says so instead.
                self.status =
                    "N/A on native threads: the session is append-only. Use /fork to branch."
                        .into();
                true
            }
            Slash::Usage => {
                self.status = self.native_usage_status();
                true
            }
            Slash::Models => {
                self.live_mut()
                    .push(("assistant".into(), mark_slash_result(&catalog_line())));
                self.stamp_current_access();
                self.persist();
                true
            }
            Slash::Workflow(_) | Slash::WorkflowCtl { .. } => {
                self.status = "N/A: workflows stay on the Grok CLI".into();
                true
            }
            _ => false,
        }
    }

    /// CLI slashes the cabin parser does not own. Native threads only.
    pub(super) fn apply_unparsed_native_slash(&mut self, text: &str) -> bool {
        let thread_native = self
            .threads
            .get(self.thread_idx)
            .is_some_and(|thread| thread.native);
        if !grokhub_agent::manual_compact_targets_native(self.cfg.native_engine, thread_native) {
            return false;
        }
        let Some(cmd) = grokhub_agent::unparsed_native_slash(text) else {
            return false;
        };
        match cmd {
            grokhub_agent::UnparsedSlash::Flush => {
                if self.scratch() {
                    self.status = "Scratch — no memory writes".into();
                } else {
                    self.prompt_native_memory("/flush", "Flushing memory…");
                }
            }
            grokhub_agent::UnparsedSlash::ContextWindow => {
                let model = grokhub_core::cabin_spawn_model(&self.cfg.model);
                let len = grokhub_agent::context_length_for(model, &[]);
                self.status = format!("Context window {len} tokens");
            }
            grokhub_agent::UnparsedSlash::Copy => {
                let last = self
                    .messages
                    .iter()
                    .rev()
                    .find(|message| message.0 == "assistant")
                    .map(|message| one_line(&message.1, 160));
                self.status = match last {
                    Some(text) => format!("Last reply: {text}"),
                    None => "Nothing to copy".into(),
                };
            }
            grokhub_agent::UnparsedSlash::Tasks => {
                self.status = "Native tasks show on the turn while a reply is running".into();
            }
            grokhub_agent::UnparsedSlash::History => {
                self.nav = Nav::History;
                self.status = "History".into();
            }
            grokhub_agent::UnparsedSlash::Transcript => {
                let id = self
                    .threads
                    .get(self.thread_idx)
                    .and_then(|thread| thread.grok_session.clone())
                    .unwrap_or_default();
                self.status = if id.trim().is_empty() {
                    "No native transcript yet".into()
                } else {
                    match grokhub_agent::session_file(&id) {
                        Ok(path) => format!("Transcript {id} ({})", path.display()),
                        Err(_) => format!("Transcript {id}"),
                    }
                };
            }
            grokhub_agent::UnparsedSlash::Recap => {
                let mut lines: Vec<String> = self
                    .messages
                    .iter()
                    .rev()
                    .filter(|message| message.0 == "user")
                    .take(3)
                    .map(|message| one_line(&message.1, 120))
                    .collect();
                lines.reverse();
                self.status = if lines.is_empty() {
                    "Nothing to recap".into()
                } else {
                    lines.join(" · ")
                };
            }
            grokhub_agent::UnparsedSlash::Settings => {
                self.nav = Nav::Settings;
                self.status = "Settings".into();
            }
            grokhub_agent::UnparsedSlash::Memory => {
                self.nav = Nav::Memory;
                self.status = "Memory".into();
            }
            grokhub_agent::UnparsedSlash::ModelCatalog => {
                self.live_mut()
                    .push(("assistant".into(), mark_slash_result(&catalog_line())));
                self.stamp_current_access();
                self.persist();
            }
            grokhub_agent::UnparsedSlash::EffortHint => {
                self.status = "Effort: none | low | medium | high | xhigh".into();
            }
            grokhub_agent::UnparsedSlash::Note(text) => {
                self.status = text.into();
            }
        }
        true
    }

    fn native_slash_workspace(&self) -> std::path::PathBuf {
        self.threads
            .get(self.thread_idx)
            .and_then(|thread| thread.grok_cwd.clone())
            .filter(|cwd| !cwd.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.grok_cwd())
    }

    fn show_native_session_info(&mut self) {
        self.ensure_native_listing();
        let mut lines = Vec::new();
        let id = self
            .threads
            .get(self.thread_idx)
            .and_then(|thread| thread.grok_session.clone())
            .filter(|id| !id.trim().is_empty());
        match id {
            Some(id) => match grokhub_agent::load_session(&id) {
                Ok(info) => lines.push(format!(
                    "session {} · {} · {}",
                    info.id, info.title, info.cwd
                )),
                Err(err) => lines.push(format!("session {id}: {err}")),
            },
            None => lines.push("session: none".into()),
        }
        let skills: Vec<&str> = self
            .native_skills
            .iter()
            .take(8)
            .map(|skill| skill.name.as_str())
            .collect();
        lines.push(format!(
            "skills {}{}",
            self.native_skills.len(),
            if skills.is_empty() {
                String::new()
            } else {
                format!(" ({})", skills.join(", "))
            }
        ));
        lines.push(format!(
            "hooks {} trusted {}",
            self.native_hooks.len(),
            self.native_hooks_trusted
        ));
        let rows = grokhub_agent::configured();
        lines.push(format!("mcp {}", rows.len()));
        for row in rows.iter().take(8) {
            lines.push(format!("  {} {}", row.name, row.status));
        }
        self.inspect_text = lines.join("\n");
        self.nav = Nav::Connectors;
        self.status = "Native session".into();
    }

    fn fork_native_thread(&mut self) {
        let (sid, cwd, user_home) = self
            .threads
            .get(self.thread_idx)
            .map(|thread| {
                (
                    thread.grok_session.clone(),
                    thread.grok_cwd.clone(),
                    thread.grok_user_home,
                )
            })
            .unwrap_or((None, None, false));
        let forked = sid
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .and_then(|id| grokhub_agent::fork_session(id).ok());
        self.new_thread(false);
        if let Some(thread) = self.threads.get_mut(self.thread_idx) {
            thread.native = true;
            thread.grok_fork = true;
            thread.title = "Fork".into();
            thread.grok_user_home = user_home;
            if let Some(info) = forked {
                thread.grok_session = Some(info.id);
                thread.grok_cwd = if info.cwd.trim().is_empty() {
                    cwd
                } else {
                    Some(info.cwd)
                };
                self.status = "Forked the native session".into();
            } else {
                thread.grok_session = None;
                thread.grok_cwd = cwd;
                self.status = "Forked — native session starts fresh".into();
            }
        }
        self.acp = None;
        self.stamp_current_access();
        self.persist();
    }

    fn native_usage_status(&self) -> String {
        let mut cabin = usage_line(&self.usage);
        if self.cfg.daily_token_budget > 0 {
            cabin = format!(
                "{cabin} · budget {}",
                grokhub_core::budget_line(&self.usage, self.cfg.daily_token_budget)
            );
        }
        let native = grokhub_agent::usage_label(
            self.grok_usage.input_tokens,
            self.grok_usage.output_tokens,
            self.grok_usage.reasoning_tokens,
            self.grok_usage.cost_in_usd_ticks,
            &self.grok_usage.meter,
        );
        if native.is_empty() {
            cabin
        } else {
            format!("{cabin} · {native}")
        }
    }
}

fn one_line(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = flat.chars().count();
    if count <= max_chars {
        return flat;
    }
    let mut out: String = flat.chars().take(max_chars).collect();
    out.push('…');
    out
}
