//! Event-stream and saved-condition waits for automations.
//!
//! A job may still use a clock or heartbeat schedule to become due; when
//! [`WaitTrigger`] is set, the shell evaluates the wait on each tick **before**
//! claiming a run. A non-match advances `next_run` a short poll later with no
//! model call and no `run_count` bump. Cron / `check_command` / gate paths are
//! unchanged when `wait` is plain.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Most event-stream text kept when priming the model.
pub const WAIT_ASK_CHARS: usize = 8 * 1024;
/// Folder under the config dir for append-only stream files.
pub const STREAMS_DIR: &str = "automation-streams";

/// How an automation waits before a claimed run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WaitTrigger {
    /// `condition` or `event_stream`. Empty means no wait (clock / check only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// Condition expression, or event-stream id.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Optional substring a stream event must contain.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub match_text: String,
    /// Last matched event id (stream) or empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cursor: String,
}

impl WaitTrigger {
    /// Nothing set: older `automations.json` rows stay as they were.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }

    pub fn is_condition(&self) -> bool {
        self.kind.eq_ignore_ascii_case("condition") && !self.source.trim().is_empty()
    }

    pub fn is_event_stream(&self) -> bool {
        self.kind.eq_ignore_ascii_case("event_stream") && !self.source.trim().is_empty()
    }

    pub fn active(&self) -> bool {
        self.is_condition() || self.is_event_stream()
    }

    /// Cursor after a matching stream event ran.
    pub fn remember_cursor(&self, cursor: &str) -> Self {
        Self {
            cursor: cursor.trim().to_string(),
            ..self.clone()
        }
    }
}

/// Delivery / run / completion tracked separately from [`crate::AutoOutcome`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LifePhase {
    #[default]
    None,
    Pending,
    Started,
    Ok,
    Failed,
    Stopped,
}

impl LifePhase {
    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

fn phase_none(p: &LifePhase) -> bool {
    p.is_none()
}

/// One fire's delivery, run, and completion.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoLifecycle {
    #[serde(default, skip_serializing_if = "phase_none")]
    pub delivery: LifePhase,
    #[serde(default, skip_serializing_if = "phase_none")]
    pub run: LifePhase,
    #[serde(default, skip_serializing_if = "phase_none")]
    pub completion: LifePhase,
}

impl AutoLifecycle {
    pub fn is_clear(&self) -> bool {
        *self == Self::default()
    }

    /// Claim starts a run: delivery not yet settled, run started.
    pub fn started() -> Self {
        Self {
            delivery: LifePhase::Pending,
            run: LifePhase::Started,
            completion: LifePhase::Pending,
        }
    }

    /// Finish a fire. Delivery fail does not require a failed run.
    pub fn settled(run_ok: bool, delivery_ok: bool) -> Self {
        let run = if run_ok {
            LifePhase::Ok
        } else {
            LifePhase::Failed
        };
        let delivery = if delivery_ok {
            LifePhase::Ok
        } else {
            LifePhase::Failed
        };
        let completion = if run_ok && delivery_ok {
            LifePhase::Ok
        } else {
            LifePhase::Failed
        };
        Self {
            delivery,
            run,
            completion,
        }
    }
}

/// What a due job with a wait trigger does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitStep {
    /// No wait configured.
    None,
    /// Condition unmet / no new matching events — poll again later.
    Wait,
    /// Ready to claim; `context` can prime the model; `new_cursor` for streams.
    Ready { context: String, new_cursor: String },
}

impl WaitStep {
    pub fn waits(&self) -> bool {
        matches!(self, Self::Wait)
    }
}

/// Evaluate `wait` under `config_dir`. Pure for conditions; streams read disk.
pub fn wait_step(wait: &WaitTrigger, config_dir: &Path) -> WaitStep {
    if wait.is_condition() {
        return condition_step(wait.source.trim());
    }
    if wait.is_event_stream() {
        return stream_step(config_dir, wait.source.trim(), &wait.match_text, &wait.cursor);
    }
    WaitStep::None
}

fn condition_step(source: &str) -> WaitStep {
    match eval_condition(source) {
        Ok(true) => WaitStep::Ready {
            context: format!("Saved condition `{source}` is true."),
            new_cursor: String::new(),
        },
        Ok(false) => WaitStep::Wait,
        Err(why) => WaitStep::Ready {
            // Surface a broken condition as a run (health) rather than silent wait.
            context: format!("Saved condition `{source}` could not be checked: {why}"),
            new_cursor: String::new(),
        },
    }
}

/// `file:<path>`, `file-missing:<path>`, `env:<NAME>`, `env:<NAME>=value`.
pub fn eval_condition(source: &str) -> Result<bool, String> {
    let source = source.trim();
    if let Some(path) = source.strip_prefix("file-missing:") {
        let path = expand_home(path.trim());
        if path.as_os_str().is_empty() {
            return Err("file-missing: needs a path".into());
        }
        return Ok(!path.exists());
    }
    if let Some(path) = source.strip_prefix("file:") {
        let path = expand_home(path.trim());
        if path.as_os_str().is_empty() {
            return Err("file: needs a path".into());
        }
        return Ok(path.is_file() || path.is_dir());
    }
    if let Some(rest) = source.strip_prefix("env:") {
        let rest = rest.trim();
        if rest.is_empty() {
            return Err("env: needs a name".into());
        }
        if let Some((name, want)) = rest.split_once('=') {
            let name = name.trim();
            if name.is_empty() {
                return Err("env: needs a name".into());
            }
            let got = std::env::var(name).unwrap_or_default();
            return Ok(got == want);
        }
        match std::env::var(rest) {
            Ok(v) => Ok(!v.is_empty()),
            Err(_) => Ok(false),
        }
    } else {
        Err(format!(
            "unknown condition `{source}` (use file:, file-missing:, or env:)"
        ))
    }
}

fn expand_home(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = crate::paths::user_home() {
            return home.join(rest);
        }
    }
    if raw == "~" {
        if let Some(home) = crate::paths::user_home() {
            return home;
        }
    }
    PathBuf::from(raw)
}

fn stream_step(config_dir: &Path, stream_id: &str, match_text: &str, cursor: &str) -> WaitStep {
    let path = stream_path(config_dir, stream_id);
    if !path.is_file() {
        return WaitStep::Wait;
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return WaitStep::Wait,
    };
    let needle = match_text.trim();
    let mut after_cursor = cursor.is_empty();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (id, text) = parse_stream_line(line);
        if !after_cursor {
            if id == cursor {
                after_cursor = true;
            }
            continue;
        }
        if !needle.is_empty() && !text.contains(needle) {
            continue;
        }
        let context = if needle.is_empty() {
            format!("Event stream `{stream_id}` has a new event:\n{}", clip(&text, WAIT_ASK_CHARS))
        } else {
            format!(
                "Event stream `{stream_id}` matched `{needle}`:\n{}",
                clip(&text, WAIT_ASK_CHARS)
            )
        };
        return WaitStep::Ready {
            context,
            new_cursor: id,
        };
    }
    WaitStep::Wait
}

/// One JSON object per line: `{"id":"…","text":"…"}`. Plain text lines get a
/// synthetic id from their byte offset label `line-N`.
fn parse_stream_line(line: &str) -> (String, String) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let text = v
            .get("text")
            .and_then(|x| x.as_str())
            .unwrap_or(line)
            .to_string();
        if !id.is_empty() {
            return (id, text);
        }
    }
    (format!("line-{}", simple_hash(line)), line.to_string())
}

fn simple_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Safe file name for a stream id.
pub fn stream_file_stem(stream_id: &str) -> String {
    let mut out = String::new();
    for c in stream_id.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("stream");
    }
    out
}

pub fn stream_path(config_dir: &Path, stream_id: &str) -> PathBuf {
    config_dir
        .join(STREAMS_DIR)
        .join(format!("{}.jsonl", stream_file_stem(stream_id)))
}

/// Append one event to a monitored stream (tests and shell helpers).
pub fn append_stream_event(
    config_dir: &Path,
    stream_id: &str,
    id: &str,
    text: &str,
) -> Result<PathBuf, String> {
    let dir = config_dir.join(STREAMS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = stream_path(config_dir, stream_id);
    let id = if id.trim().is_empty() {
        format!("e-{}", crate::now_ms())
    } else {
        id.trim().to_string()
    };
    let line = serde_json::json!({ "id": id, "text": text }).to_string();
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())?;
    Ok(path)
}

/// Row / summary copy for a wait trigger.
pub fn wait_line(wait: &WaitTrigger) -> Option<String> {
    if wait.is_condition() {
        return Some(format!("waits until `{}`", wait.source.trim()));
    }
    if wait.is_event_stream() {
        let mut s = format!("watches event stream `{}`", wait.source.trim());
        if !wait.match_text.trim().is_empty() {
            s.push_str(&format!(" for `{}`", wait.match_text.trim()));
        }
        return Some(s);
    }
    None
}

/// Short lifecycle summary for the Automations row.
pub fn lifecycle_line(life: &AutoLifecycle) -> Option<String> {
    if life.is_clear() {
        return None;
    }
    Some(format!(
        "run {} · delivery {} · completion {}",
        phase_word(life.run),
        phase_word(life.delivery),
        phase_word(life.completion)
    ))
}

fn phase_word(p: LifePhase) -> &'static str {
    match p {
        LifePhase::None => "—",
        LifePhase::Pending => "pending",
        LifePhase::Started => "started",
        LifePhase::Ok => "ok",
        LifePhase::Failed => "failed",
        LifePhase::Stopped => "stopped",
    }
}

/// Prompt suffix when a wait became ready.
pub fn wait_prompt(instructions: &str, context: &str) -> String {
    format!("{}\n\n{}", instructions.trim(), context.trim())
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Milliseconds to wait before re-checking a non-matching wait.
pub const WAIT_POLL_MS: u64 = 30_000;

/// Advance next poll without counting a run (condition / stream not ready).
pub fn mark_automation_waiting(mut a: crate::Automation, now_ms: u64) -> crate::Automation {
    a.next_run = Some(now_ms.saturating_add(WAIT_POLL_MS));
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn plain_wait_is_none_step() {
        assert_eq!(wait_step(&WaitTrigger::default(), Path::new("/tmp")), WaitStep::None);
    }

    #[test]
    fn file_condition_ready_when_present() {
        let dir = std::env::temp_dir().join(format!("gh-wait-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("ready.flag");
        std::fs::write(&marker, b"1").unwrap();
        let wait = WaitTrigger {
            kind: "condition".into(),
            source: format!("file:{}", marker.display()),
            ..Default::default()
        };
        assert!(matches!(wait_step(&wait, &dir), WaitStep::Ready { .. }));
        let _ = std::fs::remove_file(&marker);
        assert_eq!(wait_step(&wait, &dir), WaitStep::Wait);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_condition_matches_value() {
        let _g = env_lock();
        // SAFETY: exclusive lock above; test-only env key.
        unsafe {
            std::env::set_var("GROKHUB_WAIT_TEST", "yes");
        }
        assert_eq!(eval_condition("env:GROKHUB_WAIT_TEST=yes"), Ok(true));
        assert_eq!(eval_condition("env:GROKHUB_WAIT_TEST=no"), Ok(false));
        assert_eq!(eval_condition("env:GROKHUB_WAIT_TEST"), Ok(true));
        unsafe {
            std::env::remove_var("GROKHUB_WAIT_TEST");
        }
        assert_eq!(eval_condition("env:GROKHUB_WAIT_TEST"), Ok(false));
    }

    #[test]
    fn event_stream_waits_until_new_match() {
        let dir = std::env::temp_dir().join(format!("gh-wait-stream-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        append_stream_event(&dir, "disk", "e1", "ok").unwrap();
        let wait = WaitTrigger {
            kind: "event_stream".into(),
            source: "disk".into(),
            match_text: "alert".into(),
            cursor: String::new(),
        };
        assert_eq!(wait_step(&wait, &dir), WaitStep::Wait);
        append_stream_event(&dir, "disk", "e2", "disk alert now").unwrap();
        match wait_step(&wait, &dir) {
            WaitStep::Ready {
                new_cursor,
                context,
            } => {
                assert_eq!(new_cursor, "e2");
                assert!(context.contains("disk alert now"));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
        let after = WaitTrigger {
            cursor: "e2".into(),
            ..wait
        };
        assert_eq!(wait_step(&after, &dir), WaitStep::Wait);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lifecycle_separates_delivery_from_run() {
        let life = AutoLifecycle::settled(true, false);
        assert_eq!(life.run, LifePhase::Ok);
        assert_eq!(life.delivery, LifePhase::Failed);
        assert_eq!(life.completion, LifePhase::Failed);
        let both = AutoLifecycle::settled(true, true);
        assert_eq!(both.completion, LifePhase::Ok);
        let run_fail = AutoLifecycle::settled(false, true);
        assert_eq!(run_fail.run, LifePhase::Failed);
        assert_eq!(run_fail.delivery, LifePhase::Ok);
        assert_eq!(run_fail.completion, LifePhase::Failed);
    }

    #[test]
    fn wait_and_lifecycle_lines() {
        let wait = WaitTrigger {
            kind: "condition".into(),
            source: "file:/tmp/ready".into(),
            ..Default::default()
        };
        assert_eq!(
            wait_line(&wait).as_deref(),
            Some("waits until `file:/tmp/ready`")
        );
        let life = AutoLifecycle::settled(true, true);
        assert_eq!(
            lifecycle_line(&life).as_deref(),
            Some("run ok · delivery ok · completion ok")
        );
    }

    #[test]
    fn mark_waiting_polls_in_thirty_seconds() {
        let a = crate::Automation {
            id: "a".into(),
            name: "n".into(),
            schedule: "heartbeat".into(),
            time: "09:00".into(),
            times: vec![],
            instructions: "go".into(),
            heartbeat_every_min: 1,
            check_command: String::new(),
            gate: Default::default(),
            wait: Default::default(),
            enabled: true,
            last_run: None,
            next_run: Some(1),
            run_count: 0,
            health: Default::default(),
            lifecycle: Default::default(),
        };
        let b = mark_automation_waiting(a, 1_000);
        assert_eq!(b.next_run, Some(1_000 + WAIT_POLL_MS));
        assert_eq!(b.run_count, 0);
    }
}
