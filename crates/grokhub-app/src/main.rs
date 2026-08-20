//! GrokHub native cabin. No Electron. No Tauri.

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

use app::Cabin;
use cli::{parse_args, Launch};
use eframe::egui;
use grokhub_core::{doctor_lines, doctor_ok, DEFAULT_PORT, HUB_KIND};
use std::env;

fn main() {
    match parse_args(&env::args().collect::<Vec<_>>()) {
        Launch::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Launch::Help => {
            eprint!(
                "grokhub {} — native cabin\n\n  grokhub           cabin (close stays in the tray)\n  grokhub --agent   cabin in the tray, window hidden\n  grokhub --hub     LAN hub only\n  grokhub --oauth   xAI device-code (Grok)\n  grokhub --update  git pull + install.sh --user\n  grokhub --doctor  auth / memory / hub kind\n  grokhub --version\n",
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
                    println!(
                        "connected {}",
                        tokens.email.or(tokens.name).unwrap_or_else(|| "grok".into())
                    );
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

fn run_doctor() {
    let cfg = config::load();
    let sec = secrets::load();
    let mem_ok = std::fs::create_dir_all(config::memory_dir()).is_ok();
    let authed = grokhub_core::has_auth(
        secrets::console_key(&sec, &cfg.api_key),
        &secrets::access_token(&sec),
    );
    let mut lines = doctor_lines(authed, mem_ok, HUB_KIND);
    lines.extend(grokhub_core::doctor_extras(None, crate::skills::list_skills().len()));
    for l in &lines {
        println!("{} {}", if l.ok { "ok " } else { "ERR" }, l.text);
    }
    if !doctor_ok(&lines) {
        std::process::exit(1);
    }
}

fn run_update_cli() {
    let cfg = config::load();
    let Some(src) = update::resolve_source(&cfg.source_dir) else {
        eprintln!("no GrokHub source tree — set GROKHUB_SRC or Settings → source");
        std::process::exit(1);
    };
    match update::run_update(&src) {
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

fn run_cabin(hidden: bool) -> eframe::Result<()> {
    if !tray::try_claim_cabin() {
        return Ok(());
    }
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
