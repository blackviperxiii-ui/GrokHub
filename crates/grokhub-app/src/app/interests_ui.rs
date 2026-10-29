//! Settings → Pulse → Interests, and the interest profile behind it. The profile lives in
//! `{config}/interests.json`. It is seeded once from memory, recent chats and liked cards,
//! steers every feed lookup, and moves a little with each open, like and skip.
//! Chat titles only become Interests after cross-chat evidence; edges link related topics.

use super::*;
use grokhub_core::interests::{self, InterestProfile, InterestSignal, InterestSource, SeedFrom};

#[derive(Default)]
pub(super) struct InterestsState {
    /// Read from disk the first time anything needs it.
    pub profile: Option<InterestProfile>,
    /// The Add box in Settings.
    pub draft: String,
}

pub(super) fn interests_path() -> std::path::PathBuf {
    crate::config::config_dir().join(interests::FILE_NAME)
}

/// One Settings row's button.
enum RowAct {
    Pin(bool),
    Mute(bool),
    Remove,
}

impl Cabin {
    pub(super) fn interests_now(&mut self) -> &mut InterestProfile {
        self.interests
            .profile
            .get_or_insert_with(|| interests::load(&interests_path()))
    }

    pub(super) fn persist_interests(&mut self) {
        let profile = self.interests_now().clone();
        if let Err(err) = interests::save(&interests_path(), &profile) {
            self.status = err;
        }
    }

    /// Edge-only feedback (topic weights already moved elsewhere).
    pub(super) fn interests_edge_feedback(
        &mut self,
        title: &str,
        anchor: Option<&str>,
        signal: InterestSignal,
    ) {
        let now = now_ms();
        let profile = self.interests_now();
        let before = profile.clone();
        profile.edge_feedback(title, anchor, signal, now);
        if *profile != before {
            self.persist_interests();
        }
    }

    /// First feed lookup only: topics from USER.md, MEMORY.md, the newest chats and
    /// liked cards. Topics the person removed never come back. Chat titles are
    /// observed into the interest graph and promoted only with cross-chat evidence.
    pub(super) fn seed_interests(&mut self, user_md: &str, memory_md: &str) {
        if self.interests_now().seeded {
            return;
        }
        let chats = self.recent_chat_mentions(interests::SEED_CHATS);
        let saved_titles: Vec<String> = self
            .updates
            .iter()
            .filter(|c| c.reaction == Some(grokhub_core::CardReaction::Up))
            .map(|c| c.title.clone())
            .collect();
        let from = SeedFrom {
            user_md,
            memory_md,
            chats: &chats,
            saved_titles: &saved_titles,
        };
        self.interests_now().seed(&from, now_ms());
        self.persist_interests();
    }

    /// Recent user chats as (id, title) for graph observe / seed.
    /// Skips scratch and background cabin threads (e.g. the hidden "Background" job chat).
    fn recent_chat_mentions(&self, limit: usize) -> Vec<(String, String)> {
        let mut chats: Vec<&threads::ChatThread> = self
            .threads
            .iter()
            .filter(|t| !t.scratch && !t.background && !t.title.trim().is_empty())
            .collect();
        chats.sort_by_key(|t| std::cmp::Reverse(t.accessed_ms));
        chats
            .into_iter()
            .take(limit)
            .map(|t| (t.id.clone(), t.title.clone()))
            .collect()
    }

    /// Observe recent chats, promote evidence-gated Interests, persist if anything moved.
    pub(super) fn interests_observe_recent(&mut self) {
        let chats = self.recent_chat_mentions(interests::OBSERVE_CHATS);
        let now = now_ms();
        let profile = self.interests_now();
        let before = profile.clone();
        profile.observe_chats(&chats, now);
        profile.promote_from_evidence(now);
        if *profile != before {
            self.persist_interests();
        }
    }

    /// What the next lookup is told about the person's interests.
    pub(super) fn interests_prompt(&mut self) -> String {
        self.interests_now().prompt_section()
    }

    /// A card was opened, liked or skipped: topics its title names move.
    pub(super) fn interests_feedback(&mut self, title: &str, signal: InterestSignal) {
        self.interests_feedback_anchored(title, None, signal);
    }

    /// Like [`interests_feedback`], with the card's interest topic as the graph anchor.
    pub(super) fn interests_feedback_anchored(
        &mut self,
        title: &str,
        anchor: Option<&str>,
        signal: InterestSignal,
    ) {
        let now = now_ms();
        let profile = self.interests_now();
        let before = profile.clone();
        profile.feedback_anchored(title, anchor, signal, now);
        if *profile != before {
            self.persist_interests();
        }
    }

    pub(super) fn add_interest(&mut self, text: &str) {
        let topic = interests::clean_topic(text);
        if topic.is_empty() {
            return;
        }
        if self
            .interests_now()
            .add(&topic, InterestSource::Explicit, now_ms())
        {
            self.persist_interests();
            self.status = format!("Pulse will look for {topic}.");
        } else {
            self.status = format!("{topic} is already on your list.");
        }
    }

    fn act_on_interest(&mut self, topic: &str, act: RowAct) {
        let profile = self.interests_now();
        let changed = match act {
            RowAct::Pin(on) => profile.set_pinned(topic, on),
            RowAct::Mute(on) => profile.set_muted(topic, on),
            RowAct::Remove => profile.remove(topic),
        };
        if changed {
            self.persist_interests();
        }
    }

    pub(super) fn ui_settings_pulse(&mut self, ui: &mut egui::Ui) {
        crate::cards::help_text(
            ui,
            "What Pulse looks for. I start from your memory, recent chats and liked posts, then learn from what you open, like and skip. Pinned topics always come first; muted ones never show up.",
        );
        ui.add_space(12.0);
        ui.label(
            RichText::new("Interests")
                .size(crate::theme::FONT_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.add_space(6.0);
        let mut add = false;
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.interests.draft)
                    .id(egui::Id::new("pulse-interest-add"))
                    .hint_text("Add a topic, like air compressors")
                    .char_limit(interests::TOPIC_CHARS)
                    .desired_width(280.0),
            );
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                add = true;
            }
            if crate::cards::white_pill(ui, "Add") {
                add = true;
            }
        });
        if add {
            let draft = std::mem::take(&mut self.interests.draft);
            self.add_interest(&draft);
        }
        ui.add_space(10.0);
        let mut rows: Vec<interests::Interest> = self.interests_now().topics.clone();
        if rows.is_empty() {
            crate::cards::settings_note(
                ui,
                "No topics yet. Add a few, or let the first feed lookup pick some from your memory and chats.",
            );
        }
        // Pinned, then live by weight, then muted at the bottom.
        rows.sort_by(|a, b| {
            a.muted
                .cmp(&b.muted)
                .then(b.pinned.cmp(&a.pinned))
                .then(b.weight.cmp(&a.weight))
                .then(a.topic.cmp(&b.topic))
        });
        let mut act: Option<(String, RowAct)> = None;
        for row in &rows {
            ui.horizontal(|ui| {
                let color = if row.muted {
                    crate::theme::subtle()
                } else {
                    crate::theme::fg()
                };
                ui.label(
                    RichText::new(&row.topic)
                        .size(crate::theme::FONT_BODY)
                        .color(color),
                );
                let note = if row.muted {
                    "Muted".to_string()
                } else if row.pinned {
                    format!("Pinned · {}", row.source.label())
                } else {
                    row.source.label().to_string()
                };
                ui.label(
                    RichText::new(note)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::cards::ghost_pill(ui, "Remove") {
                        act = Some((row.topic.clone(), RowAct::Remove));
                    }
                    let mute = if row.muted { "Unmute" } else { "Mute" };
                    if crate::cards::ghost_pill(ui, mute) {
                        act = Some((row.topic.clone(), RowAct::Mute(!row.muted)));
                    }
                    let pin = if row.pinned { "Unpin" } else { "Pin" };
                    if crate::cards::ghost_pill(ui, pin) {
                        act = Some((row.topic.clone(), RowAct::Pin(!row.pinned)));
                    }
                });
            });
            ui.add_space(4.0);
        }
        if let Some((topic, act)) = act {
            self.act_on_interest(&topic, act);
        }
        ui.add_space(16.0);
        self.ui_know_me(ui);
        self.ui_hobby_paths(ui);
        self.ui_pulse_sources(ui);
        self.ui_reactions(ui);
        self.ui_daily_drop(ui);
        self.ui_nudge_watchers(ui);
        ui.add_space(12.0);
        self.ui_pc_health_row(ui);
        self.ui_print_alerts(ui);
    }
}

/// How a Pulse ledger reason moves the interest profile. Wrong is about the card's
/// facts, not the topic.
pub(super) fn ledger_signal(reason: grokhub_core::pulse::LedgerReason) -> Option<InterestSignal> {
    use grokhub_core::pulse::LedgerReason;
    match reason {
        LedgerReason::Liked => Some(InterestSignal::Saved),
        LedgerReason::Accepted => Some(InterestSignal::Accepted),
        LedgerReason::Dismiss => Some(InterestSignal::Dismissed),
        LedgerReason::NotThis => Some(InterestSignal::NotInterested),
        // Used-then-dismissed: neutral-complete, not a bury.
        LedgerReason::Used | LedgerReason::Wrong => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cabin() -> (
        std::sync::MutexGuard<'static, ()>,
        crate::config::TestConfigDir,
        Cabin,
    ) {
        let lock = crate::config::hold_test_config();
        let root = crate::config::test_config_root("interests-ui");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pin = crate::config::TestConfigDir::set(root);
        let cabin = Cabin::quiet_for_test();
        (lock, pin, cabin)
    }

    #[test]
    fn seeding_reads_liked_cards_and_evidence_gates_chat_titles() {
        let (_lock, _root, mut cabin) = cabin();
        let mut old = threads::ChatThread::new("old", false);
        old.title = "Sourdough".into();
        old.accessed_ms = 1;
        let mut new = threads::ChatThread::new("new", false);
        new.title = "Home espresso".into();
        new.accessed_ms = 9;
        // Second chat with the same title supplies the evidence threshold.
        let mut again = threads::ChatThread::new("again", false);
        again.title = "Home espresso".into();
        again.accessed_ms = 10;
        // Hidden cabin job chat must not enter the interest graph.
        let mut bg = threads::ChatThread::new(threads::BACKGROUND_THREAD_TITLE, false);
        bg.background = true;
        bg.accessed_ms = 11;
        cabin.threads = vec![old, new, again, bg];
        let mut liked = grokhub_core::digest_card("edition", "Bambu printers", "body", 5);
        liked.reaction = Some(grokhub_core::CardReaction::Up);
        cabin.updates = vec![liked];
        cabin.seed_interests("Interests: Rust", "");
        let topics: Vec<String> = cabin
            .interests_now()
            .topics
            .iter()
            .map(|t| t.topic.clone())
            .collect();
        assert_eq!(topics, vec!["Rust", "Bambu printers", "Home espresso"]);
        assert!(cabin.interests_now().find("Sourdough").is_none());
        assert!(cabin.interests_now().find("Background").is_none());
        let on_disk = interests::load(&interests_path());
        assert!(on_disk.seeded);
        assert_eq!(on_disk.topics.len(), 3);
        assert!(on_disk
            .graph
            .evidence
            .iter()
            .any(|e| e.topic == "Sourdough" && e.chat_ids.len() == 1));
        assert!(!on_disk
            .graph
            .evidence
            .iter()
            .any(|e| e.topic.eq_ignore_ascii_case("Background")));
        assert_eq!(
            cabin.interests_prompt(),
            "Their interests, strongest first: Rust, Bambu printers, Home espresso."
        );
    }

    #[test]
    fn settings_edits_and_card_feedback_persist() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.add_interest("  air compressors ");
        assert_eq!(cabin.status, "Pulse will look for air compressors.");
        cabin.add_interest("Air Compressors");
        assert_eq!(cabin.status, "Air Compressors is already on your list.");
        cabin.add_interest("crypto");
        cabin.act_on_interest("crypto", RowAct::Mute(true));
        cabin.interests_feedback("New air compressors for small shops", InterestSignal::Saved);
        let on_disk = interests::load(&interests_path());
        let rows: Vec<(&str, u8, bool)> = on_disk
            .topics
            .iter()
            .map(|t| (t.topic.as_str(), t.weight, t.muted))
            .collect();
        assert_eq!(
            rows,
            vec![("air compressors", 80, false), ("crypto", 70, true)]
        );
        cabin.act_on_interest("crypto", RowAct::Remove);
        assert_eq!(
            interests::load(&interests_path()).removed,
            vec!["crypto".to_string()]
        );
        // A fresh cabin reads the same file.
        cabin.interests.profile = None;
        assert_eq!(
            cabin.interests_prompt(),
            "Their interests, strongest first: air compressors."
        );
    }

    #[test]
    fn pulse_reasons_map_to_interest_signals() {
        use grokhub_core::pulse::LedgerReason;
        assert_eq!(
            ledger_signal(LedgerReason::Liked),
            Some(InterestSignal::Saved)
        );
        assert_eq!(
            ledger_signal(LedgerReason::NotThis),
            Some(InterestSignal::NotInterested)
        );
        assert_eq!(
            ledger_signal(LedgerReason::Dismiss),
            Some(InterestSignal::Dismissed)
        );
        assert_eq!(
            ledger_signal(LedgerReason::Accepted),
            Some(InterestSignal::Accepted)
        );
        assert_eq!(ledger_signal(LedgerReason::Wrong), None);
        assert_eq!(
            ledger_signal(LedgerReason::Used),
            None,
            "used-then-dismissed is not an interest bury"
        );
    }
}
