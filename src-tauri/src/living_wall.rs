//! Living wall (Settings → Behavior): while the cabin is closed, Imagine
//! paints a new cover for its gallery every few hours. Twenty stay; the
//! oldest leaves first. Each cover is two low-quality 1k stills from a fixed
//! seed (`grokhub_core::WALL_SEEDS`), so no model writes the prompts.
//!
//! The switch is the cabin's `imagineWall` in app.json. It is off until the
//! person turns it on; a file the cabin saved with it on keeps painting.
//!
//! A look paints nothing while:
//! - the switch is off;
//! - the cabin is open (its scheduler lease is fresh), so only one app paints;
//! - the window hasn't reported its clock (quiet hours need it);
//! - Halt holds background work (`night::halted`);
//! - a chat is running, a decision waits or a game is on;
//! - the chat you have open is a scratch chat;
//! - there is nothing to sign an Imagine call with;
//! - the last cover (or Make) is under three hours old, or it is quiet hours;
//! - Imagine is already making something on the page.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use grokhub_core::{WallGif, WallSeed};
use serde::Serialize;
use serde_json::json;

/// The cabin's app.json key.
pub const KEY: &str = "imagineWall";

/// Sent with the new cover when one lands, so the Imagine page reloads.
pub const PAINTED_EVENT: &str = "imagine://wall";

/// How often the thread looks. The wall's own interval bounds real paints.
const EVERY: Duration = Duration::from_secs(60);

/// A failed cover tries again this much later, not a full interval later.
const RETRY_MS: u64 = 15 * 60 * 1000;

/// Settings → Behavior → Living wall.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub on: bool,
}

pub fn is_on(config_dir: &Path) -> bool {
    grokhub_core::app_json::bool_or(config_dir, KEY, false)
}

pub fn view(config_dir: &Path) -> View {
    View {
        on: is_on(config_dir),
    }
}

pub fn set(config_dir: &Path, on: bool) -> Result<View, String> {
    grokhub_core::app_json::patch(config_dir, &[(KEY, json!(on))])?;
    Ok(view(config_dir))
}

/// Why a look painted nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Off,
    CabinOpen,
    NoClock,
    Halted,
    Busy,
    Scratch,
    NoSignIn,
    NotDue,
    Quiet,
    /// The Imagine page is making something.
    Making,
}

/// What one look did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    Held(Hold),
    Painted(WallGif),
    Failed(String),
}

/// The look's inputs that come from the app, not the disk.
pub struct Cx<'a> {
    pub config_dir: &'a Path,
    pub pid: u32,
    pub now: u64,
    /// The window's UTC offset in minutes, once it has reported it.
    pub offset_min: Option<i32>,
    /// A chat is running, a decision waits or a game is on.
    pub busy: bool,
    /// Something can sign an Imagine call (`imagine::ready`).
    pub signed_in: bool,
}

/// Paints one cover: the seed, its id, and the page's Stop.
pub type Paint<'a> = dyn Fn(&WallSeed, &str, &dyn Fn() -> bool) -> Result<WallGif, String> + 'a;

pub fn look(cx: &Cx<'_>, paint: &Paint<'_>) -> Look {
    let dir = cx.config_dir;
    if !is_on(dir) {
        return Look::Held(Hold::Off);
    }
    if !shell_may_fire(read_lease(dir).as_ref(), cx.pid, cx.now) {
        return Look::Held(Hold::CabinOpen);
    }
    let Some(offset) = cx.offset_min else {
        return Look::Held(Hold::NoClock);
    };
    if crate::night::halted(dir, cx.now) {
        return Look::Held(Hold::Halted);
    }
    if cx.busy {
        return Look::Held(Hold::Busy);
    }
    let settings = crate::settings::load(dir);
    if grokhub_agent::scratch::is_scratch(&settings.last_chat) {
        return Look::Held(Hold::Scratch);
    }
    if !cx.signed_in {
        return Look::Held(Hold::NoSignIn);
    }
    let Ok((last_ms, titles)) = crate::imagine::wall_state(dir) else {
        return Look::Held(Hold::NotDue);
    };
    let clock = crate::scheduler::local_clock(cx.now, offset);
    let quiet = crate::proactive::is_quiet(&settings.behavior, clock.hour, clock.minute);
    if !grokhub_core::wall_can_paint(true, true, false, false, quiet, last_ms, cx.now) {
        return Look::Held(if quiet && last_ms != 0 {
            Hold::Quiet
        } else {
            Hold::NotDue
        });
    }
    let Ok(run) = crate::imagine::start() else {
        return Look::Held(Hold::Making);
    };
    let taken: Vec<&str> = titles.iter().map(String::as_str).collect();
    let seed = grokhub_core::pick_fresh_seed(cx.now, &taken);
    let id = format!("{:x}", cx.now);
    let painted = paint(seed, &id, &|| run.stopped())
        .and_then(|gif| crate::imagine::pin_cover(dir, gif.clone(), cx.now).map(|()| gif));
    match painted {
        Ok(gif) => Look::Painted(gif),
        Err(err) => {
            let retry = cx
                .now
                .saturating_sub(grokhub_core::WALL_GIF_EVERY_MS)
                .saturating_add(RETRY_MS);
            let _ = crate::imagine::hold_wall(dir, retry.max(1));
            Look::Failed(err)
        }
    }
}

/// Paints with this PC's Imagine sign-in, falling back to chat's as a Make
/// does when xAI refuses Imagine's own.
fn paint_here(
    config_dir: &Path,
    seed: &WallSeed,
    id: &str,
    stop: &dyn Fn() -> bool,
) -> Result<WallGif, String> {
    let (bearer, own) = crate::imagine::bearer()?;
    let now = grokhub_core::now_ms();
    let paint = |bearer: &str| {
        let api = grokhub_agent::imagine_client(bearer);
        crate::imagine::paint_cover(config_dir, api.as_ref(), stop, seed, id, now)
    };
    match paint(&bearer) {
        Err(err) if own && err == crate::imagine::NEED_SIGNIN => {
            paint(&crate::imagine::shared_bearer()?)
        }
        done => done,
    }
}

/// Start the thread. `busy` is a running chat, a waiting decision or a game;
/// `painted` runs with each new cover.
pub fn start<B, P>(config_dir: PathBuf, busy: B, painted: P)
where
    B: Fn() -> bool + Send + 'static,
    P: Fn(&WallGif) + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("living-wall".into())
        .spawn(move || loop {
            std::thread::sleep(EVERY);
            let cx = Cx {
                config_dir: &config_dir,
                pid: std::process::id(),
                now: grokhub_core::now_ms(),
                offset_min: crate::scheduler::utc_offset(),
                busy: busy(),
                signed_in: crate::imagine::ready(),
            };
            let paint = |seed: &WallSeed, id: &str, stop: &dyn Fn() -> bool| {
                paint_here(&config_dir, seed, id, stop)
            };
            if let Look::Painted(gif) = look(&cx, &paint) {
                painted(&gif);
            }
        });
}

#[cfg(test)]
#[path = "living_wall_tests.rs"]
mod tests;
