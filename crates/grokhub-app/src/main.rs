//! GrokHub native cabin. No Electron. No Tauri.
//!
//! Windows Explorer/Start must not allocate a console. Closing that console
//! kills the cabin. CLI flags attach the parent console when there is one.

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod app;
mod engine_handle;
mod helpers;
mod titlebar;
mod cards;
mod icons;
mod theme;
mod motion;
mod cli;
mod config;
#[cfg(feature = "fx")]
mod fx;
mod desktop;
mod desktop_mcp;
mod github;
mod google_mcp_via_cli;
mod host;
mod markdown;
mod native_mcp;
mod native_plugins;
mod night;
mod loops;
mod card_prefs;
mod feed;
mod recipes;
mod notify;
mod store;
mod oauth;
mod imagine_auth;
mod secrets;
mod self_mcp;
mod skills;
mod threads;
mod tray;
mod window;
mod update;
mod xai;
#[cfg(windows)]
mod win_acl;
#[cfg(windows)]
mod win_audio;
#[cfg(windows)]
mod win_native;

use app::Cabin;
use cli::{parse_args, Launch};
use eframe::egui;
use grokhub_core::{
    doctor_cabin_line, doctor_lines, doctor_ok, hub_kind_from_health,
    DEFAULT_PORT,
};
use std::env;

fn main() {
    // `sudo`'s helper (card 34): answer it and leave before anything else starts.
    let args: Vec<String> = env::args().collect();
    if let Some(socket) = cli::askpass_socket(&args) {
        std::process::exit(grokhub_agent::sudo_pass::client(socket));
    }
    grokhub_core::proc_util::silence_windows_hard_errors();
    // Spike-4b: the learned-tier key lives in the OS keyring (asked lazily, never at start).
    grokhub_agent::harness::use_os_keyring();
    grokhub_agent::route::providers::use_os_vault();
    #[cfg(windows)]
    ensure_windows_home();
    let launch = parse_args(&args);
    match launch {
        Launch::Cabin | Launch::Agent | Launch::McpDesktop | Launch::McpCua | Launch::McpSelf => {}
        Launch::Hub => attach_cli_console(true),
        Launch::Version | Launch::Help | Launch::Doctor | Launch::Update | Launch::Oauth => {
            attach_cli_console(false)
        }
    }
    match launch {
        Launch::Version => {
            println!("{}", update::build_version_line());
        }
        Launch::Help => {
            #[cfg(windows)]
            eprint!(
                "grokhub {} — native cabin\n\n  grokhub           cabin (close stays in the tray)\n  grokhub --agent   cabin in the tray, window hidden\n  grokhub --hub     LAN hub only\n  grokhub --mcp-desktop  desktop tools on stdin (no window)\n  grokhub --mcp-cua      Cua Driver gate on stdin (Linux, spike flag)\n  grokhub --mcp-self  Grok's skill, connection, and automation tools on stdin (no window)\n  grokhub --oauth   xAI device-code (Grok)\n  grokhub --update  when the cabin is newer: GitHub zip or source overlay\n  grokhub --doctor  auth / memory / hub kind\n  grokhub --version\n",
                env!("CARGO_PKG_VERSION")
            );
            #[cfg(not(windows))]
            eprint!(
                "grokhub {} — native cabin\n\n  grokhub           cabin (close stays in the tray)\n  grokhub --agent   cabin in the tray, window hidden\n  grokhub --hub     LAN hub only\n  grokhub --mcp-desktop  desktop tools on stdin (no window)\n  grokhub --mcp-cua      Cua Driver gate on stdin (Linux, spike flag)\n  grokhub --mcp-self  Grok's skill, connection, and automation tools on stdin (no window)\n  grokhub --oauth   xAI device-code (Grok)\n  grokhub --update  when the cabin is newer: git pull + install.sh --user\n  grokhub --doctor  auth / memory / hub kind\n  grokhub --version\n",
                env!("CARGO_PKG_VERSION")
            );
        }
        Launch::Doctor => run_doctor(),
        Launch::Oauth => run_oauth_cli(),
        Launch::Update => run_update_cli(),
        Launch::Hub => run_hub(),
        Launch::McpDesktop => {
            let code = desktop_mcp::run_stdio();
            std::process::exit(code);
        }
        Launch::McpCua => {
            let code = desktop_mcp::run_cua_stdio();
            std::process::exit(code);
        }
        Launch::McpSelf => {
            let code = self_mcp::run_stdio();
            std::process::exit(code);
        }
        Launch::Agent => {
            if let Err(e) = run_cabin(true) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Launch::Cabin => {
            if let Err(e) = run_cabin(false) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
}

fn run_oauth_cli() {
    match oauth::start_device() {
        Ok(start) => {
            println!("Grok OAuth user code: {}", start.user_code);
            println!("{}", start.verification_uri);
            if let Some(u) = &start.verification_uri_complete {
                println!("{u}");
                let _ = oauth::open_browser(u);
            } else {
                let _ = oauth::open_browser(&start.verification_uri);
            }
            match oauth::poll_until_ready(&start.device_code, start.interval) {
                Ok(tokens) => {
                    let mut s = secrets::load();
                    s.oauth = Some(tokens.clone());
                    if let Err(e) = secrets::save(&s) {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                    let who = tokens
                        .name
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("grok");
                    println!("connected {who}");
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn probe_hub_health_body() -> Option<String> {
    let port = env::var("GROKHUB_HUB_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    desktop::probe_hub_health_body(port)
}

fn cabin_running() -> bool {
    let dir = config::config_dir();
    std::fs::read_to_string(tray::cabin_pid_path(&dir))
        .ok()
        .and_then(|t| tray::parse_cabin_pid(&t))
        .is_some_and(tray::cabin_pid_alive)
}

fn run_doctor() {
    let cfg = config::load();
    let sec = secrets::load();
    let mem_ok = std::fs::create_dir_all(config::memory_dir()).is_ok();
    let authed = grokhub_core::has_auth(
        secrets::console_key(&sec, &cfg.api_key),
        &secrets::access_token(&sec),
    );
    let kind = hub_kind_from_health(probe_hub_health_body().as_deref());
    let mut lines = doctor_lines(authed, mem_ok, &kind);
    lines.extend(grokhub_core::doctor_extras(None, crate::skills::list_skills().len()));
    lines.push(doctor_cabin_line(cabin_running()));
    for l in &lines {
        println!("{} {}", if l.ok { "ok " } else { "ERR" }, l.text);
    }
    if !doctor_ok(&lines) {
        std::process::exit(1);
    }
}

fn run_update_cli() {
    let cfg = config::load();
    let src = update::resolve_source(&cfg.source_dir);
    let channel = update::installed_channel();
    let probe = update::blocking_update_probe();
    let pending = grokhub_core::pending_on_channel(
        grokhub_core::pending_from_versions(env!("CARGO_PKG_VERSION"), probe.cabin_tag.as_deref()),
        channel,
    );
    if pending == grokhub_core::UpdatePending::None {
        println!("{}", grokhub_core::update_hint(pending));
        return;
    }
    if let Some(src) = src
        .as_ref()
        .filter(|p| grokhub_core::overlay_clone_usable_in(p, channel))
    {
        update::remember_source(src);
    }
    let cmds = match grokhub_core::update_cmds_for_host_in(src.as_deref(), cfg!(windows), channel) {
        Ok(cmds) => cmds,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if let Some(note) = update::stray_beta_receipt_note() {
        eprintln!("{note}");
    }
    if grokhub_core::update_wipes_config(&cmds) {
        eprintln!("refusing an update that would wipe config");
        std::process::exit(1);
    }
    match update::run_update_cmds(&cmds) {
        Ok(out) => {
            update::log_update_attempt(channel, &cmds, true, &out);
            print!("{out}")
        }
        Err(e) => {
            update::log_update_attempt(channel, &cmds, false, &e);
            eprintln!("{e}");
            eprintln!(
                "{}",
                update::update_failure_status(&e, false, &update::update_log_path())
            );
            std::process::exit(1);
        }
    }
}

fn run_hub() {
    let port = env::var("GROKHUB_HUB_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let banner = format!("grokhub {} hub", env!("CARGO_PKG_VERSION"));
    if let Err(e) = grokhub_hub::run(port, config::hub_state_path(), &banner) {
        eprintln!("hub failed: {e}");
        std::process::exit(1);
    }
}

fn attach_cli_console(alloc_if_orphan: bool) {
    #[cfg(windows)]
    {
        win_console::attach(alloc_if_orphan);
    }
    let _ = alloc_if_orphan;
}

/// Stock Windows sessions have USERPROFILE but not HOME.
#[cfg(windows)]
fn ensure_windows_home() {
    if env::var_os("HOME").is_some() {
        return;
    }
    if let Ok(up) = env::var("USERPROFILE") {
        if !up.is_empty() {
            env::set_var("HOME", up);
        }
    }
}

fn cabin_window_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../packaging/icons/hicolor/256x256/apps/grokhub.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

#[cfg(windows)]
mod win_console {
    use windows_sys::Win32::System::Console::{
        AllocConsole, AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE,
    };

    #[link(name = "ucrt")]
    extern "C" {
        fn _open_osfhandle(osfhandle: isize, flags: i32) -> i32;
        fn _dup2(fd1: i32, fd2: i32) -> i32;
    }

    const O_TEXT: i32 = 0x4000;

    pub fn attach(alloc_if_orphan: bool) {
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                if !alloc_if_orphan {
                    return;
                }
                if AllocConsole() == 0 {
                    return;
                }
            }
            bind_stdio(STD_OUTPUT_HANDLE, 1);
            bind_stdio(STD_ERROR_HANDLE, 2);
        }
    }

    unsafe fn bind_stdio(std_id: u32, fd: i32) {
        let h = GetStdHandle(std_id);
        if h.is_null() || h == (-1isize as _) {
            return;
        }
        let osfh = _open_osfhandle(h as isize, O_TEXT);
        if osfh != -1 {
            let _ = _dup2(osfh, fd);
        }
    }
}

fn run_cabin(hidden: bool) -> eframe::Result<()> {
    if !tray::try_claim_cabin() {
        return Ok(());
    }
    #[cfg(windows)]
    crate::win_native::set_app_user_model_id();
    tray::pin_session_bus();
    // `sudo` in agent commands asks once per session through the OS dialog.
    if let Ok(exe) = env::current_exe() {
        let _ = grokhub_agent::sudo_pass::install(exe);
    }
    tray::force_x11_for_close_to_tray(
        env::var_os("DISPLAY").is_some(),
        env::var_os("WAYLAND_DISPLAY").is_some() || env::var_os("WAYLAND_SOCKET").is_some(),
    );
    let geom = window::clamp_geom(config::load().window);
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(window::launch_size(&geom))
        .with_min_inner_size([window::WIN_MIN_W, window::WIN_MIN_H])
        .with_title("GrokHub")
        .with_app_id("grokhub")
        .with_decorations(false)
        .with_maximized(geom.maximized)
        .with_visible(!hidden);
    if let Some(pos) = window::launch_pos(&geom) {
        viewport = viewport.with_position(pos);
    }
    if let Some(icon) = cabin_window_icon() {
        viewport = viewport.with_icon(icon);
    }
    #[cfg(feature = "fx")]
    {
        run_cabin_with_fx(hidden, viewport)
    }
    #[cfg(not(feature = "fx"))]
    {
        let opts = eframe::NativeOptions {
            viewport,
            // eframe window persistence also restores visibility; close-to-tray would come back withdrawn.
            persist_window: false,
            ..Default::default()
        };
        eframe::run_native(
            "GrokHub",
            opts,
            Box::new(move |cc| {
                crate::theme::install_fonts(&cc.egui_ctx);
                Ok(Box::new(Cabin::new(hidden)))
            }),
        )
    }
}

/// wgpu when the switch is on and the last launch finished a frame. Otherwise glow.
/// A failed wgpu start retries once in this process on the OpenGL renderer.
#[cfg(feature = "fx")]
fn run_cabin_with_fx(hidden: bool, viewport: egui::ViewportBuilder) -> eframe::Result<()> {
    let mut renderer = crate::fx::prepare_launch();
    let mut tried_fallback = false;
    loop {
        let want_wgpu = renderer == eframe::Renderer::Wgpu;
        let opts = eframe::NativeOptions {
            viewport: viewport.clone(),
            persist_window: false,
            renderer,
            ..Default::default()
        };
        let result = eframe::run_native(
            "GrokHub",
            opts,
            Box::new(move |cc| {
                if want_wgpu {
                    let Some(state) = cc.wgpu_render_state.as_ref() else {
                        return Err("wgpu render state missing".into());
                    };
                    crate::fx::install(state);
                }
                crate::theme::install_fonts(&cc.egui_ctx);
                Ok(Box::new(Cabin::new(hidden)))
            }),
        );
        match result {
            Err(err) if want_wgpu && !tried_fallback => {
                eprintln!("composer glow: GPU renderer failed ({err}), falling back to OpenGL");
                crate::fx::note_wgpu_failed();
                renderer = eframe::Renderer::Glow;
                tried_fallback = true;
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_cabin_is_a_gui_subsystem() {
        let src = include_str!("main.rs");
        assert!(
            src.contains("windows_subsystem = \"windows\""),
            "Explorer must not attach a console that kills the cabin when closed: {src}"
        );
        assert!(
            src.contains("AttachConsole"),
            "grokhub --version from PowerShell must still print: {src}"
        );
        assert!(
            src.contains("not(test)"),
            "cargo test must keep a console: {src}"
        );
        assert!(
            src.contains("cabin_window_icon") && src.contains("with_icon"),
            "undecorated cabin still needs a taskbar / alt-tab icon: {src}"
        );
    }

    #[test]
    fn update_cli_runs_without_a_windows_clone() {
        let src = include_str!("main.rs");
        let upd = src
            .split("fn run_update_cli()")
            .nth(1)
            .and_then(|s| s.split("fn run_hub(").next())
            .expect("run_update_cli");
        assert!(
            upd.contains("update_cmds_for_host_in(src.as_deref(), cfg!(windows), channel)")
                && upd.contains("pending_from_versions")
                && upd.contains("pending_on_channel")
                && upd.contains("update_hint")
                && upd.contains("overlay_clone_usable")
                && !upd.contains("pending_for_manual_update")
                && !upd.contains("no GrokHub source tree")
                && !upd.contains("grok update"),
            "grokhub --update runs only when the cabin is newer, takes the Windows zip without a clone, and never updates the CLI: {upd}"
        );
    }

    #[test]
    fn windows_exe_embeds_the_cabin_icon() {
        let build = include_str!("../build.rs");
        assert!(
            build.contains("set_icon") && build.contains("grokhub.ico"),
            "grokhub.exe must carry an ICON resource, not a sidecar .ico: {build}"
        );
        assert!(
            include_bytes!("../../../packaging/windows/grokhub.ico").len() > 64,
            "packaging/windows/grokhub.ico must exist for winresource and Inno SetupIconFile"
        );
        let src = include_str!("main.rs");
        assert!(
            src.contains("hicolor/256x256/apps/grokhub.png"),
            "taskbar / alt-tab icon must be the Linux cabin PNG: {src}"
        );
        let window = image::load_from_memory(include_bytes!(
            "../../../packaging/icons/hicolor/256x256/apps/grokhub.png"
        ))
        .unwrap()
        .into_rgba8();
        assert_eq!(window.dimensions(), (256, 256));
    }
}
