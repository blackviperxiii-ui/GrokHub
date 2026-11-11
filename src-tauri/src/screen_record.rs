//! "Record my screen" in the window app, as the cabin does it: `/record
//! [what's wrong]` takes a still every 2 seconds into the Recordings folder
//! of your files until Stop, `/record stop`, Halt or the 2-minute cap. It
//! needs consent first (`screen_record` in `app.json`, the cabin's setting),
//! never starts from a remote browser, and is held while an anti-cheat game
//! runs. Nothing is sent anywhere; the stills stay in their local folder.

use crate::slash::Effect;
use grokhub_core::screen_record as sr;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The consent switch in `app.json`, shared with the cabin's Settings.
const CONSENT_KEY: &str = "screen_record";
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
}

/// What the top bar's indicator and the notice bell read.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Event {
    /// `record` or `audio`.
    pub kind: &'static str,
    pub ok: bool,
    pub text: String,
}

#[derive(Default)]
struct Hub {
    rec: Option<Live>,
    events: Vec<Event>,
}

static HUB: Mutex<Hub> = Mutex::new(Hub {
    rec: None,
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
        events: std::mem::take(&mut h.events),
    }
}

pub fn push_event(event: Event) {
    hub().events.push(event);
}

/// Stop the recording now (the indicator's Stop, `/record stop`, Halt).
/// False when none was running.
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

/// Whether Settings → Screen recording is on in `app.json`.
pub fn consented(config_dir: &Path) -> bool {
    let path = app_json(config_dir);
    if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= APP_JSON_CAP) {
        return false;
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| v.get(CONSENT_KEY).and_then(Value::as_bool))
        .unwrap_or(false)
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
    app.as_object_mut()
        .ok_or("The settings file isn't an object.")?
        .insert(CONSENT_KEY.into(), json!(true));
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

fn finished_text(done: &Done, note: &str) -> (bool, String) {
    let title = sr::recording_title(done.secs, note);
    if done.frames.is_empty() {
        let why = done
            .err
            .clone()
            .unwrap_or_else(|| "no stills were taken".into());
        return (
            false,
            format!("{title}: the recording didn't start: {why}."),
        );
    }
    let stills = match done.frames.len() {
        1 => "1 still".to_string(),
        n => format!("{n} stills"),
    };
    let stopped = done
        .err
        .as_ref()
        .map(|e| format!(" It ended early: {e}."))
        .unwrap_or_default();
    (
        true,
        format!(
            "{title}: {stills} saved in {}. They stay on this computer; nothing was sent.{stopped}",
            done.dir.display()
        ),
    )
}

/// `/record`, `/record stop` and `/audiocheck` for a line typed on this
/// computer. `None` when `line` is some other command.
pub fn slash(line: &str, config_dir: &Path, remote: bool) -> Option<Result<Effect, String>> {
    use grokhub_core::Slash;
    let parsed = grokhub_core::parse_slash(line)?;
    let result = match parsed {
        Slash::Record(note) => start_or_ask(config_dir, &note, remote),
        Slash::RecordStop => stop_line(remote),
        Slash::AudioCheck(want) => crate::audio_check::start(&want, remote).map_err(Ask::No),
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

fn start_or_ask(config_dir: &Path, note: &str, remote: bool) -> Result<String, Ask> {
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
                prompt: "Screen recording is off. Turn it on and record your screen now? It takes a still every 2 seconds into your Recordings folder and sends nothing.".into(),
                yes: format!("/record {ALLOW} {note}").trim_end().to_string(),
            }));
        }
        allow(config_dir)?;
    }
    start(config_dir, note, held_reason, grabber).map_err(Ask::No)
}

/// Start the capture thread. The game guard is read first, so a held
/// recording makes no folder.
fn start<G: FnMut() -> Result<Still, String>>(
    config_dir: &Path,
    note: &str,
    guard: impl FnOnce() -> Option<String>,
    make_grabber: impl FnOnce() -> G + Send + 'static,
) -> Result<String, String> {
    if let Some(why) = guard() {
        return Err(why);
    }
    let recordings = grokhub_core::user_files::folder(
        config_dir,
        grokhub_core::user_files::UserFolder::Recordings,
    )
    .map_err(|e| grokhub_core::user_files::plain_io_error(&e))?;
    let dir = sr::recording_dir(&recordings, &stamp(now_ms()));
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut h = hub();
        if h.rec.is_some() {
            return Err(sr::ALREADY_RECORDING.into());
        }
        h.rec = Some(Live {
            stop: Arc::clone(&stop),
            started: Instant::now(),
        });
    }
    let into = dir.clone();
    let note = note.to_string();
    std::thread::spawn(move || {
        let held = || held_reason().is_some();
        let done = record_loop(&into, &stop, make_grabber(), held, now_ms, || {
            std::thread::sleep(Duration::from_millis(250))
        });
        let (ok, text) = finished_text(&done, &note);
        let mut h = hub();
        h.rec = None;
        h.events.push(Event {
            kind: "record",
            ok,
            text,
        });
    });
    Ok(format!(
        "Recording your screen into {}. Show the problem, then press Stop or send /record stop. It stops on its own at {}.",
        dir.display(),
        sr::clock(sr::MAX_SECS)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let (ok, text) = finished_text(&done, "video stutters");
        assert!(ok);
        assert!(text.starts_with("Screen recording 0:07: video stutters: 4 stills saved in "));
        assert!(text.contains("nothing was sent"));
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
        let (ok, text) = finished_text(&done, "");
        assert!(!ok);
        assert!(text.contains("no stills were taken"));
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
        let (ok, text) = finished_text(&done, "x");
        assert!(ok);
        assert!(text.ends_with("It ended early: the display went away."));
    }

    #[test]
    fn no_consent_means_a_question_and_no_folder() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("consent").join("cfg");
        let got = slash("/record video stutters", &cfg, false)
            .unwrap()
            .unwrap();
        assert_eq!(
            got,
            Effect::Confirm {
                prompt: "Screen recording is off. Turn it on and record your screen now? It takes a still every 2 seconds into your Recordings folder and sends nothing.".into(),
                yes: "/record --allow video stutters".into(),
            }
        );
        assert!(!consented(&cfg));
        assert!(!recordings(&cfg).exists());
        assert!(hub().rec.is_none());
    }

    #[test]
    fn saying_yes_turns_consent_on_and_keeps_the_other_settings() {
        let cfg = scratch("allow").join("cfg");
        std::fs::write(cfg.join("app.json"), r#"{"weekly_cap_usd":20}"#).unwrap();
        allow(&cfg).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(cfg.join("app.json")).unwrap()).unwrap();
        assert_eq!(v, json!({"weekly_cap_usd": 20, "screen_record": true}));
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
        std::fs::write(cfg.join("app.json"), r#"{"screen_record":true}"#).unwrap();
        for line in ["/record", "/record stop", "/audiocheck"] {
            let err = slash(line, &cfg, true).unwrap().unwrap_err();
            assert!(err.contains("your computer"), "{line}: {err}");
        }
        assert!(hub().rec.is_none());
    }

    #[test]
    fn other_commands_are_not_ours() {
        let cfg = scratch("other").join("cfg");
        assert!(slash("/new", &cfg, false).is_none());
        assert!(slash("hello", &cfg, false).is_none());
    }

    #[test]
    fn start_then_stop_saves_the_stills_and_posts_one_event() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = scratch("e2e").join("cfg");
        std::fs::write(cfg.join("app.json"), r#"{"screen_record":true}"#).unwrap();
        let _ = status();
        let said = start(&cfg, "slow menu", || None, || png_ok).unwrap();
        assert!(said.starts_with("Recording your screen into "));
        assert!(said.ends_with("It stops on its own at 2:00."));
        assert_eq!(
            slash("/record again", &cfg, false).unwrap().unwrap_err(),
            "Already recording. Stop it first."
        );
        let live = status();
        assert!(live.recording.unwrap().text.starts_with("● Recording 0:0"));
        // Wait for the first still, then stop.
        let folder = recordings(&cfg);
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(5)
            && !std::fs::read_dir(&folder)
                .ok()
                .and_then(|mut d| d.next())
                .is_some_and(|e| {
                    e.ok()
                        .is_some_and(|e| e.path().join("frame-001.png").exists())
                })
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            slash("/record stop", &cfg, false).unwrap().unwrap(),
            Effect::Notice {
                text: "Stopping the recording…".into()
            }
        );
        let t = Instant::now();
        let mut events = Vec::new();
        while events.is_empty() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
            events = status().events;
        }
        assert_eq!(events.len(), 1);
        assert!(events[0].ok);
        assert!(
            events[0].text.contains("1 still saved in "),
            "{}",
            events[0].text
        );
        assert!(status().recording.is_none());
        assert!(!stop(), "nothing left to stop");
    }

    #[test]
    fn a_held_guard_refuses_before_any_folder_is_made() {
        let cfg = scratch("guard").join("cfg");
        let err = start(
            &cfg,
            "",
            || Some("An anti-cheat game is running.".into()),
            || png_ok,
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
    fn the_record_row_promises_stills_not_a_diagnosis() {
        let rec = crate::slash::hits("/record", false);
        assert_eq!(rec[0].cmd, "/record");
        assert_eq!(
            rec[0].hint,
            "Record my screen as stills, 2 minutes at most (say what's wrong)"
        );
    }
}
