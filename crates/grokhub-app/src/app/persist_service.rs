//! Single ordered worker for threads / projects / settings disk writes.
//! Secrets and OAuth refresh stay on their own spawn + `persist_io` (Stage 7).

use super::*;
use std::path::PathBuf;

enum PersistMsg {
    FullSnap {
        dir: PathBuf,
        snap: PersistSnap,
        gen: u64,
        done: Option<mpsc::Sender<()>>,
    },
    Settings {
        dir: PathBuf,
        slot: Arc<Mutex<CfgSlot>>,
        gen: u64,
        err: Arc<Mutex<Option<String>>>,
    },
    Projects {
        nodes: Vec<ProjectNode>,
    },
    WindowCfg {
        cfg: AppConfig,
    },
    Flush(mpsc::Sender<()>),
}

/// Owns the one background persist worker. Cabin schedules work with `submit_*`
/// instead of spawning a new thread per write.
#[derive(Clone)]
pub(super) struct PersistService {
    tx: mpsc::Sender<PersistMsg>,
}

impl PersistService {
    pub(super) fn start(io: Arc<Mutex<()>>, mark: Arc<Mutex<PersistMark>>) -> Self {
        let (tx, rx) = mpsc::channel();
        // The only persist spawn for threads/projects/settings (Stage 6).
        std::thread::spawn(move || persist_service_worker(rx, io, mark));
        Self { tx }
    }

    pub(super) fn submit_full(
        &self,
        dir: PathBuf,
        snap: PersistSnap,
        gen: u64,
        done: Option<mpsc::Sender<()>>,
    ) {
        let _ = self.tx.send(PersistMsg::FullSnap {
            dir,
            snap,
            gen,
            done,
        });
    }

    pub(super) fn submit_settings(
        &self,
        dir: PathBuf,
        slot: Arc<Mutex<CfgSlot>>,
        gen: u64,
        err: Arc<Mutex<Option<String>>>,
    ) {
        let _ = self.tx.send(PersistMsg::Settings {
            dir,
            slot,
            gen,
            err,
        });
    }

    pub(super) fn submit_projects(&self, nodes: Vec<ProjectNode>) {
        let _ = self.tx.send(PersistMsg::Projects { nodes });
    }

    pub(super) fn submit_window_cfg(&self, cfg: AppConfig) {
        let _ = self.tx.send(PersistMsg::WindowCfg { cfg });
    }

    /// Drain every message queued before this call. Used by tests and `on_exit`.
    pub(super) fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(PersistMsg::Flush(tx)).is_ok() {
            let _ = rx.recv();
        }
    }
}

fn persist_service_worker(
    rx: mpsc::Receiver<PersistMsg>,
    io: Arc<Mutex<()>>,
    mark: Arc<Mutex<PersistMark>>,
) {
    while let Ok(msg) = rx.recv() {
        match msg {
            PersistMsg::FullSnap {
                dir,
                snap,
                gen,
                done,
            } => {
                let _pin = pin_scheduled_dir(dir.clone());
                if let Ok(_g) = io.lock() {
                    write_persist_disk_in_order(&dir, &snap, gen, &mark);
                }
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
                    continue;
                };
                let cfg = match slot.lock() {
                    Ok(g) => cfg_if_current(&g, gen),
                    Err(_) => continue,
                };
                if let Some(cfg) = cfg {
                    note_persist_err(&err, config::save_in(&dir, &cfg), "settings");
                }
            }
            PersistMsg::Projects { nodes } => {
                if let Ok(_g) = io.lock() {
                    let _ = crate::store::save_projects(&nodes);
                }
            }
            PersistMsg::WindowCfg { cfg } => {
                if let Ok(_g) = io.lock() {
                    let _ = config::save(&cfg);
                }
            }
            PersistMsg::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}
