//! Chat tabs and history delete.

use super::*;

impl Cabin {

    pub(super) fn switch_thread(&mut self, idx: usize) {
        self.apply_switch_thread(idx);
        self.persist_bg();
    }

    pub(super) fn live_mut(&mut self) -> &mut Vec<(String, String)> {
        Arc::make_mut(&mut self.messages)
    }

    pub(super) fn apply_switch_thread(&mut self, idx: usize) {
        let idx = idx.min(self.threads.len().saturating_sub(1));
        let leaving = idx != self.thread_idx;
        if leaving {
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.messages = self.messages.clone();
                flush_visible_goal(&mut t.goal, self.goal_step, &self.cfg.goal_pin);
            }
            self.thread_idx = idx;
            self.messages = self
                .threads
                .get(self.thread_idx)
                .map(|t| t.messages.clone())
                .unwrap_or_else(|| Arc::new(Vec::new()));
            self.pin_chat_tail();
        } else if let Some(t) = self.threads.get_mut(self.thread_idx) {
            flush_visible_goal(&mut t.goal, self.goal_step, &self.cfg.goal_pin);
        }
        self.rename_idx = None;
        self.imagine_last =
            last_imagine_receipt(self.messages.iter().map(|(_, c)| c.as_str())).unwrap_or_default();
        self.cfg.goal_pin = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.goal.label.clone())
            .unwrap_or_default();
        self.goal_step = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.goal.step)
            .unwrap_or(0);
        if leaving {
            self.drop_leaving_thread_chrome();
        }
        self.stamp_current_access();
        self.composer_want_focus = true;
    }

    /// One hidden thread for night, loops-via-chat, inbox, and anticipate.
    /// It is not a History row.
    pub(super) fn ensure_background_history_thread(&mut self) -> usize {
        if let Some(i) = self.threads.iter().position(|t| t.background) {
            return i;
        }
        let mut created = ChatThread::new("Background", false);
        created.background = true;
        self.threads.push(created);
        self.threads.len() - 1
    }

    pub(super) fn stamp_current_access(&mut self) {
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.accessed_ms = now_ms();
        }
    }

    pub(super) fn open_recent_chat(&mut self) {
        self.pin_chat_tail();
        if let Some(idx) = threads::most_recently_accessed_index(&self.threads) {
            if idx != self.thread_idx {
                self.switch_thread(idx);
                return;
            }
        }
        self.stamp_current_access();
        self.composer_want_focus = true;
    }

    pub(super) fn land_on_real_chat(&mut self) {
        if self.scratch() {
            if let Some(idx) = threads::most_recently_accessed_index(&self.threads) {
                if idx != self.thread_idx {
                    self.apply_switch_thread(idx);
                }
            }
        }
        self.nav = Nav::Chat;
    }

    pub(super) fn new_thread(&mut self, scratch: bool) {
        let want_project = self.project_sel.clone();
        let reuse = {
            let views: Vec<ThreadReuseView> = self
                .threads
                .iter()
                .enumerate()
                .map(|(i, t)| ThreadReuseView {
                    title: t.title.as_str(),
                    scratch: t.scratch,
                    empty: if i == self.thread_idx {
                        self.messages.is_empty()
                    } else {
                        t.messages.is_empty()
                    },
                    has_session: t
                        .grok_session
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty()),
                })
                .collect();
            reuse_empty_thread_idx(&views, self.thread_idx, scratch).filter(|&idx| {
                self.threads.get(idx).and_then(|t| t.project_id.as_deref())
                    == want_project.as_deref()
            })
        };
        if let Some(idx) = reuse {
            if idx != self.thread_idx {
                self.apply_switch_thread(idx);
            } else {
                self.drop_leaving_thread_chrome();
            }
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_session = None;
                t.grok_cwd = None;
                t.project_id = want_project.clone();
            }
            self.stamp_current_access();
            self.persist();
            self.status = if scratch {
                "Scratch — no memory writes".into()
            } else {
                "New chat".into()
            };
            self.composer_want_focus = true;
            return;
        }
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.messages = self.messages.clone();
            flush_visible_goal(&mut t.goal, self.goal_step, &self.cfg.goal_pin);
        }
        let title = if scratch { "Scratch" } else { "Chat" };
        let mut created = ChatThread::new(title, scratch);
        created.project_id = want_project;
        self.threads.push(created);
        self.thread_idx = self.threads.len() - 1;
        self.messages = Arc::new(Vec::new());
        self.imagine_last.clear();
        self.cfg.goal_pin.clear();
        self.goal_step = 0;
        self.drop_leaving_thread_chrome();
        self.status = if scratch {
            "Scratch — no memory writes".into()
        } else {
            "New chat".into()
        };
        self.stamp_current_access();
        self.persist();
        self.composer_want_focus = true;
    }

    pub(super) fn begin_chat_rename(&mut self, idx: usize) {
        self.rename_buf = self.thread_rail_title(idx);
        self.rename_lock = if self.rename_buf.is_empty() {
            None
        } else {
            Some(self.rename_buf.clone())
        };
        self.rename_idx = Some(idx);
        self.rename_focus = true;
    }

    pub(super) fn rename_thread(&mut self, idx: usize, title: &str) {
        let applied = {
            let Some(t) = self.threads.get_mut(idx) else {
                return;
            };
            let mut tab = ThreadTab {
                title: t.title.clone(),
                pinned: t.pinned,
                title_locked: t.title_locked,
            };
            if apply_manual_rename(&mut tab, title) {
                t.title = tab.title;
                t.title_locked = true;
                t.pinned = tab.pinned;
                t.accessed_ms = now_ms();
                Some((t.title.clone(), t.grok_session.clone()))
            } else {
                None
            }
        };
        self.rename_idx = None;
        self.rename_focus = false;
        self.rename_lock = None;
        if let Some((shown, sid)) = applied {
            if let Some(id) = sid {
                if let Some(s) = self.grok_sessions.iter_mut().find(|s| s.id == id) {
                    s.title = shown.clone();
                }
            }
            self.status = format!("Renamed {shown}");
            self.persist();
        } else if let Some(t) = self.threads.get(idx) {
            self.status = format!("Kept {}", t.title);
        }
    }

    pub(super) fn pin_thread(&mut self, idx: usize) {
        let Some(t) = self.threads.get_mut(idx) else {
            return;
        };
        t.pinned = toggle_pin(t.pinned);
        let now = now_ms();
        t.accessed_ms = now;
        if t.pinned {
            t.pinned_ms = now;
        }
        self.status = if t.pinned {
            format!("Pinned {}", t.title)
        } else {
            format!("Unpinned {}", t.title)
        };
        self.persist();
    }

    pub(super) fn thread_for_grok(&self, id: &str) -> Option<usize> {
        self.threads
            .iter()
            .position(|t| t.grok_session.as_deref() == Some(id))
    }

    pub(super) fn session_row_title(&self, id: &str, grok_title: &str) -> String {
        if let Some(i) = self.thread_for_grok(id) {
            let t = &self.threads[i];
            return grokhub_acp::preferred_history_title(
                &t.title,
                t.title_locked,
                Some(grok_title),
                Some(id),
            );
        }
        let title = grok_title.trim();
        if title.is_empty() || title == id {
            id.to_string()
        } else {
            title.to_string()
        }
    }

    pub(super) fn session_sort_key(&self, id: &str, list_rank: u64) -> threads::SessionSortKey {
        if let Some(i) = self.thread_for_grok(id) {
            let t = &self.threads[i];
            threads::SessionSortKey {
                pinned: t.pinned,
                pinned_ms: t.pinned_ms,
                accessed_ms: t.accessed_ms,
                list_rank,
            }
        } else {
            threads::SessionSortKey {
                pinned: false,
                pinned_ms: 0,
                accessed_ms: 0,
                list_rank,
            }
        }
    }

    /// Bind a Grok session onto a cabin thread so pin and rename persist in threads.json.
    /// Does not switch chats or change the project filter.
    pub(super) fn ensure_grok_thread(&mut self, id: &str) -> Option<usize> {
        let id = id.trim();
        if id.is_empty() {
            return None;
        }
        if let Some(i) = self.thread_for_grok(id) {
            if self.threads[i].messages.is_empty() {
                self.threads[i].grok_show_pending = true;
            }
            self.kick_session_show(id);
            return Some(i);
        }
        let sess = self.grok_sessions.iter().find(|s| s.id == id).cloned();
        let title = sess
            .as_ref()
            .map(|s| self.session_row_title(&s.id, &s.title))
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| id.chars().take(24).collect());
        let mut created = ChatThread::new(&title, false);
        created.grok_session = Some(id.to_string());
        created.grok_user_home = sess.as_ref().is_none_or(|s| !s.cabin);
        created.grok_cwd = sess
            .as_ref()
            .and_then(|s| s.cwd.clone())
            .map(|p| p.display().to_string())
            .filter(|s| !s.is_empty());
        created.grok_show_pending = true;
        self.threads.push(created);
        self.kick_session_show(id);
        Some(self.threads.len() - 1)
    }

    pub(super) fn delete_thread_at(&mut self, idx: usize) {
        let grok_id = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty());
        let retired = self
            .threads
            .get(idx)
            .map(|t| t.retired_sessions.clone())
            .unwrap_or_default();
        let was_current = idx == self.thread_idx;
        match delete_thread(self.threads.len(), idx, self.thread_idx) {
            DeleteOutcome::ResetLast => {
                self.halt_in_flight();
                self.finish_hub_dispatch("Chat deleted", false);
                self.threads.clear();
                self.threads.push(ChatThread::new("Chat", false));
                self.thread_idx = 0;
                self.messages = self.threads[0].messages.clone();
                self.imagine_last.clear();
                self.cfg.goal_pin.clear();
                self.goal_step = 0;
                self.drop_leaving_thread_chrome();
                self.status = "Chat deleted".into();
                self.stamp_current_access();
            }
            DeleteOutcome::Removed { next } => {
                let gone = self.threads.remove(idx);
                if self.chat_job_thread.as_deref() == Some(gone.id.as_str()) {
                    self.halt_in_flight();
                    self.finish_hub_dispatch("Chat deleted", false);
                }
                self.thread_idx = next;
                if was_current {
                    self.messages = self
                        .threads
                        .get(next)
                        .map(|t| t.messages.clone())
                        .unwrap_or_else(|| Arc::new(Vec::new()));
                    self.imagine_last =
                        last_imagine_receipt(self.messages.iter().map(|m| m.1.as_str()))
                            .unwrap_or_default();
                    self.cfg.goal_pin = self
                        .threads
                        .get(next)
                        .map(|t| t.goal.label.clone())
                        .unwrap_or_default();
                    self.goal_step = self.threads.get(next).map(|t| t.goal.step).unwrap_or(0);
                    self.drop_leaving_thread_chrome();
                }
                self.status = format!("Deleted {}", gone.title);
            }
        }
        let ids = threads::sessions_deleted_with_chat(grok_id.as_deref(), &retired);
        if let Some((first, rest)) = ids.split_first() {
            self.forget_grok_build_session(first, rest);
        }
        self.rename_idx = None;
        self.persist();
    }

    pub(super) fn delete_all_history(&mut self) {
        self.halt_in_flight();
        self.finish_hub_dispatch("Chats deleted", false);
        let mut ids: Vec<String> = Vec::new();
        for t in &self.threads {
            for id in
                threads::sessions_deleted_with_chat(t.grok_session.as_deref(), &t.retired_sessions)
            {
                if !ids.iter().any(|s| s == &id) {
                    ids.push(id);
                }
            }
        }
        for s in &self.grok_sessions {
            if !ids.iter().any(|id| id == &s.id) {
                ids.push(s.id.clone());
            }
        }
        for id in &ids {
            self.pending_grok_deletes.insert(id.clone());
        }
        self.grok_sessions.clear();
        self.grok_list_gen = self.grok_list_gen.wrapping_add(1);
        let gen = self.grok_list_gen;
        self.grok_sessions_inflight = self.grok_sessions_inflight.saturating_add(1);
        let bin = grokhub_acp::find_grok();
        let cwd = self.grok_cli_cwd();
        let tx = self.grok_sessions_tx.clone();
        std::thread::spawn(move || {
            let mut error = None;
            if let Some(bin) = bin.as_ref() {
                for id in &ids {
                    if let Err(e) = grokhub_acp::delete_session(bin, &cwd, id) {
                        if error.is_none() {
                            error = Some(e);
                        }
                    }
                }
            }
            let listed = match bin.as_ref() {
                Some(bin) => grokhub_acp::list_sessions(bin, &cwd).unwrap_or_default(),
                None => Vec::new(),
            };
            let rows = grok_session_rows(listed, cwd);
            let _ = tx.send(GrokSessMsg::Listed {
                gen,
                rows,
                done: ids,
                error,
            });
        });
        self.threads.clear();
        self.threads.push(ChatThread::new("Chat", false));
        self.thread_idx = 0;
        self.messages = self.threads[0].messages.clone();
        self.imagine_last.clear();
        self.cfg.goal_pin.clear();
        self.goal_step = 0;
        self.drop_leaving_thread_chrome();
        self.rename_idx = None;
        self.status = "Deleted all chats".into();
        self.stamp_current_access();
        self.persist();
    }

    pub(super) fn nav_from_id(id: &str) -> Nav {
        match id {
            "settings" => Nav::Settings,
            "imagine" => Nav::Imagine,
            "history" => Nav::History,
            "workboard" => Nav::Workboard,
            "skills" => Nav::Skills,
            "night" | "automations" => Nav::Night,
            "agents" | "queue" => Nav::Agents,
            "devices" => Nav::Devices,
            "memory" => Nav::Memory,
            "connectors" => Nav::Connectors,
            "command" => Nav::Command,
            "chat" => Nav::Chat,
            _ => Nav::Chat,
        }
    }

    /// Plan pill: switch the session mode. Do not write the thread title.
    /// The persist id still clears so the next turn is a new Grok session.
    pub(super) fn select_plan_without_rename(&mut self) {
        self.confirm = None;
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
        self.status = format!("Session {}", SessionMode::Plan.as_str());
    }

    /// Freeze the title and the History label before Plan drops the session id.
    pub(super) fn hold_chat_name_for_plan(&mut self) {
        let idx = self.thread_idx;
        let Some(t) = self.threads.get(idx) else {
            return;
        };
        let kept = grokhub_acp::title_after_selecting_plan(&t.title);
        if t.title != kept {
            return;
        }
        let shown = self.thread_rail_title(idx);
        let sid = t.grok_session.clone();
        let Some(id) = sid else {
            return;
        };
        if let Some(s) = self.grok_sessions.iter_mut().find(|s| s.id == id) {
            s.title = grokhub_acp::history_label_after_plan(&shown, &s.title);
        }
    }

    pub(super) fn thread_rail_title(&self, idx: usize) -> String {
        let Some(t) = self.threads.get(idx) else {
            return String::new();
        };
        let grok_title = t.grok_session.as_deref().and_then(|id| {
            self.grok_sessions
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.title.as_str())
        });
        grokhub_acp::preferred_history_title(
            &t.title,
            t.title_locked,
            grok_title,
            t.grok_session.as_deref(),
        )
    }
}
