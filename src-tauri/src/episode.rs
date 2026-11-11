//! Desktop episodes in the Tauri app (parity with the cabin's Spike-3b).
//!
//! While Settings → Let Grok control the desktop is on, a message opens an
//! episode on its chat: the engine's observe, act, verify kernel
//! (`grokhub_agent::episode`) runs the steps, every step goes through the
//! same hard gate, and a hard-class step (delete, send, money, credentials)
//! parks for an OK in every approvals mode, Always included. This file is the
//! shell's side: the panel's snapshot read from the episode's spans and park
//! files, the last screenshot kept in memory for its thumbnail, and the Cua
//! driver switch. No new executor lives here.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::episode as ep;
use grokhub_agent::harness as hx;
use grokhub_agent::{DesktopOps, ToolOutput};
use serde::Serialize;

/// The page's channel for the episode panel.
pub const EPISODE_EVENT: &str = "episode://event";
/// How often the watcher re-reads the spans and park files.
pub const WATCH_EVERY: Duration = Duration::from_millis(300);
/// The newest steps the panel lists.
pub const STEP_ROWS: usize = 50;
/// A screenshot past this many bytes of data URL is not sent to the page.
pub const FRAME_CAP: usize = 2_000_000;

/// The turn stop reasons that end an episode (the engine's `Done` words).
pub fn ends(stop: &str) -> bool {
    matches!(
        stop,
        "episode_verified" | "halted" | "cancelled" | "episode_idle" | "episode_unconfirmed"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRow {
    pub n: u32,
    pub tool: String,
    pub decision: String,
    pub result: String,
    pub goal_step: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParkRow {
    pub id: String,
    pub tool: String,
    pub action: String,
    pub class: String,
}

/// What the panel draws: one episode on one chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    pub conversation_id: String,
    pub episode_id: String,
    /// "Desktop session · 12 steps · 4 min".
    pub header: String,
    pub steps: Vec<StepRow>,
    pub parks: Vec<ParkRow>,
    /// The latest screenshot as a data URL; `None` when there is none or the
    /// screen may show a secret.
    pub frame: Option<String>,
    /// "running", "paused" or "ended".
    pub state: String,
    /// How it ended (`EpisodeEnd::as_str`), once it has.
    pub ended: Option<String>,
    /// Steps run through the Cua driver's gate instead of the built-in tools.
    pub cua: bool,
}

/// A step of an episode, as the header counts it: a tool call the gate
/// decided (not a reply, a check, a ladder rung, or an episode marker).
pub fn is_step(s: &hx::Span, id: &str) -> bool {
    s.episode == id
        && !matches!(
            s.tool.as_str(),
            hx::REPLY_TOOL | hx::VERIFY_TOOL | hx::RECOVERY_TOOL | ep::EPISODE_TOOL
        )
        && matches!(s.decision.as_str(), "allow" | "park" | "deny")
}

/// A park the episode kernel posted for episode `id`.
pub fn owns(park_id: &str, id: &str) -> bool {
    park_id.starts_with(&format!("{}{id}-", ep::PARK_PREFIX))
}

fn one_line(text: &str, cap: usize) -> String {
    grokhub_core::redact_secrets(text)
        .replace('\n', " ")
        .chars()
        .take(cap)
        .collect()
}

/// The panel for episode `id` on chat `chat`, read from disk.
pub fn panel(
    config_dir: &Path,
    chat: &str,
    id: &str,
    started_ms: u64,
    now_ms: u64,
    frame: Option<String>,
    paused: bool,
    cua: bool,
) -> Panel {
    let spans: Vec<hx::Span> = hx::read_spans(config_dir, chat)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.episode == id)
        .collect();
    // The newest begin or end marker says whether it is over: a paused
    // episode that runs again writes a new begin after its old end.
    let ended = spans
        .iter()
        .rev()
        .find(|s| s.tool == ep::EPISODE_TOOL && matches!(s.decision.as_str(), "begin" | "end"))
        .filter(|s| s.decision == "end")
        .map(|s| s.result.clone());
    let all: Vec<StepRow> = spans
        .iter()
        .filter(|s| is_step(s, id))
        .enumerate()
        .map(|(i, s)| StepRow {
            n: i as u32 + 1,
            tool: s.tool.clone(),
            decision: s.decision.clone(),
            result: one_line(&s.result, ep::STEP_RESULT_CAP),
            goal_step: one_line(&s.goal_step, 60),
        })
        .collect();
    let total = all.len() as u32;
    let skip = all.len().saturating_sub(STEP_ROWS);
    let parks: Vec<ParkRow> = hx::pending_parks(config_dir)
        .into_iter()
        .filter(|p| owns(&p.id, id))
        .map(|p| ParkRow {
            id: p.id,
            tool: p.tool,
            action: one_line(&p.action, 200),
            class: p.class,
        })
        .collect();
    // No picture while a credentials step waits: the screen may show the field.
    let frame = frame.filter(|_| !parks.iter().any(|p| p.class == "credentials"));
    let state = match (&ended, paused) {
        (_, true) => "paused",
        (Some(_), false) => "ended",
        (None, false) => "running",
    };
    Panel {
        conversation_id: chat.to_string(),
        episode_id: id.to_string(),
        header: ep::episode_header(
            total,
            Duration::from_millis(now_ms.saturating_sub(started_ms)),
        ),
        steps: all.into_iter().skip(skip).collect(),
        parks,
        frame,
        state: state.into(),
        ended,
        cua,
    }
}

/// OK or Deny on a card the episode posted. Only this episode's cards
/// answer; the answer file is what the kernel reads.
pub fn answer(config_dir: &Path, id: &str, park_id: &str, approve: bool) -> Result<(), String> {
    if !owns(park_id, id) {
        return Err("That step isn't waiting on this episode.".into());
    }
    if !hx::pending_parks(config_dir)
        .iter()
        .any(|p| p.id == park_id)
    {
        return Err("That step already has an answer.".into());
    }
    hx::answer_park(config_dir, park_id, approve)
}

/// The latest screenshot, held in memory only: never written to a span, a
/// park file or the chat's saved turns.
#[derive(Clone, Default)]
pub struct FrameSlot(Arc<Mutex<Option<String>>>);

impl FrameSlot {
    pub fn get(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, frame: Option<String>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = frame;
    }
}

/// The desktop the engine drives, with its screenshots copied into the slot.
/// A locked screen or an oversized picture clears the slot instead.
pub struct FrameTap {
    pub inner: Box<dyn DesktopOps>,
    pub slot: FrameSlot,
}

impl DesktopOps for FrameTap {
    fn halted(&self) -> bool {
        self.inner.halted()
    }

    fn locked(&self) -> bool {
        self.inner.locked()
    }

    fn call(&self, name: &str, args: &serde_json::Value) -> ToolOutput {
        let out = self.inner.call(name, args);
        if name == "screenshot" && !out.failed {
            let keep = out
                .image_data_url
                .clone()
                .filter(|url| url.len() <= FRAME_CAP && !self.inner.locked());
            self.slot.set(keep);
        }
        out
    }

    fn hint(&self, name: &str, args: &serde_json::Value) -> serde_json::Value {
        self.inner.hint(name, args)
    }
}

/// One open episode on a chat, as the shell tracks it.
#[derive(Debug, Clone)]
pub struct Open {
    pub id: String,
    pub started_ms: u64,
    pub turn: u32,
    pub paused: bool,
    pub cua: bool,
}

/// Re-reads the panel until `stop` is set, calling `emit` whenever it
/// changed, and once more at the end (with `ended` when the chat's episode
/// closed meanwhile). `current` is the chat's open episode.
pub fn watch(
    stop: &AtomicBool,
    config_dir: &Path,
    chat: &str,
    current: &dyn Fn() -> Option<Open>,
    frame: &dyn Fn() -> Option<String>,
    emit: &mut dyn FnMut(&Panel),
) {
    let mut last: Option<(Open, Panel)> = None;
    loop {
        let done = stop.load(Ordering::SeqCst);
        let open = current();
        let seen = open
            .clone()
            .or_else(|| last.as_ref().map(|(o, _)| o.clone()));
        if let Some(seen) = seen {
            let mut p = panel(
                config_dir,
                chat,
                &seen.id,
                seen.started_ms,
                grokhub_core::now_ms(),
                frame(),
                seen.paused,
                seen.cua,
            );
            if open.is_none() && p.ended.is_none() {
                p.state = "ended".into();
                p.ended = Some("stop".into());
            }
            // The header's minutes move on their own; compare without it.
            let same = last.as_ref().is_some_and(|(_, l)| {
                Panel {
                    header: p.header.clone(),
                    ..l.clone()
                } == p
            });
            if !same {
                emit(&p);
            }
            last = Some((seen, p));
        }
        if done {
            return;
        }
        std::thread::sleep(WATCH_EVERY);
    }
}

/// Where the bundled `grokhub-hub` can be, best first, for an app at `me`:
/// next to the app (Windows, macOS, a plain folder), then `../lib/loadout/`
/// (the deb and AppImage layout, `/usr/bin/loadout` beside
/// `/usr/lib/loadout/grokhub-hub`).
pub fn hub_candidates(me: &Path, windows: bool) -> Vec<PathBuf> {
    let name = if windows {
        "grokhub-hub.exe"
    } else {
        "grokhub-hub"
    };
    let Some(dir) = me.parent() else {
        return Vec::new();
    };
    let mut found = vec![dir.join(name)];
    if !windows {
        found.push(dir.join("../lib/loadout").join(name));
    }
    found
}

/// The `grokhub-hub` binary that serves `--mcp-cua` and `--askpass`.
pub fn hub_exe() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    hub_candidates(&me, cfg!(windows))
        .into_iter()
        .find(|p| p.is_file())
}

/// `sudo` in agent commands asks once per session through the OS dialog: the
/// askpass server starts with the bundled `grokhub-hub` as the helper the
/// script runs. Linux only; elsewhere, or without the binary, nothing starts.
/// Returns whether the server started.
#[cfg(desktop)]
pub fn start_askpass(hub: Option<PathBuf>) -> bool {
    match hub {
        Some(exe) if cfg!(target_os = "linux") => grokhub_agent::sudo_pass::install(exe).is_ok(),
        _ => false,
    }
}

/// Registers (or removes) the Cua gate as the engine's native MCP server:
/// only on Linux, with desktop control and `cuaDriver` both on, and only when
/// the hub binary is there. Returns whether it is registered.
pub fn sync_cua(desktop_control: bool, cua_flag: bool, hub: Option<PathBuf>) -> bool {
    let wanted = grokhub_agent::harness::ComputerUseBackend::selected(cua_flag, desktop_control)
        == grokhub_agent::harness::ComputerUseBackend::CuaDriver;
    let exe = hub.filter(|_| wanted);
    let on = exe.is_some();
    grokhub_agent::mcp::set_cabin_cua(exe);
    on
}

/// Whether this chat's steps run through the Cua gate right now.
pub fn cua_in_use(config_dir: &Path) -> bool {
    cfg!(target_os = "linux") && cua_flag(config_dir) && hub_exe().is_some()
}

/// The end of an episode no loop is running: its marker span, and every card
/// it left open is withdrawn and denied with a span.
pub fn write_end(config_dir: &Path, chat: &str, id: &str, why: &str) {
    for park in hx::pending_parks(config_dir)
        .into_iter()
        .filter(|p| owns(&p.id, id))
    {
        hx::clear_park(config_dir, &park.id);
        let span = hx::Span::deny(
            chat,
            &park.tool,
            "{}",
            "episode ended — fail-closed Deny",
            &park.class,
        );
        let _ = hx::append_span(config_dir, &span.in_episode(id).on_path("E"));
    }
    let mut span = hx::Span::deny(chat, ep::EPISODE_TOOL, "{}", why, "soft");
    span.decision = "end".into();
    let _ = hx::append_span(config_dir, &span.in_episode(id).on_path("E"));
}

/// The Cua flag from the cabin's saved `app.json` (the shell has no switch
/// of its own for it), so both apps read one setting.
pub fn cua_flag(config_dir: &Path) -> bool {
    std::fs::read_to_string(config_dir.join("app.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("cuaDriver").and_then(|b| b.as_bool()))
        == Some(true)
}

#[cfg(test)]
#[path = "episode_tests.rs"]
mod tests;
