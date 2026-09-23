//! Background disk writes for threads, config, hub, and usage.

use super::*;


pub(super) struct PersistSnap {
    pub(super) threads: Vec<ChatThread>,
    pub(super) msgs: Vec<(String, String)>,
    pub(super) board: Vec<BoardCard>,
    pub(super) automations: Vec<Automation>,
    pub(super) grok_loops: Vec<GrokLoop>,
    pub(super) rewind_rows: Vec<RewindRecord>,
    pub(super) learning: LearningState,
    pub(super) suggestions: SuggestionStore,
    pub(super) usage: UsageDay,
    pub(super) chip_memory: ChipMemory,
    pub(super) wall: ImagineWall,
    pub(super) cfg: AppConfig,
    pub(super) hub: Option<Arc<Mutex<HubState>>>,
    pub(super) projects: Option<Vec<ProjectNode>>,
    pub(super) secrets: Option<crate::secrets::Secrets>,
}


pub(super) fn write_persist_disk(snap: &PersistSnap) {
    let _ = threads::save(&snap.threads);
    let msgs = snap
        .threads
        .iter()
        .find(|t| t.id == snap.cfg.current_thread)
        .or_else(|| snap.threads.first())
        .map(|t| t.messages.as_slice())
        .unwrap_or(snap.msgs.as_slice());
    let _ = config::save_chat(msgs);
    let _ = config::save_board(&snap.board);
    let _ = crate::night::save(&snap.automations);
    let _ = crate::loops::save(&snap.grok_loops);
    let _ = crate::night::save_rewinds(&snap.rewind_rows);
    let _ = crate::store::save_learning(&snap.learning);
    let _ = crate::store::save_suggestions(&snap.suggestions);
    let _ = crate::store::save_usage(&snap.usage);
    let _ = crate::store::save_chips(&snap.chip_memory);
    let _ = crate::store::save_wall(&snap.wall);
    if let Some(p) = &snap.projects {
        let _ = crate::store::save_projects(p);
    }
    let _ = config::save(&snap.cfg);
    if let Some(s) = &snap.secrets {
        let _ = secrets::save(s);
    }
    if let Some(hub) = &snap.hub {
        let disk = hub.lock().ok().map(|st| state_for_disk(&st));
        if let Some(disk) = disk {
            let _ = save_hub_state(&config::hub_state_path(), &disk);
        }
    }
}

impl Cabin {

    pub(super) fn persist(&mut self) {
        let mut snap = self.persist_snap();
        if snap.projects.is_some() {
            self.projects_dirty = false;
        }
        self.sync_hub_voice();
        snap.secrets = Some(self.secrets.clone());
        self.persist_idle_key = self.persist_idle_now();
        self.last_persist = Instant::now();
        self.geom_dirty = false;
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                write_persist_disk(&snap);
            }
        });
    }

    pub(super) fn persist_snap(&mut self) -> PersistSnap {
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.messages = self.messages.clone();
            self.cfg.current_thread = t.id.clone();
        }
        let projects = if self.projects_dirty {
            Some(self.projects.clone())
        } else {
            None
        };
        PersistSnap {
            threads: self.threads.clone(),
            msgs: Vec::new(),
            board: self.board.clone(),
            automations: self.automations.clone(),
            grok_loops: self.grok_loops.clone(),
            rewind_rows: self.rewind_rows.clone(),
            learning: self.learning.clone(),
            suggestions: self.suggestions.clone(),
            usage: self.usage.clone(),
            chip_memory: self.chip_memory.clone(),
            wall: self.wall.clone(),
            cfg: {
                let mut cfg = self.cfg.clone();
                cfg.api_key.clear();
                cfg
            },
            hub: Some(self.hub.clone()),
            projects,
            // Idle persist must not write secrets.json from a stale snap.
            secrets: None,
        }
    }

    pub(super) fn persist_idle_now(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.threads.len(),
            self.thread_idx,
            self.messages.len(),
            self.messages.last().map(|(_, c)| c.len()).unwrap_or(0),
            self.board.len(),
            self.automations.len(),
            self.grok_loops.len(),
            self.usage.day,
            self.usage.messages,
            self.cfg.current_thread,
            self.threads
                .get(self.thread_idx)
                .and_then(|t| t.grok_session.as_deref())
                .unwrap_or(""),
            self.threads
                .get(self.thread_idx)
                .and_then(|t| t.grok_cwd.as_deref())
                .unwrap_or(""),
        )
    }

    pub(super) fn persist_bg(&mut self) {
        if self.running {
            return;
        }
        if self.persist_rx.is_some() {
            return;
        }
        let key = self.persist_idle_now();
        if !self.projects_dirty && self.persist_idle_key == key {
            self.last_persist = Instant::now();
            return;
        }
        self.persist_idle_key = key;
        let snap = self.persist_snap();
        if snap.projects.is_some() {
            self.projects_dirty = false;
        }
        self.sync_hub_voice();
        self.last_persist = Instant::now();
        self.geom_dirty = false;
        let io = self.persist_io.clone();
        let (tx, rx) = mpsc::channel();
        self.persist_rx = Some(rx);
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                write_persist_disk(&snap);
            }
            let _ = tx.send(());
        });
    }

    pub(super) fn poll_persist(&mut self) {
        let Some(rx) = self.persist_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {}
            Err(mpsc::TryRecvError::Empty) => {
                self.persist_rx = Some(rx);
            }
        }
    }

    pub(super) fn persist_hub(&self) {
        let hub = self.hub.clone();
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let disk = hub.lock().ok().map(|st| state_for_disk(&st));
                if let Some(disk) = disk {
                    let _ = save_hub_state(&config::hub_state_path(), &disk);
                }
            }
        });
    }

    pub(super) fn persist_cfg(&self) {
        let io = self.persist_io.clone();
        let slot = self.cfg_slot.clone();
        let mut cfg = self.cfg.clone();
        cfg.api_key.clear();
        let gen = match slot.lock() {
            Ok(mut g) => publish_cfg(&mut g, cfg),
            Err(_) => return,
        };
        std::thread::spawn(move || {
            let Ok(_disk) = io.lock() else {
                return;
            };
            let cfg = match slot.lock() {
                Ok(g) => cfg_if_current(&g, gen),
                Err(_) => return,
            };
            if let Some(cfg) = cfg {
                let _ = config::save(&cfg);
            }
        });
    }

    /// Hide/quit must not clone every thread when idle persist already wrote.
    pub(super) fn persist_if_dirty(&mut self) {
        if !self.projects_dirty && self.persist_idle_key == self.persist_idle_now() {
            self.persist_cfg();
        } else {
            self.persist();
        }
    }

    pub(super) fn persist_secrets(&self) {
        let io = self.persist_io.clone();
        let secrets = self.secrets.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = secrets::save(&secrets);
            }
        });
    }

    pub(super) fn persist_usage(&self) {
        let io = self.persist_io.clone();
        let usage = self.usage.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_usage(&usage);
            }
        });
    }

    pub(super) fn persist_loops(&mut self) {
        let list = self.grok_loops.clone();
        std::thread::spawn(move || {
            let _ = crate::loops::save(&list);
        });
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn persist_suggestions(&mut self) {
        let suggestions = self.suggestions.clone();
        std::thread::spawn(move || {
            let _ = crate::store::save_suggestions(&suggestions);
        });
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn persist_automations(&mut self) {
        let list = self.automations.clone();
        std::thread::spawn(move || {
            let _ = crate::night::save(&list);
        });
        self.persist_idle_key = self.persist_idle_now();
    }
}
