//! "Record my screen" in the window app, as the cabin does it: `/record
//! [what's wrong]` takes a still every 2 seconds into the Recordings folder
//! of your files until Stop, `/record stop`, Halt or the 2-minute cap. It
//! needs consent first (`screen_record` and `screen_record_send` in
//! `app.json`), never starts from a remote browser, and is held while an
//! anti-cheat game runs. When it stops, a few stills and the note go to Grok
//! once ([`crate::screen_report`]); the report goes into the chat that asked
//! and onto the recording's Pulse card. The stills stay in their folder.

use crate::slash::Effect;
use grokhub_agent::CancelToken;
use grokhub_core::screen_record as sr;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The consent switch in `app.json`, shared with the cabin's Settings.
const CONSENT_KEY: &str = "screen_record";
/// The consent to send a few stills for the report. An older yes, given
/// when recording sent nothing, doesn't cover it, so that asks again.
const SEND_KEY: &str = "screen_record_send";
/// The question before the first recording.
pub const CONSENT_PROMPT: &str = "Record your screen now? It takes a still every 2 seconds into your Recordings folder. When it stops, up to 6 stills and your note go to Grok once, and the report comes back in this chat.";
/// A bigger `app.json` than this is not read or rewritten.
const APP_JSON_CAP: u64 = 4 * 1024 * 1024;
/// A `/record` line that says yes to the "turn it on?" question.
const ALLOW: &str = "--allow";

/// A still: the image bytes and their MIME type.
pub type Still = (Vec<u8>, String);

/// What one recording ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub secs: u64,
    pub frames: Vec<PathBuf>,
    pub dir: PathBuf,
    pub err: Option<String>,
}

fn still_ext(mime: &str) -> &'static str {
    if mime.contains("jpeg") || mime.contains("jpg") {
        "jpg"
    } else {
        "png"
    }
}

/// The capture loop, off the UI thread. Takes a still whenever the recorder
/// says one is due, until `stop` or the cap. While `held` (the game guard)
/// no still is taken and the loop keeps waiting, so Stop and the cap still
/// end it. Any other still that cannot be taken or saved ends the recording
/// with what it has.
pub fn record_loop(
    dir: &Path,
    stop: &AtomicBool,
    mut grab: impl FnMut() -> Result<Still, String>,
    mut held: impl FnMut() -> bool,
    mut now_ms: impl FnMut() -> u64,
    mut wait: impl FnMut(),
) -> Done {
    let mut done = Done {
        secs: 0,
        frames: Vec::new(),
        dir: dir.to_path_buf(),
        err: None,
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        done.err = Some(format!("could not make the recording folder: {e}"));
        return done;
    }
    let mut rec = sr::Recorder::default();
    let _ = rec.start(true, now_ms());
    loop {
        let now = now_ms();
        if stop.load(Ordering::Relaxed) || rec.should_stop(now) {
            break;
        }
        if rec.frame_due(now) && !held() {
            let saved = grab().and_then(|(bytes, mime)| {
                let path = dir.join(sr::frame_name(done.frames.len(), still_ext(&mime)));
                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(path)
            });
            match saved {
                Ok(path) => {
                    done.frames.push(path);
                    rec.note_frame(now);
                }
                // The guard took hold between the check and the grab.
                Err(_) if held() => {}
                Err(e) => {
                    done.err = Some(e);
                    break;
                }
            }
        }
        wait();
    }
    done.secs = rec.stop(now_ms()).map(|(secs, _)| secs).unwrap_or(0);
    done
}

struct Live {
    stop: Arc<AtomicBool>,
    started: Instant,
    /// Halt during the capture: the report sends nothing either.
    cancel: CancelToken,
}

/// What the top bar's indicator and the notice bell read.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Event {
    /// `record` or `audio`.
    pub kind: &'static str,
    pub ok: bool,
    pub text: String,
    /// The chat the report went into; empty when none did.
    pub chat: String,
}

/// A finished recording while Grok looks at its stills. A new recording can
/// start meanwhile, so each report keeps its own entry.
struct Diag {
    id: u64,
    cancel: CancelToken,
    stills: usize,
    /// "Looking at" for a report, "Reading the steps from" for a skill.
    doing: &'static str,
}

/// Names each report's [`Diag`].
static NEXT_DIAG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A chat's newest recording whose stills went to Grok: what `/record
/// --skill` in that chat learns from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub dir: PathBuf,
    pub note: String,
    pub secs: u64,
}

/// A call about a recording's stills is under way: it shows on the top bar
/// as "`doing` N stills…" and Halt cancels it. Hand the id to [`end_call`].
pub(crate) fn begin_call(cancel: &CancelToken, stills: usize, doing: &'static str) -> u64 {
    let id = NEXT_DIAG.fetch_add(1, Ordering::Relaxed);
    hub().diag.push(Diag {
        id,
        cancel: cancel.clone(),
        stills,
        doing,
    });
    id
}

pub(crate) fn end_call(id: u64) {
    hub().diag.retain(|d| d.id != id);
}

/// The newest recording diagnosed for `chat`, the one its report offered
/// to save. A halted or local-only recording never takes its place, so its
/// stills are never sent by a later `/record --skill`.
pub(crate) fn last_recorded(chat: &str) -> Option<Recorded> {
    hub().offered.get(chat).cloned()
}

#[derive(Default)]
struct Hub {
    rec: Option<Live>,
    diag: Vec<Diag>,
    /// Keyed by chat.
    offered: BTreeMap<String, Recorded>,
    events: Vec<Event>,
}

static HUB: Mutex<Hub> = Mutex::new(Hub {
    rec: None,
    diag: Vec::new(),
    offered: BTreeMap::new(),
    events: Vec::new(),
});

fn hub() -> std::sync::MutexGuard<'static, Hub> {
    HUB.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the window asks for about every second.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Status {
    /// The red indicator line while recording, with the seconds so far.
    pub recording: Option<Line>,
    /// The audio check's line while the microphone is open.
    pub listening: Option<Line>,
    /// After Stop, while Grok looks at the stills.
    pub diagnosing: Option<Line>,
    /// Finished runs, each handed over once.
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Line {
    pub secs: u64,
    pub text: String,
}

pub fn status() -> Status {
    let mut h = hub();
    Status {
        recording: h.rec.as_ref().map(|live| {
            let secs = live.started.elapsed().as_secs().min(sr::MAX_SECS);
            Line {
                secs,
                text: sr::indicator_line(secs),
            }
        }),
        listening: crate::audio_check::listening(),
        diagnosing: h.diag.last().map(|d| Line {
            secs: 0,
            text: match d.stills {
                1 => format!("{} 1 still…", d.doing),
                n => format!("{} {n} stills…", d.doing),
            },
        }),
        events: std::mem::take(&mut h.events),
    }
}

pub fn push_event(event: Event) {
    hub().events.push(event);
}

/// Halt: stop the recording, and the report's call if one is under way, so
/// nothing more is sent.
pub fn halt() {
    let h = hub();
    if let Some(live) = h.rec.as_ref() {
        live.stop.store(true, Ordering::Relaxed);
        live.cancel.cancel();
    }
    for diag in &h.diag {
        diag.cancel.cancel();
    }
}

/// Stop the recording now (the indicator's Stop, `/record stop`). Its report
/// still goes. False when none was running.
pub fn stop() -> bool {
    match hub().rec.as_ref() {
        Some(live) => {
            live.stop.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

fn app_json(config_dir: &Path) -> PathBuf {
    config_dir.join("app.json")
}

/// Whether recording, and sending its stills for the report, are both on
/// in `app.json`.
pub fn consented(config_dir: &Path) -> bool {
    let path = app_json(config_dir);
    if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= APP_JSON_CAP) {
        return false;
    }
    let on = |v: &Value, key: &str| v.get(key).and_then(Value::as_bool) == Some(true);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .is_some_and(|v| on(&v, CONSENT_KEY) && on(&v, SEND_KEY))
}

/// Turn the consent on, keeping every other key in `app.json`. A file that
/// doesn't parse is left alone.
pub fn allow(config_dir: &Path) -> Result<(), String> {
    let path = app_json(config_dir);
    let mut app = match std::fs::read_to_string(&path) {
        Ok(raw) if raw.len() as u64 > APP_JSON_CAP => {
            return Err("The settings file is too big to change here.".into());
        }
        Ok(raw) if raw.trim().is_empty() => json!({}),
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .ok()
            .filter(Value::is_object)
            .ok_or("The settings file can't be read, so it was left as it is.")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.to_string()),
    };
    let keys = app
        .as_object_mut()
        .ok_or("The settings file isn't an object.")?;
    keys.insert(CONSENT_KEY.into(), json!(true));
    keys.insert(SEND_KEY.into(), json!(true));
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let tmp = config_dir.join("app.json.tmp");
    let body = serde_json::to_string_pretty(&app).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// `2026-11-10-1768000000`: the day, then the second it started.
fn stamp(now_ms: u64) -> String {
    let at = grokhub_core::oauth::unix_ms_to_rfc3339(now_ms);
    format!("{}-{}", at.get(..10).unwrap_or("recording"), now_ms / 1000)
}

/// The phone has no screen to grab from the app: recording is a desktop job.
#[cfg(desktop)]
fn grabber() -> impl FnMut() -> Result<Still, String> {
    grokhub_desktop::screen_grabber()
}

#[cfg(not(desktop))]
fn grabber() -> impl FnMut() -> Result<Still, String> {
    || Err("Screen recording runs on a computer.".into())
}

/// The game guard's reason, when an anti-cheat holds screen recording.
#[cfg(desktop)]
fn held_reason() -> Option<String> {
    grokhub_desktop::game_guard::current()
        .blocks(grokhub_desktop::game_guard::Guarded::ScreenRecording)
}

#[cfg(not(desktop))]
fn held_reason() -> Option<String> {
    None
}

/// The bell's line for a recording that took no stills.
fn not_started_text(done: &Done, note: &str) -> String {
    let title = sr::recording_title(done.secs, note);
    let why = done
        .err
        .clone()
        .unwrap_or_else(|| "no stills were taken".into());
    format!("{title}: the recording didn't start: {why}.")
}

/// The bell's line once the report is in: how many stills were saved, the
/// card's line, and why it ended early, if it did.
fn finished_text(done: &Done, note: &str, summary: &str) -> String {
    let title = sr::recording_title(done.secs, note);
    let stills = match done.frames.len() {
        1 => "1 still".to_string(),
        n => format!("{n} stills"),
    };
    let stopped = done
        .err
        .as_ref()
        .map(|e| format!(" It ended early: {e}."))
        .unwrap_or_default();
    let summary = summary.trim().trim_end_matches('.');
    format!("{title}: {stills} saved. {summary}.{stopped}")
}

/// What a finished recording reports through: the model client (`Err` with
/// no sign-in) and the chat the report goes into.
pub struct Reporter {
    pub client: Box<dyn FnOnce() -> crate::screen_report::Client + Send>,
    /// Puts the report in the chat `chat` (never called with an empty one).
    pub deliver: Deliver,
    /// Shows a question with Yes in the chat: the chat, the prompt, and the
    /// line Yes sends. Saved nowhere; Yes runs that line as if typed.
    pub confirm: ConfirmIn,
}

/// Puts a report (the second argument) into a chat (the first).
pub type Deliver = Box<dyn FnOnce(&str, &str) + Send>;
/// Asks in a chat (first), with this prompt (second) and Yes line (third).
pub type ConfirmIn = Box<dyn FnOnce(&str, &str, &str) + Send>;

impl Reporter {
    /// No sign-in and no chat: the report stays on the card. For a
    /// recording with no window to talk to (tests).
    pub fn local() -> Self {
        Self {
            client: Box::new(|| Err("no client".into())),
            deliver: Box::new(|_, _| {}),
            confirm: Box::new(|_, _, _| {}),
        }
    }
}

/// `/record`, `/record stop` and `/audiocheck` for a line typed on this
/// computer, in the chat `chat`. `None` when `line` is some other command.
pub fn slash(
    line: &str,
    config_dir: &Path,
    remote: bool,
    chat: Option<&str>,
    reporter: impl FnOnce() -> Reporter,
) -> Option<Result<Effect, String>> {
    use grokhub_core::Slash;
    let parsed = grokhub_core::parse_slash(line)?;
    let result = match parsed {
        Slash::Record(note) => match crate::screen_skill::parse(&note) {
            Some(ask) => {
                crate::screen_skill::run(config_dir, ask, remote, chat.unwrap_or(""), reporter)
                    .map_err(Ask::No)
            }
            None => start_or_ask(config_dir, &note, remote, chat.unwrap_or(""), reporter),
        },
        Slash::RecordStop => stop_line(remote),
        Slash::AudioCheck(want) => {
            crate::audio_check::start(config_dir, chat.unwrap_or(""), &want, remote)
                .map_err(Ask::No)
        }
        _ => return None,
    };
    Some(
        result
            .map(|text| Effect::Notice { text })
            .or_else(|e| match e {
                Ask::Confirm(effect) => Ok(effect),
                Ask::No(text) => Err(text),
            }),
    )
}

/// A refusal, or a question the chat shows with Yes.
enum Ask {
    No(String),
    Confirm(Effect),
}

impl From<String> for Ask {
    fn from(s: String) -> Self {
        Ask::No(s)
    }
}

fn stop_line(remote: bool) -> Result<String, Ask> {
    if remote {
        return Err(Ask::No("Recording works on your computer.".into()));
    }
    if stop() {
        Ok("Stopping the recording…".into())
    } else {
        Ok("Not recording.".into())
    }
}

fn start_or_ask(
    config_dir: &Path,
    note: &str,
    remote: bool,
    chat: &str,
    reporter: impl FnOnce() -> Reporter,
) -> Result<String, Ask> {
    if remote {
        return Err(Ask::No(
            "Recording works on your computer, not from a browser.".into(),
        ));
    }
    let (allowed, note) = match note.trim().strip_prefix(ALLOW) {
        Some(rest) => (true, rest.trim()),
        None => (false, note.trim()),
    };
    if hub().rec.is_some() {
        return Err(Ask::No(sr::ALREADY_RECORDING.into()));
    }
    if !consented(config_dir) {
        if !allowed {
            return Err(Ask::Confirm(Effect::Confirm {
                prompt: CONSENT_PROMPT.into(),
                yes: format!("/record {ALLOW} {note}").trim_end().to_string(),
            }));
        }
        allow(config_dir)?;
    }
    start(config_dir, note, chat, held_reason, grabber, reporter()).map_err(Ask::No)
}

/// Start the capture thread. The game guard is read first, so a held
/// recording makes no folder. When the capture ends, the indicator clears
/// at once; the report follows (see [`report`]).
fn start<G: FnMut() -> Result<Still, String>>(
    config_dir: &Path,
    note: &str,
    chat: &str,
    guard: impl FnOnce() -> Option<String>,
    make_grabber: impl FnOnce() -> G + Send + 'static,
    reporter: Reporter,
) -> Result<String, String> {
    if let Some(why) = guard() {
        return Err(why);
    }
    let recordings = grokhub_core::user_files::folder(
        config_dir,
        grokhub_core::user_files::UserFolder::Recordings,
    )
    .map_err(|e| grokhub_core::user_files::plain_io_error(&e))?;
    // Two recordings in the same second get their own folders.
    let at = stamp(now_ms());
    let mut dir = sr::recording_dir(&recordings, &at);
    for n in 2.. {
        if !dir.exists() {
            break;
        }
        dir = sr::recording_dir(&recordings, &format!("{at}-{n}"));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let cancel = CancelToken::new();
    {
        let mut h = hub();
        if h.rec.is_some() {
            return Err(sr::ALREADY_RECORDING.into());
        }
        h.rec = Some(Live {
            stop: Arc::clone(&stop),
            started: Instant::now(),
            cancel: cancel.clone(),
        });
    }
    let into = dir.clone();
    let note = note.to_string();
    let chat = chat.trim().to_string();
    let config_dir = config_dir.to_path_buf();
    std::thread::spawn(move || {
        let _config = grokhub_agent::perm::ConfigGuard::set(&config_dir);
        let held = || held_reason().is_some();
        let done = record_loop(&into, &stop, make_grabber(), held, now_ms, || {
            std::thread::sleep(Duration::from_millis(250))
        });
        let event = report(&config_dir, &done, &note, &chat, reporter, cancel);
        hub().events.push(event);
    });
    Ok(format!(
        "Recording your screen into {}. Show the problem, then press Stop or send /record stop. It stops on its own at {}.",
        dir.display(),
        sr::clock(sr::MAX_SECS)
    ))
}

/// After the capture: clear the indicator, then let Grok look once, post
/// the recording's card, and put the report in the chat that asked. Halt
/// while it looks cancels the call ([`halt`]).
fn report(
    config_dir: &Path,
    done: &Done,
    note: &str,
    chat: &str,
    reporter: Reporter,
    cancel: CancelToken,
) -> Event {
    {
        let mut h = hub();
        h.rec = None;
        if done.frames.is_empty() {
            return Event {
                kind: "record",
                ok: false,
                text: not_started_text(done, note),
                chat: String::new(),
            };
        }
    }
    let stills = sr::sample_keyframes(done.frames.len(), sr::KEYFRAMES).len();
    let id = begin_call(&cancel, stills, "Looking at");
    let title = sr::recording_title(done.secs, note);
    let found = crate::screen_report::diagnose(
        &title,
        note,
        done.secs,
        &done.dir,
        &done.frames,
        reporter.client,
        &cancel,
    );
    let card = grokhub_core::screen_recording_card(
        &done.dir.display().to_string(),
        &title,
        &found.summary,
        chat,
        now_ms(),
    );
    let _ = crate::pulse::add_post(config_dir, card);
    if !chat.is_empty() {
        let text = if found.sent {
            hub().offered.insert(
                chat.to_string(),
                Recorded {
                    dir: done.dir.clone(),
                    note: note.to_string(),
                    secs: done.secs,
                },
            );
            format!("{}\n\n{}", found.text, crate::screen_skill::OFFER)
        } else {
            found.text.clone()
        };
        (reporter.deliver)(chat, &text);
    }
    end_call(id);
    Event {
        kind: "record",
        ok: true,
        text: finished_text(done, note, &found.summary),
        chat: chat.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::ModelClient;
    use std::sync::Mutex as TestLock;

    /// Tests share the one recording hub.
    static SERIAL: TestLock<()> = TestLock::new(());

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-screenrec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cfg")).unwrap();
        std::fs::create_dir_all(dir.join("files")).unwrap();
        grokhub_core::user_files::set_files_root(&dir.join("cfg"), &dir.join("files")).unwrap();
        dir
    }

    fn recordings(cfg: &Path) -> PathBuf {
        cfg.parent().unwrap().join("files").join("Recordings")
    }

    fn png() -> Still {
        (vec![0x89, b'P', b'N', b'G'], "image/png".into())
    }

    #[test]
    fn a_recording_takes_a_still_every_two_seconds_until_stop() {
        let dir = scratch("loop").join("rec");
        let stop = AtomicBool::new(false);
        let clock = std::cell::Cell::new(0u64);
        let waits = std::cell::Cell::new(0u32);
        let done = record_loop(
            &dir,
            &stop,
            png_ok,
            || false,
            || clock.get(),
            || {
                clock.set(clock.get() + 1000);
                waits.set(waits.get() + 1);
                if waits.get() == 7 {
                    stop.store(true, Ordering::Relaxed);
                }
            },
        );
        // Stills at 0, 2, 4 and 6 seconds; Stop lands at 7.
        assert_eq!(done.frames.len(), 4);
        assert_eq!(done.secs, 7);
        assert_eq!(done.err, None);
        assert_eq!(done.frames[0], dir.join("frame-001.png"));
        assert_eq!(std::fs::read(&done.frames[3]).unwrap(), png().0);
        assert_eq!(
            finished_text(&done, "video stutters", "Likely cause: Decoding is off."),
            "Screen recording 0:07: video stutters: 4 stills saved. Likely cause: Decoding is off."
        );
    }

    fn png_ok() -> Result<Still, String> {
        Ok(png())
    }

    #[test]
    fn the_cap_ends_a_recording_at_two_minutes() {
        let dir = scratch("cap").join("rec");
        let stop = AtomicBool::new(false);
        let clock = std::cell::Cell::new(0u64);
        let done = record_loop(
            &dir,
            &stop,
            png_ok,
            || false,
            || clock.get(),
            || clock.set(clock.get() + 1000),
        );
        assert_eq!(done.secs, 120);
        assert_eq!(done.frames.len(), 60);
    }

    #[test]
    fn a_held_game_guard_takes_no_stills_but_stop_still_ends_it() {
        let dir = scratch("held").join("rec");
        let stop = AtomicBool::new(false);
        let clock = std::cell::Cell::new(0u64);
        let done = record_loop(
            &dir,
            &stop,
            || panic!("no still while held"),
            || true,
            || clock.get(),
            || {
                clock.set(clock.get() + 1000);
                if clock.get() >= 5000 {
                    stop.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(done.frames.is_empty());
        assert_eq!(
            not_started_text(&done, ""),
            "Screen recording 0:05 (no note): the recording didn't start: no stills were taken."
        );
    }

    #[test]
    fn a_failed_still_ends_the_recording_with_what_it_has() {
        let dir = scratch("fail").join("rec");
        let stop = AtomicBool::new(false);
        let clock = std::cell::Cell::new(0u64);
        let mut n = 0;
        let done = record_loop(
            &dir,
            &stop,
            || {
                n += 1;
                if n == 2 {
                    Err("the display went away".into())
                } else {
                    Ok(png())
                }
            },
            || false,
            || clock.get(),
            || clock.set(clock.get() + 1000),
        );
        assert_eq!(done.frames.len(), 1);
        assert_eq!(done.err.as_deref(), Some("the display went away"));
        assert_eq!(
            finished_text(&done, "x", "Nothing stood out in the stills"),
            "Screen recording 0:02: x: 1 still saved. Nothing stood out in the stills. It ended early: the display went away."
        );
    }

    #[test]
    fn no_consent_means_a_question_and_no_folder() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("consent").join("cfg");
        let got = slash("/record video stutters", &cfg, false, None, Reporter::local)
            .unwrap()
            .unwrap();
        assert_eq!(
            got,
            Effect::Confirm {
                prompt: "Record your screen now? It takes a still every 2 seconds into your Recordings folder. When it stops, up to 6 stills and your note go to Grok once, and the report comes back in this chat.".into(),
                yes: "/record --allow video stutters".into(),
            }
        );
        assert!(!consented(&cfg));
        assert!(!recordings(&cfg).exists());
        assert!(hub().rec.is_none());
    }

    /// A yes from when recording sent nothing doesn't cover the report.
    #[test]
    fn an_older_yes_that_sent_nothing_asks_again() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("old-yes").join("cfg");
        std::fs::write(cfg.join("app.json"), r#"{"screen_record":true}"#).unwrap();
        assert!(!consented(&cfg));
        let got = slash("/record", &cfg, false, None, Reporter::local)
            .unwrap()
            .unwrap();
        assert!(
            matches!(&got, Effect::Confirm { prompt, yes } if prompt == CONSENT_PROMPT && yes == "/record --allow"),
            "{got:?}"
        );
        assert!(!recordings(&cfg).exists());
    }

    #[test]
    fn saying_yes_turns_consent_on_and_keeps_the_other_settings() {
        let cfg = scratch("allow").join("cfg");
        std::fs::write(cfg.join("app.json"), r#"{"weekly_cap_usd":20}"#).unwrap();
        allow(&cfg).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(cfg.join("app.json")).unwrap()).unwrap();
        assert_eq!(
            v,
            json!({"weekly_cap_usd": 20, "screen_record": true, "screen_record_send": true})
        );
        assert!(consented(&cfg));
        // A file that doesn't parse is left as it is.
        std::fs::write(cfg.join("app.json"), "{nope").unwrap();
        assert!(allow(&cfg).is_err());
        assert_eq!(
            std::fs::read_to_string(cfg.join("app.json")).unwrap(),
            "{nope"
        );
    }

    #[test]
    fn a_remote_browser_can_never_start_or_stop_a_recording() {
        let cfg = scratch("remote").join("cfg");
        std::fs::write(cfg.join("app.json"), CONSENTED).unwrap();
        for line in ["/record", "/record stop", "/audiocheck"] {
            let err = slash(line, &cfg, true, None, Reporter::local)
                .unwrap()
                .unwrap_err();
            assert!(err.contains("your computer"), "{line}: {err}");
        }
        assert!(hub().rec.is_none());
    }

    #[test]
    fn other_commands_are_not_ours() {
        let cfg = scratch("other").join("cfg");
        assert!(slash("/new", &cfg, false, None, Reporter::local).is_none());
        assert!(slash("hello", &cfg, false, None, Reporter::local).is_none());
    }

    const CONSENTED: &str = r#"{"screen_record":true,"screen_record_send":true}"#;

    const REPLY: &str = "Seen: The settings menu opens slowly.\nLikely cause: The menu loads every plugin first.\nFix steps:\n1. Turn off unused plugins.";

    /// A real 32 by 18 PNG still, so the report can read it back.
    fn real_png() -> Result<Still, String> {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(32, 18, image::Rgb([200, 40, 40]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .map_err(|e| e.to_string())?;
        Ok((bytes, "image/png".into()))
    }

    /// Answers once `go` says so; a Halt first ends the call as cancelled.
    struct Held {
        go: Mutex<std::sync::mpsc::Receiver<()>>,
        asked: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ModelClient for Held {
        fn stream(
            &self,
            req: &grokhub_agent::ResponsesRequest,
            cancel: &CancelToken,
            _sink: &mut dyn FnMut(grokhub_agent::StreamEvent),
        ) -> Result<grokhub_agent::TurnOutput, grokhub_agent::ClientError> {
            assert_eq!(req.conversation_id, "screen-record");
            self.asked.fetch_add(1, Ordering::SeqCst);
            let go = self.go.lock().unwrap();
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(10) {
                if cancel.is_cancelled() {
                    return Err(grokhub_agent::ClientError::Cancelled);
                }
                if go.recv_timeout(Duration::from_millis(20)).is_ok() {
                    return Ok(grokhub_agent::TurnOutput {
                        text: REPLY.into(),
                        ..Default::default()
                    });
                }
            }
            Err(grokhub_agent::ClientError::Protocol(
                "test timed out".into(),
            ))
        }
    }

    type Delivered = Arc<Mutex<Vec<(String, String)>>>;

    /// A reporter whose call waits for the test, and the chat it fills.
    fn held_reporter() -> (
        Reporter,
        std::sync::mpsc::Sender<()>,
        Arc<std::sync::atomic::AtomicUsize>,
        Delivered,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = Held {
            go: Mutex::new(rx),
            asked: Arc::clone(&asked),
        };
        let delivered: Delivered = Arc::default();
        let into = Arc::clone(&delivered);
        let reporter = Reporter {
            client: Box::new(move || Ok(Box::new(client) as Box<dyn ModelClient>)),
            deliver: Box::new(move |chat, text| {
                into.lock().unwrap().push((chat.into(), text.into()));
            }),
            confirm: Box::new(|_, _, _| {}),
        };
        (reporter, tx, asked, delivered)
    }

    fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
        let t = Instant::now();
        while !ready() {
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "waited too long for {what}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn first_still(cfg: &Path) -> bool {
        std::fs::read_dir(recordings(cfg))
            .ok()
            .and_then(|mut d| d.next())
            .is_some_and(|e| {
                e.ok()
                    .is_some_and(|e| e.path().join("frame-001.png").exists())
            })
    }

    fn events_now() -> Vec<Event> {
        let mut events = Vec::new();
        wait_for("the finished event", || {
            events = status().events;
            !events.is_empty()
        });
        events
    }

    #[test]
    fn stop_clears_the_indicator_then_the_report_reaches_the_chat_and_pulse() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("e2e").join("cfg");
        std::fs::write(cfg.join("app.json"), CONSENTED).unwrap();
        let _ = status();
        let (reporter, go, asked, delivered) = held_reporter();
        let said = start(&cfg, "slow menu", "chat-1", || None, || real_png, reporter).unwrap();
        assert!(said.starts_with("Recording your screen into "));
        assert!(said.ends_with("It stops on its own at 2:00."));
        assert_eq!(
            slash("/record again", &cfg, false, None, Reporter::local)
                .unwrap()
                .unwrap_err(),
            "Already recording. Stop it first."
        );
        let live = status();
        assert!(live.recording.unwrap().text.starts_with("● Recording 0:0"));
        wait_for("the first still", || first_still(&cfg));
        assert_eq!(
            slash("/record stop", &cfg, false, None, Reporter::local)
                .unwrap()
                .unwrap(),
            Effect::Notice {
                text: "Stopping the recording…".into()
            }
        );
        // Grok is asked while the red clock is already gone.
        wait_for("the call", || asked.load(Ordering::SeqCst) == 1);
        let mid = status();
        assert_eq!(mid.recording, None);
        assert_eq!(
            mid.diagnosing.map(|l| l.text).as_deref(),
            Some("Looking at 1 still…")
        );
        assert!(delivered.lock().unwrap().is_empty(), "no report yet");
        go.send(()).unwrap();
        let events = events_now();
        assert_eq!(events.len(), 1);
        assert!(events[0].ok);
        assert_eq!(events[0].chat, "chat-1");
        assert!(
            events[0].text.ends_with(
                ": slow menu: 1 still saved. Likely cause: The menu loads every plugin first."
            ),
            "{}",
            events[0].text
        );
        let delivered = delivered.lock().unwrap().clone();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].0, "chat-1");
        let dir = std::fs::read_dir(recordings(&cfg))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let title = delivered[0].1.lines().next().unwrap().to_string();
        assert!(title.ends_with(": slow menu"), "{title}");
        assert_eq!(
            delivered[0].1,
            format!(
                "{title}\n\nWhat I saw: The settings menu opens slowly.\nLikely cause: The menu loads every plugin first.\nSuggested fix steps:\n1. Turn off unused plugins.\n\nNothing was changed on this computer. The recording is in {}.\n\nTo keep the steps you showed as a skill, send /record --skill.",
                dir.display()
            )
        );
        let cards: Vec<_> = crate::pulse::cards(&cfg)
            .unwrap()
            .into_iter()
            .filter(|c| c.source_id.starts_with("screenrec:"))
            .collect();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].source_id, format!("screenrec:{}", dir.display()));
        assert_eq!(cards[0].title, title);
        assert_eq!(
            cards[0].body.as_deref(),
            Some("Likely cause: The menu loads every plugin first.")
        );
        assert_eq!(
            cards[0].action,
            Some(grokhub_core::UpdateAction::OpenSession {
                thread_id: "chat-1".into()
            })
        );
        let after = status();
        assert_eq!((after.recording, after.diagnosing), (None, None));
        assert!(!stop(), "nothing left to stop");
    }

    #[test]
    fn halt_while_grok_looks_cancels_the_call_and_keeps_the_recording_local() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("halt").join("cfg");
        let _ = status();
        let (reporter, _go, asked, delivered) = held_reporter();
        start(&cfg, "", "chat-2", || None, || real_png, reporter).unwrap();
        wait_for("the first still", || first_still(&cfg));
        assert!(stop());
        wait_for("the call", || asked.load(Ordering::SeqCst) == 1);
        halt();
        let events = events_now();
        assert!(
            events[0].text.ends_with(
                "1 still saved. Saved on this computer, not diagnosed: Halt stopped it."
            ),
            "{}",
            events[0].text
        );
        let delivered = delivered.lock().unwrap().clone();
        assert!(
            delivered[0]
                .1
                .contains("Saved on this computer only, not diagnosed: Halt stopped it."),
            "{}",
            delivered[0].1
        );
        assert_eq!(status().diagnosing, None);
    }

    /// A second recording can start while Grok still looks at the first.
    /// Each report keeps its own line and its own Halt.
    #[test]
    fn two_reports_in_flight_each_keep_their_line_and_their_halt() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("two").join("cfg");
        let _ = status();
        let (first, go_first, asked_first, _) = held_reporter();
        start(&cfg, "one", "chat-a", || None, || real_png, first).unwrap();
        wait_for("the first still", || first_still(&cfg));
        assert!(stop());
        wait_for("the first call", || asked_first.load(Ordering::SeqCst) == 1);
        let (second, _go_second, asked_second, delivered_second) = held_reporter();
        start(&cfg, "two", "chat-b", || None, || real_png, second).unwrap();
        let stills = || {
            std::fs::read_dir(recordings(&cfg))
                .map(|d| {
                    d.flatten()
                        .filter(|e| e.path().join("frame-001.png").exists())
                        .count()
                })
                .unwrap_or(0)
        };
        wait_for("the second recording's own folder", || stills() == 2);
        assert!(stop());
        wait_for("the second call", || {
            asked_second.load(Ordering::SeqCst) == 1
        });
        go_first.send(()).unwrap();
        let events = events_now();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].chat, "chat-a");
        assert_eq!(
            status().diagnosing.map(|l| l.text).as_deref(),
            Some("Looking at 1 still…"),
            "the second report still shows its line"
        );
        halt();
        let events = events_now();
        assert_eq!(events[0].chat, "chat-b");
        assert!(
            events[0]
                .text
                .ends_with("Saved on this computer, not diagnosed: Halt stopped it."),
            "{}",
            events[0].text
        );
        assert!(delivered_second.lock().unwrap()[0]
            .1
            .contains("not diagnosed: Halt stopped it."));
        assert_eq!(status().diagnosing, None);
    }

    #[test]
    fn halt_during_the_capture_sends_nothing() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("halt-rec").join("cfg");
        let _ = status();
        let (reporter, _go, asked, delivered) = held_reporter();
        start(&cfg, "", "chat-3", || None, || real_png, reporter).unwrap();
        wait_for("the first still", || first_still(&cfg));
        halt();
        let events = events_now();
        assert!(
            events[0].text.ends_with(
                "1 still saved. Saved on this computer, not diagnosed: Halt stopped it before anything was sent."
            ),
            "{}",
            events[0].text
        );
        assert_eq!(asked.load(Ordering::SeqCst), 0, "Grok was never asked");
        assert_eq!(delivered.lock().unwrap().len(), 1);
    }

    /// Answers every call with `reply` at once.
    struct Says(&'static str);

    impl ModelClient for Says {
        fn stream(
            &self,
            _req: &grokhub_agent::ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(grokhub_agent::StreamEvent),
        ) -> Result<grokhub_agent::TurnOutput, grokhub_agent::ClientError> {
            Ok(grokhub_agent::TurnOutput {
                text: self.0.into(),
                ..Default::default()
            })
        }
    }

    type Asked3 = Arc<Mutex<Vec<(String, String, String)>>>;

    /// A reporter that answers `reply` and keeps what it shows.
    fn says(reply: &'static str) -> (Reporter, Delivered, Asked3) {
        let delivered: Delivered = Arc::default();
        let asked: Asked3 = Arc::default();
        let (into, ask_into) = (Arc::clone(&delivered), Arc::clone(&asked));
        let reporter = Reporter {
            client: Box::new(move || Ok(Box::new(Says(reply)) as Box<dyn ModelClient>)),
            deliver: Box::new(move |chat, text| {
                into.lock().unwrap().push((chat.into(), text.into()));
            }),
            confirm: Box::new(move |chat, prompt, yes| {
                ask_into
                    .lock()
                    .unwrap()
                    .push((chat.into(), prompt.into(), yes.into()));
            }),
        };
        (reporter, delivered, asked)
    }

    /// Records one still in `cfg` with the note, and waits for the report.
    fn record_one(cfg: &Path, note: &str, reporter: Reporter) -> Vec<Event> {
        let _ = status();
        start(cfg, note, "chat-s", || None, || real_png, reporter).unwrap();
        wait_for("the first still", || first_still(cfg));
        assert!(stop());
        events_now()
    }

    const STEPS: &str = "1. Open the Reports menu.\n2. Click Export.\n3. Choose PDF.";

    #[test]
    fn a_diagnosed_report_offers_save_as_skill_and_yes_saves_it_through_the_ledger() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("skill").join("cfg");
        std::fs::write(cfg.join("app.json"), CONSENTED).unwrap();
        let (first, delivered, _) = says(REPLY);
        record_one(&cfg, "export the report", first);
        let report = delivered.lock().unwrap()[0].1.clone();
        assert!(
            report.ends_with("\n\nTo keep the steps you showed as a skill, send /record --skill."),
            "{report}"
        );
        let (learn, delivered, asked) = says(STEPS);
        assert_eq!(
            slash("/record --skill", &cfg, false, Some("chat-s"), || learn)
                .unwrap()
                .unwrap(),
            Effect::Notice {
                text: "Reading the steps from 1 still…".into()
            }
        );
        wait_for("the draft", || !asked.lock().unwrap().is_empty());
        let (chat, prompt, yes) = asked.lock().unwrap()[0].clone();
        assert_eq!(chat, "chat-s");
        assert_eq!(
            prompt,
            "Save these steps as the skill /export-the-report?\n\n1. Open the Reports menu.\n2. Click Export.\n3. Choose PDF.\n\nSteps that need your approval still ask every time it runs."
        );
        assert!(yes.starts_with("/record --save-skill "), "{yes}");
        assert!(delivered.lock().unwrap().is_empty());
        let file = cfg.join("skills/export-the-report/SKILL.md");
        assert!(!file.exists(), "nothing is saved before Yes");
        assert_eq!(
            slash(&yes, &cfg, false, Some("chat-s"), Reporter::local)
                .unwrap()
                .unwrap(),
            Effect::Notice {
                text: "Saved the skill /export-the-report. /skills undo export-the-report takes it back. Steps that need your approval still ask every time it runs.".into()
            }
        );
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(
            saved.contains("1. Open the Reports menu.\n2. Click Export.\n3. Choose PDF."),
            "{saved}"
        );
        assert_eq!(
            slash(&yes, &cfg, false, Some("chat-s"), Reporter::local)
                .unwrap()
                .unwrap_err(),
            "That skill draft is gone. Send /record --skill to read the steps again."
        );
        crate::changes::undo(
            &cfg,
            grokhub_agent::harness::ChangeKind::Skill,
            "export-the-report",
            false,
        )
        .unwrap();
        assert!(!file.exists(), "undo takes the new skill back");
        assert_eq!(status().diagnosing, None);
    }

    #[test]
    fn save_as_skill_refuses_without_a_recording_a_chat_or_from_a_browser() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("skill-no").join("cfg");
        std::fs::write(cfg.join("app.json"), CONSENTED).unwrap();
        hub().offered.clear();
        let no = |line: &str, chat: Option<&str>, remote: bool| {
            slash(line, &cfg, remote, chat, Reporter::local)
                .unwrap()
                .unwrap_err()
        };
        assert_eq!(
            no("/record --skill", Some("chat-s"), false),
            "No diagnosed recording in this chat to learn from. Show the steps with /record first."
        );
        assert_eq!(
            no("/record --skill", None, false),
            "Send /record --skill from a chat, so the draft has somewhere to go."
        );
        assert_eq!(
            no("/record --skill", Some("chat-s"), true),
            "Save as skill works on your computer, not from a browser."
        );
        assert_eq!(
            no("/record --save-skill 1", Some("chat-s"), true),
            "Save as skill works on your computer, not from a browser."
        );
    }

    #[test]
    fn a_recording_with_no_clear_steps_saves_nothing_and_says_so() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("skill-none").join("cfg");
        std::fs::write(cfg.join("app.json"), CONSENTED).unwrap();
        record_one(&cfg, "", says(REPLY).0);
        let (learn, delivered, asked) = says("The screen shows a desktop.");
        slash("/record --skill", &cfg, false, Some("chat-s"), || learn)
            .unwrap()
            .unwrap();
        wait_for("the answer", || !delivered.lock().unwrap().is_empty());
        let (chat, text) = delivered.lock().unwrap()[0].clone();
        assert_eq!(chat, "chat-s");
        assert!(
            text.ends_with("\n\nNo clear steps found in the recording. Nothing was saved."),
            "{text}"
        );
        assert!(asked.lock().unwrap().is_empty());
        assert!(!cfg.join("skills").exists());
    }

    /// How many recordings in `cfg` have their first still.
    fn with_stills(cfg: &Path) -> usize {
        std::fs::read_dir(recordings(cfg)).map_or(0, |d| {
            d.filter_map(Result::ok)
                .filter(|e| e.path().join("frame-001.png").exists())
                .count()
        })
    }

    /// Learns in `chat` with a model that answers `steps`, and returns the
    /// draft's question, or the chat's answer when no draft came.
    fn learn_in(cfg: &Path, chat: &str, steps: &'static str) -> Result<String, String> {
        let (learn, delivered, asked) = says(steps);
        slash("/record --skill", cfg, false, Some(chat), || learn).unwrap()?;
        wait_for("the draft or the answer", || {
            !asked.lock().unwrap().is_empty() || !delivered.lock().unwrap().is_empty()
        });
        let asked = asked.lock().unwrap().clone();
        match asked.first() {
            Some((at, prompt, _)) => {
                assert_eq!(at, chat);
                Ok(prompt.clone())
            }
            None => Err(delivered.lock().unwrap()[0].1.clone()),
        }
    }

    #[test]
    fn save_as_skill_learns_from_the_chats_sent_recording_never_a_halted_or_local_one() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("skill-which").join("cfg");
        std::fs::write(cfg.join("app.json"), CONSENTED).unwrap();
        hub().offered.clear();
        record_one(&cfg, "export the report", says(REPLY).0);
        // A second recording in the same chat, halted while Grok looks.
        let (reporter, _go, asked, _) = held_reporter();
        start(
            &cfg,
            "rename the files",
            "chat-s",
            || None,
            || real_png,
            reporter,
        )
        .unwrap();
        wait_for("the second still", || with_stills(&cfg) == 2);
        assert!(stop());
        wait_for("the call", || asked.load(Ordering::SeqCst) == 1);
        halt();
        let _ = events_now();
        // And a third, which stays local: no sign-in.
        start(
            &cfg,
            "clean the desktop",
            "chat-s",
            || None,
            || real_png,
            Reporter::local(),
        )
        .unwrap();
        wait_for("the third still", || with_stills(&cfg) == 3);
        assert!(stop());
        let _ = events_now();
        assert_eq!(
            learn_in(&cfg, "chat-s", STEPS).unwrap(),
            "Save these steps as the skill /export-the-report?\n\n1. Open the Reports menu.\n2. Click Export.\n3. Choose PDF.\n\nSteps that need your approval still ask every time it runs.",
            "the sent recording, not the halted or the local one"
        );
        assert_eq!(
            learn_in(&cfg, "chat-t", STEPS).unwrap_err(),
            "No diagnosed recording in this chat to learn from. Show the steps with /record first."
        );
    }

    #[test]
    fn a_note_with_no_usable_name_takes_the_first_steps_name_or_says_why_it_cant() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("skill-name").join("cfg");
        std::fs::write(cfg.join("app.json"), CONSENTED).unwrap();
        record_one(&cfg, "导出报告", says(REPLY).0);
        let prompt = learn_in(&cfg, "chat-s", STEPS).unwrap();
        assert!(
            prompt.starts_with("Save these steps as the skill /open-the-reports-menu?\n\n"),
            "{prompt}"
        );
        let text = learn_in(&cfg, "chat-s", "1. 打开报告菜单。\n2. 点击导出。").unwrap_err();
        assert!(
            text.ends_with("\n\nFound the steps, but they can't be kept as a skill: a skill needs a name made of letters or digits and steps that are safe to replay. Nothing was saved."),
            "{text}"
        );
        assert!(!cfg.join("skills").exists());
    }

    #[test]
    fn a_held_guard_refuses_before_any_folder_is_made() {
        let cfg = scratch("guard").join("cfg");
        let err = start(
            &cfg,
            "",
            "",
            || Some("An anti-cheat game is running.".into()),
            || png_ok,
            Reporter::local(),
        )
        .unwrap_err();
        assert_eq!(err, "An anti-cheat game is running.");
        assert!(!recordings(&cfg).exists());
    }

    #[test]
    fn the_command_list_offers_both_here_and_greys_them_for_a_browser() {
        let local = crate::slash::hits("/re", false);
        let rec = local.iter().find(|h| h.cmd == "/record").unwrap();
        assert_eq!(rec.insert, "/record ");
        assert!(!rec.run_on_pick);
        assert_eq!(rec.disabled, None);
        let audio = crate::slash::hits("/audio", false);
        assert_eq!(audio[0].cmd, "/audiocheck");
        assert!(audio[0].run_on_pick);
        assert_eq!(audio[0].disabled, None);
        let remote = crate::slash::hits("/audio", true);
        assert_eq!(remote[0].disabled.as_deref(), Some("On your computer"));
    }

    #[test]
    fn the_record_row_promises_a_report_on_what_it_shows() {
        let rec = crate::slash::hits("/record", false);
        assert_eq!(rec[0].cmd, "/record");
        assert_eq!(
            rec[0].hint,
            "Record my screen for up to 2 minutes, then get a report (say what's wrong)"
        );
    }
}
