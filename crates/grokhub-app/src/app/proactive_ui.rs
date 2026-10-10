//! Spike-6a: the cabin side of the proactive engine. The rules live in
//! `grokhub_core::proactive`; this file feeds them the cabin's state (board,
//! automation health, the user model, consent grants), spends an Anticipate
//! act only when a card may go up, and posts the cards on the existing
//! Pulse/Home surfaces. Nothing runs without your click, and a click meets
//! `harness::decide` on its normal path with origin proactive: a hard step
//! (send, pay, delete, credentials) parks the white hard card even under
//! Always and Full.

use super::*;
use grokhub_agent::harness as hx;
use grokhub_core::proactive::{self as pro, Candidate, ProactiveBudget};
use grokhub_core::UpdateCard;

/// Span session for proactive clicks and answers, beside the chat span files.
pub(super) const PROACTIVE_TRACE: &str = "proactive";
/// The engine looks at most this often; quiet ticks in between cost nothing.
pub(super) const PROACTIVE_EVERY_MS: u64 = 15 * 60 * 1000;
/// MindCheck reads this many recent proactive span lines.
const PROACTIVE_MIND_LINES: usize = 2_000;
const PROACTIVE_FILE: &str = "proactive.json";

/// The saved card budget, or a fresh one.
pub(super) fn load_budget() -> ProactiveBudget {
    std::fs::read_to_string(crate::config::config_dir().join(PROACTIVE_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// `proactive:<topic>`: the MindCheck key a proactive span on this topic folds into.
pub(super) fn proactive_key(topic: &str) -> String {
    format!("{}:{}", hx::Origin::Proactive.as_str(), pro::topic_key(topic))
}

impl Cabin {
    pub(super) fn save_proactive(&self) {
        let dir = crate::config::config_dir();
        if let Ok(json) = serde_json::to_string(&self.proactive) {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(PROACTIVE_FILE), json);
        }
    }

    /// Scope keys you granted in Settings. Only these sources are read.
    fn proactive_granted(&mut self) -> Vec<&'static str> {
        let ledger = self.consent();
        [(hx::Scope::Calendar, "calendar"), (hx::Scope::Mail, "mail"), (hx::Scope::SystemState, "system_state")]
            .into_iter()
            .filter(|(scope, _)| ledger.scope_grant(scope).is_some())
            .map(|(_, key)| key)
            .collect()
    }

    /// Open board cards as the engine sees them.
    fn proactive_work(&self) -> Vec<pro::WorkItem> {
        self.board
            .iter()
            .filter_map(|c| {
                let blocked = c.status == grokhub_core::BoardStatus::Blocked;
                let fresh_report = c.status == grokhub_core::BoardStatus::FollowUp && c.fresh;
                (blocked || fresh_report).then(|| pro::WorkItem {
                    title: c.title.clone(),
                    blocked,
                    fresh_report,
                    due_at: None,
                })
            })
            .collect()
    }

    /// MindCheck over the proactive span log: Not this is a deny, a click an approve.
    fn proactive_mind(&self) -> hx::MindCheck {
        let (spans, _) = hx::read_spans_tail(&crate::config::config_dir(), PROACTIVE_TRACE, PROACTIVE_MIND_LINES);
        let mut mind = hx::MindCheck::new(std::sync::Arc::new(hx::SystemClock));
        mind.extend(hx::signals_from_spans(&spans));
        mind
    }

    /// Ranked candidates now, from every watched source you allow.
    pub(super) fn proactive_candidates(&mut self, now: u64) -> Vec<Candidate> {
        let granted = self.proactive_granted();
        let work = self.proactive_work();
        let health: Vec<String> = self
            .automations
            .iter()
            .filter_map(grokhub_core::automation_health_line)
            .collect();
        let watch = pro::Watch {
            cards: &self.updates,
            work: &work,
            automation_health: &health,
            failed: &[],
            needs: &[],
        };
        pro::ProactiveEngine::new(std::sync::Arc::new(move || now)).gather(&watch, &[], &granted)
    }

    /// From `tick_anticipate`. Busy: nothing. Quiet hours: candidates queue for
    /// the end of the window and nothing surfaces. Otherwise, when a card may go
    /// up, the Anticipate act is spent through `heartbeat_may` and the card
    /// budget picks what surfaces.
    pub(super) fn tick_proactive(&mut self, now: u64, quiet: bool, busy: bool) {
        if busy || now.saturating_sub(self.last_proactive_ms) < PROACTIVE_EVERY_MS {
            return;
        }
        let ranked = self.proactive_candidates(now);
        if quiet {
            self.last_proactive_ms = now;
            self.proactive.surface(ranked, now, true, false);
            self.save_proactive();
            return;
        }
        if (ranked.is_empty() && self.proactive.queue.is_empty()) || self.proactive.room(now) == 0 {
            self.last_proactive_ms = now;
            return;
        }
        if !self.heartbeat_may(grokhub_core::ProactiveAct::Anticipate, now) {
            return;
        }
        self.last_proactive_ms = now;
        let posted = self.post_proactive(ranked, now);
        if posted == 0 {
            self.heartbeat_outcome(grokhub_core::ProactiveAct::Anticipate, grokhub_core::ActOutcome::Empty);
        }
    }

    /// Surface what the budget allows as Pulse cards. Returns how many went up.
    pub(super) fn post_proactive(&mut self, ranked: Vec<Candidate>, now: u64) -> usize {
        let out = self.proactive.surface(ranked, now, false, false);
        let mind = self.proactive_mind();
        let mut posted = 0;
        for c in &out {
            // Unsure = MindCheck history says you might mind (an ask-first
            // window or p_mind at or over 0.2). No history is still a card.
            let key = proactive_key(&c.topic);
            // "Don't do this again": never suggested again.
            if mind.prior(&key).is_some_and(|p| p.never) {
                continue;
            }
            let unsure = mind.prior(&key).is_some()
                && mind.mind_route(&hx::Candidate { key: &key, hard: None }) == hx::MindRoute::Ask;
            let Some(route) = pro::route(c, unsure) else {
                continue;
            };
            let card = pro::proactive_card(c, route, now);
            if pro::notify_os(c, now, self.cfg.proactive_reminders) && !self.quiet_now() {
                crate::notify::ping("GrokHub", &grokhub_core::pulse::i_can_title(&card));
            }
            grokhub_core::post_update(&mut self.updates, card);
            posted += 1;
        }
        if posted > 0 {
            self.persist_updates();
        }
        self.save_proactive();
        posted
    }

    /// One proactive span line: the topic as the tool, the decision, origin
    /// proactive. No card text beyond the topic.
    fn proactive_span(&self, topic: &str, decision: &str, reason: &str) {
        let mut span = hx::Span::soft_allow(
            PROACTIVE_TRACE,
            &pro::topic_key(topic),
            "",
            reason,
            "proactive card",
            self.access_mode(),
            "none",
        )
        .from_origin(hx::Origin::Proactive)
        .on_path("proactive");
        span.decision = decision.into();
        let _ = hx::append_span(&crate::config::config_dir(), &span);
    }

    /// A click on a proactive card's Run, Send… or Yes. The step meets
    /// `harness::decide` first: hard class parks the white hard card (no
    /// Always, Enter never approves, Esc and timeout deny); soft runs on its
    /// normal path. Returns what happened, for the status line and tests.
    pub(super) fn proactive_click(&mut self, id: &str) -> Option<hx::GateOutcome> {
        let card = self.updates.iter().find(|c| c.id == id).cloned()?;
        let meta = card.pulse.proactive.clone()?;
        self.proactive.engaged();
        self.save_proactive();
        let action = meta.final_step.clone().unwrap_or_else(|| card.idea_action());
        let args = serde_json::json!({ "action": action }).to_string();
        let outcome = hx::decide(hx::Step::Tool { name: &meta.tool, arguments: &args });
        match &outcome {
            hx::GateOutcome::Park { hard: Some(class), .. } => {
                let span = hx::Span::hard_park(PROACTIVE_TRACE, &meta.tool, &args, *class)
                    .from_origin(hx::Origin::Proactive)
                    .on_path("proactive");
                let _ = hx::append_span(&crate::config::config_dir(), &span);
                self.park_hard(
                    harness_ui::ParkSource::Proactive(card.id.clone()),
                    *class,
                    "proactive",
                    meta.tool.clone(),
                    action,
                );
                // The white hard card lives in the chat column.
                self.nav = Nav::Chat;
            }
            hx::GateOutcome::Refuse { reason } => {
                self.status = format!("Denied: {reason}");
            }
            _ => {
                self.proactive_span(&meta.topic, "approve", "clicked");
                // The turn it starts carries origin proactive (Spike-4c).
                self.harness.next_origin = Some(hx::Origin::Proactive);
                self.pulse_run_line(id);
            }
        }
        Some(outcome)
    }

    /// Dismiss on a proactive card, quietly: the topic stays off the feed
    /// for a day, MindCheck hears a Dismiss (+0.1), and two in a row halve
    /// the card budget for a day. No prompt, no status line.
    pub(super) fn proactive_dismissed(&mut self, card: &UpdateCard) {
        let Some(meta) = &card.pulse.proactive else {
            return;
        };
        self.proactive.dismiss(&meta.topic, now_ms());
        self.proactive_span(&meta.topic, "dismiss", "dismissed");
        self.save_proactive();
    }

    /// Not this on a proactive card: the topic is muted for 7 days, MindCheck
    /// hears a deny, and it counts toward the dismissal streak.
    pub(super) fn proactive_not_this(&mut self, card: &UpdateCard) {
        let Some(meta) = &card.pulse.proactive else {
            return;
        };
        self.proactive.not_this(&meta.topic, now_ms());
        self.proactive_span(&meta.topic, "deny", "not this");
        self.save_proactive();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::HardClass;
    use grokhub_core::proactive::{CandidateClass, CandidateSource, ProactiveRoute, Reversibility};

    const T0: u64 = 1_800_000_000_000;

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    fn sam() -> Candidate {
        let mut c = Candidate::soft(CandidateSource::Mail, "reply to Sam's email", "sam reply", 0.9, 0.8, "Sam asked about Friday.");
        c.class = CandidateClass::Hard;
        c.reversible = Reversibility::Irreversible;
        c.tool = "send_email".into();
        c.prepared = Some("Hi Sam, Friday works for me.".into());
        c
    }

    fn proactive_cards(cabin: &Cabin) -> Vec<UpdateCard> {
        cabin.updates.iter().filter(|c| c.pulse.proactive.is_some()).cloned().collect()
    }

    #[test]
    fn reply_to_sam_is_a_prepared_draft_and_send_parks_hard_under_always_and_full() {
        let (_pin, root) = pinned("proactive-sam");
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        cabin.cfg.desktop_control = true;
        cabin.harness.access_full = true;
        assert_eq!(cabin.access_mode(), hx::AccessMode::Full);
        assert_eq!(cabin.post_proactive(vec![sam()], T0), 1);
        let card = proactive_cards(&cabin).pop().expect("a card");
        assert_eq!(card.title, "I can draft this: reply to Sam's email");
        assert_eq!(card.body.as_deref(), Some("Hi Sam, Friday works for me."));
        assert_eq!(super::super::pulse_ui::run_label(&card), "Send…");
        assert!(cabin.harness.park.is_none() && !cabin.running, "preparing ran nothing");

        let out = cabin.proactive_click(&card.id).expect("a proactive card");
        assert!(matches!(out, hx::GateOutcome::Park { hard: Some(HardClass::Send), .. }), "{out:?}");
        let park = cabin.harness.park.clone().expect("the hard send card");
        assert_eq!(park.class, HardClass::Send);
        assert_eq!(park.source, harness_ui::ParkSource::Proactive(card.id.clone()));
        assert_eq!((park.tool.as_str(), park.action.as_str()), ("send_email", "reply to Sam's email"));
        assert!(matches!(cabin.nav, Nav::Chat), "the hard card is in the chat column");
        assert!(!cabin.running, "Always and Full sent nothing");
        // Enter never approves a hard card; Esc denies.
        assert_eq!(hx::hard_card_key(true, false, false), None);
        assert_eq!(hx::hard_card_key(false, true, false), Some(hx::HardAnswer::Deny));
        cabin.resolve_hard_park(false, "Esc");
        assert!(cabin.harness.park.is_none() && !cabin.running);
        let spans = hx::read_spans(&root, PROACTIVE_TRACE).unwrap();
        let got: Vec<(&str, &str, &str, &str)> = spans
            .iter()
            .map(|s| (s.tool.as_str(), s.decision.as_str(), s.approval_class.as_str(), s.origin.as_str()))
            .collect();
        assert_eq!(got, vec![("send_email", "park", "send", "proactive")]);
    }

    #[test]
    fn a_half_reversible_candidate_is_a_card_only_and_runs_on_click() {
        let (_pin, _root) = pinned("proactive-half");
        let mut cabin = Cabin::quiet_for_test();
        let c = Candidate::soft(CandidateSource::Workboard, "tidy the board", "board tidy", 0.9, 0.9, "w");
        assert_eq!(c.reversible, Reversibility::ByHand);
        assert_eq!(cabin.post_proactive(vec![c], T0), 1);
        let card = proactive_cards(&cabin).pop().unwrap();
        assert_eq!(card.title, "I can tidy the board");
        assert_eq!(card.pulse.proactive.as_ref().map(|p| (p.route, p.help)), Some((ProactiveRoute::ICan, 405)));
        assert!(!cabin.running && cabin.messages.is_empty() && cabin.harness.park.is_none(), "a card, never an action");
        // Your click is what runs it: soft, so no hard card, and the turn is
        // tagged origin proactive.
        let out = cabin.proactive_click(&card.id).unwrap();
        assert_eq!(out, hx::GateOutcome::Allow);
        assert!(cabin.harness.park.is_none());
        assert_eq!(cabin.harness.next_origin, Some(hx::Origin::Proactive));
    }

    const NATO: [&str; 20] = [
        "Alpha", "Bravo", "Charlie", "Delta", "Foxtrot", "Golf", "Hotel", "India", "Juliet", "Kilo", "Lima", "Mike",
        "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango", "Uniform",
    ];

    fn board_of(n: usize) -> Vec<grokhub_core::BoardCard> {
        (0..n)
            .map(|i| {
                let mut c: grokhub_core::BoardCard = serde_json::from_value(serde_json::json!({
                    "id": format!("b{i}"),
                    "title": NATO[i],
                    "detail": "",
                    "status": "blocked",
                }))
                .unwrap();
                c.status = grokhub_core::BoardStatus::Blocked;
                c
            })
            .collect()
    }

    #[test]
    fn twenty_candidates_surface_three_quiet_hours_queue_and_busy_does_nothing() {
        let (_pin, _root) = pinned("proactive-budget");
        let mut cabin = Cabin::quiet_for_test();
        cabin.board = board_of(20);
        cabin.tick_proactive(T0, false, true);
        assert!(proactive_cards(&cabin).is_empty() && cabin.proactive.queue.is_empty(), "busy: nothing");
        assert_eq!(cabin.last_proactive_ms, 0);

        cabin.tick_proactive(T0, true, false);
        assert!(proactive_cards(&cabin).is_empty(), "quiet hours: nothing surfaces");
        assert_eq!(cabin.proactive.queue.len(), grokhub_core::proactive::QUIET_QUEUE_MAX);

        let after = T0 + PROACTIVE_EVERY_MS;
        cabin.tick_proactive(after, false, false);
        assert_eq!(proactive_cards(&cabin).len(), 3, "the queue drains after the window, 3 per 4 hours");
        for i in 1..4 {
            cabin.tick_proactive(after + i * PROACTIVE_EVERY_MS, false, false);
        }
        assert_eq!(proactive_cards(&cabin).len(), 3, "still 3 within the hour");
        // The budget is saved for the next launch.
        assert_eq!(load_budget().shown.len(), 3);
    }

    #[test]
    fn not_this_mutes_the_topic_for_seven_days_then_it_asks_first() {
        let (_pin, _root) = pinned("proactive-not-this");
        let mut cabin = Cabin::quiet_for_test();
        let prep = || Candidate::soft(CandidateSource::UserModel, "get standup prep ready", "standup prep", 0.8, 0.8, "You do this daily.");
        // The cabin's clock is the wall clock; the budget takes it as `now`.
        let t0 = now_ms();
        let day = grokhub_core::proactive::DAY_MS;
        assert_eq!(cabin.post_proactive(vec![prep()], t0), 1);
        let card = proactive_cards(&cabin).pop().unwrap();
        assert_eq!(card.title, "I can get standup prep ready");
        cabin.pulse_not_this(&card.id, "2027-01-15");
        assert_eq!(cabin.post_proactive(vec![prep()], t0 + 6 * day), 0, "muted");
        assert_eq!(cabin.post_proactive(vec![prep()], t0 + 8 * day), 1, "7 days passed");
        let back = proactive_cards(&cabin).pop().unwrap();
        assert_eq!(back.title, "Get standup prep ready", "MindCheck heard a deny: a one-tap suggestion");
        assert_eq!(super::super::pulse_ui::run_label(&back), "Do it");
    }

    #[test]
    fn an_unsure_step_is_a_one_tap_suggestion_and_dismiss_is_quiet_for_a_day() {
        let (_pin, root) = pinned("proactive-suggest");
        let mut cabin = Cabin::quiet_for_test();
        let tidy = || {
            let mut c = Candidate::soft(
                CandidateSource::Workboard,
                "tidy Downloads: move 14 installers to ~/Downloads/old",
                "downloads tidy",
                0.9,
                0.9,
                "w",
            );
            c.reversible = Reversibility::Ledger;
            c
        };
        let t0 = now_ms();
        let day = grokhub_core::proactive::DAY_MS;
        // MindCheck heard a deny on this topic once: unsure.
        cabin.proactive_span("downloads tidy", "deny", "not this");
        assert_eq!(cabin.post_proactive(vec![tidy()], t0), 1);
        let card = proactive_cards(&cabin).pop().unwrap();
        assert_eq!(card.title, "Tidy Downloads: move 14 installers to ~/Downloads/old");
        assert_eq!(card.pulse.proactive.as_ref().map(|p| p.route), Some(ProactiveRoute::Suggest));
        assert_eq!(super::super::pulse_ui::run_label(&card), "Do it");
        assert!(cabin.harness.park.is_none() && !cabin.running, "posting asks nothing and runs nothing");

        // One tap runs it once through the gate: no approval card.
        assert_eq!(cabin.proactive_click(&card.id), Some(hx::GateOutcome::Allow));
        assert!(cabin.harness.park.is_none());
        assert_eq!(cabin.harness.next_origin, Some(hx::Origin::Proactive));
        let approvals = |root: &std::path::Path| {
            hx::read_spans(root, PROACTIVE_TRACE).unwrap().iter().filter(|s| s.decision == "approve").count()
        };
        assert_eq!(approvals(&root), 1);

        // The next one is dismissed: no prompt, no status line, +0.1, and the
        // topic stays off the feed for the rest of the day.
        assert_eq!(cabin.post_proactive(vec![tidy()], t0 + 1_000), 1);
        let again = proactive_cards(&cabin).into_iter().find(|c| c.id != card.id).unwrap();
        let status = cabin.status.clone();
        cabin.pulse_dismiss(&again.id, "2027-01-15");
        assert_eq!(cabin.status, status, "dismiss is quiet");
        assert!(cabin.harness.park.is_none());
        let prior = cabin.proactive_mind().prior(&proactive_key("downloads tidy")).unwrap();
        assert_eq!(prior.hundredths, 65, "deny 0.60, approve -0.05, dismiss +0.10");
        assert_eq!(approvals(&root), 1, "dismiss ran nothing");
        assert_eq!(cabin.post_proactive(vec![tidy()], t0 + 2_000), 0, "not posted again that day");
        assert_eq!(cabin.post_proactive(vec![tidy()], t0 + day + 2_000), 1);
    }

    #[test]
    fn dont_do_this_again_means_the_topic_is_never_suggested() {
        let (_pin, _root) = pinned("proactive-never");
        let mut cabin = Cabin::quiet_for_test();
        let c = Candidate::soft(CandidateSource::Workboard, "archive 3 finished board cards", "board archive", 0.9, 0.9, "w");
        cabin.proactive_span("board archive", "never", "user answer on a Done-for-you card");
        assert_eq!(cabin.post_proactive(vec![c], now_ms()), 0);
        assert!(proactive_cards(&cabin).is_empty());
    }

    #[test]
    fn two_proactive_dismissals_in_a_row_halve_the_card_budget() {
        let (_pin, _root) = pinned("proactive-dismiss");
        let mut cabin = Cabin::quiet_for_test();
        let a = Candidate::soft(CandidateSource::Workboard, "sort receipts", "receipts sorting", 0.9, 0.9, "w");
        let b = Candidate::soft(CandidateSource::Workboard, "file the invoices", "invoices filing", 0.9, 0.9, "w");
        assert_eq!(cabin.post_proactive(vec![a, b], T0), 2);
        for card in proactive_cards(&cabin) {
            cabin.pulse_dismiss(&card.id, "2027-01-15");
        }
        assert_eq!(cabin.proactive.caps(now_ms()), (1, 4));
    }

    #[test]
    fn proactive_cards_add_no_nav_page_or_panel() {
        // Exhaustive: a new Nav variant fails to compile here.
        fn known(nav: Nav) -> bool {
            match nav {
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
                | Nav::Agents
                | Nav::Settings => true,
            }
        }
        let (_pin, _root) = pinned("proactive-nav");
        let mut cabin = Cabin::quiet_for_test();
        cabin.nav = Nav::Pulse;
        let c = Candidate::soft(CandidateSource::Workboard, "tidy the board", "board tidy", 0.9, 0.9, "w");
        cabin.post_proactive(vec![c], T0);
        assert!(known(cabin.nav) && matches!(cabin.nav, Nav::Pulse), "posting a card never moves you");
        let card = proactive_cards(&cabin).pop().unwrap();
        assert_eq!(card.kind, grokhub_core::UpdateKind::Suggestion, "an existing Pulse card kind");
        assert!(grokhub_core::pulse::is_idea_card(&card));
    }
}
