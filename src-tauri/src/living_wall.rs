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
//! - the last cover (or Make) is under three hours old, or it is quiet hours;
//! - a failed cover is under fifteen minutes old (kept in memory as well as
//!   on disk, so a wall file that can't be written still holds it);
//! - there is nothing to sign an Imagine call with;
//! - Imagine is already making something on the page.
//!
//! The keychain is read only once every cheaper gate has passed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use grokhub_core::{WallGif, WallSeed};
use serde::Serialize;
use serde_json::json;

/// The cabin's app.json key.
pub const KEY: &str = "imagineWall";

/// Sent as a cover starts and ends ([`News`]), so the Imagine page shows it
/// painting, offers Stop, and reloads when it lands.
pub const EVENT: &str = "imagine://wall";

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

/// What the Imagine page hears about a cover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum News {
    Painting { title: String },
    Painted { title: String },
    Failed,
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
    /// No paint before this time: a failed cover's retry, kept by the thread.
    pub not_before: u64,
    /// Something can sign an Imagine call (`imagine::ready`). It reads the
    /// keychain, so it is asked last.
    pub signed_in: &'a dyn Fn() -> bool,
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
    if cx.now < cx.not_before {
        return Look::Held(Hold::NotDue);
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
    if !(cx.signed_in)() {
        return Look::Held(Hold::NoSignIn);
    }
    let Ok(run) = crate::imagine::start() else {
        return Look::Held(Hold::Making);
    };
    let taken: Vec<&str> = titles.iter().map(String::as_str).collect();
    let seed = grokhub_core::pick_fresh_seed(cx.now, &taken);
    let id = format!("{:x}", cx.now);
    let painted = paint(seed, &id, &|| run.stopped()).and_then(|gif| {
        match crate::imagine::pin_cover(dir, gif.clone(), cx.now) {
            Ok(()) => Ok(gif),
            Err(err) => {
                // No row points at these frames now, so nothing would ever
                // evict them.
                let _ = std::fs::remove_file(&gif.path_a);
                let _ = std::fs::remove_file(&gif.path_b);
                Err(err)
            }
        }
    });
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

/// The thread's own hold after a look: a failed cover waits [`RETRY_MS`]
/// even when the wall file couldn't take the hold.
pub fn next_try(look: &Look, now: u64, not_before: u64) -> u64 {
    match look {
        Look::Failed(_) => now.saturating_add(RETRY_MS),
        _ => not_before,
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
/// `tell` hears each cover start and end.
pub fn start<B, T>(config_dir: PathBuf, busy: B, tell: T)
where
    B: Fn() -> bool + Send + 'static,
    T: Fn(News) + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("living-wall".into())
        .spawn(move || {
            let mut not_before = 0;
            loop {
                std::thread::sleep(EVERY);
                let now = grokhub_core::now_ms();
                let cx = Cx {
                    config_dir: &config_dir,
                    pid: std::process::id(),
                    now,
                    offset_min: crate::scheduler::utc_offset(),
                    busy: busy(),
                    not_before,
                    signed_in: &crate::imagine::ready,
                };
                let paint = |seed: &WallSeed, id: &str, stop: &dyn Fn() -> bool| {
                    tell(News::Painting {
                        title: seed.title.to_string(),
                    });
                    paint_here(&config_dir, seed, id, stop)
                };
                let looked = look(&cx, &paint);
                not_before = next_try(&looked, now, not_before);
                match looked {
                    Look::Painted(gif) => tell(News::Painted { title: gif.title }),
                    Look::Failed(_) => tell(News::Failed),
                    Look::Held(_) => {}
                }
            }
        });
}

#[cfg(test)]
#[path = "living_wall_tests.rs"]
mod tests;
