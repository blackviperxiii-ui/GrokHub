//! Single ordered worker for threads / projects / settings disk writes.
//! Secrets and OAuth refresh stay on their own spawn + `persist_io` (Stage 7).

use super::*;
use std::path::PathBuf;

enum PersistMsg {
    FullSnap {
        dir: PathBuf,
        snap: Box<PersistSnap>,
        gen: u64,
        done: Option<mpsc::Sender<()>>,
        err: Arc<Mutex<Option<String>>>,
    },
    Settings {
        dir: PathBuf,
        slot: Arc<Mutex<CfgSlot>>,
        gen: u64,
        err: Arc<Mutex<Option<String>>>,
    },
    Projects {
        dir: PathBuf,
        nodes: Vec<ProjectNode>,
        err: Arc<Mutex<Option<String>>>,
    },
    WindowCfg {
        dir: PathBuf,
        cfg: Box<AppConfig>,
        err: Arc<Mutex<Option<String>>>,
    },
    Flush(mpsc::Sender<()>),
}

/// Owns the one background persist worker. Cabin schedules work with `submit_*`
/// instead of spawning a new thread per write.
#[derive(Clone)]
pub(super) struct PersistService {
    tx: mpsc::Sender<PersistMsg>,
    // Kept so a dead worker still lets `send` write the job on the caller.
    io: Arc<Mutex<()>>,
    mark: Arc<Mutex<PersistMark>>,
}

impl PersistService {
    pub(super) fn start(io: Arc<Mutex<()>>, mark: Arc<Mutex<PersistMark>>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (wio, wmark) = (io.clone(), mark.clone());
        // The only persist spawn for threads/projects/settings (Stage 6).
        std::thread::spawn(move || persist_service_worker(rx, wio, wmark));
        Self { tx, io, mark }
    }

    /// Queue a job; if the worker is gone, write it directly so data is not lost.
    fn send(&self, msg: PersistMsg) {
        if let Err(mpsc::SendError(msg)) = self.tx.send(msg) {
            run_persist_msg(msg, &self.io, &self.mark);
        }
    }

    pub(super) fn submit_full(
        &self,
        dir: PathBuf,
        snap: PersistSnap,
        gen: u64,
        done: Option<mpsc::Sender<()>>,
        err: Arc<Mutex<Option<String>>>,
    ) {
        self.send(PersistMsg::FullSnap {
            dir,
            snap: Box::new(snap),
            gen,
            done,
            err,
        });
    }

    pub(super) fn submit_settings(
        &self,
        dir: PathBuf,
        slot: Arc<Mutex<CfgSlot>>,
        gen: u64,
        err: Arc<Mutex<Option<String>>>,
    ) {
        self.send(PersistMsg::Settings {
            dir,
            slot,
            gen,
            err,
        });
    }

    pub(super) fn submit_projects(
        &self,
        dir: PathBuf,
        nodes: Vec<ProjectNode>,
        err: Arc<Mutex<Option<String>>>,
    ) {
        self.send(PersistMsg::Projects { dir, nodes, err });
    }

    pub(super) fn submit_window_cfg(
        &self,
        dir: PathBuf,
        cfg: AppConfig,
        err: Arc<Mutex<Option<String>>>,
    ) {
        self.send(PersistMsg::WindowCfg {
            dir,
            cfg: Box::new(cfg),
            err,
        });
    }

    /// Drain every message queued before this call. Used by tests and `on_exit`.
    pub(super) fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(PersistMsg::Flush(tx)).is_ok() {
            let _ = rx.recv();
        }
    }
}

fn persist_msg_err(msg: &PersistMsg) -> Option<Arc<Mutex<Option<String>>>> {
    match msg {
        PersistMsg::FullSnap { err, .. }
        | PersistMsg::Settings { err, .. }
        | PersistMsg::Projects { err, .. }
        | PersistMsg::WindowCfg { err, .. } => Some(err.clone()),
        PersistMsg::Flush(_) => None,
    }
}

fn persist_msg_what(msg: &PersistMsg) -> &'static str {
    match msg {
        PersistMsg::FullSnap { .. } => "chats",
        PersistMsg::Settings { .. } => "settings",
        PersistMsg::Projects { .. } => "projects",
        PersistMsg::WindowCfg { .. } => "window",
        PersistMsg::Flush(_) => "persist",
    }
}

fn persist_service_worker(
    rx: mpsc::Receiver<PersistMsg>,
    io: Arc<Mutex<()>>,
    mark: Arc<Mutex<PersistMark>>,
) {
    while let Ok(msg) = rx.recv() {
        run_persist_msg(msg, &io, &mark);
    }
}

/// One panicked write must not kill the worker (or the caller on fallback).
fn run_persist_msg(msg: PersistMsg, io: &Arc<Mutex<()>>, mark: &Arc<Mutex<PersistMark>>) {
    let err = persist_msg_err(&msg);
    let what = persist_msg_what(&msg);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_persist_msg(msg, io, mark);
    }));
    if result.is_err() {
        if let Some(err) = err {
            note_persist_err(&err, Err("persist worker panicked".into()), what);
        }
    }
}

fn handle_persist_msg(msg: PersistMsg, io: &Arc<Mutex<()>>, mark: &Arc<Mutex<PersistMark>>) {
    #[cfg(test)]
    maybe_panic_for_test();
    match msg {
        PersistMsg::FullSnap {
            dir,
            snap,
            gen,
            done,
            err,
        } => {
            let _pin = pin_scheduled_dir(dir.clone());
            match io.lock() {
                Ok(_g) => {
                    if let Err(e) = write_persist_disk_in_order(&dir, &snap, gen, mark) {
                        note_persist_err(&err, Err(e), "chats");
                    }
                }
                Err(_) => {
                    note_persist_err(&err, Err("persist lock poisoned".into()), "chats");
                }
            }
            // Always release waiters; `poll_persist_err` clears the idle key on failure
            // so the next idle tick retries instead of treating disk as clean.
            if let Some(done) = done {
                let _ = done.send(());
            }
        }
        PersistMsg::Settings {
            dir,
            slot,
            gen,
            err,
        } => {
            let _pin = pin_scheduled_dir(dir.clone());
            let Ok(_disk) = io.lock() else {
                note_persist_err(&err, Err("persist lock poisoned".into()), "settings");
                return;
            };
            let cfg = match slot.lock() {
                Ok(g) => cfg_if_current(&g, gen),
                Err(_) => return,
            };
            if let Some(cfg) = cfg {
                note_persist_err(&err, config::save_in(&dir, &cfg), "settings");
            }
        }
        PersistMsg::Projects { dir, nodes, err } => {
            let _pin = pin_scheduled_dir(dir.clone());
            match io.lock() {
                Ok(_g) => {
                    // Write into the dir the job was queued for, not whatever
                    // `config_dir()` says by the time the worker runs.
                    let saved = serde_json::to_string(&nodes)
                        .map_err(|e| e.to_string())
                        .and_then(|body| {
                            config::atomic_write(&dir.join("projects.json"), body.as_bytes())
                        });
                    note_persist_err(&err, saved, "projects");
                }
                Err(_) => {
                    note_persist_err(&err, Err("persist lock poisoned".into()), "projects");
                }
            }
        }
        PersistMsg::WindowCfg { dir, cfg, err } => {
            let _pin = pin_scheduled_dir(dir.clone());
            match io.lock() {
                Ok(_g) => {
                    note_persist_err(&err, config::save_in(&dir, &cfg), "window");
                }
                Err(_) => {
                    note_persist_err(&err, Err("persist lock poisoned".into()), "window");
                }
            }
        }
        PersistMsg::Flush(done) => {
            let _ = done.send(());
        }
    }
}

#[cfg(test)]
static PANIC_NEXT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Next PersistService job panics once (worker stays up for the following write).
#[cfg(test)]
pub(super) fn persist_panic_next() {
    PANIC_NEXT.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
fn maybe_panic_for_test() {
    if PANIC_NEXT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        panic!("persist test panic");
    }
}

#[cfg(test)]
#[test]
fn projects_job_writes_into_its_own_dir() {
    let dir = std::env::temp_dir().join(format!("gh-persist-proj-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let svc = PersistService::start(
        Arc::new(Mutex::new(())),
        Arc::new(Mutex::new(PersistMark::default())),
    );
    let err = Arc::new(Mutex::new(None));
    svc.submit_projects(dir.clone(), Vec::new(), err.clone());
    svc.flush();
    assert!(dir.join("projects.json").exists());
    assert!(err.lock().unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A service whose worker already exited, to exercise the direct-write fallback.
#[cfg(test)]
pub(super) fn dead_persist_service(
    io: Arc<Mutex<()>>,
    mark: Arc<Mutex<PersistMark>>,
) -> PersistService {
    let (tx, rx) = mpsc::channel();
    drop(rx);
    PersistService { tx, io, mark }
}
