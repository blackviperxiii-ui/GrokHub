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
            reuse_empty_thread_idx(&views, self.thread_idx, scratch)
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
        self.threads.push(ChatThread::new(title, scratch));
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
            t.accessed_ms = now_ms();
            self.status = format!("Renamed {}", t.title);
            self.rename_idx = None;
            self.rename_focus = false;
            self.rename_lock = None;
            self.persist();
        }
    }

    pub(super) fn pin_thread(&mut self, idx: usize) {
        let Some(t) = self.threads.get_mut(idx) else {
            return;
        };
        t.pinned = toggle_pin(t.pinned);
        t.accessed_ms = now_ms();
        self.status = if t.pinned {
            format!("Pinned {}", t.title)
        } else {
            format!("Unpinned {}", t.title)
        };
        self.persist();
    }

    pub(super) fn delete_thread_at(&mut self, idx: usize) {
        let grok_id = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty());
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
        if let Some(id) = grok_id {
            self.forget_grok_build_session(&id);
        }
        self.rename_idx = None;
        self.persist();
    }

    pub(super) fn delete_all_history(&mut self) {
        self.halt_in_flight();
        self.finish_hub_dispatch("Chats deleted", false);
        let mut ids: Vec<String> = self
            .threads
            .iter()
            .filter_map(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty())
            .collect();
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
