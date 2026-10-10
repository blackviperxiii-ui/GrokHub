//! Cold-start timing. `begin` runs first thing in `main`; each mark records
//! the time since then as a `startup:*` speed span. The first frame writes one
//! line per launch to `{config}/startup.jsonl` (newest [`KEEP`] kept), off the
//! UI thread.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Launch lines kept in `startup.jsonl`.
pub const KEEP: usize = 20;

static T0: OnceLock<Instant> = OnceLock::new();
static MARKS: Mutex<Vec<(&'static str, u32)>> = Mutex::new(Vec::new());
#[cfg(not(test))]
static WROTE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The process started. Later marks measure from here.
pub fn begin() {
    let _ = T0.get_or_init(Instant::now);
}

/// Time since [`begin`], in microseconds.
fn since_begin() -> u32 {
    let took = T0.get().map(|t0| t0.elapsed()).unwrap_or(Duration::ZERO);
    u32::try_from(took.as_micros()).unwrap_or(u32::MAX)
}

/// `stage` was reached: record the time since the process started.
pub fn mark(stage: &'static str) {
    let us = since_begin();
    grokhub_agent::timing::record(stage, Duration::from_micros(u64::from(us)));
    MARKS.lock().unwrap_or_else(|e| e.into_inner()).push((stage, us));
}

/// The stages reached so far, in order, with microseconds since start.
pub fn marks() -> Vec<(&'static str, u32)> {
    MARKS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Consecutive pieces of one stage: each [`Steps::mark`] records the time
/// since the last one as its own speed span.
pub struct Steps(Instant);

impl Steps {
    pub fn new() -> Self {
        Self(Instant::now())
    }

    pub fn mark(&mut self, name: &'static str) {
        let now = Instant::now();
        grokhub_agent::timing::record(name, now.duration_since(self.0));
        self.0 = now;
    }
}

/// One launch as a JSON line: the marks in order, then `steps`, the slowest
/// pieces of building the cabin (each a duration of its own).
pub fn launch_line(ts_ms: u64, version: &str, marks: &[(&'static str, u32)], steps: &[(&'static str, u32)]) -> String {
    let marks: serde_json::Map<String, serde_json::Value> =
        marks.iter().map(|(name, us)| ((*name).to_string(), (*us).into())).collect();
    let steps: serde_json::Map<String, serde_json::Value> =
        steps.iter().map(|(name, us)| ((*name).to_string(), (*us).into())).collect();
    serde_json::json!({ "ts_ms": ts_ms, "version": version, "marks_us": marks, "steps_us": steps }).to_string()
}

/// `text` (the old file) plus `line`, keeping the newest [`KEEP`] lines.
pub fn append_kept(text: &str, line: &str) -> String {
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines.push(line);
    let skip = lines.len().saturating_sub(KEEP);
    let mut out = lines[skip..].join("\n");
    out.push('\n');
    out
}

/// Has the first frame been marked? (Never in tests: a test cabin grabs no
/// hotkeys and writes no launch line.)
#[cfg(not(test))]
pub fn painted() -> bool {
    WROTE.load(std::sync::atomic::Ordering::SeqCst)
}

/// The first frame is done: mark it and write this launch's line, once.
#[cfg(not(test))]
pub fn first_frame() {
    if WROTE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    mark("startup:first_frame");
    let marks = marks();
    let steps: Vec<(&'static str, u32)> = grokhub_agent::timing::stats()
        .into_iter()
        .filter(|s| s.name.starts_with("startup:new:"))
        .map(|s| (s.name, s.max_us))
        .collect();
    let line = launch_line(grokhub_core::now_ms(), env!("CARGO_PKG_VERSION"), &marks, &steps);
    let path = crate::config::config_dir().join("startup.jsonl");
    std::thread::spawn(move || {
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::write(&path, append_kept(&old, &line));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_line_lists_marks_and_steps() {
        let line = launch_line(
            1_700_000_000_000,
            "2.13.0",
            &[("startup:config", 1_200), ("startup:window", 90_000), ("startup:first_frame", 140_000)],
            &[("startup:new:threads", 4_000)],
        );
        assert_eq!(
            line,
            r#"{"marks_us":{"startup:config":1200,"startup:first_frame":140000,"startup:window":90000},"steps_us":{"startup:new:threads":4000},"ts_ms":1700000000000,"version":"2.13.0"}"#
        );
    }

    #[test]
    fn marks_are_kept_in_the_order_reached() {
        begin();
        mark("startup:config");
        mark("startup:window");
        mark("startup:cabin");
        let got = marks();
        let names: Vec<&str> = got.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["startup:config", "startup:window", "startup:cabin"]);
        assert!(got.windows(2).all(|w| w[0].1 <= w[1].1), "{got:?}");
    }

    #[test]
    fn building_the_cabin_runs_no_subprocess_or_hotkey_grab() {
        let src = include_str!("app/mod.rs");
        let new = src
            .split("pub fn new(hidden: bool) -> Self {")
            .nth(1)
            .and_then(|s| s.split("pub fn warm_startup_caches()").next())
            .expect("Cabin::new");
        // Grabbing the hotkeys takes ~50 ms on X11: a shown window does it after its first frame.
        assert!(!new.contains("GlobalHotKeyManager::new"), "{new}");
        assert!(new.contains("if hidden {\n                c.register_hotkeys_once();"), "{new}");
        // ...in `logic`, which eframe runs even while the window is covered.
        let logic = src.split("fn logic(&mut self").nth(1).and_then(|s| s.split("self.poll_job();").next()).expect("logic");
        assert!(logic.contains("self.register_hotkeys_once();"), "{logic}");
        // The local clock runs `date` and the theme probe runs `gsettings`.
        for blocking in ["local_clock(", "desktop_prefers_dark(", "Command::new("] {
            assert!(!new.contains(blocking), "{blocking} in Cabin::new");
        }
        let feed = include_str!("app/feed_ui.rs");
        let ensure = feed
            .split("fn ensure_useful_ideas(")
            .nth(1)
            .and_then(|s| s.split("\n    }\n").next())
            .expect("ensure_useful_ideas");
        assert!(ensure.contains("self.idea_inputs()") && !ensure.contains("idea_request"), "{ensure}");
        let main = include_str!("main.rs");
        let run = main.split("fn run_cabin(").nth(1).expect("run_cabin");
        assert!(
            run.find("Cabin::warm_startup_caches()").expect("warm") < run.find("eframe::run_native").expect("run"),
            "the clock and theme warm while the window is created"
        );
    }

    #[test]
    fn the_file_keeps_the_newest_lines() {
        let old: String = (0..KEEP).map(|i| format!("{{\"n\":{i}}}\n")).collect();
        let out = append_kept(&old, "{\"n\":99}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), KEEP);
        assert_eq!(lines[0], "{\"n\":1}");
        assert_eq!(lines[KEEP - 1], "{\"n\":99}");
        assert_eq!(append_kept("", "{\"n\":1}"), "{\"n\":1}\n");
    }
}
