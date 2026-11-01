//! L2: bootstrap failures surface a reason and clear `installed()`.

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
    assert!(!local::installed(), "installed false once bootstrap failed");

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
