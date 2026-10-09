//! "Record my screen": a consented, local-only recording made of stills taken
//! every few seconds, then one diagnosis of a few keyframes. Nothing here
//! touches the screen, the disk or the network; the app drives it.

use std::path::{Path, PathBuf};

/// Longest recording. The recorder stops itself here.
pub const MAX_SECS: u64 = 120;
/// One still every this many seconds while recording.
pub const FRAME_EVERY_SECS: u64 = 2;
/// Stills sent for diagnosis, spread across the recording.
pub const KEYFRAMES: usize = 6;
/// Characters of the user's note kept in a card title.
pub const NOTE_CHARS: usize = 60;

pub const CONSENT_OFF: &str =
    "Screen recording is off. Turn it on in Settings → Cabin defaults → Screen recording, then try again.";
pub const ALREADY_RECORDING: &str = "Already recording. Stop it first.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecState {
    Idle,
    Recording { started_ms: u64 },
    Stopped { secs: u64, frames: usize },
}

/// Start, frame timing, max length and stop. Starting needs consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorder {
    pub state: RecState,
    frames: usize,
    last_frame_ms: Option<u64>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            state: RecState::Idle,
            frames: 0,
            last_frame_ms: None,
        }
    }
}

impl Recorder {
    pub fn start(&mut self, consented: bool, now_ms: u64) -> Result<(), &'static str> {
        if !consented {
            return Err(CONSENT_OFF);
        }
        if self.is_recording() {
            return Err(ALREADY_RECORDING);
        }
        *self = Self {
            state: RecState::Recording { started_ms: now_ms },
            ..Self::default()
        };
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.state, RecState::Recording { .. })
    }

    pub fn elapsed_secs(&self, now_ms: u64) -> u64 {
        match self.state {
            RecState::Recording { started_ms } => now_ms.saturating_sub(started_ms) / 1000,
            RecState::Stopped { secs, .. } => secs,
            RecState::Idle => 0,
        }
    }

    /// A still is due: the first one right away, then every `FRAME_EVERY_SECS`.
    pub fn frame_due(&self, now_ms: u64) -> bool {
        self.is_recording()
            && !self.should_stop(now_ms)
            && self
                .last_frame_ms
                .is_none_or(|at| now_ms.saturating_sub(at) >= FRAME_EVERY_SECS * 1000)
    }

    pub fn note_frame(&mut self, now_ms: u64) {
        if self.is_recording() {
            self.frames += 1;
            self.last_frame_ms = Some(now_ms);
        }
    }

    /// The recording reached `MAX_SECS`.
    pub fn should_stop(&self, now_ms: u64) -> bool {
        self.is_recording() && self.elapsed_secs(now_ms) >= MAX_SECS
    }

    /// Stop and say how long it ran and how many stills it took.
    pub fn stop(&mut self, now_ms: u64) -> Option<(u64, usize)> {
        if !self.is_recording() {
            return None;
        }
        let secs = self.elapsed_secs(now_ms).min(MAX_SECS);
        self.state = RecState::Stopped {
            secs,
            frames: self.frames,
        };
        Some((secs, self.frames))
    }
}

/// `want` frame indexes spread evenly over `total`, first and last included.
pub fn sample_keyframes(total: usize, want: usize) -> Vec<usize> {
    if total == 0 || want == 0 {
        return Vec::new();
    }
    if total <= want {
        return (0..total).collect();
    }
    if want == 1 {
        return vec![total - 1];
    }
    let mut out: Vec<usize> = (0..want)
        .map(|i| (i * (total - 1) + (want - 1) / 2) / (want - 1))
        .collect();
    out.dedup();
    out
}

/// `0:42`, `2:00`.
pub fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The always-visible indicator while recording.
pub fn indicator_line(secs: u64) -> String {
    format!("● Recording {} of {}", clock(secs), clock(MAX_SECS))
}

fn clip(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        return s;
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// `Screen recording 0:42: video stutters on 4K YouTube`. A recording with no
/// note says so instead of naming nothing.
pub fn recording_title(secs: u64, note: &str) -> String {
    let note = clip(note, NOTE_CHARS);
    if note.is_empty() {
        format!("Screen recording {} (no note)", clock(secs))
    } else {
        format!("Screen recording {}: {note}", clock(secs))
    }
}

/// `~/GrokHub/recordings/<stamp>`: visible, local, one folder per recording.
pub fn recording_dir(home: &Path, stamp: &str) -> PathBuf {
    home.join("GrokHub").join("recordings").join(stamp)
}

/// `frame-001.png` for the first still.
pub fn frame_name(index: usize, ext: &str) -> String {
    format!("frame-{:03}.{ext}", index + 1)
}

/// Only a folder directly under `~/GrokHub/recordings` may be deleted.
pub fn is_recording_dir(home: &Path, dir: &Path) -> bool {
    let root = home.join("GrokHub").join("recordings");
    dir.parent() == Some(root.as_path())
        && dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| !n.is_empty() && n != "." && n != "..")
}

const SYSTEM_PROMPT: &str = "You are GrokHub's screen diagnosis. You get stills from a short screen recording, in order, and the user's note. Say only what the stills show. Do not take or suggest automatic actions; the user decides. Answer in exactly three parts:\nSeen: one or two sentences naming the app or window and what happens.\nLikely cause: one sentence.\nFix steps:\n1. a step the user can do\n2. another step (at most five)";

/// System and user messages for the diagnosis call.
pub fn diagnosis_messages(note: &str, secs: u64, stills: usize) -> Vec<(String, String)> {
    let note = note.trim();
    let note = if note.is_empty() {
        "(no note: say what looks wrong, if anything)"
    } else {
        note
    };
    vec![
        ("system".into(), SYSTEM_PROMPT.into()),
        (
            "user".into(),
            format!(
                "Recording: {}, {stills} stills in order.\nWhat the user says is wrong: {note}",
                clock(secs)
            ),
        ),
    ]
}

/// The model's three parts. A reply that ignores the shape keeps its text in `seen`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScreenReport {
    pub seen: String,
    pub cause: String,
    pub steps: Vec<String>,
}

fn strip_step(line: &str) -> &str {
    let t = line.trim().trim_start_matches(['-', '*', '•']).trim_start();
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            return r.trim();
        }
    }
    t
}

pub fn parse_report(raw: &str) -> ScreenReport {
    let mut out = ScreenReport::default();
    let mut part = "";
    for line in raw.lines() {
        let plain = line.trim().trim_matches('*').trim();
        let lower = plain.to_ascii_lowercase();
        let (head, rest) = if let Some(r) = lower.strip_prefix("seen:") {
            ("seen", &plain[plain.len() - r.len()..])
        } else if let Some(r) = lower.strip_prefix("likely cause:") {
            ("cause", &plain[plain.len() - r.len()..])
        } else if let Some(r) = lower.strip_prefix("fix steps:") {
            ("steps", &plain[plain.len() - r.len()..])
        } else {
            ("", plain)
        };
        if !head.is_empty() {
            part = head;
        }
        let text = rest.trim().trim_start_matches('*').trim();
        if text.is_empty() {
            continue;
        }
        match part {
            "seen" => push_sentence(&mut out.seen, text),
            "cause" => push_sentence(&mut out.cause, text),
            "steps" => out.steps.push(strip_step(text).to_string()),
            _ => push_sentence(&mut out.seen, text),
        }
    }
    out.steps.retain(|s| !s.is_empty());
    out.steps.truncate(5);
    out
}

fn push_sentence(into: &mut String, text: &str) {
    if !into.is_empty() {
        into.push(' ');
    }
    into.push_str(text);
}

/// The chat report: what was seen, the likely cause, fix steps, and where the
/// recording is. Says that nothing was changed.
pub fn report_text(title: &str, report: &ScreenReport, dir: &Path) -> String {
    let mut out = format!("{title}\n\n");
    let seen = if report.seen.is_empty() {
        "Nothing stood out in the stills."
    } else {
        report.seen.as_str()
    };
    out.push_str(&format!("What I saw: {seen}\n"));
    if !report.cause.is_empty() {
        out.push_str(&format!("Likely cause: {}\n", report.cause));
    }
    if !report.steps.is_empty() {
        out.push_str("Suggested fix steps:\n");
        for (i, step) in report.steps.iter().enumerate() {
            out.push_str(&format!("{}. {step}\n", i + 1));
        }
    }
    out.push_str(&format!(
        "\nNothing was changed on this computer. The recording is in {}. Delete it from its card in the feed.",
        dir.display()
    ));
    out
}

/// The report when no model could look: the recording stays on this computer.
pub fn local_only_text(title: &str, dir: &Path, why: &str) -> String {
    format!(
        "{title}\n\nSaved on this computer only, not diagnosed: {}. The recording is in {}.",
        why.trim(),
        dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_needs_consent_takes_a_still_every_two_seconds_and_stops_at_two_minutes() {
        let mut r = Recorder::default();
        assert_eq!(r.start(false, 0), Err(CONSENT_OFF));
        assert_eq!(r.state, RecState::Idle);
        assert_eq!(r.start(true, 1_000), Ok(()));
        assert_eq!(r.start(true, 1_500), Err(ALREADY_RECORDING));
        assert!(r.frame_due(1_000), "the first still is right away");
        r.note_frame(1_000);
        assert!(!r.frame_due(2_999));
        assert!(r.frame_due(3_000));
        r.note_frame(3_000);
        assert_eq!(r.elapsed_secs(43_000), 42);
        assert!(!r.should_stop(120_999));
        assert!(r.should_stop(121_000));
        assert!(!r.frame_due(121_000), "no stills past the max length");
        assert_eq!(r.stop(125_000), Some((120, 2)));
        assert_eq!(r.state, RecState::Stopped { secs: 120, frames: 2 });
        assert_eq!(r.stop(126_000), None);
        r.note_frame(127_000);
        assert_eq!(r.state, RecState::Stopped { secs: 120, frames: 2 }, "a stopped recorder takes no stills");
    }

    #[test]
    fn keyframes_spread_over_the_recording_with_first_and_last() {
        assert_eq!(sample_keyframes(60, 6), vec![0, 12, 24, 35, 47, 59]);
        assert_eq!(sample_keyframes(4, 6), vec![0, 1, 2, 3]);
        assert_eq!(sample_keyframes(7, 6), vec![0, 1, 2, 4, 5, 6]);
        assert_eq!(sample_keyframes(10, 1), vec![9]);
        assert!(sample_keyframes(0, 6).is_empty());
    }

    #[test]
    fn titles_name_the_recording_and_the_note() {
        assert_eq!(clock(42), "0:42");
        assert_eq!(indicator_line(12), "● Recording 0:12 of 2:00");
        assert_eq!(
            recording_title(42, " video   stutters on 4K YouTube "),
            "Screen recording 0:42: video stutters on 4K YouTube"
        );
        assert_eq!(recording_title(5, ""), "Screen recording 0:05 (no note)");
        let long = recording_title(5, &"x".repeat(90));
        assert_eq!(long, format!("Screen recording 0:05: {}…", "x".repeat(NOTE_CHARS - 1)));
    }

    #[test]
    fn recordings_live_in_a_visible_folder_and_only_those_folders_can_be_deleted() {
        let home = Path::new("/home/ada");
        let dir = recording_dir(home, "2026-10-09-104200");
        assert_eq!(dir, PathBuf::from("/home/ada/GrokHub/recordings/2026-10-09-104200"));
        assert_eq!(frame_name(0, "png"), "frame-001.png");
        assert!(is_recording_dir(home, &dir));
        assert!(!is_recording_dir(home, Path::new("/home/ada/GrokHub/recordings")));
        assert!(!is_recording_dir(home, Path::new("/home/ada/GrokHub")));
        assert!(!is_recording_dir(home, Path::new("/home/ada/GrokHub/recordings/a/b")));
        assert!(!is_recording_dir(home, Path::new("/home/ada/GrokHub/recordings/..")));
    }

    #[test]
    fn a_report_reads_the_three_parts_and_says_nothing_was_changed() {
        let raw = "**Seen:** Firefox plays a 4K YouTube video.\nThe picture freezes every few seconds.\n**Likely cause:** Hardware video decoding is off.\n**Fix steps:**\n1. Open about:support and check Hardware Video Decoding.\n2) Set media.ffmpeg.vaapi.enabled to true.\n- Restart Firefox.\n";
        let report = parse_report(raw);
        assert_eq!(
            report,
            ScreenReport {
                seen: "Firefox plays a 4K YouTube video. The picture freezes every few seconds.".into(),
                cause: "Hardware video decoding is off.".into(),
                steps: vec![
                    "Open about:support and check Hardware Video Decoding.".into(),
                    "Set media.ffmpeg.vaapi.enabled to true.".into(),
                    "Restart Firefox.".into(),
                ],
            }
        );
        let dir = Path::new("/home/ada/GrokHub/recordings/r1");
        assert_eq!(
            report_text("Screen recording 0:42: video stutters", &report, dir),
            "Screen recording 0:42: video stutters\n\nWhat I saw: Firefox plays a 4K YouTube video. The picture freezes every few seconds.\nLikely cause: Hardware video decoding is off.\nSuggested fix steps:\n1. Open about:support and check Hardware Video Decoding.\n2. Set media.ffmpeg.vaapi.enabled to true.\n3. Restart Firefox.\n\nNothing was changed on this computer. The recording is in /home/ada/GrokHub/recordings/r1. Delete it from its card in the feed."
        );
        let loose = parse_report("The screen looks fine.");
        assert_eq!(loose.seen, "The screen looks fine.");
        assert!(loose.cause.is_empty() && loose.steps.is_empty());
    }

    #[test]
    fn the_diagnosis_asks_for_three_parts_and_carries_the_note() {
        let m = diagnosis_messages("video stutters", 42, 6);
        assert_eq!(m[0].0, "system");
        assert!(m[0].1.contains("Seen:") && m[0].1.contains("Likely cause:") && m[0].1.contains("Fix steps:"));
        assert_eq!(m[1], ("user".into(), "Recording: 0:42, 6 stills in order.\nWhat the user says is wrong: video stutters".into()));
        assert_eq!(
            local_only_text("Screen recording 0:05 (no note)", Path::new("/r"), "Connect Grok in Settings"),
            "Screen recording 0:05 (no note)\n\nSaved on this computer only, not diagnosed: Connect Grok in Settings. The recording is in /r."
        );
    }
}
