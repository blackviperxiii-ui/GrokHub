//! Cabin organs: greet, room, passenger, presence, redirect.

use crate::attach::bound_scan;
use crate::chat_view::is_workload_user;
use crate::goal::is_auto_continue_prompt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidThoughtGreet {
    pub goal: Option<String>,
    pub last_fail: Option<String>,
    pub rewind_job_id: Option<String>,
    pub dream_prompt: String,
}

pub fn greet_from_last_job(
    goal: Option<&str>,
    receipts: &[(String, bool)],
    rewind_job_id: Option<&str>,
) -> MidThoughtGreet {
    let fail = receipts.iter().rev().find(|(_, ok)| !*ok).map(|(s, _)| s.clone());
    let ok_bits: Vec<&str> = receipts
        .iter()
        .filter(|(_, ok)| *ok)
        .map(|(s, _)| s.as_str())
        .take(6)
        .collect();
    let dream_prompt = [
        "Cinematic night-shift memory of a Linux desktop,".into(),
        goal.map(|g| format!("goal: {g},")).unwrap_or_default(),
        if ok_bits.is_empty() {
            "quiet night,".into()
        } else {
            format!("worked: {},", ok_bits.join("; "))
        },
        fail.as_ref()
            .map(|f| format!("then failed: {f},"))
            .unwrap_or_else(|| "finished clean,".into()),
        "Grok's hands on the Arch desk, no faces, 1080p, living wallpaper.".into(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" ");
    MidThoughtGreet {
        goal: goal.map(|s| s.to_string()),
        last_fail: fail,
        rewind_job_id: rewind_job_id.map(|s| s.to_string()),
        dream_prompt,
    }
}

/// Host/computer receipt bodies from this thread, oldest first.
pub fn thread_host_receipts(messages: &[(String, String)]) -> Vec<String> {
    thread_host_receipts_from(messages.iter().map(|(r, c)| (r.as_str(), c.as_str())))
}

/// Same scan without cloning an 8MB transcript onto the UI thread.
pub fn thread_host_receipts_from<'a, I>(messages: I) -> Vec<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    messages
        .into_iter()
        .filter(|(role, content)| {
            *role == "user"
                && (content.trim_start().starts_with("HOST_RESULT")
                    || content.trim_start().starts_with("COMPUTER_RESULT"))
        })
        .map(|(_, content)| {
            content
                .lines()
                .skip(1)
                .collect::<Vec<_>>()
                .join("\n")
                .chars()
                .take(160)
                .collect()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomPlan {
    pub slug: String,
    pub project_rel: String,
    pub workspace: String,
    pub host_script: String,
}

pub fn plan_room(utterance: &str, home: &str) -> RoomPlan {
    let u = utterance.trim();
    let stripped = u
        .trim_start_matches(|_c| false)
        .to_string();
    let stripped = strip_stage_prefix(&stripped);
    let slug = slugify(if stripped.is_empty() { "lab" } else { stripped });
    let docs_only = {
        let l = u.to_ascii_lowercase();
        l.contains("doc") && !(l.contains("flash") || l.contains("lab") || l.contains("firmware") || l.contains("code"))
    };
    let workspace = if docs_only { "docs".into() } else { slug.clone() };
    let root = format!("{}/GrokHub-Work/{workspace}", home.trim_end_matches('/'));
    let quoted = sh_single(&root);
    let script = format!(
        "mkdir -p {quoted} && command -v hyprctl >/dev/null && hyprctl dispatch workspace name:{workspace} || true && command -v qdbus >/dev/null && qdbus org.kde.KWin /KWin org.kde.KWin.showDesktop false || true"
    );
    RoomPlan {
        slug,
        project_rel: format!("GrokHub-Work/{workspace}"),
        workspace,
        host_script: script,
    }
}

fn strip_stage_prefix(s: &str) -> &str {
    let l = s.to_ascii_lowercase();
    for p in ["make this a ", "make this an ", "make this the ", "make this ", "set up a ", "set up an ", "set up the ", "set up ", "stage a ", "stage an ", "stage the ", "stage "]
    {
        if l.starts_with(p) {
            return s[p.len()..].trim();
        }
    }
    s
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let s = out.trim_matches('-');
    let s: String = s.chars().take(40).collect();
    if s.is_empty() {
        "room".into()
    } else {
        s
    }
}

fn sh_single(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'"'"'"#))
}

pub fn passenger_label(autonomy: u8) -> &'static str {
    match autonomy {
        0 => "You drive",
        1 => "Lane keep",
        2 => "Suggest",
        3 => "Supervised",
        4 => "Night / Dispatch",
        _ => "Lane keep",
    }
}

pub fn on_wheel_grab(running: bool) -> (bool, bool) {
    (running, running)
}

pub const PRESENCE_RING_MS: u64 = 10 * 60 * 1000;
pub const PRESENCE_WIPE_MS: u64 = 24 * 60 * 60 * 1000;

pub fn presence_should_stream(job_running: bool, previewing: bool) -> bool {
    job_running || previewing
}

pub fn replay_frame_delay(speed: f32) -> u64 {
    let s = if speed.is_finite() && speed > 0.0 { speed } else { 4.0 };
    ((1000.0 / s).round() as u64).max(16)
}

pub fn should_keep_frame(ts: u64, now: u64, max_ms: u64) -> bool {
    now.saturating_sub(ts) <= max_ms && ts <= now.saturating_add(1000)
}

pub fn presence_orb_state(voice: &str, previewing: bool) -> &'static str {
    if voice == "hands" || previewing {
        "hands"
    } else {
        match voice {
            "listening" => "listening",
            "speaking" => "speaking",
            _ => "idle",
        }
    }
}

pub fn redirect_prompt(prev_user: &str, next: &str) -> String {
    format!("New direction: {next}\n\n(Previous ask: {prev_user})")
}

pub fn last_user_scan<'a, I>(messages: I) -> Option<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
    I::IntoIter: DoubleEndedIterator,
{
    messages
        .into_iter()
        .rev()
        .find(|(r, c)| *r == "user" && !is_workload_user(c) && !is_auto_continue_prompt(c))
        .map(|(_, c)| bound_scan(c).into_owned())
}

pub fn last_user_text(messages: &[(String, String)]) -> Option<String> {
    last_user_scan(messages.iter().map(|(r, c)| (r.as_str(), c.as_str())))
}

pub fn clipboard_context_block(text: &str) -> String {
    format!("Clipboard:\n```\n{}\n```", text.trim())
}

pub fn parse_local_clock(date_out: &str, now_ms: u64) -> Option<LocalClock> {
    let mut bits = date_out.split_whitespace();
    let weekday = bits.next()?.parse::<u8>().ok()?;
    let hour = bits.next()?.parse::<u32>().ok()?;
    let minute = bits.next()?.parse::<u32>().ok()?;
    Some(LocalClock {
        now_ms,
        weekday: weekday.min(6),
        hour: hour.min(23),
        minute: minute.min(59),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalClock {
    pub now_ms: u64,
    pub weekday: u8,
    pub hour: u32,
    pub minute: u32,
}

impl LocalClock {
    pub fn hm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

pub fn quiet_hours_active(now_hm: &str, start: &str, end: &str) -> bool {
    let Some(now) = hm_min(now_hm) else {
        return false;
    };
    let Some(s) = hm_min(start) else {
        return false;
    };
    let Some(e) = hm_min(end) else {
        return false;
    };
    if s == e {
        return false;
    }
    if s < e {
        now >= s && now < e
    } else {
        now >= s || now < e
    }
}

pub fn hm_min(hm: &str) -> Option<u32> {
    let mut p = hm.trim().split(':');
    let h = p.next()?.parse::<u32>().ok()?;
    let m = p.next().unwrap_or("0").parse::<u32>().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

pub fn daily_units_blocked(used: u32, cap: u32) -> bool {
    cap > 0 && used >= cap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_room_passenger_presence() {
        let g = greet_from_last_job(Some("flash pi"), &[("dd ok".into(), true), ("verify fail".into(), false)], Some("j1"));
        assert!(g.dream_prompt.contains("flash pi"));
        assert_eq!(g.last_fail.as_deref(), Some("verify fail"));
        let r = plan_room("stage a docs desk", "/home/jeremy");
        assert_eq!(r.workspace, "docs");
        assert_eq!(r.project_rel, "GrokHub-Work/docs");
        assert!(
            r.host_script.contains("'/home/jeremy/GrokHub-Work/docs'"),
            "bind path and mkdir must be the same folder: {}",
            r.host_script
        );
        assert!(r.host_script.contains("mkdir -p"));
        assert_eq!(passenger_label(4), "Night / Dispatch");
        assert_eq!(on_wheel_grab(true), (true, true));
        assert_eq!(presence_orb_state("listening", false), "listening");
        assert_eq!(presence_orb_state("idle", true), "hands");
        assert!(should_keep_frame(100, 200, 200));
        assert!(!should_keep_frame(100, 10_000, 200));
        assert_eq!(redirect_prompt("old", "new"), "New direction: new\n\n(Previous ask: old)");
        assert!(quiet_hours_active("23:00", "22:00", "07:00"));
        assert!(!quiet_hours_active("10:00", "22:00", "07:00"));
        assert!(daily_units_blocked(40, 40));
        let c = parse_local_clock("5 16 42", 1).unwrap();
        assert_eq!(c.weekday, 5);
        assert_eq!(c.hm(), "16:42");
        assert_eq!(
            last_user_text(&[
                ("user".into(), "check the box".into()),
                ("user".into(), "HOST_RESULT (facts only):\n$ uname -a\n".into()),
                ("user".into(), crate::goal::FOLLOWUP_PROMPT.into()),
            ])
            .as_deref(),
            Some("check the box")
        );
        let src = include_str!("organs.rs");
        let last = src
            .split("pub fn last_user_scan")
            .nth(1)
            .and_then(|s| s.split("pub fn last_user_text(").next())
            .expect("last_user_scan");
        let take = last
            .find("into_owned")
            .or_else(|| last.find("to_string"))
            .expect("user take");
        assert!(
            last[..take].contains("bound_scan")
                || last[..take].contains("TEXT_FILE_CAP")
                || last[..take].contains("chip_scan"),
            "last user must not clone an 8MB paste: {last}"
        );
        let receipts = thread_host_receipts(&[
            ("user".into(), "flash the pi".into()),
            (
                "user".into(),
                "HOST_RESULT (facts only):\n$ dd if=img of=/dev/sda\nexit 0 · 3ms\n".into(),
            ),
            (
                "user".into(),
                "COMPUTER_RESULT (facts only):\nclicked 10,20\n".into(),
            ),
            ("assistant".into(), "HOST_RESULT (facts only):\nignore".into()),
        ]);
        assert_eq!(
            receipts,
            vec![
                "$ dd if=img of=/dev/sda\nexit 0 · 3ms".to_string(),
                "clicked 10,20".to_string(),
            ]
        );
    }
}
