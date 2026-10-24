//! Shared cabin test helpers (rearch stage 4).
//!
//! Temp config homes, wait helpers, and QuietCabin live here so feature test
//! splits (stage 5) do not re-copy them. Declared from `tests.rs` via `#[path]`.

// Loaded as a child of `tests` via `#[path]`, so Cabin lives two modules up.
use super::super::{Cabin, PersistService};

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
    cabin.persist_svc.flush();
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
        self.cabin.persist_svc.flush();
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

/// Drain the PersistService queue so disk catches up with scheduled writes.
pub(super) fn join_persist(svc: &PersistService) {
    svc.flush();
}

/// An Auto (unattended) cabin with one native chat. Native `/bg` runs talk to
/// the in-process `BgFake` model under test: one write, then "bg finished".
#[cfg(unix)]
pub(super) fn bg_cabin(label: &str) -> (std::path::PathBuf, Cabin) {
    let (root, mut cabin) = isolated_cabin(label);
    let _ = std::fs::create_dir_all(&root);
    cabin.permission_mode = grokhub_core::wire::PermissionMode::Auto;
    cabin.session_mode = grokhub_core::wire::SessionMode::Chat;
    cabin.cfg.project_dir = root.display().to_string();
    let mut thread = crate::threads::ChatThread::new("Chat", false);
    thread.native = true;
    thread.grok_cwd = Some(root.display().to_string());
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    (root, cabin)
}

/// A chat reply in flight on the open tab. The test holds the sender.
#[cfg(unix)]
pub(super) fn live_reply(
    cabin: &mut Cabin,
    ask: &str,
    said: &str,
) -> std::sync::mpsc::Sender<grokhub_core::wire::GrokPEvent> {
    cabin.live_mut().push(("user".into(), ask.into()));
    cabin.live_mut().push(("assistant".into(), said.into()));
    let (tx, rx) = std::sync::mpsc::channel();
    cabin.grok_p_rx = Some(rx);
    cabin.running = true;
    cabin.chat_job_thread = Some(cabin.visible_thread_id());
    tx
}

/// Poll the live turn and the background runs until `done` or `secs` pass.
#[cfg(unix)]
pub(super) fn poll_until(cabin: &mut Cabin, secs: u64, done: impl Fn(&Cabin) -> bool) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(secs) {
        cabin.poll_single();
        cabin.poll_bg_runs();
        if done(cabin) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

#[cfg(unix)]
pub(super) fn end_bg_test(root: std::path::PathBuf, mut cabin: Cabin) {
    cabin.stop_all_bg_runs();
    cabin.halt_in_flight();
    cabin.kill_bg_runs();
    release_isolated(&root, cabin);
}

pub(super) fn test_automation(
    id: &str,
    name: &str,
    instructions: &str,
) -> grokhub_core::Automation {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": name,
        "schedule": "daily",
        "time": "09:00",
        "instructions": instructions,
    }))
    .expect("automation")
}

/// One config root for both this thread (`TestConfigDir`) and the skill-write
/// threads (`GROKHUB_CONFIG`), with one skill saved the way the cabin saves it.
pub(super) fn pin_skill_config(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    (crate::config::TestConfigDir::set(root.clone()), root)
}

pub(super) fn last_chat_text(cabin: &Cabin) -> String {
    cabin
        .messages
        .last()
        .map(|m| m.1.clone())
        .unwrap_or_default()
}

// Spike-4b: Settings → Permissions paints the scope rows under "What GrokHub
// can read". Texts with their centers, and which rects are filled.
pub(super) struct ScopePaint {
    pub(super) texts: Vec<(String, eframe::egui::Pos2)>,
    pub(super) filled: Vec<eframe::egui::Rect>,
}

impl ScopePaint {
    pub(super) fn has(&self, want: &str) -> bool {
        self.texts.iter().any(|t| t.0 == want)
    }
    pub(super) fn at(&self, want: &str) -> eframe::egui::Pos2 {
        self.texts
            .iter()
            .find(|t| t.0 == want)
            .unwrap_or_else(|| {
                panic!(
                    "{want} not painted: {:?}",
                    self.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
                )
            })
            .1
    }
    /// The pill (`Allow` / `Revoke`) on the same row as `title`.
    pub(super) fn pill_by(&self, title: &str, pill: &str) -> eframe::egui::Pos2 {
        let y = self.at(title).y;
        self.texts
            .iter()
            .filter(|t| t.0 == pill)
            .min_by(|a, b| (a.1.y - y).abs().total_cmp(&(b.1.y - y).abs()))
            .map(|t| t.1)
            .unwrap_or_else(|| panic!("no {pill} by {title}"))
    }
}

pub(super) fn paint_permissions(
    ctx: &eframe::egui::Context,
    cabin: &mut Cabin,
    events: Vec<eframe::egui::Event>,
) -> ScopePaint {
    fn walk(shape: &eframe::egui::Shape, p: &mut ScopePaint) {
        match shape {
            eframe::egui::Shape::Text(t) => p.texts.push((
                t.galley.text().to_string(),
                t.pos + t.galley.rect.center().to_vec2(),
            )),
            eframe::egui::Shape::Rect(r) if r.fill == crate::theme::fg() => p.filled.push(r.rect),
            eframe::egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, p)),
            _ => {}
        }
    }
    let input = eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(900.0, 2400.0),
        )),
        events,
        ..Default::default()
    };
    let out = crate::theme::test_pass(ctx, input, |ui| {
        eframe::egui::CentralPanel::default().show(ui, |ui| cabin.ui_permission_editor(ui));
    });
    let mut p = ScopePaint {
        texts: Vec::new(),
        filled: Vec::new(),
    };
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut p);
    }
    p
}

pub(super) fn click_at(ctx: &eframe::egui::Context, cabin: &mut Cabin, at: eframe::egui::Pos2) {
    let press = |pressed| eframe::egui::Event::PointerButton {
        pos: at,
        button: eframe::egui::PointerButton::Primary,
        pressed,
        modifiers: eframe::egui::Modifiers::NONE,
    };
    let _ = paint_permissions(ctx, cabin, vec![eframe::egui::Event::PointerMoved(at)]);
    let _ = paint_permissions(ctx, cabin, vec![press(true)]);
    let _ = paint_permissions(ctx, cabin, vec![press(false)]);
}

pub(super) fn key(key: eframe::egui::Key) -> eframe::egui::Event {
    eframe::egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: eframe::egui::Modifiers::NONE,
    }
}

pub(super) fn health_job(id: &str) -> grokhub_core::Automation {
    grokhub_core::Automation {
        id: id.into(),
        name: "Board summary".into(),
        schedule: "daily".into(),
        time: "09:00".into(),
        times: Vec::new(),
        instructions: "summarize the board".into(),
        heartbeat_every_min: 0,
        check_command: String::new(),
        enabled: true,
        last_run: None,
        next_run: None,
        run_count: 0,
        health: Default::default(),
    }
}

pub(super) fn with_fonts_ui(mut add: impl FnMut(&mut eframe::egui::Ui)) {
    let ctx = eframe::egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        eframe::egui::CentralPanel::default().show(ui, |ui| add(ui));
    });
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
