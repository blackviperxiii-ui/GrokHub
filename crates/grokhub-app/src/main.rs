//! GrokHub native cabin. No Electron. No Tauri.
//!
//! Windows Explorer/Start must not allocate a console. Closing that console
//! kills the cabin. CLI flags attach the parent console when there is one.

#![cfg_attr(not(test), windows_subsystem = "windows")]
// Cabin leftovers (Ask ACP, unused tray/update/audio helpers) until a dedicated sweep.
// Workspace clippy stays -D warnings without -A dead_code.
#![allow(dead_code)]

mod app;
mod build_agent;
mod helpers;
mod titlebar;
mod cards;
mod icons;
mod theme;
mod cli;
mod config;
mod desktop;
mod github;
mod host;
mod markdown;
mod night;
mod loops;
mod feed;
mod recipes;
mod notify;
mod store;
mod voice_ws;
mod oauth;
mod secrets;
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
    doctor_cabin_line, doctor_grok_cli_line, doctor_lines, doctor_ok, hub_kind_from_health,
    DEFAULT_PORT,
};
use std::env;

fn main() {
    grokhub_acp::silence_windows_hard_errors();
    #[cfg(windows)]
    ensure_windows_home();
    let launch = parse_args(&env::args().collect::<Vec<_>>());
    match launch {
        Launch::Cabin | Launch::Agent => {}
        Launch::Hub => attach_cli_console(true),
        Launch::Version | Launch::Help | Launch::Doctor | Launch::Update | Launch::Oauth => {
            attach_cli_console(false)
        }
    }
    match launch {
        Launch::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Launch::Help => {
            #[cfg(windows)]
            eprint!(
                "grokhub {} — native cabin\n\n  grokhub           cabin (close stays in the tray)\n  grokhub --agent   cabin in the tray, window hidden\n  grokhub --hub     LAN hub only\n  grokhub --oauth   xAI device-code (Grok)\n  grokhub --update  only what is newer: grok update --alpha, then GitHub zip or source overlay\n  grokhub --doctor  auth / memory / hub kind\n  grokhub --version\n",
                env!("CARGO_PKG_VERSION")
            );
            #[cfg(not(windows))]
            eprint!(
                "grokhub {} — native cabin\n\n  grokhub           cabin (close stays in the tray)\n  grokhub --agent   cabin in the tray, window hidden\n  grokhub --hub     LAN hub only\n  grokhub --oauth   xAI device-code (Grok)\n  grokhub --update  only what is newer: grok update --alpha, then git pull + install.sh --user\n  grokhub --doctor  auth / memory / hub kind\n  grokhub --version\n",
                env!("CARGO_PKG_VERSION")
            );
        }
        Launch::Doctor => run_doctor(),
        Launch::Oauth => run_oauth_cli(),
        Launch::Update => run_update_cli(),
        Launch::Hub => run_hub(),
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
                    if let Err(e) = grokhub_acp::write_cli_auth_if_needed(&tokens) {
                        eprintln!("grok auth.json: {e}");
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
    let (grok_ok, grok_text) =
        grokhub_acp::doctor_grok_line_blocking(grokhub_acp::find_grok().as_deref());
    lines.push(doctor_grok_cli_line(grok_ok, grok_text));
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
    let probe = update::blocking_update_probe();
    let pending = grokhub_core::pending_from_versions(
        env!("CARGO_PKG_VERSION"),
        probe.cabin_tag.as_deref(),
        probe.cli_installed.as_deref(),
        probe.cli_alpha.as_deref(),
    );
    if pending == grokhub_core::UpdatePending::None {
        println!("{}", grokhub_core::combined_update_hint(pending));
        return;
    }
    if pending != grokhub_core::UpdatePending::Cli
        && src
            .as_ref()
            .is_some_and(|p| grokhub_core::overlay_clone_usable(p))
    {
        if let Some(src) = src.as_ref() {
            update::remember_source(src);
        }
    }
    let plan = match grokhub_core::combined_update_cmds(src.as_deref(), pending) {
        Ok(plan) => plan,
        Err(e) if pending == grokhub_core::UpdatePending::Both => {
            match grokhub_core::combined_update_cmds(src.as_deref(), grokhub_core::UpdatePending::Cli)
            {
                Ok(mut plan) => {
                    plan.cabin_skipped = Some(e);
                    plan
                }
                Err(cli_e) => {
                    eprintln!("{cli_e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if let Some(note) = &plan.cabin_skipped {
        eprintln!("{note}");
    }
    if grokhub_core::update_wipes_config(&plan.cmds) {
        eprintln!("refusing an update that would wipe config");
        std::process::exit(1);
    }
    match update::run_update_cmds(&plan.cmds) {
        Ok(out) => print!("{out}"),
        Err(e) => {
            eprintln!("{e}");
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
            upd.contains("combined_update_cmds")
                && upd.contains("pending_from_versions")
                && upd.contains("combined_update_hint")
                && upd.contains("overlay_clone_usable")
                && upd.contains("UpdatePending::Cli")
                && upd.contains("UpdatePending::Both")
                && !upd.contains("pending_for_manual_update")
                && !upd.contains("update_cmds_for")
                && !upd.contains("no GrokHub source tree")
                && !upd.contains("--stable"),
            "grokhub --update runs only what is newer and does not require a Windows clone: {upd}"
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
