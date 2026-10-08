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
//! frame and the agent cursor marker. The step-cap pause is a ladder pause
//! card (Continue / Stop) counted in `decisions_waiting()`. No new chrome.

use super::*;
use grokhub_agent::episode as ep;
use grokhub_agent::harness as hx;

/// The soft park detector for a long-run limit.
pub(super) const EPISODE_CAP_DETECTOR: &str = "episode_cap";
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
    /// The cabin's own limits (Grok Build turns); the native kernel keeps its own.
    pub step_cap: u32,
    pub wall_cap_ms: u64,
    pub paused: bool,
    /// The next native prompt carries the user's Continue.
    pub resume: bool,
    pub counted_ms: u64,
}

impl EpisodeUi {
    fn new(chat_id: &str, now: u64) -> Self {
        Self {
            id: ep::new_episode_id(now),
            chat_id: chat_id.into(),
            started_ms: now,
            last_ms: now,
            steps: 0,
            step_cap: ep::EPISODE_MAX_STEPS,
            wall_cap_ms: ep::EPISODE_MAX_WALL.as_millis() as u64,
            paused: false,
            resume: false,
            counted_ms: 0,
        }
    }

    pub(super) fn header(&self, now: u64) -> String {
        ep::episode_header(self.steps, Duration::from_millis(now.saturating_sub(self.started_ms)))
    }

    /// A Grok Build turn's limit: the native kernel checks its own.
    fn cap_hit(&self, now: u64) -> Option<ep::CapHit> {
        if self.steps >= self.step_cap {
            return Some(ep::CapHit::Steps(self.steps));
        }
        let ran = now.saturating_sub(self.started_ms);
        (ran >= self.wall_cap_ms).then_some(ep::CapHit::Wall(ran / 60_000))
    }
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
    /// `resumed` is true when it answered a long-run pause.
    pub(super) fn episode_user_sent(&mut self, resumed: bool) {
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
            Some(e) => {
                e.last_ms = now;
                if resumed {
                    e.resume = true;
                }
            }
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

    /// The user's Continue (typed reply or the card): the cap moves on by
    /// another full allowance. Nothing else calls this.
    pub(super) fn resume_episode(&mut self, _by: ep::Continue) {
        let now = now_ms();
        let native = self.native_engine_for_current();
        let Some(e) = self.harness.episode.as_mut() else {
            return;
        };
        e.paused = false;
        e.resume = true;
        e.step_cap = e.steps.saturating_add(ep::EPISODE_MAX_STEPS);
        e.wall_cap_ms = now.saturating_sub(e.started_ms) + ep::EPISODE_MAX_WALL.as_millis() as u64;
        e.last_ms = now;
        if !native {
            self.write_episode_marker("resume", "continue", "you answered the pause");
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
        }
        self.harness.soft_parks.retain(|p| p.detector != EPISODE_CAP_DETECTOR);
        self.harness.episode = None;
    }

    fn write_episode_marker(&self, decision: &str, result: &str, claim: &str) {
        let mut span = hx::Span::deny(&self.trace_id(), ep::EPISODE_TOOL, "{}", result, "soft");
        span.decision = decision.into();
        span.claim = claim.into();
        self.write_span(span, "E");
    }

    /// Throttled from `poll_harness`: the idle end, the header's step count,
    /// and the step / wall limit on Grok Build turns.
    pub(super) fn poll_episode(&mut self) {
        let now = now_ms();
        let Some(e) = self.harness.episode.as_ref() else {
            return;
        };
        if self.running {
            if let Some(e) = self.harness.episode.as_mut() {
                e.last_ms = now;
            }
        } else if now.saturating_sub(e.last_ms) >= ep::EPISODE_IDLE.as_millis() as u64 && !e.paused {
            self.end_episode(ep::EpisodeEnd::Idle);
            return;
        }
        let Some(e) = self.harness.episode.as_ref() else {
            return;
        };
        if now.saturating_sub(e.counted_ms) < STEP_COUNT_EVERY_MS {
            return;
        }
        let (id, chat) = (e.id.clone(), e.chat_id.clone());
        let steps = hx::read_spans(&crate::config::config_dir(), &chat)
            .map(|spans| spans.iter().filter(|s| is_episode_step(s, &id)).count() as u32)
            .unwrap_or(0);
        let native = self.native_engine_for_current();
        let Some(e) = self.harness.episode.as_mut() else {
            return;
        };
        e.counted_ms = now;
        e.steps = steps;
        if native || e.paused || !self.running {
            return;
        }
        if let Some(hit) = e.cap_hit(now) {
            self.pause_episode(hit);
        }
    }

    /// A long-run limit: stop the live turn like a Steer (parks and their
    /// spans stay), then the pause card waits for the user. Nothing resumes
    /// on its own.
    pub(super) fn pause_episode(&mut self, hit: ep::CapHit) {
        let Some(e) = self.harness.episode.as_mut() else {
            return;
        };
        e.paused = true;
        let chat = e.chat_id.clone();
        if self.running && !self.native_engine_for_current() {
            let parks = self.take_parks_for_steer();
            self.halt_in_flight();
            self.restore_parks_after_steer(parks);
            self.write_episode_marker("pause", hit.key(), &hit.question());
        }
        self.harness.repair = None;
        self.harness.soft_parks.retain(|p| p.detector != EPISODE_CAP_DETECTOR);
        self.harness.soft_parks.push(super::harness_ui::SoftPark {
            detector: EPISODE_CAP_DETECTOR.into(),
            reason: hit.question(),
            evidence: Vec::new(),
            chat_id: chat,
        });
        if self.chrome_here() {
            self.status = crate::motion::needs_attention_summary(self.decisions_waiting());
        }
    }

    /// A native turn ended: read how (`Done` stop reason) and the kernel's
    /// newest pause or ladder span.
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
            "episode_paused" => {
                let spans = self.episode_spans();
                let hit = spans.iter().rev().find(|s| s.tool == ep::EPISODE_TOOL && s.decision == "pause").map(|s| {
                    let n = s.claim.split_whitespace().nth(2).and_then(|n| n.parse::<u64>().ok());
                    if s.result == "wall" {
                        ep::CapHit::Wall(n.unwrap_or(ep::EPISODE_MAX_WALL.as_secs() / 60))
                    } else {
                        ep::CapHit::Steps(n.map_or(ep::EPISODE_MAX_STEPS, |n| n as u32))
                    }
                });
                self.pause_episode(hit.unwrap_or(ep::CapHit::Steps(ep::EPISODE_MAX_STEPS)));
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

    /// The seed the native engine gets with the next prompt. Taking it
    /// clears the one-shot resume.
    pub(super) fn native_episode_seed(&mut self) -> Option<grokhub_agent::EpisodeSeed> {
        let trace = self.trace_id();
        let turn = self.turn_no();
        let access = self.access_mode();
        let held = self.secret_hold.clone();
        let e = self.harness.episode.as_mut().filter(|e| e.chat_id == trace)?;
        let seed = grokhub_agent::EpisodeSeed {
            id: e.id.clone(),
            chat_id: e.chat_id.clone(),
            turn,
            resume: std::mem::take(&mut e.resume),
            config_dir: crate::config::config_dir(),
            held,
            access,
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
    fn the_step_cap_pause_is_counted_and_only_the_user_continues() {
        let (_pin, root) = pinned("episode-cap");
        let mut cabin = desk_cabin();
        cabin.harness_user_sent();
        for x in 0..ep::EPISODE_MAX_STEPS {
            step(&cabin, x);
        }
        let (_tx, rx) = mpsc::channel();
        cabin.grok_p_rx = Some(rx);
        cabin.running = true;
        cabin.poll_episode();
        let e = cabin.episode_here().unwrap().clone();
        assert_eq!((e.steps, e.paused), (60, true));
        assert!(!cabin.running, "the turn stopped at the cap");
        assert_eq!(cabin.harness.soft_parks.len(), 1);
        assert_eq!(cabin.harness.soft_parks[0].reason, "Paused after 60 steps. Continue?");
        assert_eq!(cabin.decisions_waiting(), 1, "the pause counts in the needs-attention line");
        // Nothing resumes on its own: polls leave it paused with one card.
        cabin.harness.episode.as_mut().unwrap().counted_ms = 0;
        cabin.poll_episode();
        assert!(cabin.episode_here().unwrap().paused);
        assert_eq!(cabin.decisions_waiting(), 1);
        let pauses = spans(&root).iter().filter(|s| s.decision == "pause" && s.tool == ep::EPISODE_TOOL).count();
        assert_eq!(pauses, 1);
        // The user's typed reply continues it with another 60 steps.
        cabin.harness_user_sent();
        let e = cabin.episode_here().unwrap().clone();
        assert_eq!((e.paused, e.step_cap, e.id), (false, 120, cabin.episode_span_id()));
        assert_eq!(cabin.decisions_waiting(), 0);
        assert!(spans(&root).iter().any(|s| s.tool == ep::EPISODE_TOOL && s.decision == "resume"));
    }

    #[test]
    fn stop_on_the_pause_card_ends_the_episode() {
        let (_pin, root) = pinned("episode-cap-stop");
        let mut cabin = desk_cabin();
        cabin.harness_user_sent();
        cabin.pause_episode(ep::CapHit::Wall(30));
        assert_eq!(cabin.harness.soft_parks[0].reason, "Paused after 30 minutes. Continue?");
        cabin.answer_soft_park(0, false, "Denied");
        assert!(cabin.harness.episode.is_none());
        let end = spans(&root).into_iter().rev().find(|s| s.tool == ep::EPISODE_TOOL).unwrap();
        assert_eq!((end.decision.as_str(), end.result.as_str()), ("end", "stop"));
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
