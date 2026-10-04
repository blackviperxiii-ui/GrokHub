//! Background disk writes for threads, config, hub, and usage.

use super::*;

/// Tests pin `config_dir` to the directory captured when the write was scheduled.
/// A worker blocked on `persist_io` must keep that directory after `GROKHUB_CONFIG` moves.
pub(super) struct ScheduledDir {
    #[cfg(test)]
    _pin: crate::config::TestConfigDir,
}

pub(super) fn pin_scheduled_dir(dir: std::path::PathBuf) -> ScheduledDir {
    #[cfg(test)]
    {
        ScheduledDir {
            _pin: crate::config::TestConfigDir::set(dir),
        }
    }
    #[cfg(not(test))]
    {
        let _ = dir;
        ScheduledDir {}
    }
}

pub(super) struct PersistSnap {
    pub(super) threads: Vec<ChatThread>,
    pub(super) msgs: Vec<(String, String)>,
    pub(super) board: Vec<BoardCard>,
    pub(super) automations: Vec<Automation>,
    pub(super) grok_loops: Vec<GrokLoop>,
    pub(super) updates: Vec<grokhub_core::UpdateCard>,
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


pub(super) fn write_persist_disk(dir: &std::path::Path, snap: &PersistSnap) {
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
    let _ = crate::feed::save(&snap.updates);
    let _ = crate::night::save_rewinds(&snap.rewind_rows);
    let _ = crate::store::save_learning(&snap.learning);
    let _ = crate::store::save_suggestions(&snap.suggestions);
    let _ = crate::store::save_usage(&snap.usage);
    let _ = crate::store::save_chips(&snap.chip_memory);
    let _ = crate::store::save_wall(&snap.wall);
    if let Some(p) = &snap.projects {
        let _ = crate::store::save_projects(p);
    }
    let _ = config::save_in(dir, &snap.cfg);
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

/// What the full-snapshot workers have written. Each snapshot carries the
/// generation it was taken at.
#[derive(Default)]
pub(super) struct PersistMark {
    gen: u64,
    projects: u64,
    secrets: u64,
}

/// Write a full snapshot unless a newer one already reached the disk. Persist
/// workers are separate threads, so two of them can take `persist_io` in either
/// order: Delete all persists once from `halt_in_flight` (old chats) and once
/// at the end (one fresh Chat), and the old snapshot used to land last. An older
/// snapshot only writes the projects or secrets it alone carries. Call this
/// with `persist_io` held.
pub(super) fn write_persist_disk_in_order(
    dir: &std::path::Path,
    snap: &PersistSnap,
    gen: u64,
    mark: &Mutex<PersistMark>,
) {
    let mut mark = mark.lock().unwrap_or_else(|e| e.into_inner());
    if gen > mark.gen {
        write_persist_disk(dir, snap);
        mark.gen = gen;
        if snap.projects.is_some() {
            mark.projects = gen;
        }
        if snap.secrets.is_some() {
            mark.secrets = gen;
        }
        return;
    }
    if let Some(p) = &snap.projects {
        if gen > mark.projects {
            let _ = crate::store::save_projects(p);
            mark.projects = gen;
        }
    }
    if let Some(s) = &snap.secrets {
        if gen > mark.secrets {
            let _ = secrets::save(s);
            mark.secrets = gen;
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
        let dir = config::config_dir();
        let gen = self.next_persist_gen();
        let mark = self.persist_mark.clone();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir.clone());
            if let Ok(_g) = io.lock() {
                write_persist_disk_in_order(&dir, &snap, gen, &mark);
            }
        });
    }

    pub(super) fn next_persist_gen(&mut self) -> u64 {
        self.persist_gen = self.persist_gen.wrapping_add(1).max(1);
        self.persist_gen
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
            updates: self.updates.clone(),
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
        let dir = config::config_dir();
        let (tx, rx) = mpsc::channel();
        self.persist_rx = Some(rx);
        let gen = self.next_persist_gen();
        let mark = self.persist_mark.clone();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir.clone());
            if let Ok(_g) = io.lock() {
                write_persist_disk_in_order(&dir, &snap, gen, &mark);
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
        let dir = config::config_dir();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
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
        let dir = config::config_dir();
        let mut cfg = self.cfg.clone();
        cfg.api_key.clear();
        let gen = match slot.lock() {
            Ok(mut g) => publish_cfg(&mut g, cfg),
            Err(_) => return,
        };
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir.clone());
            let Ok(_disk) = io.lock() else {
                return;
            };
            let cfg = match slot.lock() {
                Ok(g) => cfg_if_current(&g, gen),
                Err(_) => return,
            };
            if let Some(cfg) = cfg {
                let _ = config::save_in(&dir, &cfg);
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
        let dir = config::config_dir();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            if let Ok(_g) = io.lock() {
                let _ = secrets::save(&secrets);
            }
        });
    }

    pub(super) fn persist_usage(&self) {
        let io = self.persist_io.clone();
        let usage = self.usage.clone();
        let dir = config::config_dir();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_usage(&usage);
            }
        });
    }

    pub(super) fn persist_loops(&mut self) {
        let list = self.grok_loops.clone();
        let dir = config::config_dir();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            let _ = crate::loops::save(&list);
        });
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn persist_suggestions(&mut self) {
        let suggestions = self.suggestions.clone();
        let dir = config::config_dir();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            let _ = crate::store::save_suggestions(&suggestions);
        });
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn persist_automations(&mut self) {
        let list = self.automations.clone();
        let dir = config::config_dir();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            let _ = crate::night::save(&list);
        });
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn persist_updates(&mut self) {
        let list = self.updates.clone();
        let dir = config::config_dir();
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            let _ = crate::feed::save(&list);
        });
        self.persist_idle_key = self.persist_idle_now();
    }
}
