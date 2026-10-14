//! Shared cabin test helpers (rearch stage 4).
//!
//! Temp config homes, wait helpers, and QuietCabin live here so feature test
//! splits (stage 5) do not re-copy them. Declared from `tests.rs` via `#[path]`.

// Loaded as a child of `tests` via `#[path]`, so Cabin lives two modules up.
use super::super::Cabin;
use std::sync::Arc;

/// Restore one env var on drop.
pub(super) struct RestoreEnv {
    key: &'static str,
    prev: Option<String>,
}

impl RestoreEnv {
    pub(super) fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let prev = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, prev }
    }
}

impl Drop for RestoreEnv {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

pub(super) fn restore_env(key: &str, prev: Option<std::ffi::OsString>) {
    match prev {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

pub(super) fn isolated_cabin(label: &str) -> (std::path::PathBuf, Cabin) {
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    (root, Cabin::quiet_for_test())
}

pub(super) fn release_isolated(root: &std::path::Path, cabin: Cabin) {
    let io = cabin.persist_io.clone();
    drop(cabin);
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(3) {
        if io.try_lock().is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(30));
            if io.try_lock().is_ok() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(root);
    std::env::remove_var("GROKHUB_CONFIG");
}

/// Isolated cabin. Skips the grok installer and the update probe. Restores env on drop.
pub(super) struct QuietCabin {
    pub(super) cabin: Cabin,
    pub(super) boot: QuietBoot,
    _lock: std::sync::MutexGuard<'static, ()>,
}

pub(super) struct QuietBoot {
    pub(super) root: std::path::PathBuf,
    prev_config: Option<std::ffi::OsString>,
    prev_grok: Option<std::ffi::OsString>,
    prev_quiet: Option<std::ffi::OsString>,
    prev_tray: Option<std::ffi::OsString>,
    restored: bool,
}

impl QuietBoot {
    fn apply(label: &str) -> Self {
        let prev_config = std::env::var_os("GROKHUB_CONFIG");
        let prev_grok = std::env::var_os("GROKHUB_GROK");
        let prev_quiet = std::env::var_os("GROKHUB_QUIET_BOOT");
        let prev_tray = std::env::var_os("GROKHUB_TRAY");
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        std::env::set_var("GROKHUB_GROK", "/no/such/grok-binary-xyz");
        std::env::set_var("GROKHUB_QUIET_BOOT", "1");
        std::env::set_var("GROKHUB_TRAY", "0");
        Self {
            root,
            prev_config,
            prev_grok,
            prev_quiet,
            prev_tray,
            restored: false,
        }
    }

    fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        restore_env("GROKHUB_CONFIG", self.prev_config.take());
        restore_env("GROKHUB_GROK", self.prev_grok.take());
        restore_env("GROKHUB_QUIET_BOOT", self.prev_quiet.take());
        restore_env("GROKHUB_TRAY", self.prev_tray.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Drop for QuietBoot {
    fn drop(&mut self) {
        self.restore();
    }
}

impl QuietCabin {
    pub(super) fn boot(label: &str) -> Self {
        let lock = crate::config::hold_test_config();
        let boot = QuietBoot::apply(label);
        let mut cabin = Cabin::quiet_for_test();
        // `Cabin::new` loaded this isolated config and opened one chat. Keep that
        // home without the installer, tray, hotkey, or update probe.
        let mut cfg = crate::config::load();
        if cfg.device_name.trim().is_empty() {
            cfg.device_name = crate::config::default_device_name();
            let _ = crate::config::save(&cfg);
        }
        crate::config::ensure_memory_seeds();
        cabin.mem_name = "SOUL.md".into();
        cabin.mem_body = crate::config::read_memory(&cabin.mem_name);
        cabin.mem_cache_at = [crate::config::memory_updated_at("SOUL.md"), 0, 0];
        cabin.mem_cache_body = [cabin.mem_body.clone(), String::new(), String::new()];
        let mut threads = crate::threads::load();
        if threads.is_empty() {
            let mut thread = crate::threads::ChatThread::new("Chat", false);
            thread.messages = std::sync::Arc::new(crate::config::load_chat());
            threads.push(thread);
        }
        cabin.messages = threads
            .first()
            .map(|t| t.messages.clone())
            .unwrap_or_else(|| std::sync::Arc::new(Vec::new()));
        cabin.threads = threads;
        cabin.thread_idx = 0;
        cabin.session_mode = grokhub_core::wire::SessionMode::parse(&cfg.session_mode)
            .unwrap_or(grokhub_core::wire::SessionMode::Chat);
        cabin.permission_mode = grokhub_core::wire::PermissionMode::parse(&cfg.permission_mode)
            .unwrap_or(grokhub_core::wire::PermissionMode::Ask);
        cabin.cfg = cfg.clone();
        if let Ok(mut slot) = cabin.cfg_slot.lock() {
            slot.cfg = cfg;
        }
        Self {
            cabin,
            boot,
            _lock: lock,
        }
    }

    /// Let background config writes finish before the config dir changes.
    pub(super) fn settle(&self) {
        let io = self.cabin.persist_io.clone();
        std::thread::sleep(std::time::Duration::from_millis(40));
        for _ in 0..8 {
            drop(io.lock().ok());
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
}

impl Drop for QuietCabin {
    fn drop(&mut self) {
        self.settle();
        // The config lock field is still held until this Drop returns.
        self.boot.restore();
    }
}

/// Deterministic wait helper (study: `wait_until`). Prefer this over raw sleeps.
pub(super) fn wait_until(label: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..80 {
        if ready() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("{label}");
}

/// Alias kept for existing call sites.
pub(super) fn wait_for(label: &str, ready: impl FnMut() -> bool) {
    wait_until(label, ready);
}

pub(super) struct IsolatedConfig {
    prev: Option<std::ffi::OsString>,
    pub(super) root: std::path::PathBuf,
}

impl IsolatedConfig {
    pub(super) fn arm(label: &str) -> Self {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("config root");
        let prev = std::env::var_os("GROKHUB_CONFIG");
        std::env::set_var("GROKHUB_CONFIG", &root);
        Self { prev, root }
    }
}

impl Drop for IsolatedConfig {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
            None => std::env::remove_var("GROKHUB_CONFIG"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn wait_file_has(path: &std::path::Path, needle: &str) -> String {
    let start = std::time::Instant::now();
    loop {
        if let Ok(body) = std::fs::read_to_string(path) {
            if body.contains(needle) {
                return body;
            }
        }
        if start.elapsed() > std::time::Duration::from_secs(5) {
            panic!("{} never contained {needle}", path.display());
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

pub(super) fn wait_tree_contains(root: &std::path::Path, needle: &str) {
    let start = std::time::Instant::now();
    loop {
        if let Ok(rd) = std::fs::read_dir(root) {
            for ent in rd.filter_map(|e| e.ok()) {
                let path = ent.path();
                if !path.is_file() {
                    continue;
                }
                if std::fs::read_to_string(&path)
                    .ok()
                    .is_some_and(|body| body.contains(needle))
                {
                    return;
                }
            }
        }
        if start.elapsed() > std::time::Duration::from_secs(5) {
            panic!("{} never contained {needle}", root.display());
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

pub(super) fn join_persist(io: &Arc<std::sync::Mutex<()>>) {
    drop(io.lock().unwrap_or_else(|e| e.into_inner()));
}

#[test]
fn wait_until_returns_when_ready() {
    let mut n = 0;
    wait_until("counter should reach 2", || {
        n += 1;
        n >= 2
    });
    assert_eq!(n, 2);
}

#[test]
fn quiet_cabin_boots_an_isolated_home() {
    let quiet = QuietCabin::boot("test-support-home");
    assert!(
        quiet.boot.root.exists(),
        "QuietCabin must create its config root"
    );
    assert!(!quiet.cabin.running, "quiet boot must not start a run");
    assert_eq!(
        std::env::var("GROKHUB_QUIET_BOOT").as_deref(),
        Ok("1"),
        "quiet boot env must be set while QuietCabin lives"
    );
    let root = quiet.boot.root.clone();
    drop(quiet);
    assert!(
        !root.exists(),
        "QuietCabin drop must remove the isolated config root"
    );
}
