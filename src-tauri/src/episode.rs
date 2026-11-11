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
    /// The picture is the one the page already has, so it is not sent again.
    pub frame_same: bool,
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

/// One listed step and, while it is a hard step nobody settled yet, the card
/// it waits on and the tool and args its settling span will carry.
#[derive(Debug, Clone)]
struct Seen {
    row: StepRow,
    card: Option<(String, String)>,
}

/// What the panel has read of one episode's spans. A re-read parses only the
/// whole lines appended since (as the cabin's step count does), so a long
/// chat's span file is not parsed again three times a second, and a torn or
/// bad line is skipped instead of blanking the list.
#[derive(Debug, Default)]
pub struct Cursor {
    chat: String,
    id: String,
    /// Bytes of the span file read so far: the end of its last whole line.
    at: u64,
    rows: Vec<Seen>,
    ended: Option<String>,
}

/// The tool and args a hard step's own spans share (its park, then its
/// approve or deny).
fn card_key(s: &hx::Span) -> String {
    format!("{}\n{}", s.tool, s.args_redacted)
}

/// The card a parked step's span waits on: the kernel names it after the
/// episode and the step number in the span's claim (`episode step N`).
fn card_of(s: &hx::Span, id: &str) -> Option<String> {
    let n: u32 = s.claim.strip_prefix("episode step ")?.parse().ok()?;
    Some(format!("{}{id}-{n}", ep::PARK_PREFIX))
}

impl Cursor {
    /// Reads what was appended to the chat's span file since the last call.
    fn read(&mut self, config_dir: &Path, chat: &str, id: &str) {
        use std::io::{Read, Seek, SeekFrom};
        if self.chat != chat || self.id != id {
            *self = Self {
                chat: chat.to_string(),
                id: id.to_string(),
                ..Self::default()
            };
        }
        let Ok(mut file) = std::fs::File::open(hx::span_path(config_dir, chat)) else {
            return;
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.at {
            // A shorter file is another file: read it from the start.
            self.at = 0;
            self.rows.clear();
            self.ended = None;
        }
        let mut buf = Vec::new();
        if file.seek(SeekFrom::Start(self.at)).is_err() || file.read_to_end(&mut buf).is_err() {
            return;
        }
        // A last line still being written waits for its newline.
        let whole = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        self.at += whole as u64;
        for line in String::from_utf8_lossy(&buf[..whole]).lines() {
            if let Ok(span) = serde_json::from_str::<hx::Span>(line.trim()) {
                if span.episode == self.id {
                    self.note(span);
                }
            }
        }
    }

    fn note(&mut self, s: hx::Span) {
        if s.tool == ep::EPISODE_TOOL {
            // The newest begin or end marker says whether it is over.
            match s.decision.as_str() {
                "begin" => self.ended = None,
                "end" => self.ended = Some(s.result),
                _ => {}
            }
            return;
        }
        // An approve or a deny settles the oldest waiting step with the same
        // tool and args. A park past its TTL is denied for now and waits on.
        let settles = match s.decision.as_str() {
            "approve" => true,
            "deny" => !s.result.ends_with(ep::STILL_WAITING),
            _ => false,
        };
        if settles {
            let key = card_key(&s);
            let waiting = self
                .rows
                .iter_mut()
                .find(|seen| seen.card.as_ref().is_some_and(|(_, k)| *k == key));
            if let Some(seen) = waiting {
                seen.card = None;
                if s.decision == "approve" {
                    seen.row.decision = "approve".into();
                    seen.row.result = one_line(&s.result, ep::STEP_RESULT_CAP);
                } else {
                    seen.row.decision = "deny".into();
                    seen.row.result = String::new();
                }
            }
        }
        if !is_step(&s, &self.id) {
            return;
        }
        let card = (s.decision == "park")
            .then(|| card_of(&s, &self.id))
            .flatten()
            .map(|card| (card, card_key(&s)));
        self.rows.push(Seen {
            row: StepRow {
                n: self.rows.len() as u32 + 1,
                tool: s.tool,
                decision: s.decision,
                result: one_line(&s.result, ep::STEP_RESULT_CAP),
                goal_step: one_line(&s.goal_step, 60),
            },
            card,
        });
    }

    /// The panel for episode `id` on chat `chat` as it stands now.
    pub fn panel(
        &mut self,
        config_dir: &Path,
        chat: &str,
        id: &str,
        started_ms: u64,
        now_ms: u64,
        frame: Option<String>,
        paused: bool,
        cua: bool,
    ) -> Panel {
        self.read(config_dir, chat, id);
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
        let skip = self.rows.len().saturating_sub(STEP_ROWS);
        let steps: Vec<StepRow> = self.rows[skip..]
            .iter()
            .map(|seen| {
                let mut row = seen.row.clone();
                // A parked step whose card has its answer: allowed and not
                // run yet (no loop took the answer, or it is running), or
                // denied and not settled yet.
                if let Some((card, _)) = &seen.card {
                    if !parks.iter().any(|p| p.id == *card) {
                        let answer = hx::park_dir(config_dir).join(format!("{card}.answer"));
                        let denied =
                            std::fs::read_to_string(answer).is_ok_and(|word| word.trim() == "deny");
                        row.decision = if denied { "deny" } else { "allowed" }.into();
                        row.result = String::new();
                    }
                }
                row
            })
            .collect();
        // No picture while a credentials step waits: the screen may show the field.
        let frame = frame.filter(|_| !parks.iter().any(|p| p.class == "credentials"));
        let state = match (&self.ended, paused) {
            (_, true) => "paused",
            (Some(_), false) => "ended",
            (None, false) => "running",
        };
        Panel {
            conversation_id: chat.to_string(),
            episode_id: id.to_string(),
            header: ep::episode_header(
                self.rows.len() as u32,
                Duration::from_millis(now_ms.saturating_sub(started_ms)),
            ),
            steps,
            parks,
            frame,
            state: state.into(),
            ended: self.ended.clone(),
            cua,
            frame_same: false,
        }
    }
}

/// The panel for episode `id` on chat `chat`, read from disk in one go.
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
    Cursor::default().panel(config_dir, chat, id, started_ms, now_ms, frame, paused, cua)
}

/// The panel of an episode that was ended with no loop running (Stop, Halt,
/// or the desktop switch going off): ended as `why` unless its spans say how.
pub fn ended_panel(
    config_dir: &Path,
    chat: &str,
    open: &Open,
    frame: Option<String>,
    why: &str,
) -> Panel {
    let mut panel = panel(
        config_dir,
        chat,
        &open.id,
        open.started_ms,
        grokhub_core::now_ms(),
        frame,
        false,
        open.cua,
    );
    if panel.ended.is_none() {
        panel.ended = Some(why.into());
    }
    panel.state = "ended".into();
    panel
}

/// The desktop behind the live Settings switch. The gate reads the switch
/// once per message; this refuses every later call of a running turn once
/// the switch goes off, and reads nothing off the screen for it.
pub struct Switched {
    pub inner: Box<dyn DesktopOps>,
    pub on: Arc<AtomicBool>,
}

impl DesktopOps for Switched {
    fn halted(&self) -> bool {
        self.inner.halted()
    }

    fn locked(&self) -> bool {
        self.inner.locked()
    }

    fn call(&self, name: &str, args: &serde_json::Value) -> ToolOutput {
        if !self.on.load(Ordering::SeqCst) {
            return ToolOutput::err(*grokhub_core::desktop_mcp::OFF_MSG);
        }
        self.inner.call(name, args)
    }

    fn hint(&self, name: &str, args: &serde_json::Value) -> serde_json::Value {
        if !self.on.load(Ordering::SeqCst) {
            return args.clone();
        }
        self.inner.hint(name, args)
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

/// Everything the page draws but the header, whose minutes move on their own.
fn same_but_header(a: &Panel, b: &Panel) -> bool {
    (
        &a.conversation_id,
        &a.episode_id,
        &a.steps,
        &a.parks,
        &a.frame,
        &a.state,
        &a.ended,
        a.cua,
    ) == (
        &b.conversation_id,
        &b.episode_id,
        &b.steps,
        &b.parks,
        &b.frame,
        &b.state,
        &b.ended,
        b.cua,
    )
}

/// Re-reads the panel until `stop` is set, calling `emit` whenever it
/// changed, and once more at the end (with `ended` when the chat's episode
/// closed meanwhile). `current` is the chat's open episode. Only the spans
/// appended since the last look are parsed, and a picture goes to the page
/// once: a panel that changed around the same picture says `frame_same`.
pub fn watch(
    stop: &AtomicBool,
    config_dir: &Path,
    chat: &str,
    current: &dyn Fn() -> Option<Open>,
    frame: &dyn Fn() -> Option<String>,
    emit: &mut dyn FnMut(&Panel),
) {
    let mut last: Option<(Open, Panel)> = None;
    let mut cursor = Cursor::default();
    loop {
        let done = stop.load(Ordering::SeqCst);
        let open = current();
        let seen = open
            .clone()
            .or_else(|| last.as_ref().map(|(o, _)| o.clone()));
        if let Some(seen) = seen {
            let mut p = cursor.panel(
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
            let before = last.as_ref().map(|(_, l)| l);
            if !before.is_some_and(|l| same_but_header(l, &p)) {
                if p.frame.is_some() && before.is_some_and(|l| l.frame == p.frame) {
                    let frame = p.frame.take();
                    p.frame_same = true;
                    emit(&p);
                    p.frame = frame;
                    p.frame_same = false;
                } else {
                    emit(&p);
                }
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
/// it still has on disk is taken back and denied with a span. A card that was
/// already allowed goes too: an Allow no loop picked up must not be left for
/// a later one to run.
pub fn write_end(config_dir: &Path, chat: &str, id: &str, why: &str) {
    let dir = hx::park_dir(config_dir);
    let mine = format!("{}{id}-", ep::PARK_PREFIX);
    let left: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let named = |path: &Path| {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(&mine))
    };
    let mut cards: Vec<hx::ParkRequest> = left
        .iter()
        .filter(|path| named(path) && path.extension().is_some_and(|x| x == "json"))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .filter_map(|body| serde_json::from_str::<hx::ParkRequest>(&body).ok())
        .filter(|park| owns(&park.id, id))
        .collect();
    cards.sort_by_key(|park| park.ts_ms);
    for park in cards {
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
    // Whatever else carries the episode's card names (an answer with no
    // request, a file half written) goes with them.
    for path in left.iter().filter(|path| named(path)) {
        let _ = std::fs::remove_file(path);
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
