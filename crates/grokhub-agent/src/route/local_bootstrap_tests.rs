//! L2: bootstrap failures surface a reason and pause the route, while the
//! files on disk still read as installed.

use super::local;
use super::local_sidecar::stop_global;

fn llama_server_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

#[test]
fn bootstrap_error_surfaces_when_sidecar_wont_start() {
    let dir = std::env::temp_dir().join(format!("gh-l2-boot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).expect("bin dir");
    std::fs::create_dir_all(dir.join("models")).expect("models dir");
    std::fs::write(dir.join("bin").join(llama_server_name()), b"not-a-server").expect("binary");
    std::fs::write(dir.join("models").join("tiny.gguf"), b"gguf").expect("gguf");

    let prev = std::env::var_os("GROKHUB_CONFIG");
    unsafe { std::env::set_var("GROKHUB_CONFIG", dir.as_os_str()) };
    stop_global();
    local::set_enabled(true);

    assert!(
        super::local_sidecar::assets_present(&dir),
        "binary and gguf on disk"
    );
    assert!(
        local::installed(),
        "assets present before first boot attempt"
    );

    assert!(local::runtime().is_none());
    let reason = local::bootstrap_error().expect("bootstrap reason");
    assert!(!reason.is_empty(), "reason names the failure: {reason}");
    // The files are still there, as the models page says: the route pauses
    // instead, and a local call says why rather than "isn't installed".
    assert!(
        local::installed(),
        "installed stays true after a failed start"
    );
    assert!(!local::available(), "the route waits out the retry pause");
    assert_eq!(
        local::no_runtime_msg(),
        format!(
            "{}{}. It tries again on your next message.",
            local::START_FAILED_HEAD,
            reason.trim().trim_end_matches('.')
        )
    );
    // Background work waits; a person's turn is the retry.
    local::retry_for_turn("background:summarize");
    assert!(local::start_failed_recently());
    local::retry_for_turn("chat:default");
    assert!(!local::start_failed_recently());
    assert!(local::available(), "a person's turn starts it again");
    assert!(local::runtime().is_none());
    assert_eq!(local::bootstrap_error().as_deref(), Some(reason.as_str()));
    assert_eq!(
        local::start_failure().as_deref(),
        Some(reason.as_str()),
        "the self-check reads the same reason"
    );
    // One failed start no longer keeps the model away for the session: past
    // the retry pause it reads as set up again and the next turn retries.
    local::age_start_error(local::START_RETRY_SECS);
    assert!(local::available(), "a retry is due after the pause");
    assert!(
        local::start_failure().is_some(),
        "the reason stays readable"
    );
    local::clear_start_error();
    assert!(local::start_failure().is_none());

    local::set_enabled(false);
    stop_global();
    if let Some(p) = prev {
        unsafe { std::env::set_var("GROKHUB_CONFIG", p) };
    } else {
        unsafe { std::env::remove_var("GROKHUB_CONFIG") };
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_normal_launch_finds_the_runtime_without_grokhub_config_set() {
    let dir = std::env::temp_dir().join(format!("gh-l2-noenv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).expect("bin dir");
    std::fs::create_dir_all(dir.join("models")).expect("models dir");
    std::fs::write(dir.join("bin").join(llama_server_name()), b"x").expect("binary");
    std::fs::write(dir.join("models").join("tiny.gguf"), b"gguf").expect("gguf");
    let prev = std::env::var_os("GROKHUB_CONFIG");
    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
    // The cabin's config folder, as the platform default resolves it.
    let pin = crate::perm::ConfigGuard::set(&dir);
    stop_global();
    assert!(
        local::installed(),
        "the route reads the cabin's config folder, not only GROKHUB_CONFIG"
    );
    drop(pin);
    if let Some(p) = prev {
        unsafe { std::env::set_var("GROKHUB_CONFIG", p) };
    }
    let _ = std::fs::remove_dir_all(&dir);
}
