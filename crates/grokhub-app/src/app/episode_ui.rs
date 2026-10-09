//! Spike-3b: the supervised desktop episode in the cabin.
//!
//! A message the user types while "Let Grok control the desktop" is on opens
//! an episode on that chat; every later turn, Steer and pause shares its id,
//! and every span the cabin, the desktop MCP (`turn.json`) or the native
//! kernel writes carries it. Halt, Stop, `VERIFY_OK` after `GOAL_COMPLETE`
//! (native kernel), or [`ep::EPISODE_IDLE`] with no step end it.
//!
//! The live view is the Work tree that is already there: a group header
//! ("Desktop session · 12 steps · 4 min") above the step rows, the last
//! frame and the agent cursor marker. There is no step or time cap. An
//! Approve with no reply running resumes the run ([`Cabin::resume_parked_run`]).
//! No new chrome.

use super::*;
use grokhub_agent::episode as ep;
use grokhub_agent::harness as hx;

/// The turn an Approve with no reply running sends to resume the run.
pub(super) const RESUME_TEXT: &str = "Continue";
/// How often the header re-counts the episode's steps from its spans.
const STEP_COUNT_EVERY_MS: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EpisodeUi {
    pub id: String,
    pub chat_id: String,
    pub started_ms: u64,
    /// Last step or turn activity, for the idle end.
    pub last_ms: u64,
    pub steps: u32,
    /// The next native prompt resumes the run. Set until that prompt was
    /// actually sent ([`Cabin::episode_resume_sent`]).
    pub resume: bool,
    pub counted_ms: u64,
    /// How far into the chat's span file `steps` has counted, so each count
    /// reads only the lines written since (the file grows every step).
    pub counted_bytes: u64,
}

impl EpisodeUi {
    fn new(chat_id: &str, now: u64) -> Self {
        Self {
            id: ep::new_episode_id(now),
            chat_id: chat_id.into(),
            started_ms: now,
            last_ms: now,
            steps: 0,
            resume: false,
            counted_ms: 0,
            counted_bytes: 0,
        }
    }

    pub(super) fn header(&self, now: u64) -> String {
        ep::episode_header(self.steps, Duration::from_millis(now.saturating_sub(self.started_ms)))
    }
}

/// Steps of episode `id` in the span file past byte `from`, and the byte the
/// count reached (the end of the last whole line). A file shorter than `from`
/// is counted from the start. Runs on the UI thread, so it never re-reads
/// what it already counted.
fn count_new_steps(path: &std::path::Path, from: u64, id: &str) -> (u32, u64) {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return (0, 0);
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = if len < from { 0 } else { from };
    let mut buf = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut buf).is_err() {
        return (0, start);
    }
    let whole = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let steps = String::from_utf8_lossy(&buf[..whole])
        .lines()
        .filter_map(|l| serde_json::from_str::<hx::Span>(l.trim()).ok())
        .filter(|s| is_episode_step(s, id))
        .count() as u32;
    (steps, start + whole as u64)
}

/// A step of an episode, as the header counts it: a tool call the gate
/// decided (not a reply, a check, a ladder rung, or an episode marker).
pub(super) fn is_episode_step(s: &hx::Span, id: &str) -> bool {
    s.episode == id
        && !matches!(s.tool.as_str(), hx::REPLY_TOOL | hx::VERIFY_TOOL | hx::RECOVERY_TOOL | ep::EPISODE_TOOL)
        && matches!(s.decision.as_str(), "allow" | "park" | "deny")
}

impl Cabin {
    /// The open episode on the visible chat.
    pub(super) fn episode_here(&self) -> Option<&EpisodeUi> {
        self.harness.episode.as_ref().filter(|e| e.chat_id == self.trace_id())
    }

    /// The id spans on the visible chat carry (empty with no open episode).
    pub(super) fn episode_span_id(&self) -> String {
        self.episode_here().map(|e| e.id.clone()).unwrap_or_default()
    }

    /// The user typed a message (no reply running). With desktop control on
    /// it opens an episode on this chat, or continues the open one.
    pub(super) fn episode_user_sent(&mut self) {
        let now = now_ms();
        if !self.cfg.desktop_control {
            if self.harness.episode.is_some() {
                self.end_episode(ep::EpisodeEnd::Stop);
            }
            return;
        }
        let trace = self.trace_id();
        if self.harness.episode.as_ref().is_some_and(|e| e.chat_id != trace) {
            self.end_episode(ep::EpisodeEnd::Stop);
        }
        match self.harness.episode.as_mut() {
            Some(e) => e.last_ms = now,
            None => {
                let e = EpisodeUi::new(&trace, now);
                let native = self.native_engine_for_current();
                self.harness.episode = Some(e);
                // The native kernel writes its own begin marker.
                if !native {
                    self.write_episode_marker("begin", "open", "goal: your message");
                }
            }
        }
    }

    /// The user approved a park with no reply running: the run goes on as
    /// a real turn. The open episode keeps its goal step, and approved
    /// kernel parks run at the kernel's next loop. Mid-turn this does
    /// nothing: the live run already picks the answer up.
    pub(super) fn resume_parked_run(&mut self) {
        if self.running {
            return;
        }
        let native = self.native_engine_for_current();
        let Some(e) = self.harness.episode.as_mut() else {
            self.send_chat(RESUME_TEXT.into());
            return;
        };
        e.resume = true;
        e.last_ms = now_ms();
        if !native {
            self.write_episode_marker("resume", "continue", "you approved; the run goes on");
        }
        self.send_chat(RESUME_TEXT.into());
    }

    /// The native prompt that carried the resume was sent.
    pub(super) fn episode_resume_sent(&mut self) {
        if let Some(e) = self.harness.episode.as_mut() {
            e.resume = false;
        }
    }

    /// End the open episode. The native kernel writes its own end span for a
    /// turn it is running; otherwise the cabin writes it here.
    pub(super) fn end_episode(&mut self, why: ep::EpisodeEnd) {
        let Some(e) = self.harness.episode.clone() else {
            return;
        };
        let kernel_writes = self.running && self.native_engine_for_current() && why != ep::EpisodeEnd::Idle;
        if !kernel_writes {
            let mut span = hx::Span::deny(&e.chat_id, ep::EPISODE_TOOL, "{}", why.as_str(), "soft");
            span.decision = "end".into();
            span.claim = e.header(now_ms());
            let span = span.in_episode(&e.id).on_path("E").in_turn(&e.chat_id, self.turn_no());
            let _ = hx::append_span(&crate::config::config_dir(), &span);
            self.deny_episode_parks(&e);
        }
        self.harness.episode = None;
    }

    /// An episode ended with no kernel turn to read its parks: their cards go
    /// and each is denied with a span, so a later Approve can't land nowhere.
    fn deny_episode_parks(&mut self, e: &EpisodeUi) {
        let prefix = format!("{}{}-", ep::PARK_PREFIX, e.id);
        let mine = |p: &super::harness_ui::HardParkUi| matches!(&p.source, super::harness_ui::ParkSource::Desk(id) if id.starts_with(&prefix));
        let mut gone: Vec<super::harness_ui::HardParkUi> = Vec::new();
        if self.harness.park.as_ref().is_some_and(mine) {
            gone.extend(self.harness.park.take());
        }
        let (drop, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.harness.queue).into_iter().partition(|p| mine(p));
        self.harness.queue = keep.into();
        gone.extend(drop);
        if self.harness.park.is_none() {
            self.harness.park = self.harness.queue.pop_front();
        }
        let dir = crate::config::config_dir();
        for park in gone {
            if let super::harness_ui::ParkSource::Desk(id) = &park.source {
                hx::clear_park(&dir, id);
            }
            let args = super::harness_ui::span_args(&park.tool, &park.action);
            let span = hx::Span::deny(&e.chat_id, &park.tool, &args, "episode ended — fail-closed Deny", park.class.as_str());
            let _ = hx::append_span(&dir, &span.in_episode(&e.id).on_path("E"));
        }
    }

    fn write_episode_marker(&self, decision: &str, result: &str, claim: &str) {
        let mut span = hx::Span::deny(&self.trace_id(), ep::EPISODE_TOOL, "{}", result, "soft");
        span.decision = decision.into();
        span.claim = claim.into();
        self.write_span(span, "E");
    }

    /// Throttled from `poll_harness`: the idle end and the header's step count.
    pub(super) fn poll_episode(&mut self) {
        let now = now_ms();
        let Some(e) = self.harness.episode.as_ref() else {
            return;
        };
        if self.running {
            if let Some(e) = self.harness.episode.as_mut() {
                e.last_ms = now;
            }
        } else if now.saturating_sub(e.last_ms) >= ep::EPISODE_IDLE.as_millis() as u64 {
            self.end_episode(ep::EpisodeEnd::Idle);
            return;
        }
        let Some(e) = self.harness.episode.as_ref() else {
            return;
        };
        if now.saturating_sub(e.counted_ms) < STEP_COUNT_EVERY_MS {
            return;
        }
        let (id, chat, from) = (e.id.clone(), e.chat_id.clone(), e.counted_bytes);
        let (new_steps, upto) = count_new_steps(&hx::span_path(&crate::config::config_dir(), &chat), from, &id);
        let Some(e) = self.harness.episode.as_mut() else {
            return;
        };
        e.counted_ms = now;
        if upto < from {
            // The file was cut or replaced: count it again from the start.
            e.steps = 0;
        }
        e.steps = e.steps.saturating_add(new_steps);
        e.counted_bytes = upto;
    }

    /// A native turn ended: read how (`Done` stop reason) and the kernel's
    /// newest ladder pause span.
    pub(super) fn episode_turn_done(&mut self, stop_reason: &str) {
        if self.harness.episode.is_none() || !self.native_engine_for_current() {
            return;
        }
        if let Some(e) = self.harness.episode.as_mut() {
            e.last_ms = now_ms();
        }
        match stop_reason {
            "episode_verified" | "halted" | "cancelled" | "episode_idle" => {
                self.harness.episode = None;
            }
            "episode_ladder_pause" => {
                let spans = self.episode_spans();
                if let Some(s) = spans.iter().rev().find(|s| s.tool == hx::RECOVERY_TOOL && s.decision == "pause") {
                    let chat = self.trace_id();
                    let args: serde_json::Value = serde_json::from_str(&s.args_redacted).unwrap_or_default();
                    self.harness.soft_parks.push(super::harness_ui::SoftPark {
                        detector: args["detector"].as_str().unwrap_or("episode").into(),
                        reason: s.claim.clone(),
                        evidence: Vec::new(),
                        chat_id: chat,
                    });
                }
            }
            _ => {}
        }
    }

    fn episode_spans(&self) -> Vec<hx::Span> {
        let Some(e) = self.harness.episode.as_ref() else {
            return Vec::new();
        };
        hx::read_spans(&crate::config::config_dir(), &e.chat_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.episode == e.id)
            .collect()
    }

    /// The seed the native engine gets with the next prompt. Reading it
    /// leaves the resume flag set: the config can be published more than
    /// once before the prompt goes ([`Self::episode_resume_sent`] clears it).
    pub(super) fn native_episode_seed(&self) -> Option<grokhub_agent::EpisodeSeed> {
        let trace = self.trace_id();
        let e = self.harness.episode.as_ref().filter(|e| e.chat_id == trace)?;
        let seed = grokhub_agent::EpisodeSeed {
            id: e.id.clone(),
            chat_id: e.chat_id.clone(),
            turn: self.turn_no(),
            resume: e.resume,
            config_dir: crate::config::config_dir(),
            held: self.secret_hold.clone(),
            access: self.access_mode(),
        };
        Some(seed)
    }

    /// The Work-tree group header for the open episode on this chat.
    pub(super) fn paint_episode_header(&self, ui: &mut egui::Ui) {
        let Some(e) = self.episode_here() else {
            return;
        };
        ui.label(
            RichText::new(e.header(now_ms()))
                .size(crate::theme::FONT_META)
                .color(crate::theme::fg()),
        );
        ui.add_space(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::harness_ui::ParkSource;

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::create_dir_all(&root);
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    fn desk_cabin() -> Cabin {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        cabin
    }

    fn step(cabin: &Cabin, x: u32) {
        let span = hx::Span::soft_allow(&cabin.trace_id(), "click", &format!(r#"{{"x":{x}}}"#), "ok", "c", hx::AccessMode::Supervised, "grok_build");
        cabin.write_span(span, "A");
    }

    fn spans(root: &std::path::Path) -> Vec<hx::Span> {
        hx::read_spans(root, "session").unwrap()
    }

    #[test]
    fn a_typed_message_opens_one_episode_its_spans_and_turn_file_carry_it_and_halt_ends_it() {
        let (_pin, root) = pinned("episode-open");
        let mut cabin = desk_cabin();
        assert!(cabin.harness.episode.is_none());
        cabin.harness_user_sent();
        let id = cabin.episode_here().expect("an episode opened").id.clone();
        assert!(id.starts_with("ep-"), "{id}");
        step(&cabin, 1);
        cabin.harness.last_poll = None;
        cabin.poll_harness();
        assert_eq!(hx::read_turn_context(&root).episode, id, "path A spans pick the id up from turn.json");
        // A second typed message on the same chat is the same episode.
        cabin.harness_user_sent();
        assert_eq!(cabin.episode_span_id(), id);
        step(&cabin, 2);
        cabin.halt_everything("Halted");
        assert!(cabin.harness.episode.is_none());
        let got = spans(&root);
        assert!(got.iter().all(|s| s.episode == id), "{got:?}");
        let marks: Vec<(&str, &str)> = got
            .iter()
            .filter(|s| s.tool == ep::EPISODE_TOOL)
            .map(|s| (s.decision.as_str(), s.result.as_str()))
            .collect();
        assert_eq!(marks, vec![("begin", "open"), ("end", "halt")]);
        // With desktop control off, a message opens nothing.
        cabin.cfg.desktop_control = false;
        cabin.harness_user_sent();
        assert!(cabin.harness.episode.is_none());
    }

    #[test]
    fn the_step_count_reads_only_new_lines_and_never_counts_twice() {
        let (_pin, root) = pinned("episode-count");
        let mut cabin = desk_cabin();
        cabin.harness_user_sent();
        for x in 0..3 {
            step(&cabin, x);
        }
        cabin.poll_episode();
        let seen = cabin.episode_here().unwrap().counted_bytes;
        assert_eq!(cabin.episode_here().unwrap().steps, 3);
        let path = hx::span_path(&root, &cabin.episode_here().unwrap().chat_id);
        assert_eq!(seen, std::fs::metadata(&path).unwrap().len());
        for x in 3..5 {
            step(&cabin, x);
        }
        cabin.harness.episode.as_mut().unwrap().counted_ms = 0;
        cabin.poll_episode();
        assert_eq!(cabin.episode_here().unwrap().steps, 5);
        cabin.harness.episode.as_mut().unwrap().counted_ms = 0;
        cabin.poll_episode();
        assert_eq!(cabin.episode_here().unwrap().steps, 5, "nothing new, nothing added");
    }

    fn resume_marks(root: &std::path::Path) -> usize {
        spans(root).iter().filter(|s| s.tool == ep::EPISODE_TOOL && s.decision == "resume").count()
    }

    fn soft_park(cabin: &mut Cabin) {
        let chat_id = cabin.trace_id();
        cabin.harness.soft_parks.push(super::super::harness_ui::SoftPark {
            detector: "action_loop".into(),
            reason: "the screenshot failed".into(),
            evidence: Vec::new(),
            chat_id,
        });
    }

    #[test]
    fn a_120_step_turn_never_posts_a_card_or_pauses() {
        let (_pin, root) = pinned("episode-no-cap");
        let mut cabin = desk_cabin();
        cabin.harness_user_sent();
        for x in 0..120 {
            step(&cabin, x);
        }
        let (_tx, rx) = mpsc::channel();
        cabin.grok_p_rx = Some(rx);
        cabin.running = true;
        cabin.poll_episode();
        assert_eq!(cabin.episode_here().unwrap().steps, 120);
        assert!(cabin.running, "the turn keeps going");
        assert!(cabin.harness.soft_parks.is_empty());
        assert_eq!(cabin.decisions_waiting(), 0);
        assert!(!spans(&root).iter().any(|s| s.decision == "pause"));
    }

    #[test]
    fn the_resume_flag_survives_every_publish_and_clears_once_the_prompt_is_sent() {
        let (_pin, _root) = pinned("episode-resume-flag");
        let mut cabin = desk_cabin();
        cabin.harness_user_sent();
        cabin.harness.episode.as_mut().unwrap().resume = true;
        // `ensure_native_engine` and a second publish both read the seed.
        assert!(cabin.native_episode_seed().unwrap().resume);
        assert!(cabin.native_episode_seed().unwrap().resume);
        cabin.episode_resume_sent();
        assert!(!cabin.native_episode_seed().unwrap().resume);
        // The turn kick publishes once, and clears the flag only on a sent prompt.
        let src = include_str!("native_engine.rs");
        let kick = src.split("fn kick_native_turn(").nth(1).and_then(|s| s.split("fn prompt_native_memory").next()).unwrap();
        assert!(!kick.contains("publish_native_cfg"), "{kick}");
        assert!(kick.contains("Some(Ok(())) => {\n                self.episode_resume_sent();"), "{kick}");
    }

    #[test]
    fn approving_the_last_pause_with_no_reply_running_resumes_and_never_mid_turn() {
        let (_pin, root) = pinned("episode-approve-resume");
        let _hide = crate::app::tests::HideGrok::arm();
        let mut cabin = desk_cabin();
        cabin.harness_user_sent();
        soft_park(&mut cabin);
        soft_park(&mut cabin);
        cabin.answer_soft_park(0, true, "");
        assert_eq!(resume_marks(&root), 0, "one pause is still open");
        cabin.answer_soft_park(0, true, "");
        assert_eq!(resume_marks(&root), 1, "the last Approve resumes the run");
        assert!(cabin.episode_here().unwrap().resume);
        assert_eq!(cabin.status, "Resumed");
        // Mid-turn the live run picks the answer up: nothing is sent twice.
        cabin.episode_resume_sent();
        soft_park(&mut cabin);
        let (_tx, rx) = mpsc::channel();
        cabin.grok_p_rx = Some(rx);
        cabin.running = true;
        cabin.answer_soft_park(0, true, "");
        assert_eq!(resume_marks(&root), 1);
        assert!(!cabin.episode_here().unwrap().resume);
        // Deny is unchanged: the episode stays open and nothing resumes.
        cabin.running = false;
        cabin.grok_p_rx = None;
        soft_park(&mut cabin);
        cabin.answer_soft_park(0, false, "Denied");
        assert_eq!(cabin.status, "Stopped that step");
        assert_eq!(resume_marks(&root), 1);
        assert!(cabin.harness.episode.is_some());
    }

    #[test]
    fn approving_a_kernel_park_after_the_turn_ended_goes_on() {
        let (_pin, root) = pinned("episode-park-resume");
        let _hide = crate::app::tests::HideGrok::arm();
        let mut cabin = desk_cabin();
        cabin.harness_user_sent();
        let eid = cabin.episode_span_id();
        let post = |n: u32| {
            let id = format!("{}{eid}-{n}", ep::PARK_PREFIX);
            hx::post_park(
                &root,
                &hx::ParkRequest { id: id.clone(), path: "E".into(), tool: "click".into(), action: "click".into(), class: "send".into(), ts_ms: 1 },
            )
            .unwrap();
            id
        };
        let id = post(3);
        cabin.poll_harness();
        cabin.resolve_hard_park(true, "");
        assert_eq!(hx::take_answer(&root, &id), Some(true), "the kernel reads the Approve");
        assert_eq!(cabin.status, "Approved once · Send · going on");
        assert_eq!(resume_marks(&root), 1);
        assert!(cabin.episode_here().unwrap().resume);
        // With a reply running the live kernel runs it; no second send.
        cabin.episode_resume_sent();
        let id = post(5);
        cabin.harness.last_poll = None;
        cabin.poll_harness();
        let (_tx, rx) = mpsc::channel();
        cabin.grok_p_rx = Some(rx);
        cabin.running = true;
        cabin.resolve_hard_park(true, "");
        assert_eq!(hx::take_answer(&root, &id), Some(true));
        assert_eq!(cabin.status, "Approved once · Send");
        assert_eq!(resume_marks(&root), 1);
        assert!(!cabin.episode_here().unwrap().resume);
    }

    #[test]
    fn ten_idle_minutes_end_the_episode() {
        let (_pin, root) = pinned("episode-idle");
        let mut cabin = desk_cabin();
        cabin.harness_user_sent();
        cabin.harness.episode.as_mut().unwrap().last_ms = now_ms() - ep::EPISODE_IDLE.as_millis() as u64;
        cabin.poll_episode();
        assert!(cabin.harness.episode.is_none());
        let end = spans(&root).into_iter().rev().find(|s| s.tool == ep::EPISODE_TOOL).unwrap();
        assert_eq!(end.result, "idle");
    }

    #[test]
    fn a_kernel_park_answered_on_its_card_writes_no_second_span() {
        let (_pin, root) = pinned("episode-park");
        let mut cabin = desk_cabin();
        let id = format!("{}ep-1-8", ep::PARK_PREFIX);
        hx::post_park(
            &root,
            &hx::ParkRequest { id: id.clone(), path: "E".into(), tool: "click".into(), action: "click".into(), class: "send".into(), ts_ms: 1 },
        )
        .unwrap();
        cabin.poll_harness();
        assert_eq!(cabin.harness.park.as_ref().map(|p| p.source.clone()), Some(ParkSource::Desk(id.clone())));
        cabin.resolve_hard_park(false, "Denied");
        assert_eq!(hx::take_answer(&root, &id), Some(false), "the kernel reads the answer");
        assert!(spans(&root).is_empty(), "the kernel writes the deny span, not the cabin");
    }

    #[test]
    fn an_episode_that_ends_idle_denies_its_open_parks() {
        let (_pin, root) = pinned("episode-park-idle");
        let mut cabin = desk_cabin();
        cabin.episode_user_sent();
        let eid = cabin.harness.episode.as_ref().unwrap().id.clone();
        let id = format!("{}{eid}-3", ep::PARK_PREFIX);
        hx::post_park(
            &root,
            &hx::ParkRequest { id: id.clone(), path: "E".into(), tool: "click".into(), action: "click".into(), class: "send".into(), ts_ms: 1 },
        )
        .unwrap();
        cabin.poll_harness();
        assert_eq!(cabin.harness.park.as_ref().map(|p| p.source.clone()), Some(ParkSource::Desk(id.clone())));
        cabin.end_episode(ep::EpisodeEnd::Idle);
        assert!(cabin.harness.park.is_none(), "the card goes with the episode");
        assert_eq!(hx::take_answer(&root, &id), None, "no answer is left for anyone to act on");
        let deny = spans(&root).into_iter().find(|s| s.tool == "click").expect("a deny span");
        assert_eq!((deny.decision.as_str(), deny.result.as_str()), ("deny", "episode ended — fail-closed Deny"));
        assert_eq!(deny.episode, eid);
    }

    #[test]
    fn work_tree_header_reads_steps_and_minutes() {
        let mut e = EpisodeUi::new("session", 1_000);
        e.steps = 12;
        assert_eq!(e.header(1_000 + 4 * 60_000 + 30_000), "Desktop session · 12 steps · 4 min");
    }

    #[test]
    fn the_episode_adds_no_nav_variant_or_page() {
        let all = [
            Nav::Chat,
            Nav::Devices,
            Nav::Memory,
            Nav::Workboard,
            Nav::Pulse,
            Nav::Imagine,
            Nav::Skills,
            Nav::Night,
            Nav::History,
            Nav::Command,
            Nav::Connectors,
            Nav::Agents,
            Nav::Settings,
        ];
        // No wildcard: a new Nav variant fails to compile here.
        for n in all {
            match n {
                Nav::Chat
                | Nav::Devices
                | Nav::Memory
                | Nav::Workboard
                | Nav::Pulse
                | Nav::Imagine
                | Nav::Skills
                | Nav::Night
                | Nav::History
                | Nav::Command
                | Nav::Connectors
                | Nav::Agents
                | Nav::Settings => {}
            }
        }
        assert_eq!(all.len(), 13);
    }
}
