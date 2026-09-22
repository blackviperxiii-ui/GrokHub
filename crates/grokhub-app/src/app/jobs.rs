//! Host, connector, consult, and leftover JobOut poll.

use super::*;

impl Cabin {

    pub(super) fn poll_job(&mut self) {
        let Some(rx) = self.rx.take() else { return };
        match rx.try_recv() {
            Ok(JobOut::Consult(detail)) => {
                self.running = false;
                self.push_bound_msg("assistant", detail.clone());
                self.status.clear();
                self.chat_job_thread = None;
                self.persist();
                self.maybe_continue_ptt();
            }
            Ok(JobOut::HostLine(line)) => {
                self.rx = Some(rx);
                self.host_live = line.clone();
                self.status = "Host…".into();
                self.voice_orb = "hands".into();
            }
            Ok(JobOut::HostDone(block)) => {
                self.running = false;
                self.voice_orb = "idle".into();
                self.host_live.clear();
                self.host_reserved = 0;
                let ok = !crate::update::host_receipt_failed(&block);
                self.last_receipt_ok = Some(ok);
                self.last_receipts
                    .push((block.chars().take(160).collect(), ok));
                if self.last_receipts.len() > 12 {
                    self.last_receipts.remove(0);
                }
                if let Some(cite) = summarize_write(
                    self.last_host.last().map(|s| s.as_str()).unwrap_or(""),
                    &block,
                ) {
                    self.status = cite;
                }
                if self.watch_once {
                    self.watched_steps = teachable_steps(&self.last_host);
                    self.watch_once = false;
                    if self.watched_steps.is_empty() {
                        self.status =
                            "Nothing to follow. Run the routine once, then teach it.".into();
                    } else {
                        self.status = "Watched once. Name a schedule on Automations.".into();
                    }
                }
                let all_hands = !self.last_host.is_empty()
                    && self
                        .last_host
                        .iter()
                        .all(|c| parse_computer_op(c).is_some());
                let any_hands = self
                    .last_host
                    .iter()
                    .any(|c| parse_computer_op(c).is_some());
                let prefix = if all_hands {
                    "COMPUTER_RESULT (facts only):"
                } else {
                    "HOST_RESULT (facts only):"
                };
                let vis = self.visible_thread_id();
                let stored_scratch: Vec<(String, bool)> = self
                    .threads
                    .iter()
                    .map(|t| (t.id.clone(), t.scratch))
                    .collect();
                let job_scratch = job_is_scratch(
                    self.chat_job_thread.as_deref(),
                    &vis,
                    self.scratch(),
                    &stored_scratch,
                );
                self.push_bound_msg("user", format!("{prefix}\n{block}"));
                bump_usage(&mut self.usage, "host");
                self.persist();
                if any_hands {
                    self.hands_attach = true;
                    self.eyes_attach = true;
                    if let Some(url) = self.capture_cabin_frame_this_turn() {
                        self.kick_frame = Some(url);
                    }
                    let cmds = self.last_host.clone();
                    std::thread::spawn(move || {
                        let rows = collect_rows();
                        let _ = lock_titles();
                        if let Some(recipe) = recipe_from_cmds(&cmds, screen_from_rows(&rows)) {
                            let _ = crate::recipes::save_recipe("last", &recipe);
                        }
                    });
                    if let Some(recipe) = recipe_from_cmds(&self.last_host, None) {
                        self.last_recipe = Some(recipe);
                    }
                    if !job_scratch {
                        let user = self.last_user_on_job();
                        let proposed = propose_skill_from_turn(&user, &block, &self.last_host);
                        self.commit_proposed_skill(proposed);
                    }
                }
                self.plan_pending = retain_held_plan(self.plan_pending.take(), &self.last_host);
                self.run_skill_verify();
                let mut defer_kick = false;
                if let Some(cite) = summarize_write(
                    self.last_host.last().map(|s| s.as_str()).unwrap_or(""),
                    &block,
                ) {
                    if let Some(path) = cite.split_whitespace().last() {
                        let path = resolve_host_cite_path(&self.cfg.project_dir, path);
                        let (tx, rx) = mpsc::channel();
                        self.host_diff_rx = Some(rx);
                        defer_kick = true;
                        std::thread::spawn(move || {
                            let out = match read_text_capped(std::path::Path::new(&path)) {
                                Ok(after) => Some(format!(
                                    "HOST_DIFF:\n{}",
                                    unified_diff_cite(&path, "", &after)
                                )),
                                Err(_) => None,
                            };
                            let _ = tx.send(out);
                        });
                    }
                }
                if !any_hands && is_hard_run(self.last_host.len() as u32, !ok, false, job_scratch) {
                    let user = self.last_user_on_job();
                    let proposed = propose_skill_from_turn(&user, &block, &self.last_host);
                    self.commit_proposed_skill(proposed);
                }
                self.append_host_trajectory(ok, &block);
                self.trim_job_result_dumps();
                if defer_kick {
                    self.host_diff_kick = true;
                } else if !self.pending_connectors.is_empty() {
                    let origin = self.chat_job_thread.clone();
                    let (id, tool, args) = self.pending_connectors.remove(0);
                    self.run_connector(&id, &tool, &args);
                    if self.chat_job_thread.is_none() {
                        self.chat_job_thread = origin;
                    }
                } else {
                    self.kick_model(false);
                }
                if !self.running {
                    self.finish_hub_dispatch(&block, ok);
                }
            }
            Ok(JobOut::Connector(detail)) => {
                self.running = false;
                self.push_bound_msg("user", format!("CONNECTOR_RESULT (facts only):\n{detail}"));
                self.persist();
                self.trim_job_result_dumps();
                if !self.pending_connectors.is_empty() {
                    let origin = self.chat_job_thread.clone();
                    let (id, tool, args) = self.pending_connectors.remove(0);
                    self.run_connector(&id, &tool, &args);
                    if self.chat_job_thread.is_none() {
                        self.chat_job_thread = origin;
                    }
                } else {
                    self.kick_model(false);
                    if !self.running {
                        self.finish_hub_dispatch(&detail, true);
                    }
                }
            }
            Ok(JobOut::Imagine(url)) => {
                self.running = false;
                self.imagine_pending = false;
                self.imagine_error.clear();
                self.imagine_last = url.clone();
                self.status = "Imagine ready".into();
                let job_prompt = self.imagine_job_prompt.clone();
                self.pin_generation_to_wall(&url, &job_prompt);
                self.push_bound_msg("assistant", format!("IMAGINE: {url}"));
                self.finish_hub_dispatch(&format!("IMAGINE: {url}"), true);
                self.chat_job_thread = None;
                self.persist();
                self.maybe_continue_ptt();
            }
            Ok(JobOut::Voice(t)) => {
                self.running = false;
                match ptt_after_stt(self.voice_is_on(), !is_voice_error(&t)) {
                    PttLine::Leave => {
                        self.status = if is_voice_error(&t) {
                            t
                        } else {
                            "Voice off".into()
                        };
                    }
                    PttLine::Listen => {
                        self.status = t;
                        self.maybe_continue_ptt();
                    }
                    PttLine::Hold => {
                        self.status = t;
                    }
                    PttLine::Chat => {
                        self.status = "Hey Grok".into();
                        self.speak_next = true;
                        self.send_chat(t);
                        self.maybe_continue_ptt();
                    }
                }
            }
            Ok(JobOut::UpdateProgress { pct, msg }) => {
                self.rx = Some(rx);
                self.update_pct = Some(pct);
                self.update_can_restart = false;
                self.status = msg;
            }
            Ok(JobOut::UpdateDone { ok }) => {
                self.running = false;
                self.last_receipt_ok = Some(ok);
                let view = overlay_update_finish(ok, self.update_pct.unwrap_or(0));
                self.update_pct = Some(view.pct);
                self.update_can_restart = view.can_restart;
                self.status = view.status;
                if ok {
                    self.note_combined_update_landed();
                }
            }
            Ok(JobOut::Err(e)) => {
                self.running = false;
                if !self.voice_is_on() {
                    self.voice_orb = "idle".into();
                }
                if self.imagine_pending {
                    self.imagine_pending = false;
                    self.imagine_error = e.clone();
                }
                remember_chip_outcome(&mut self.chip_memory, false, now_ms());
                self.status = self.apply_job_fail(&e);
                self.finish_hub_dispatch(&e, false);
                self.chat_job_thread = None;
                self.stream_buf.clear();
                self.thought_buf.clear();
                self.persist();
                self.maybe_continue_ptt();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.running = false;
                self.imagine_pending = false;
                self.status = self.apply_job_fail(worker_gone_status());
                self.finish_hub_dispatch(worker_gone_status(), false);
                self.chat_job_thread = None;
                self.stream_buf.clear();
                self.thought_buf.clear();
                self.persist();
            }
        }
    }

    pub(super) fn run_cmds(&mut self, cmds: Vec<String>) -> bool {
        if !self.cfg.host_on {
            self.status = "Host off — /host on".into();
            return false;
        }
        if self.running {
            self.status = "Busy — wait, then host".into();
            return false;
        }
        if self.host_hour_at.elapsed() > Duration::from_secs(3600) {
            self.host_hour_count = 0;
            self.host_hour_at = Instant::now();
        }
        let mut gated = Vec::new();
        let mut blocked = false;
        for c in &cmds {
            if host_hour_blocked(self.host_hour_count, self.cfg.host_hour_cap) {
                self.status = format!("Host hour cap {}", self.cfg.host_hour_cap);
                self.push_bound_msg(
                    "user",
                    format!(
                        "HOST_RESULT (facts only):\n$ {c}\nblocked: hour cap {}",
                        self.cfg.host_hour_cap
                    ),
                );
                blocked = true;
                break;
            }
            if let Some(why) = forbidden_reason(c) {
                self.push_bound_msg(
                    "user",
                    format!("HOST_RESULT (facts only):\n$ {c}\nblocked: {why}"),
                );
                blocked = true;
                continue;
            }
            if !c.trim().starts_with("COMPUTER_CMD")
                && !is_rewind_copy_cmd_in(
                    c,
                    &self.cfg.project_dir,
                    std::env::var("HOME").ok().as_deref(),
                )
                && host_cmd_leaves_project(c, &self.cfg.project_dir)
                && self.permission_mode != PermissionMode::AlwaysApprove
            {
                self.push_bound_msg(
                    "user",
                    format!("HOST_RESULT (facts only):\n$ {c}\nblocked: outside bound project"),
                );
                blocked = true;
                continue;
            }
            self.host_hour_count = self.host_hour_count.saturating_add(1);
            gated.push(c.clone());
        }
        if gated.is_empty() {
            self.plan_pending = None;
            if blocked {
                self.persist();
            }
            return blocked;
        }
        if blocked {
            self.persist();
        }
        if !gated.iter().any(|c| is_rewind_copy_cmd(c)) {
            if let Some(snap) = self.snapshot_project() {
                gated.insert(0, snap);
            }
        }
        self.last_host = gated.clone();
        self.host_halt = mint_host_halt();
        self.running = true;
        self.host_reserved = gated.len() as u32;
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        self.voice_orb = "hands".into();
        self.host_live = gated[0].clone();
        self.status = "Host…".into();
        self.plan_pending = None;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let cap = self.cfg.host_hour_cap;
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        let halt = self.host_halt.clone();
        let cwd = host_working_dir(&self.cfg.project_dir);
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut inhibit = crate::notify::inhibit_sleep();
            let mut block = String::new();
            let mut count = 0u32;
            for c in &gated {
                if halt.load(Ordering::SeqCst) {
                    block.push_str("HOST_RECEIPT: halted\n");
                    break;
                }
                if host_hour_blocked(count, cap) {
                    block.push_str("hour cap reached\n");
                    break;
                }
                count = count.saturating_add(1);
                let tx_line = tx.clone();
                let cmd = c.clone();
                let receipt = if let Some(op) = parse_computer_op(c) {
                    let _ = tx_line.send(JobOut::HostLine(format!(
                        "Hands: {}",
                        computer_cmd_line(&op)
                    )));
                    run_computer_op_cancel(&op, Some(&halt))
                } else {
                    run_host_stream(
                        c,
                        Duration::from_secs(90),
                        Some(&halt),
                        cwd.as_deref(),
                        move |line| {
                            let _ = tx_line.send(JobOut::HostLine(host_status_line(&cmd, line, 0)));
                        },
                    )
                };
                if let Some(cite) = summarize_write(c, &receipt) {
                    block.push_str(&cite);
                    block.push('\n');
                }
                block.push_str(&redact_secrets(&receipt));
                block.push_str("\n\n");
            }
            crate::notify::release_inhibit(&mut inhibit);
            crate::notify::ping_if_long_quiet(
                started.elapsed(),
                quiet,
                "GrokHub",
                "Host job finished",
            );
            let _ = tx.send(JobOut::HostDone(block));
        });
        false
    }

    pub(super) fn run_connector(&mut self, id: &str, tool: &str, args: &str) {
        if id != "github" {
            self.push_bound_msg(
                "user",
                format!(
                    "CONNECTOR_RESULT (facts only):\n{id} {tool} — not wired. GitHub is the only live connector."
                ),
            );
            self.persist();
            self.kick_model(false);
            return;
        }
        if self.running {
            self.pending_connectors
                .push((id.to_string(), tool.to_string(), args.to_string()));
            return;
        }
        self.running = true;
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        self.status = format!("GitHub {tool}…");
        let token = self.secrets.github_token.clone();
        let tool = tool.to_string();
        let args = args.to_string();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let detail = crate::github::run_github_tool(&tool, &args, &token);
            let _ = tx.send(JobOut::Connector(detail));
        });
    }

    pub(super) fn poll_host_diff(&mut self) {
        let Some(rx) = self.host_diff_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(msg)) => {
                self.push_bound_msg("user", msg);
                self.persist();
                self.finish_host_diff_kick();
            }
            Ok(None) => self.finish_host_diff_kick(),
            Err(mpsc::TryRecvError::Empty) => {
                self.host_diff_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => self.finish_host_diff_kick(),
        }
    }

    pub(super) fn finish_host_diff_kick(&mut self) {
        if !self.host_diff_kick {
            return;
        }
        self.host_diff_kick = false;
        if !self.pending_connectors.is_empty() {
            let origin = self.chat_job_thread.clone();
            let (id, tool, args) = self.pending_connectors.remove(0);
            self.run_connector(&id, &tool, &args);
            if self.chat_job_thread.is_none() {
                self.chat_job_thread = origin;
            }
        } else {
            self.kick_model(false);
        }
    }

    pub(super) fn run_consult(&mut self, q: String) {
        if !self.llm_ready() {
            self.status = "Run grok login, or Connect Grok in Settings.".into();
            return;
        }
        if self.running {
            self.halt_in_flight();
            self.finish_hub_dispatch("Interrupted by consult", false);
        }
        self.running = true;
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        self.status = "Consult…".into();
        let key = self.bearer();
        let model = model_for_mode("fast").to_string();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = grok_chat(&key, &model, &[("user".into(), q.clone())], None, None);
            let _ = tx.send(match r {
                Ok(t) => JobOut::Consult(format_consult_reply(&q, &t)),
                Err(e) => JobOut::Err(e),
            });
        });
    }
}
