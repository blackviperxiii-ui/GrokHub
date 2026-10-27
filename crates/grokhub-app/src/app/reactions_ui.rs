//! The cabin side of learned reactions (`grokhub_core::reactions`): every
//! Pulse act on an idea, a Feed post or a proactive card records its outcome
//! for the card's category, the scores feed Pulse ranking and the proactive
//! card picker, and Settings → Pulse shows what was learned with a reset.

use super::*;
use grokhub_core::proactive::Candidate;
use grokhub_core::reactions::{self, Outcome, Reactions};
use grokhub_core::UpdateCard;

fn reactions_path() -> std::path::PathBuf {
    crate::config::config_dir().join(reactions::FILE_NAME)
}

impl Cabin {
    /// Read from disk the first time anything needs it.
    pub(super) fn reactions_now(&mut self) -> &mut Reactions {
        self.sources
            .reactions
            .get_or_insert_with(|| reactions::load(&reactions_path()))
    }

    fn save_reactions(&mut self) {
        let state = self.reactions_now().clone();
        if let Err(err) = reactions::save(&reactions_path(), &state) {
            self.status = err;
        }
    }

    /// Says once that a category went quiet, and how to bring it back.
    fn announce_quiet(&mut self, quieted: &[String]) {
        if let Some(category) = quieted.first() {
            self.status = format!(
                "You've skipped {category} a few times, so I'll stop showing them. More like this on one, or Settings → Pulse, brings them back."
            );
        }
    }

    /// Records what the user did with a card, if it is a kind that learns.
    pub(super) fn react(&mut self, card: &UpdateCard, outcome: Outcome) {
        let Some((category, tag)) = reactions::card_category_tag(card) else {
            return;
        };
        let quieted = self
            .reactions_now()
            .record(outcome, &category, &tag, now_ms());
        self.save_reactions();
        self.announce_quiet(&Vec::from_iter(quieted));
    }

    /// More or less of this card's kind, from its ··· menu or the post's own buttons.
    pub(super) fn react_more(&mut self, card: &UpdateCard, more: bool) {
        let Some((category, _)) = reactions::card_category_tag(card) else {
            return;
        };
        if more {
            self.reactions_now().more(&category);
        } else {
            self.reactions_now().less(&category);
        }
        self.save_reactions();
    }

    /// The "Why am I seeing this?" line for a card: its source tag, category and score.
    pub(super) fn reaction_why(&mut self, card: &UpdateCard) -> Option<String> {
        let (category, tag) = reactions::card_category_tag(card)?;
        Some(reactions::why_line(self.reactions_now(), &category, &tag))
    }

    /// Cards left untouched for a day count as ignored, once each.
    pub(super) fn sweep_reactions(&mut self, now: u64) {
        let cards = self.updates.clone();
        let before = self.reactions_now().log.len();
        let quieted = self.reactions_now().sweep_ignored(&cards, now);
        if self.reactions_now().log.len() != before {
            self.save_reactions();
        }
        self.announce_quiet(&quieted);
    }

    /// Proactive candidates scaled by how the user reacts to their category;
    /// a quiet category's candidates are dropped. Order is ranked again.
    pub(super) fn weigh_candidates(&mut self, cands: Vec<Candidate>) -> Vec<Candidate> {
        let r = self.reactions_now().clone();
        let weighed: Vec<Candidate> = cands
            .into_iter()
            .filter_map(|mut c| {
                let w = r.weight(reactions::source_category(c.source));
                (w > 0.0).then(|| {
                    c.value = (c.value * w).min(1.0);
                    c
                })
            })
            .collect();
        grokhub_core::proactive::rank_candidates(weighed)
    }

    /// The outcome a Pulse act means for learning, recorded before the act runs.
    pub(super) fn react_to_act(&mut self, act: &super::pulse_ui::PulseAct) {
        use super::pulse_ui::{PulseAct, SourceAct};
        let (id, outcome, more) = match act {
            PulseAct::Run(id)
            | PulseAct::Always(id)
            | PulseAct::Accept(id)
            | PulseAct::Like(id) => (id, Outcome::Accepted, None),
            PulseAct::Open(id) | PulseAct::Discuss(id) => (id, Outcome::Tapped, None),
            PulseAct::Snooze(id) => (id, Outcome::Snoozed, None),
            PulseAct::Dismiss(id) | PulseAct::Wrong(id) => (id, Outcome::Dismissed, None),
            PulseAct::NotThis(id) => (id, Outcome::Dismissed, Some(false)),
            PulseAct::Source(id, SourceAct::Save) => (id, Outcome::Accepted, None),
            PulseAct::Source(id, SourceAct::MoreLike) => (id, Outcome::Accepted, Some(true)),
            PulseAct::Source(id, SourceAct::NotInterested) => (id, Outcome::Dismissed, Some(false)),
            PulseAct::MoreKind(id) => (id, Outcome::Tapped, Some(true)),
            PulseAct::LessKind(id) => (id, Outcome::Dismissed, Some(false)),
            _ => return,
        };
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        if !matches!(act, PulseAct::MoreKind(_) | PulseAct::LessKind(_)) {
            self.react(&card, outcome);
        }
        if let Some(more) = more {
            self.react_more(&card, more);
        }
        if matches!(act, PulseAct::LessKind(_)) {
            self.status = "Got it. Fewer of these.".into();
        } else if matches!(act, PulseAct::MoreKind(_)) {
            self.status = "Got it. More of these.".into();
        }
    }

    /// Settings → Pulse → What Pulse learned: each category's score, quiet
    /// ones with Bring back, and Reset.
    pub(super) fn ui_reactions(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("What Pulse learned")
                .size(crate::theme::FONT_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.add_space(2.0);
        crate::cards::help_text(
            ui,
            "From what you open, run, snooze, dismiss or skip. Only the outcome, the kind of card and its source are kept, on this computer.",
        );
        ui.add_space(6.0);
        let cats: Vec<(String, reactions::CategoryScore)> = self
            .reactions_now()
            .categories
            .iter()
            .filter(|(_, c)| c.score != 0 || c.quiet)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if cats.is_empty() {
            crate::cards::settings_note(ui, "Nothing yet.");
        }
        let mut bring_back = None;
        for (category, score) in &cats {
            let sign = if score.score > 0 { "+" } else { "" };
            let line = if score.quiet {
                format!("{category} {sign}{} · quiet", score.score)
            } else {
                format!("{category} {sign}{}", score.score)
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(line)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::fg()),
                );
                if score.quiet && crate::cards::ghost_pill(ui, "Bring back") {
                    bring_back = Some(category.clone());
                }
            });
        }
        if let Some(category) = bring_back {
            self.reactions_now().more(&category);
            self.save_reactions();
        }
        if !cats.is_empty() && crate::cards::ghost_pill(ui, "Reset what Pulse learned") {
            self.reactions_now().reset();
            self.save_reactions();
            self.status = "Pulse learning reset.".into();
        }
        ui.add_space(16.0);
    }
}

#[cfg(test)]
mod tests {
    use super::super::pulse_ui::{PulseAct, SourceAct};
    use super::*;
    use grokhub_core::proactive::{CandidateSource, Reversibility};

    fn pinned(label: &str) -> crate::config::TestConfigDir {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        crate::config::TestConfigDir::set(root)
    }

    fn steam_post(cabin: &mut Cabin, id: &str) -> UpdateCard {
        let mut card = grokhub_core::pulse_sources::item_card(
            &grokhub_core::pulse_sources::Ranked {
                item: grokhub_core::pulse_sources::SourceItem {
                    kind: grokhub_core::pulse_sources::SourceKind::Rss,
                    title: format!("Patch {id}"),
                    url: format!("https://store.steampowered.com/news/app/570/view/{id}"),
                    summary: "Notes".into(),
                    source_name: "Dota 2 on Steam".into(),
                    published_ms: 0,
                    image: None,
                    topic: None,
                    engagement: 0,
                },
                because: None,
                score: 1,
            },
            now_ms(),
        );
        card.id = id.into();
        grokhub_core::post_update(&mut cabin.updates, card.clone());
        card
    }

    #[test]
    fn pulse_acts_move_the_category_score_and_persist() {
        let _pin = pinned("reactions-acts");
        let mut cabin = Cabin::quiet_for_test();
        steam_post(&mut cabin, "a");
        steam_post(&mut cabin, "b");
        cabin.react_to_act(&PulseAct::Source("a".into(), SourceAct::Save));
        assert_eq!(cabin.reactions_now().score("game news"), 2);
        cabin.react_to_act(&PulseAct::Source("b".into(), SourceAct::NotInterested));
        // Dismissed (-2) and less like this (-3).
        assert_eq!(cabin.reactions_now().score("game news"), -3);
        let saved = reactions::load(&reactions_path());
        assert_eq!(saved.score("game news"), -3);
        assert_eq!(
            saved.log.iter().map(|r| r.outcome).collect::<Vec<_>>(),
            vec![Outcome::Accepted, Outcome::Dismissed]
        );
        assert!(saved.log.iter().all(|r| r.tag == "Dota 2 on Steam"));
        let card = cabin.pulse_card("a").unwrap();
        assert_eq!(
            cabin.reaction_why(&card).as_deref(),
            Some("From Dota 2 on Steam · game news -3")
        );
    }

    #[test]
    fn a_quiet_category_leaves_the_feed_and_more_brings_it_back() {
        let _pin = pinned("reactions-quiet");
        let mut cabin = Cabin::quiet_for_test();
        let card = steam_post(&mut cabin, "a");
        let now = now_ms();
        assert_eq!(cabin.pulse_feed_posts(now).len(), 1);
        for _ in 0..3 {
            cabin.react(&card, Outcome::Ignored);
        }
        assert!(cabin
            .status
            .starts_with("You've skipped game news a few times"));
        assert!(cabin.pulse_feed_posts(now).is_empty());
        cabin.react_to_act(&PulseAct::MoreKind("a".into()));
        assert_eq!(cabin.pulse_feed_posts(now).len(), 1);
        assert!(!reactions::load(&reactions_path()).quiet("game news"));
    }

    #[test]
    fn proactive_candidates_follow_their_category_weight() {
        let _pin = pinned("reactions-proactive");
        let mut cabin = Cabin::quiet_for_test();
        let make = |source, topic: &str| {
            let mut c = Candidate::soft(source, &format!("look at {topic}"), topic, 0.6, 1.0, "w");
            c.reversible = Reversibility::Ledger;
            c
        };
        let cands = vec![
            make(CandidateSource::Workboard, "parser fix"),
            make(CandidateSource::AutomationHealth, "nightly backup"),
        ];
        // Equal value: topic order breaks the tie.
        let order = |v: &[Candidate]| v.iter().map(|c| c.topic.clone()).collect::<Vec<_>>();
        assert_eq!(
            order(&cabin.weigh_candidates(cands.clone())),
            vec!["nightly backup", "parser fix"]
        );
        for _ in 0..4 {
            cabin
                .reactions_now()
                .record(Outcome::Accepted, "coding next step", "parser fix", 1);
        }
        let weighed = cabin.weigh_candidates(cands.clone());
        assert_eq!(order(&weighed), vec!["parser fix", "nightly backup"]);
        assert!((weighed[0].value - 0.84).abs() < 1e-9);
        for _ in 0..3 {
            cabin.reactions_now().record(
                Outcome::Ignored,
                "automation health",
                "nightly backup",
                1,
            );
        }
        assert_eq!(order(&cabin.weigh_candidates(cands)), vec!["parser fix"]);
    }

    #[test]
    fn reset_clears_what_pulse_learned() {
        let _pin = pinned("reactions-reset");
        let mut cabin = Cabin::quiet_for_test();
        let card = steam_post(&mut cabin, "a");
        cabin.react(&card, Outcome::Accepted);
        cabin.reactions_now().reset();
        cabin.save_reactions();
        assert_eq!(reactions::load(&reactions_path()), Reactions::default());
    }
}
