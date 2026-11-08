//! Dynamic chat: the voice follows the topic. Each preset lists what it's best
//! for ([`crate::personality::Personality::topics`]); every turn scores the
//! topics from plain signals, smooths them, and a guest voice steps in only once
//! a topic has clearly taken over. Hysteresis keeps it from flapping: the
//! current topic gets a momentum bonus, a challenger has to win twice in a row,
//! and a switch holds for a few turns. A low mood moves to a support voice at
//! once. The chat's own voice comes back when the talk drifts home.
//!
//! An 18+ turn (the app sets [`Signals::adult`] only with adults-only on)
//! hands the chat to an adults-only voice at once. Explicit voices speak for
//! nothing else unless chosen by hand.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::personality::Personality;

/// What a stretch of chat is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Topic {
    Support,
    Teach,
    Fix,
    Plan,
    GameStrategy,
    Story,
    Hype,
    WindDown,
    Hangout,
    /// 18+ chat, from an adult who turned adults-only on.
    Adult,
}

impl Topic {
    pub const ALL: [Topic; 10] = [
        Topic::Support,
        Topic::Teach,
        Topic::Fix,
        Topic::Plan,
        Topic::GameStrategy,
        Topic::Story,
        Topic::Hype,
        Topic::WindDown,
        Topic::Hangout,
        Topic::Adult,
    ];
}

/// What the turn looks like, from the app's own per-turn read of the message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Signals<'a> {
    pub text: &'a str,
    /// Fixing something broken.
    pub repair: bool,
    /// A task or research ask (not chat).
    pub task: bool,
    pub question: bool,
    pub creative: bool,
    /// Sad, low or venting.
    pub low_mood: bool,
    pub frustrated: bool,
    /// An 18+ ask with adults-only on. Only the app sets it; words alone never do.
    pub adult: bool,
    /// Local hour, 0-23.
    pub hour: u8,
}

const SUPPORT: &[&str] = &[
    "rough day",
    "feel like shit",
    "feeling down",
    "i'm sad",
    "im sad",
    "depressed",
    "lonely",
    "anxious",
    "stressed",
    "overwhelmed",
    "breakup",
    "broke up",
    "miss her",
    "miss him",
    "miss them",
    "vent",
    "cry",
    "grief",
    "passed away",
    "funeral",
];
const TEACH: &[&str] = &[
    "how does",
    "how do",
    "explain",
    "what is",
    "what's the difference",
    "why does",
    "teach me",
    "eli5",
    "learn",
    "understand",
];
const FIX: &[&str] = &[
    "error",
    "crash",
    "broken",
    "won't start",
    "doesn't work",
    "not working",
    "bug",
    "fix",
    "stack trace",
    "failed to",
    "exception",
    "driver",
];
const PLAN: &[&str] = &[
    "plan",
    "schedule",
    "roadmap",
    "organize",
    "to-do",
    "todo",
    "checklist",
    "deadline",
    "steps to",
    "priorit",
];
const GAME_STRATEGY: &[&str] = &[
    "build",
    "meta",
    "tier list",
    "dps",
    "loadout",
    "strat",
    "min-max",
    "minmax",
    "stats",
    "talent",
    "rotation",
    "perk",
    "gear score",
    "best weapon",
    "counter pick",
    "comp",
];
const STORY: &[&str] = &[
    "story",
    "lore",
    "campaign",
    "quest",
    "dnd",
    "d&d",
    "dungeon",
    "narrate",
    "once upon",
    "plot",
];
const HYPE: &[&str] = &[
    "let's go",
    "lets go",
    "hyped",
    "ranked",
    "tournament",
    "finals",
    "pumped",
    "match tonight",
    "clutch",
    "pog",
    "we won",
];
const WIND_DOWN: &[&str] = &[
    "tired",
    "bed",
    "sleep",
    "chill",
    "relax",
    "long day",
    "unwind",
    "winding down",
    "exhausted",
];

/// Raw topic scores for one turn.
pub fn score(s: &Signals) -> BTreeMap<Topic, f32> {
    let low = format!(" {} ", s.text.to_lowercase());
    let hits = |words: &[&str]| words.iter().filter(|w| low.contains(*w)).count() as f32;
    let mut out = BTreeMap::new();
    let mut add = |t: Topic, v: f32| {
        if v > 0.0 {
            *out.entry(t).or_insert(0.0) += v;
        }
    };
    add(
        Topic::Support,
        hits(SUPPORT) + if s.low_mood { 3.0 } else { 0.0 },
    );
    add(
        Topic::Teach,
        hits(TEACH) + if s.question { 0.5 } else { 0.0 },
    );
    add(
        Topic::Fix,
        hits(FIX)
            + if s.repair { 2.0 } else { 0.0 }
            + if s.frustrated && s.task { 1.0 } else { 0.0 },
    );
    add(
        Topic::Plan,
        hits(PLAN) + if s.task && !s.repair { 0.5 } else { 0.0 },
    );
    add(Topic::GameStrategy, hits(GAME_STRATEGY));
    add(
        Topic::Story,
        hits(STORY) + if s.creative { 1.0 } else { 0.0 },
    );
    add(Topic::Hype, hits(HYPE));
    add(Topic::Adult, if s.adult { 4.0 } else { 0.0 });
    add(
        Topic::WindDown,
        hits(WIND_DOWN) + if (0..5).contains(&s.hour) { 0.5 } else { 0.0 },
    );
    // Plain chat with nothing else going on is hanging out.
    if !s.task && !s.repair && !s.question {
        add(Topic::Hangout, 1.0);
    }
    out
}

/// Bonus the current topic keeps over a challenger.
const MOMENTUM: f32 = 1.15;
/// Turns a challenger has to win in a row.
const WINS_TO_SWITCH: u32 = 2;
/// Turns a switch holds before anything else can move it.
const MIN_DWELL: u32 = 3;
/// How much of last turn's score carries over.
const DECAY: f32 = 0.5;
/// A topic needs at least this smoothed score to take over.
const FLOOR: f32 = 1.0;

/// The voice for one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Preset id that speaks this turn.
    pub voice: String,
    /// True on the turn the voice changed (home to guest, guest to guest, or back).
    pub switched: bool,
    pub topic: Topic,
}

/// Per-chat flow state. Kept in memory with the chat.
#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    home: String,
    voice: String,
    topic: Topic,
    smooth: BTreeMap<Topic, f32>,
    challenger: Option<(Topic, u32)>,
    dwell: u32,
}

impl Flow {
    /// A flow that starts in `home`, the chat's own voice.
    pub fn new(home: &str) -> Self {
        Flow {
            home: home.to_string(),
            voice: home.to_string(),
            topic: Topic::Hangout,
            smooth: BTreeMap::new(),
            challenger: None,
            dwell: MIN_DWELL,
        }
    }

    pub fn home(&self) -> &str {
        &self.home
    }

    pub fn voice(&self) -> &str {
        &self.voice
    }

    /// Steps one user turn. `roster` is the voices that may speak (the visible
    /// presets); `pinned` keeps the home voice (a hand-picked chat voice, "talk
    /// like X", or a background run).
    pub fn step(&mut self, signals: &Signals, roster: &[Personality], pinned: bool) -> Step {
        let raw = score(signals);
        for t in Topic::ALL {
            let prev = self.smooth.get(&t).copied().unwrap_or(0.0) * DECAY;
            let now = prev + raw.get(&t).copied().unwrap_or(0.0);
            self.smooth.insert(t, now);
        }
        self.dwell = self.dwell.saturating_add(1);
        let before = self.voice.clone();
        if pinned {
            self.voice = self.home.clone();
            self.challenger = None;
            return self.finish(before);
        }
        let leader = self.leader();
        // A low mood or an 18+ ask moves at once: waiting a turn reads as a refusal.
        let urgent = (signals.low_mood && leader == Topic::Support)
            || (signals.adult && leader == Topic::Adult);
        if leader == self.topic {
            self.challenger = None;
        } else if urgent {
            self.adopt(leader, roster);
        } else {
            let wins = match self.challenger {
                Some((t, n)) if t == leader => n + 1,
                _ => 1,
            };
            self.challenger = Some((leader, wins));
            if wins >= WINS_TO_SWITCH && self.dwell >= MIN_DWELL {
                self.adopt(leader, roster);
            }
        }
        self.finish(before)
    }

    fn finish(&self, before: String) -> Step {
        Step {
            voice: self.voice.clone(),
            switched: before != self.voice,
            topic: self.topic,
        }
    }

    fn leader(&self) -> Topic {
        let weight = |t: Topic| {
            let s = self.smooth.get(&t).copied().unwrap_or(0.0);
            if t == self.topic {
                s * MOMENTUM
            } else {
                s
            }
        };
        let best = Topic::ALL
            .into_iter()
            .max_by(|a, b| weight(*a).total_cmp(&weight(*b)))
            .unwrap_or(Topic::Hangout);
        if weight(best) < FLOOR {
            self.topic
        } else {
            best
        }
    }

    fn adopt(&mut self, topic: Topic, roster: &[Personality]) {
        self.topic = topic;
        self.challenger = None;
        self.dwell = 0;
        self.voice = voice_for(topic, &self.home, roster);
    }
}

/// The voice for `topic`: home when home covers it (or it's plain hangout),
/// else the first non-explicit voice that leads with it, else the first that
/// lists it, else home. An 18+ turn goes to an adults-only voice that lists
/// [`Topic::Adult`], and explicit voices speak for nothing else.
pub fn voice_for(topic: Topic, home: &str, roster: &[Personality]) -> String {
    let home_covers = roster
        .iter()
        .find(|p| p.id == home)
        .is_some_and(|p| p.topics.contains(&topic));
    if topic == Topic::Hangout || home_covers {
        return home.to_string();
    }
    let open = || {
        roster
            .iter()
            .filter(move |p| p.explicit == (topic == Topic::Adult))
    };
    open()
        .find(|p| p.topics.first() == Some(&topic))
        .or_else(|| open().find(|p| p.topics.contains(&topic)))
        .map(|p| p.id.clone())
        .unwrap_or_else(|| home.to_string())
}

/// The turn note on the turn a guest voice steps in or home comes back.
pub fn handoff_line(name: &str) -> String {
    format!(
        "This turn {name} takes the conversation: ease into that tone, no announcement, same memory and same rules."
    )
}

/// The short reminder every turn carries so a voice doesn't drift back to the
/// model's default over a long chat: the voice's name and its first sentence
/// of tone rules after the honesty line.
pub fn depth_note(p: &Personality) -> String {
    let rules = p.tone_rules.trim();
    let first = rules
        .split_inclusive(". ")
        .find(|s| !s.contains("sycophantic"))
        .unwrap_or(rules)
        .trim();
    let mut first: String = first.chars().take(180).collect();
    if !first.ends_with('.') {
        first.push('.');
    }
    format!("Stay in voice: {}. {first}", p.name.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::personality::builtins;

    fn chat(text: &str) -> Signals<'_> {
        Signals {
            text,
            hour: 20,
            ..Signals::default()
        }
    }

    #[test]
    fn a_scripted_drift_moves_voices_only_when_the_topic_holds() {
        let roster = builtins();
        let mut flow = Flow::new("nyx");
        // Banter: home.
        for t in [
            "lol you're ridiculous",
            "what are you up to tonight",
            "same honestly",
        ] {
            assert_eq!(flow.step(&chat(t), &roster, false).voice, "nyx", "{t}");
        }
        // A build error: one blip stays home, two turns of fixing bring the engineer.
        let fix = |text| Signals {
            repair: true,
            task: true,
            ..chat(text)
        };
        assert_eq!(
            flow.step(&fix("game crashed with an error"), &roster, false)
                .voice,
            "nyx"
        );
        let s = flow.step(&fix("still broken, driver error again"), &roster, false);
        assert_eq!(s.voice, "chief-engineer");
        assert!(s.switched);
        assert_eq!(s.topic, Topic::Fix);
        // Venting with a low mood: support at once, even inside the dwell.
        let vent = Signals {
            low_mood: true,
            ..chat("honestly I feel like shit, rough day")
        };
        let s = flow.step(&vent, &roster, false);
        assert_eq!(s.voice, "gentle-listener");
        assert!(s.switched);
        // Back to banter: holds through the dwell, then home again.
        let mut voices = vec![];
        for t in [
            "ok I'm better",
            "lol thanks",
            "anyway what are you up to",
            "same",
            "haha yeah",
        ] {
            voices.push(flow.step(&chat(t), &roster, false).voice);
        }
        assert_eq!(voices.first().map(String::as_str), Some("gentle-listener"));
        assert_eq!(voices.last().map(String::as_str), Some("nyx"), "{voices:?}");
    }

    #[test]
    fn a_one_off_blip_never_switches() {
        let roster = builtins();
        let mut flow = Flow::new("nyx");
        for _ in 0..4 {
            flow.step(&chat("just chilling lol"), &roster, false);
        }
        let s = flow.step(&chat("explain how does a VPN work"), &roster, false);
        assert!(!s.switched);
        for _ in 0..3 {
            let s = flow.step(&chat("haha ok cool"), &roster, false);
            assert_eq!(s.voice, "nyx");
        }
    }

    #[test]
    fn a_pin_keeps_the_home_voice() {
        let roster = builtins();
        let mut flow = Flow::new("pirate");
        let fix = Signals {
            repair: true,
            task: true,
            ..chat("error error crash broken")
        };
        for _ in 0..6 {
            let s = flow.step(&fix, &roster, true);
            assert_eq!(s.voice, "pirate");
            assert!(!s.switched);
        }
    }

    #[test]
    fn explicit_voices_speak_only_for_18_plus_turns() {
        let mut roster = builtins();
        for p in roster.iter_mut().filter(|p| p.explicit) {
            p.topics = Topic::ALL.to_vec();
        }
        for p in roster.iter_mut().filter(|p| !p.explicit) {
            p.topics.clear();
        }
        for t in Topic::ALL.into_iter().filter(|t| *t != Topic::Adult) {
            let v = voice_for(t, "nyx", &roster);
            assert_eq!(v, "nyx", "{t:?}");
        }
        // No clean voice ever takes an 18+ turn, even one that lists it.
        let mut clean: Vec<_> = builtins().into_iter().filter(|p| !p.explicit).collect();
        for p in clean.iter_mut() {
            p.topics = vec![Topic::Adult];
        }
        assert_eq!(voice_for(Topic::Adult, "nyx", &clean), "nyx");
    }

    #[test]
    fn an_18_plus_turn_hands_the_chat_to_the_adults_only_voice_at_once() {
        let roster = builtins();
        assert_eq!(voice_for(Topic::Adult, "nyx", &roster), "whore");
        let mut flow = Flow::new("nyx");
        for t in ["lol hey", "what's up"] {
            assert_eq!(flow.step(&chat(t), &roster, false).voice, "nyx");
        }
        let adult = Signals {
            adult: true,
            ..chat("talk dirty to me")
        };
        let s = flow.step(&adult, &roster, false);
        assert_eq!((s.voice.as_str(), s.topic), ("whore", Topic::Adult));
        assert!(s.switched, "no second turn of refusals first");
        assert_eq!(flow.step(&adult, &roster, false).voice, "whore");
        // Words alone never do it: only the app's own adult signal.
        let mut words = Flow::new("nyx");
        for _ in 0..4 {
            let s = words.step(&chat("talk dirty to me"), &roster, false);
            assert_eq!(s.voice, "nyx");
        }
        // A hand-picked voice holds, and so does a roster without the voice.
        let mut pinned = Flow::new("nyx");
        assert_eq!(pinned.step(&adult, &roster, true).voice, "nyx");
        let clean: Vec<_> = builtins().into_iter().filter(|p| !p.explicit).collect();
        let mut off = Flow::new("nyx");
        assert_eq!(off.step(&adult, &clean, false).voice, "nyx");
        // Back to plain chat: home again once the talk moves on.
        let voices: Vec<String> = (0..8)
            .map(|_| flow.step(&chat("haha ok cool"), &roster, false).voice)
            .collect();
        assert_eq!(voices.last().map(String::as_str), Some("nyx"), "{voices:?}");
    }

    #[test]
    fn home_keeps_topics_it_covers() {
        let roster = builtins();
        let nyx = roster.iter().find(|p| p.id == "nyx").unwrap();
        for t in &nyx.topics {
            assert_eq!(voice_for(*t, "nyx", &roster), "nyx");
        }
        assert_eq!(voice_for(Topic::Teach, "nyx", &roster), "professor");
        assert_eq!(voice_for(Topic::Teach, "professor", &roster), "professor");
    }

    #[test]
    fn notes_are_short_and_name_the_voice() {
        let nyx = crate::personality::builtin("nyx");
        let note = depth_note(&nyx);
        assert!(
            note.starts_with(&format!("Stay in voice: {}.", nyx.name)),
            "{note}"
        );
        assert!(!note.contains("sycophantic"));
        assert!(note.len() <= 240, "{note}");
        let line = handoff_line("Chief Engineer");
        assert!(line.contains("no announcement"));
        assert!(line.len() < 160);
    }

    #[test]
    fn signals_score_the_obvious_topics() {
        let top = |s: &Signals| {
            score(s)
                .into_iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(t, _)| t)
        };
        assert_eq!(
            top(&chat("what's the best build and talent rotation for dps")),
            Some(Topic::GameStrategy)
        );
        assert_eq!(
            top(&chat("tell me a story about the lore of this dungeon")),
            Some(Topic::Story)
        );
        assert_eq!(
            top(&chat("so tired, long day, gonna chill before bed")),
            Some(Topic::WindDown)
        );
        assert_eq!(
            top(&chat("ranked finals tonight, I'm hyped, let's go")),
            Some(Topic::Hype)
        );
        assert_eq!(top(&chat("hey")), Some(Topic::Hangout));
    }
}
