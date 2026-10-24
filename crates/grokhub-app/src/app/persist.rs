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

#[derive(Clone)]
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

/// Write every full-snap store. Returns the first failure; later stores still try.
pub(super) fn write_persist_disk(dir: &std::path::Path, snap: &PersistSnap) -> Result<(), String> {
    let mut first = Ok(());
    let mut note = |r: Result<(), String>| {
        if first.is_ok() {
            if let Err(e) = r {
                first = Err(e);
            }
        }
    };
    note(threads::save(&snap.threads));
    let msgs = snap
        .threads
        .iter()
        .find(|t| t.id == snap.cfg.current_thread)
        .or_else(|| snap.threads.first())
        .map(|t| t.messages.as_slice())
        .unwrap_or(snap.msgs.as_slice());
    note(config::save_chat(msgs));
    note(config::save_board(&snap.board));
    note(crate::night::save(&snap.automations));
    note(crate::loops::save(&snap.grok_loops));
    note(crate::feed::save(&snap.updates));
    note(crate::night::save_rewinds(&snap.rewind_rows));
    note(crate::store::save_learning(&snap.learning));
    note(crate::store::save_suggestions(&snap.suggestions));
    note(crate::store::save_usage(&snap.usage));
    note(crate::store::save_chips(&snap.chip_memory));
    note(crate::store::save_wall(&snap.wall));
    if let Some(p) = &snap.projects {
        note(crate::store::save_projects(p));
    }
    note(config::save_in(dir, &snap.cfg));
    if let Some(s) = &snap.secrets {
        note(secrets::save(s));
    }
    if let Some(hub) = &snap.hub {
        let disk = hub.lock().ok().map(|st| state_for_disk(&st));
        if let Some(disk) = disk {
            note(save_hub_state(&config::hub_state_path(), &disk));
        }
    }
    first
}

/// What the full-snapshot workers have written. Each snapshot carries the
/// generation it was taken at.
#[derive(Default)]
pub(super) struct PersistMark {
    gen: u64,
    projects: u64,
    secrets: u64,
}

impl PersistMark {
    /// Claim the secrets.json write for `gen`; false when a newer one already landed.
    pub(super) fn take_secrets(&mut self, gen: u64) -> bool {
        if gen <= self.secrets {
            return false;
        }
        self.secrets = gen;
        true
    }
}

/// Write a full snapshot unless a newer one already reached the disk. The
/// PersistService worker serializes through `persist_io`, but Delete all still
/// queues two gens (halt_in_flight old chats, then one fresh Chat); generation
/// order keeps the older snap from landing last. An older snapshot only writes
/// the projects or secrets it alone carries. Call this with `persist_io` held.
pub(super) fn write_persist_disk_in_order(
    dir: &std::path::Path,
    snap: &PersistSnap,
    gen: u64,
    mark: &Mutex<PersistMark>,
) -> Result<(), String> {
    let mut mark = mark.lock().unwrap_or_else(|e| e.into_inner());
    if gen > mark.gen {
        // A newer secrets-only write may have landed first; keep its tokens.
        let r = if snap.secrets.is_some() && !mark.take_secrets(gen) {
            let mut fresh = snap.clone();
            fresh.secrets = None;
            write_persist_disk(dir, &fresh)
        } else {
            write_persist_disk(dir, snap)
        };
        mark.gen = gen;
        if snap.projects.is_some() {
            mark.projects = gen;
        }
        return r;
    }
    let mut first = Ok(());
    if let Some(p) = &snap.projects {
        if gen > mark.projects {
            if let Err(e) = crate::store::save_projects(p) {
                first = Err(e);
            }
            mark.projects = gen;
        }
    }
    if let Some(s) = &snap.secrets {
        if mark.take_secrets(gen) {
            if first.is_ok() {
                if let Err(e) = secrets::save(s) {
                    first = Err(e);
                }
            } else {
                let _ = secrets::save(s);
            }
        }
    }
    first
}

impl Cabin {
    pub(super) fn persist(&mut self) {
        let mut snap = self.persist_snap();
        if snap.projects.is_some() {
            self.projects_dirty = false;
        }
        snap.secrets = Some(self.secrets.clone());
        self.persist_idle_key = self.persist_idle_now();
        self.last_persist = Instant::now();
        self.geom_dirty = false;
        let dir = config::config_dir();
        let gen = self.next_persist_gen();
        let err = self.persist_err.clone();
        // PersistService worker takes persist_io + write_persist_disk_in_order.
        self.persist_svc.submit_full(dir, snap, gen, None, err);
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
        self.last_persist = Instant::now();
        self.geom_dirty = false;
        let dir = config::config_dir();
        let (tx, rx) = mpsc::channel();
        self.persist_rx = Some(rx);
        let gen = self.next_persist_gen();
        let err = self.persist_err.clone();
        // PersistService worker takes persist_io + write_persist_disk_in_order.
        self.persist_svc.submit_full(dir, snap, gen, Some(tx), err);
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
        // ci-guard: allow-raw-io hub state save, not on PersistService yet
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
        let slot = self.cfg_slot.clone();
        let dir = config::config_dir();
        let mut cfg = self.cfg.clone();
        cfg.api_key.clear();
        let gen = match slot.lock() {
            Ok(mut g) => publish_cfg(&mut g, cfg),
            Err(_) => return,
        };
        let err = self.persist_err.clone();
        // PersistService worker takes persist_io before save_in.
        self.persist_svc.submit_settings(dir, slot, gen, err);
    }

    /// Show a failed background write instead of leaving "Saved" up.
    pub(super) fn poll_persist_err(&mut self) {
        let failed = self
            .persist_err
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(line) = failed {
            self.status = line;
            // Disk may be stale; do not treat the optimistic idle key as clean.
            self.persist_idle_key.clear();
        }
    }

    /// Hide/quit must not clone every thread when idle persist already wrote.
    pub(super) fn persist_if_dirty(&mut self) {
        if !self.projects_dirty && self.persist_idle_key == self.persist_idle_now() {
            self.persist_cfg();
        } else {
            self.persist();
        }
    }

    pub(super) fn persist_secrets(&mut self) {
        let err = Some(self.persist_err.clone());
        self.submit_secrets(err);
    }

    /// OAuth refresh, sign-in and sign-out write secrets.json quietly, as before.
    pub(super) fn persist_oauth_secrets(&mut self) {
        self.submit_secrets(None);
    }

    fn submit_secrets(&mut self, err: Option<Arc<Mutex<Option<String>>>>) {
        let gen = self.next_persist_gen();
        let secrets = self.secrets.clone();
        // PersistService takes persist_io and drops a write older than the disk.
        self.persist_svc
            .submit_secrets(config::config_dir(), secrets, gen, err);
    }

    pub(super) fn persist_usage(&self) {
        let io = self.persist_io.clone();
        let usage = self.usage.clone();
        let dir = config::config_dir();
        // ci-guard: allow-raw-io usage ledger save, not on PersistService yet
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
        let io = self.persist_io.clone();
        let err = self.persist_err.clone();
        // ci-guard: allow-raw-io loops save, not on PersistService yet
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            match io.lock() {
                Ok(_g) => note_persist_err(&err, crate::loops::save(&list), "loops"),
                Err(_) => note_persist_err(&err, Err("persist lock poisoned".into()), "loops"),
            }
        });
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn persist_suggestions(&mut self) {
        let suggestions = self.suggestions.clone();
        let dir = config::config_dir();
        let io = self.persist_io.clone();
        let err = self.persist_err.clone();
        // ci-guard: allow-raw-io suggestions save, not on PersistService yet
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            match io.lock() {
                Ok(_g) => note_persist_err(
                    &err,
                    crate::store::save_suggestions(&suggestions),
                    "suggestions",
                ),
                Err(_) => {
                    note_persist_err(&err, Err("persist lock poisoned".into()), "suggestions")
                }
            }
        });
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn persist_automations(&mut self) {
        let list = self.automations.clone();
        let dir = config::config_dir();
        let io = self.persist_io.clone();
        let err = self.persist_err.clone();
        // ci-guard: allow-raw-io automations save, not on PersistService yet
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            match io.lock() {
                Ok(_g) => note_persist_err(&err, crate::night::save(&list), "automations"),
                Err(_) => {
                    note_persist_err(&err, Err("persist lock poisoned".into()), "automations")
                }
            }
        });
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn persist_updates(&mut self) {
        let list = self.updates.clone();
        let dir = config::config_dir();
        let io = self.persist_io.clone();
        let err = self.persist_err.clone();
        // ci-guard: allow-raw-io feed updates save, not on PersistService yet
        std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            match io.lock() {
                Ok(_g) => note_persist_err(&err, crate::feed::save(&list), "updates"),
                Err(_) => note_persist_err(&err, Err("persist lock poisoned".into()), "updates"),
            }
        });
        self.persist_idle_key = self.persist_idle_now();
    }
}

pub(super) fn note_persist_err(
    slot: &Mutex<Option<String>>,
    result: Result<(), String>,
    what: &str,
) {
    if let Err(e) = result {
        if let Ok(mut slot) = slot.lock() {
            *slot = Some(format!("Could not save {what}: {e}"));
        }
    }
}
