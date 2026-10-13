//! Chat tabs and history delete.

use super::*;

impl Cabin {
    pub(super) fn switch_thread(&mut self, idx: usize) {
        self.apply_switch_thread(idx);
        if let Some(id) = self.threads.get(self.thread_idx).and_then(|t| {
            if t.grok_show_pending && t.messages.is_empty() {
                t.grok_session.clone()
            } else {
                None
            }
        }) {
            self.kick_session_show(&id);
        }
        self.persist_bg();
    }

    pub(super) fn live_mut(&mut self) -> &mut Vec<(String, String)> {
        self.messages_rev = self.messages_rev.wrapping_add(1);
        self.messages_body_rev = self.messages_body_rev.wrapping_add(1);
        Arc::make_mut(&mut self.messages)
    }

    /// [`Self::live_mut`] for a streaming reply: replace or extend the last message, or
    /// push a new one. Nothing before the last message may change through this, so the
    /// transcript caches rebuild only that stretch (and hold it while the turn streams).
    pub(super) fn live_tail_mut(&mut self) -> &mut Vec<(String, String)> {
        self.messages_rev = self.messages_rev.wrapping_add(1);
        Arc::make_mut(&mut self.messages)
    }

    /// Everything before the last message: `live_mut` edits and wholesale swaps of
    /// `messages` change it; a streaming reply through `live_tail_mut` does not.
    pub(super) fn messages_body_mark(&self) -> (u64, usize) {
        (self.messages_body_rev, Arc::as_ptr(&self.messages) as usize)
    }

    pub(super) fn apply_switch_thread(&mut self, idx: usize) {
        let idx = idx.min(self.threads.len().saturating_sub(1));
        let leaving = idx != self.thread_idx;
        let leaving_id = self.visible_thread_id();
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
            self.stamp_open_activity(self.thread_idx);
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
            let owns_run = self
                .chat_job_thread
                .as_deref()
                .map(|id| id == leaving_id)
                .unwrap_or(true);
            if owns_run {
                self.drop_leaving_thread_chrome();
            }
        }
        self.composer_want_focus = true;
    }

    /// One hidden thread for night, loops-via-chat, inbox, and anticipate.
    /// It is not a History row.
    pub(super) fn ensure_background_history_thread(&mut self) -> usize {
        // Only the shared Background chat: an idea card's hidden chat is background too.
        if let Some(i) = self
            .threads
            .iter()
            .position(|t| t.background && t.title == threads::BACKGROUND_THREAD_TITLE)
        {
            return i;
        }
        let mut created = ChatThread::new(threads::BACKGROUND_THREAD_TITLE, false);
        created.background = true;
        self.threads.push(created);
        self.threads.len() - 1
    }

    pub(super) fn stamp_current_access(&mut self) {
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            let now = now_ms();
            t.accessed_ms = now;
            t.last_active_ms = now;
        }
    }

    /// Opening a chat counts as activity for auto-archive without moving History order.
    pub(super) fn stamp_open_activity(&mut self, idx: usize) {
        if let Some(t) = self.threads.get_mut(idx) {
            t.last_active_ms = now_ms();
        }
    }

    /// A chat with a live reply, soft/hard park, or background job is busy.
    pub(super) fn thread_is_busy(&self, id: &str) -> bool {
        if self.running && self.chat_job_thread.as_deref() == Some(id) {
            return true;
        }
        if self.bg.runs.iter().any(|r| r.thread_id == id && r.live()) {
            return true;
        }
        if self.harness.soft_parks.iter().any(|p| p.chat_id == id) {
            return true;
        }
        if self.harness.park.as_ref().is_some_and(|p| p.chat_id == id) {
            return true;
        }
        if self.harness.queue.iter().any(|p| p.chat_id == id) {
            return true;
        }
        false
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

    // Archive helpers stay after open_recent_chat: chat_side_effects_keep_the_origin_thread
    // scans switch_thread..open_recent_chat and forbids self.persist() in that window.

    pub(super) fn archive_thread_at(&mut self, idx: usize) {
        let Some(t) = self.threads.get_mut(idx) else {
            return;
        };
        let title = t.title.clone();
        let id = t.id.clone();
        if !threads::archive_thread(t, now_ms()) {
            self.status = format!("{title} is already archived");
            return;
        }
        self.archive_undo = Some(id.clone());
        self.status = format!("Archived {title}");
        self.persist();
    }

    pub(super) fn unarchive_thread_at(&mut self, idx: usize) {
        let Some(t) = self.threads.get_mut(idx) else {
            return;
        };
        let title = t.title.clone();
        let id = t.id.clone();
        if !threads::unarchive_thread(t) {
            self.status = format!("{title} is not archived");
            return;
        }
        t.last_active_ms = now_ms();
        self.archive_undo = Some(id);
        self.status = format!("Unarchived {title}");
        self.persist();
    }

    /// Toggle archive on the open chat (shortcut and header menu).
    pub(super) fn toggle_archive_current(&mut self) {
        let idx = self.thread_idx;
        let Some(t) = self.threads.get(idx) else {
            return;
        };
        if t.background || t.scratch {
            self.status = "Scratch and background chats stay put".into();
            return;
        }
        if threads::is_archived(t) {
            self.unarchive_thread_at(idx);
        } else {
            self.archive_thread_at(idx);
        }
    }

    /// Undo the last archive or unarchive toast (same chat, flip back).
    pub(super) fn undo_archive_toast(&mut self) {
        let Some(id) = self.archive_undo.take() else {
            return;
        };
        let Some(idx) = self.threads.iter().position(|t| t.id == id) else {
            return;
        };
        if threads::is_archived(&self.threads[idx]) {
            self.unarchive_thread_at(idx);
        } else {
            self.archive_thread_at(idx);
        }
        self.archive_undo = None;
    }

    /// Reply in an archived chat restores it to the main list with a quiet note.
    pub(super) fn unarchive_on_reply(&mut self) {
        let idx = self.thread_idx;
        let Some(t) = self.threads.get(idx) else {
            return;
        };
        if !threads::is_archived(t) {
            return;
        }
        let title = t.title.clone();
        if let Some(t) = self.threads.get_mut(idx) {
            let _ = threads::unarchive_thread(t);
            t.last_active_ms = now_ms();
        }
        self.archive_undo = None;
        self.status = format!("Restored {title} to chats");
        self.persist();
    }

    /// App start and once a day: archive idle chats. Returns how many moved.
    pub(super) fn run_auto_archive(&mut self, now: u64) -> usize {
        let days = self.cfg.chat_archive_days;
        if days == 0 {
            return 0;
        }
        let busy: std::collections::HashSet<String> = self
            .threads
            .iter()
            .filter(|t| self.thread_is_busy(&t.id))
            .map(|t| t.id.clone())
            .collect();
        let mut n = 0;
        for t in &mut self.threads {
            let is_busy = busy.contains(&t.id);
            if threads::should_auto_archive(t, now, days, is_busy)
                && threads::archive_thread(t, now)
            {
                n += 1;
            }
        }
        if n > 0 {
            self.persist();
            self.status = if n == 1 {
                "Archived 1 idle chat".into()
            } else {
                format!("Archived {n} idle chats")
            };
        }
        n
    }

    /// Once a local day (and on first call after launch when the day is new).
    pub(super) fn tick_chat_auto_archive(&mut self, today: &str) {
        if self.chat_archive_day.as_deref() == Some(today) {
            return;
        }
        self.chat_archive_day = Some(today.to_string());
        let _ = self.run_auto_archive(now_ms());
    }

    /// Settings → Behavior → Auto-archive idle chats.
    pub(super) fn ui_chat_archive_row(&mut self, ui: &mut egui::Ui) {
        const CHOICES: &[u32] = &[0, 7, 14, 30, 90];
        let labels: Vec<String> = CHOICES
            .iter()
            .map(|d| {
                if *d == 0 {
                    "Off".into()
                } else if *d == 1 {
                    "1 day".into()
                } else {
                    format!("{d} days")
                }
            })
            .collect();
        let hint = "Chats with no messages, opens or edits for this long leave History and search (still on disk; open the chat or Unarchive from its menu). Pinned chats and chats with a running or parked task stay. Nothing is deleted.";
        let current = if self.cfg.chat_archive_days == 0 {
            "Off".to_string()
        } else {
            format!("{} days", self.cfg.chat_archive_days)
        };
        if let Some(i) =
            crate::cards::settings_dropdown(ui, "Auto-archive idle chats", hint, &current, &labels)
        {
            self.cfg.chat_archive_days = CHOICES[i];
            self.persist_cfg();
            self.status = if self.cfg.chat_archive_days == 0 {
                "Auto-archive off".into()
            } else {
                format!("Auto-archive after {} days", self.cfg.chat_archive_days)
            };
        }
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
                t.native = true;
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
        created.native = true;
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

    /// Minimize, tray show, and the next launch land on an empty chat so the feed is first.
    /// The previous transcript and pin stay on that thread in History.
    /// A reply that is still running stays on the thread that started it.
    pub(super) fn open_fresh_home(&mut self) {
        if grokhub_core::resume_needs_fresh_chat(self.messages.len(), self.scratch()) {
            if self.running && self.chat_job_thread.is_some() {
                self.park_fresh_chat();
            } else {
                self.new_thread(false);
            }
        }
        self.nav = Nav::Chat;
    }

    /// Empty home. Does not halt or drop the in-flight reply on the previous thread.
    pub(super) fn park_fresh_chat(&mut self) {
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
            reuse_empty_thread_idx(&views, self.thread_idx, false).filter(|&idx| {
                self.threads.get(idx).and_then(|t| t.project_id.as_deref())
                    == want_project.as_deref()
            })
        };
        if let Some(idx) = reuse {
            if idx != self.thread_idx {
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
            }
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_session = None;
                t.grok_cwd = None;
                t.project_id = want_project;
            }
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
            self.imagine_last.clear();
            self.stamp_current_access();
            self.persist();
            self.status = "New chat".into();
            self.composer_want_focus = true;
            return;
        }
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.messages = self.messages.clone();
            flush_visible_goal(&mut t.goal, self.goal_step, &self.cfg.goal_pin);
        }
        let mut created = ChatThread::new("Chat", false);
        created.project_id = want_project;
        self.threads.push(created);
        self.thread_idx = self.threads.len() - 1;
        self.messages = Arc::new(Vec::new());
        self.imagine_last.clear();
        self.cfg.goal_pin.clear();
        self.goal_step = 0;
        self.status = "New chat".into();
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
                let now = now_ms();
                t.accessed_ms = now;
                t.last_active_ms = now;
                Some((t.title.clone(), t.grok_session.clone()))
            } else {
                None
            }
        };
        self.rename_idx = None;
        self.rename_focus = false;
        self.rename_lock = None;
        if let Some((shown, sid)) = applied {
            let native = self.threads.get(idx).is_some_and(|t| t.native);
            if let Some(id) = sid {
                if native {
                    let _ = grokhub_agent::rename_session(&id, &shown);
                }
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
        t.last_active_ms = now;
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

    pub(super) fn delete_thread_at(&mut self, idx: usize) {
        let grok_id = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty());
        let native = self.threads.get(idx).is_some_and(|t| t.native);
        let retired = self
            .threads
            .get(idx)
            .map(|t| t.retired_sessions.clone())
            .unwrap_or_default();
        let was_current = idx == self.thread_idx;
        match delete_thread(self.threads.len(), idx, self.thread_idx) {
            DeleteOutcome::ResetLast => {
                let old_ids: Vec<String> = self.threads.iter().map(|t| t.id.clone()).collect();
                self.halt_in_flight();
                for id in &old_ids {
                    self.drop_bg_runs_for(id);
                }
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
                self.drop_bg_runs_for(&gone.id);
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
        if native
            && self
                .acp
                .as_ref()
                .is_some_and(|handle| ids.iter().any(|id| id == &handle.session_id))
        {
            self.halt_in_flight();
            self.acp = None;
        }
        // CLI-era sessions stay in ~/.grok untouched: GrokHub only reads them.
        for id in &ids {
            if native || id.starts_with("native-") {
                let _ = grokhub_agent::delete_session(id);
            }
        }
        self.grok_sessions.retain(|s| !ids.contains(&s.id));
        self.rename_idx = None;
        self.persist();
    }

    pub(super) fn delete_all_history(&mut self) {
        let thread_ids: Vec<String> = self.threads.iter().map(|t| t.id.clone()).collect();
        self.halt_in_flight();
        for id in &thread_ids {
            self.drop_bg_runs_for(id);
        }
        self.finish_hub_dispatch("Chats deleted", false);
        let mut native_ids: Vec<String> = Vec::new();
        for t in &self.threads {
            let native_thread = t.native;
            for id in
                threads::sessions_deleted_with_chat(t.grok_session.as_deref(), &t.retired_sessions)
            {
                if (native_thread || id.starts_with("native-"))
                    && !native_ids.iter().any(|s| s == &id)
                {
                    native_ids.push(id);
                }
            }
        }
        for s in &self.grok_sessions {
            if s.id.starts_with("native-") && !native_ids.iter().any(|id| id == &s.id) {
                native_ids.push(s.id.clone());
            }
        }
        if let Some(id) = self
            .acp
            .as_ref()
            .map(|handle| handle.session_id.clone())
            .filter(|id| id.starts_with("native-"))
        {
            if !native_ids.iter().any(|saved| saved == &id) {
                native_ids.push(id);
            }
            self.acp = None;
        }
        for id in &native_ids {
            let _ = grokhub_agent::delete_session(id);
        }
        self.grok_sessions.clear();
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
            "journal" => Nav::Journal,
            "workboard" => Nav::Workboard,
            "pulse" | "ideas" => Nav::Pulse,
            "skills" => Nav::Skills,
            "night" | "automations" => Nav::Night,
            "agents" | "queue" => Nav::Agents,
            "devices" => Nav::Devices,
            "memory" => Nav::Memory,
            // Connectors moved into Settings; a saved page reopens on Skills.
            "connectors" => Nav::Skills,
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
        let kept = grokhub_core::cli_history::title_after_selecting_plan(&t.title);
        if t.title != kept {
            return;
        }
        let shown = self.thread_rail_title(idx);
        let sid = t.grok_session.clone();
        let Some(id) = sid else {
            return;
        };
        if let Some(s) = self.grok_sessions.iter_mut().find(|s| s.id == id) {
            s.title = grokhub_core::cli_history::history_label_after_plan(&shown, &s.title);
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
        grokhub_core::cli_history::preferred_history_title(
            &t.title,
            t.title_locked,
            grok_title,
            t.grok_session.as_deref(),
        )
    }
}
