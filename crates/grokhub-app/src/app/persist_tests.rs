//! Persistence tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

#[test]
fn persist_does_not_hold_hub_lock_across_disk() {
    let src = cabin_src();
    let persist = format!(
        "{}{}",
        fn_src(&src, "persist"),
        fn_src(&src, "persist_snap")
    );
    assert!(
        persist.contains("self.hub.clone()") && !persist.contains("state_for_disk"),
        "persist must not clone hub snapshot/last_frame on the UI thread: {persist}"
    );
    assert!(
        !persist.contains("if let Ok(st) = self.hub.lock()"),
        "persist must not hold hub.lock() across save_hub_state: {persist}"
    );
    let write = fn_src(&src, "write_persist_disk");
    let lock = write.find("hub.lock").expect("worker hub lock");
    let save = write.find("save_hub_state").expect("save_hub_state");
    assert!(
        lock < save && write.contains("state_for_disk(&st)"),
        "persist worker must clone hub state then drop the lock before hub-state.json: {write}"
    );
}

#[test]
fn periodic_persist_leaves_the_ui_thread() {
    let src = cabin_src();
    let beat = src
        .split("fn tick_heartbeat")
        .nth(1)
        .and_then(|s| s.split("fn tick_anticipate").next())
        .expect("tick_heartbeat");
    assert!(
        beat.contains("tick_housekeep(") && !beat.contains("self.persist()"),
        "2s housekeep persist must not block the cabin: {beat}"
    );
    let house = include_str!("heartbeat_gate.rs");
    assert!(
        house.contains("fn tick_housekeep(")
            && house.contains("persist_bg(")
            && house.contains("tick_print_alerts(")
            && !house.contains("self.persist()"),
        "tick_housekeep must persist off-thread and poll print alerts: {house}"
    );
    let paint = src
        .split("self.flush_window(ctx)")
        .nth(1)
        .and_then(|s| s.split("next_heartbeat_wait_ms").next())
        .expect("update persist");
    assert!(
        paint.contains("persist_bg(") && !paint.contains("self.persist()"),
        "2s paint persist must not block the cabin: {paint}"
    );
    let apply = src
        .split("fn apply_saved_geom(")
        .nth(1)
        .and_then(|s| s.split("fn capture_window(").next())
        .expect("apply_saved_geom");
    assert!(
        apply.contains("InnerSize") && apply.contains("OuterPosition"),
        "launch must apply the remembered inner size and outer position: {apply}"
    );
    let capture = src
        .split("fn capture_window(")
        .nth(1)
        .and_then(|s| s.split("fn flush_window(").next())
        .expect("capture_window");
    assert!(
        capture.contains("geom_can_remember") && capture.contains("apply_saved_geom"),
        "first frames must restore size/position, not clobber app.json: {capture}"
    );
    let show = src
        .split("fn show_from_tray(")
        .nth(1)
        .and_then(|s| s.split("fn poll_tray(").next())
        .expect("show_from_tray");
    assert!(
        show.contains("apply_saved_geom"),
        "Show cabin must restore size and position: {show}"
    );
    let exit = src
        .split("fn on_exit(")
        .nth(1)
        .and_then(|s| s.split("fn logic(").next())
        .expect("on_exit");
    assert!(
        exit.contains("config::save") && exit.contains("cfg.window")
            || exit.contains("config::save(&cfg)"),
        "SIGTERM must write the remembered window: {exit}"
    );
    let flush = fn_src(&src, "flush_window");
    assert!(
        flush.contains("submit_window_cfg")
            && flush.contains("persist_svc")
            && !flush.contains("thread::spawn"),
        "window geom must not freeze the cabin writing app.json: {flush}"
    );
    let bg = fn_src(&src, "persist_bg");
    assert!(
        bg.contains("submit_full") && bg.contains("persist_svc") && !bg.contains("thread::spawn"),
        "persist_bg must leave the UI thread via PersistService: {bg}"
    );
    let svc = include_str!("persist_service.rs").replace("\r\n", "\n");
    assert!(
        svc.contains("thread::spawn") && svc.contains("write_persist_disk_in_order"),
        "PersistService must own the one worker that writes threads.json: {svc}"
    );
    assert!(
        bg.contains("persist_idle_key") && bg.contains("return;"),
        "idle 2s persist must not clone every thread on the UI thread: {bg}"
    );
    let snap = bg.find("persist_snap").expect("persist_snap");
    assert!(
        bg[..snap].contains("self.running") && bg[..snap].contains("return"),
        "a growing stream must not clone every thread to persist an 8MB bubble: {bg}"
    );
    assert!(
        !bg[..snap].contains("geom_dirty"),
        "window drag must not clone every thread — flush_window owns geom: {bg}"
    );
    let idle_key = src
        .split("fn persist_idle_now(")
        .nth(1)
        .and_then(|s| s.split("fn persist_bg(").next())
        .expect("persist idle key");
    assert!(
            !idle_key.contains("projects_dirty"),
            "folder click must not clone every thread twice — persist_idle_key must ignore the dirty flag: {idle_key}"
        );
}

#[test]
fn persist_keeps_threads_in_the_dir_it_was_scheduled_with() {
    let _g = crate::config::hold_test_config();
    let first = crate::config::test_config_root("persist-pin-a");
    let second = crate::config::test_config_root("persist-pin-b");
    let prev = std::env::var_os("GROKHUB_CONFIG");
    struct Restore {
        prev: Option<std::ffi::OsString>,
        roots: Vec<std::path::PathBuf>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            restore_env("GROKHUB_CONFIG", self.prev.take());
            for root in &self.roots {
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }
    let _restore = Restore {
        prev,
        roots: vec![first.clone(), second.clone()],
    };
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &first);
    let mut cabin = super::Cabin::quiet_for_test();
    let mut thread = crate::threads::ChatThread::new("PinnedLeak", false);
    thread.pinned = true;
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    let io = cabin.persist_io.clone();
    let hold = io.lock().unwrap_or_else(|e| e.into_inner());
    cabin.persist();
    std::env::set_var("GROKHUB_CONFIG", &second);
    drop(hold);
    let scheduled = first.join("threads.json");
    let moved = second.join("threads.json");
    let mut landed = String::new();
    for _ in 0..80 {
        if let Ok(body) = std::fs::read_to_string(&scheduled) {
            if body.contains("PinnedLeak") {
                landed = body;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let leaked = std::fs::read_to_string(&moved).unwrap_or_default();
    assert!(
        landed.contains("PinnedLeak"),
        "scheduled persist must write threads.json into the captured config dir; the later dir has {leaked}"
    );
    assert!(
        !leaked.contains("PinnedLeak"),
        "a later GROKHUB_CONFIG must not receive that threads.json: {leaked}"
    );
}

#[test]
fn poisoned_persist_io_surfaces_error_and_retries_idle() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-poison");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    // Poison the shared mutex the PersistService worker takes before disk I/O.
    let io = cabin.persist_io.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _g = io.lock().unwrap();
        panic!("poison persist_io");
    }));
    cabin.persist_idle_key.clear();
    cabin.persist_bg();
    let start = std::time::Instant::now();
    while cabin.persist_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(3) {
        cabin.poll_persist();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cabin.poll_persist_err();
    assert!(
        cabin.status.contains("persist lock poisoned") || cabin.status.contains("Could not save"),
        "poisoned lock must surface via persist_err, got status={status}",
        status = cabin.status
    );
    assert!(
        cabin.persist_idle_key.is_empty(),
        "failed write must clear the idle key so the next tick retries"
    );
    drop(cfg);
}

#[test]
fn persist_worker_survives_a_panicking_job_then_writes() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-panic-survive");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Panic pier", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "before panic".into())]);
    // First job panics inside the worker; catch_unwind must keep the thread up.
    super::persist_panic_next();
    cabin.persist();
    cabin.persist_svc.flush();
    cabin.poll_persist_err();
    assert!(
        cabin.status.contains("persist worker panicked") || cabin.status.contains("Could not save"),
        "panicked job must surface via persist_err, got status={status}",
        status = cabin.status
    );
    // Second job must still land on disk through the same worker.
    cabin.status.clear();
    cabin.persist_idle_key.clear();
    if let Ok(mut slot) = cabin.persist_err.lock() {
        *slot = None;
    }
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "after panic".into())]);
    cabin.persist();
    cabin.persist_svc.flush();
    cabin.poll_persist_err();
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(
        threads.contains("after panic") || threads.contains("Panic pier"),
        "worker must still write after a panicked job: {threads}"
    );
    assert!(
        !cabin.status.contains("persist worker panicked"),
        "successful write must not leave the panic status: {status}",
        status = cabin.status
    );
    drop(cfg);
}

#[test]
fn persist_writes_directly_when_the_worker_is_gone() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-dead-worker");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Dead worker dock", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "no worker".into())]);
    cabin.persist_svc = super::dead_persist_service(
        cabin.persist_io.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default())),
    );
    cabin.persist();
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("Dead worker dock"), "{threads}");
    cabin.poll_persist_err();
    assert!(
        !cabin.status.contains("Could not save"),
        "a successful fallback write must not report failure: {status}",
        status = cabin.status
    );
    // A panic inside the fallback is caught and reported, not propagated.
    super::persist_panic_next();
    cabin.persist();
    cabin.poll_persist_err();
    assert!(
        cabin
            .status
            .starts_with("Could not save chats: persist worker panicked"),
        "got status={status}",
        status = cabin.status
    );
    drop(cfg);
}

#[test]
fn an_older_persist_snapshot_never_overwrites_a_newer_one() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-order");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier light", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    cabin.projects_dirty = true;
    let mut old = cabin.persist_snap();
    old.secrets = Some(cabin.secrets.clone());
    let old_gen = cabin.next_persist_gen();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.projects_dirty = false;
    let new = cabin.persist_snap();
    let new_gen = cabin.next_persist_gen();
    assert_eq!((old_gen, new_gen), (1, 2));
    assert!(old.projects.is_some() && new.projects.is_none());
    assert!(new.secrets.is_none());
    let mark = std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default()));
    // The newer worker takes persist_io first, the older one second.
    let _ = write_persist_disk_in_order(&cfg.root, &new, new_gen, &mark);
    let _ = write_persist_disk_in_order(&cfg.root, &old, old_gen, &mark);
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("\"title\": \"Chat\""), "{threads}");
    assert!(!threads.contains("Pier light"), "{threads}");
    // Only the older snapshot carried projects and secrets, so those still land.
    assert!(crate::store::projects_path().is_file());
    assert!(cfg.root.join("secrets.json").is_file());
    // A second older write of the same generation changes nothing.
    std::fs::remove_file(crate::store::projects_path()).expect("projects");
    let _ = write_persist_disk_in_order(&cfg.root, &old, old_gen, &mark);
    assert!(!crate::store::projects_path().exists());
    let again = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert_eq!(again, threads);
    drop(cfg);
}

#[test]
fn persist_service_queues_out_of_order_full_snaps() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-svc-order");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier light", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    cabin.projects_dirty = true;
    let mut old = cabin.persist_snap();
    old.secrets = Some(cabin.secrets.clone());
    let old_gen = cabin.next_persist_gen();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.projects_dirty = false;
    let new = cabin.persist_snap();
    let new_gen = cabin.next_persist_gen();
    assert_eq!((old_gen, new_gen), (1, 2));
    // Newer gen first, then older — through the actor, not the free fn.
    cabin.persist_svc.submit_full(
        cfg.root.clone(),
        new,
        new_gen,
        None,
        cabin.persist_err.clone(),
    );
    cabin.persist_svc.submit_full(
        cfg.root.clone(),
        old,
        old_gen,
        None,
        cabin.persist_err.clone(),
    );
    join_persist(&cabin.persist_svc);
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("\"title\": \"Chat\""), "{threads}");
    assert!(!threads.contains("Pier light"), "{threads}");
    assert!(crate::store::projects_path().is_file());
    assert!(cfg.root.join("secrets.json").is_file());
    drop(cfg);
}

fn test_tokens(access: &str) -> grokhub_core::XaiOAuthTokens {
    grokhub_core::XaiOAuthTokens {
        access_token: access.into(),
        refresh_token: Some(format!("{access}-refresh")),
        connected_at: 1,
        ..Default::default()
    }
}

fn disk_access_token(root: &std::path::Path) -> String {
    let body = std::fs::read_to_string(root.join("secrets.json")).expect("secrets.json");
    let disk: crate::secrets::Secrets = serde_json::from_str(&body).expect("secrets json");
    disk.oauth.map(|t| t.access_token).unwrap_or_default()
}

#[test]
fn an_older_oauth_refresh_never_overwrites_a_newer_token() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-secrets-order");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    let old = crate::secrets::Secrets {
        oauth: Some(test_tokens("test-old-access")),
        ..Default::default()
    };
    let new = crate::secrets::Secrets {
        oauth: Some(test_tokens("test-new-access")),
        ..Default::default()
    };
    let old_gen = cabin.next_persist_gen();
    let new_gen = cabin.next_persist_gen();
    assert_eq!((old_gen, new_gen), (1, 2));
    // The newer refresh reaches the worker first, the older one second.
    let svc = cabin.persist_svc.clone();
    svc.submit_secrets(cfg.root.clone(), new, new_gen, None);
    svc.submit_secrets(cfg.root.clone(), old.clone(), old_gen, None);
    join_persist(&svc);
    assert_eq!(disk_access_token(&cfg.root), "test-new-access");
    // An older full snapshot still writes the chats but keeps the newer token.
    let mut snap = cabin.persist_snap();
    snap.secrets = Some(old);
    let snap_gen = 1;
    svc.submit_full(
        cfg.root.clone(),
        snap,
        snap_gen,
        None,
        cabin.persist_err.clone(),
    );
    join_persist(&svc);
    assert_eq!(disk_access_token(&cfg.root), "test-new-access");
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("\"title\": \"Chat\""), "{threads}");
    assert!(cabin.persist_err.lock().unwrap().is_none());
    drop(cfg);
}

#[test]
fn oauth_refresh_and_settings_keys_write_through_persist_service() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-secrets-svc");
    let mut cabin = Cabin::quiet_for_test();
    cabin.secrets.oauth = Some(test_tokens("test-refreshed-access"));
    cabin.persist_oauth_secrets();
    join_persist(&cabin.persist_svc);
    assert_eq!(disk_access_token(&cfg.root), "test-refreshed-access");
    cabin.secrets.api_key = "sk-abcdefghijklmnopqrstuv".into();
    cabin.persist_secrets();
    join_persist(&cabin.persist_svc);
    let body = std::fs::read_to_string(cfg.root.join("secrets.json")).expect("secrets.json");
    let disk: crate::secrets::Secrets = serde_json::from_str(&body).expect("secrets json");
    assert_eq!(disk.api_key, "sk-abcdefghijklmnopqrstuv");
    assert_eq!(disk_access_token(&cfg.root), "test-refreshed-access");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(cfg.root.join("secrets.json"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert!(cabin.persist_err.lock().unwrap().is_none());
    drop(cfg);
}

#[test]
fn a_secrets_write_still_lands_when_the_persist_worker_is_gone() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-secrets-dead");
    let svc = dead_persist_service(
        std::sync::Arc::new(std::sync::Mutex::new(())),
        std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default())),
    );
    let secrets = crate::secrets::Secrets {
        oauth: Some(test_tokens("test-direct-access")),
        ..Default::default()
    };
    svc.submit_secrets(cfg.root.clone(), secrets, 1, None);
    assert_eq!(disk_access_token(&cfg.root), "test-direct-access");
    drop(cfg);
}

/// Corrupt settings, keys, Home feed and hub files: each loads defaults, keeps its
/// bytes in a `.corrupt-<ms>` copy, and gets one Home card naming it and the copy.
#[test]
fn corrupt_stores_load_defaults_keep_a_copy_and_post_a_named_card() {
    let _g = config::hold_test_config();
    let root = config::test_config_root("corrupt-stores");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let _pin = config::TestConfigDir::set(root.clone());
    let mut cabin = Cabin::quiet_for_test();
    let _ = grokhub_core::recovered::drain();
    let files = [
        ("app.json", "{\"deviceName\": \"cab", "Settings file"),
        ("secrets.json", "\u{0}\u{1}garbage", "Keys file"),
        ("updates.json", "[{\"id\": 3,", "Home feed"),
        ("hub-state.json", "not json", "Hub pairing file"),
    ];
    for (name, bytes, _) in files {
        std::fs::write(root.join(name), bytes).unwrap();
    }

    let cfg = config::load();
    assert_eq!(
        cfg.device_name,
        config::default_device_name(),
        "settings fell back to defaults"
    );
    assert_eq!(
        crate::secrets::load().api_key,
        "",
        "keys fell back to empty"
    );
    assert!(
        crate::feed::load().is_empty(),
        "the feed fell back to empty"
    );
    assert!(grokhub_core::load_hub_state(&config::hub_state_path()).is_none());
    // Checked before the cards post: posting saves a fresh updates.json on a
    // background thread, which could land before a later check.
    for (name, _, _) in files {
        assert!(!root.join(name).exists(), "{name} was moved aside");
    }
    cabin.poll_recovered_stores();

    for (name, bytes, label) in files {
        let copies: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&format!("{name}.corrupt-")))
            })
            .collect();
        assert_eq!(copies.len(), 1, "{name}: {copies:?}");
        assert_eq!(
            std::fs::read_to_string(&copies[0]).unwrap(),
            bytes,
            "{name}: the copy keeps the bytes"
        );
        let title = format!("{label} was unreadable");
        let cards: Vec<&UpdateCard> = cabin.updates.iter().filter(|c| c.title == title).collect();
        assert_eq!(cards.len(), 1, "{name}: {:?}", cabin.updates);
        let body = format!(
            "Restored defaults, kept a copy at {}.",
            grokhub_core::recovered::home_short(&copies[0])
        );
        assert_eq!(cards[0].body.as_deref(), Some(body.as_str()), "{name}");
    }
    // Draining again posts nothing new.
    let before = cabin.updates.len();
    cabin.poll_recovered_stores();
    assert_eq!(cabin.updates.len(), before);
    let _ = std::fs::remove_dir_all(&root);
}
