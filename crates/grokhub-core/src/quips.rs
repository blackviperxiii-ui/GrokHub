//! Quip bank: short jokes a voice may drop into casual chat, and the planner that
//! decides when. Lines are style examples (`assets/quips.toml`) the model riffs on
//! with the real moment; it never pastes them. The planner keeps quips rare:
//! answer first, at most one every few turns, none when the moment is serious,
//! and it backs off when the user doesn't laugh.
//!
//! Adults-only gates the heat ([`Heat`]): with it off only roast voices quip, and
//! only clean lines. Every shipped line passes [`crate::adult_guard::check`].

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::personality::Heat;

const BANK: &str = include_str!("../assets/quips.toml");

/// What set the joke up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Fail,
    Win,
    Death,
    Grind,
    Idle,
    LateNight,
    SelfOwn,
    RoastBack,
    Greeting,
}

impl Trigger {
    /// Plain words for the slot note.
    pub fn label(self) -> &'static str {
        match self {
            Trigger::Fail => "something just failed",
            Trigger::Win => "they just won",
            Trigger::Death => "they just died",
            Trigger::Grind => "a long grind",
            Trigger::Idle => "easy hangout chat",
            Trigger::LateNight => "it's very late",
            Trigger::SelfOwn => "they owned themselves",
            Trigger::RoastBack => "they're roasting you",
            Trigger::Greeting => "they just showed up",
        }
    }

    /// The trigger in a user message, from a small word list. `hour` is local
    /// time (0-23); past midnight and before 5 an idle line counts as late night.
    pub fn detect(text: &str, hour: u8) -> Trigger {
        let low = format!(" {} ", text.to_lowercase());
        let has = |words: &[&str]| words.iter().any(|w| low.contains(w));
        if has(&[
            "roast me",
            "you suck",
            "shut up",
            "bet you can't",
            "you're trash",
            "ur trash",
            "fight me",
        ]) {
            Trigger::RoastBack
        } else if has(&[
            "i'm an idiot",
            "im an idiot",
            "my bad",
            "i forgot",
            "i suck",
            "i'm dumb",
            "im dumb",
            "my fault",
        ]) {
            Trigger::SelfOwn
        } else if has(&[
            " died",
            " dead ",
            "killed me",
            " wiped",
            "one-shot",
            "one shot",
            "respawn",
        ]) {
            Trigger::Death
        } else if has(&[
            " won ",
            " won!",
            " beat ",
            "finally got",
            "clutch",
            "victory",
            " gg ",
            " pog",
        ]) {
            Trigger::Win
        } else if has(&[
            " lost ",
            " lost!",
            "wrecked",
            " failed",
            "crashed",
            "bricked",
            "threw the game",
        ]) {
            Trigger::Fail
        } else if has(&[
            " grind",
            "farming",
            " farm ",
            "for hours",
            "again and again",
        ]) {
            Trigger::Grind
        } else if has(&[" hey ", " yo ", " sup ", "good morning", " hiya", " hello"]) {
            Trigger::Greeting
        } else if hour < 5 {
            Trigger::LateNight
        } else {
            Trigger::Idle
        }
    }
}

/// How the joke works. The planner rotates these so jokes don't all sound alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mechanism {
    Simile,
    Hyperbole,
    Misdirect,
    Understatement,
    Callback,
    Absurd,
}

impl Mechanism {
    pub fn label(self) -> &'static str {
        match self {
            Mechanism::Simile => "a comparison",
            Mechanism::Hyperbole => "wild exaggeration",
            Mechanism::Misdirect => "a misdirect (set up one meaning, land another)",
            Mechanism::Understatement => "deadpan understatement",
            Mechanism::Callback => "a callback to something said earlier",
            Mechanism::Absurd => "an absurd image",
        }
    }
}

/// One example line.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quip {
    pub id: String,
    pub heat: Heat,
    pub trigger: Trigger,
    pub mechanism: Mechanism,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BankFile {
    quip: Vec<Quip>,
}

/// The shipped bank. A test pins that it parses.
pub fn bank() -> &'static [Quip] {
    static ALL: std::sync::LazyLock<Vec<Quip>> = std::sync::LazyLock::new(|| {
        toml::from_str::<BankFile>(BANK)
            .map(|f| f.quip)
            .unwrap_or_default()
    });
    &ALL
}

/// What the planner needs to know about this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// Repairing something, critical care, a low or frustrated mood, money,
    /// settings or approvals: no jokes.
    pub serious: bool,
    /// A casual chat turn (hangout, banter). Tasks and questions get no quip.
    pub casual: bool,
    pub trigger: Trigger,
    /// The user's message laughed at or played along with the last quip.
    pub landed: bool,
}

/// A quip the reply may carry this turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub trigger: Trigger,
    pub mechanism: Mechanism,
    pub examples: Vec<&'static str>,
    ids: Vec<&'static str>,
}

impl Slot {
    /// The one-line note for the turn's volatile block.
    pub fn note(&self) -> String {
        let examples = self
            .examples
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(" · ");
        format!(
            "Quip slot: after the answer, if it lands naturally, add one short joke in your own voice about this moment ({}), using {}. Style examples only, never copy them: {examples}. Skip it if it would feel forced.",
            self.trigger.label(),
            self.mechanism.label()
        )
    }
}

/// Turns between unprompted quips: 4 to 6, or 3 at Filthy.
const GAP_MIN: u32 = 4;
const GAP_SPREAD: u32 = 3;
const FILTHY_GAP: u32 = 3;
/// Quips the user let pass before the planner doubles the gap.
const IGNORED_BACKOFF: u32 = 2;
/// Turns before the same example may show again.
const EXAMPLE_COOLDOWN: u64 = 24;
/// Examples offered per slot.
const EXAMPLES: usize = 2;

/// Per-chat quip timing. Kept in memory with the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planner {
    turn: u64,
    last_quip: Option<u64>,
    gap: u32,
    ignored: u32,
    used: BTreeMap<&'static str, u64>,
    last_mechanism: Option<Mechanism>,
    seed: u64,
}

impl Planner {
    /// A planner for one chat. `seed` varies the gaps between chats (a hash of
    /// the chat id is fine).
    pub fn new(seed: u64) -> Self {
        let mut p = Planner {
            turn: 0,
            last_quip: None,
            gap: GAP_MIN,
            ignored: 0,
            used: BTreeMap::new(),
            last_mechanism: None,
            seed: seed | 1,
        };
        p.gap = p.next_gap(Heat::Cheeky);
        p
    }

    /// Steps one user turn and says whether this reply gets a quip slot.
    /// `heat` is [`Heat::Clean`] with adults-only off; `roast_voice` is the
    /// active voice's roast flag (with adults-only off only roast voices quip).
    pub fn step(&mut self, moment: Moment, heat: Heat, roast_voice: bool) -> Option<Slot> {
        self.turn += 1;
        if let Some(last) = self.last_quip {
            if self.turn == last + 1 {
                if moment.landed {
                    self.ignored = 0;
                } else {
                    self.ignored += 1;
                }
            }
        }
        if moment.serious || !moment.casual {
            return None;
        }
        if heat == Heat::Clean && !roast_voice {
            return None;
        }
        let since = self.last_quip.map(|l| self.turn - l);
        let roasting_back = moment.trigger == Trigger::RoastBack;
        let due = match since {
            None => self.turn >= u64::from(self.gap),
            // A roast-back earns a comeback on the next turn; nothing else does.
            Some(s) if roasting_back => s >= 1,
            Some(s) => s >= u64::from(self.backed_off_gap()),
        };
        if !due {
            return None;
        }
        let slot = self.pick(moment.trigger, heat)?;
        self.last_quip = Some(self.turn);
        self.last_mechanism = Some(slot.mechanism);
        for id in &slot.ids {
            self.used.insert(id, self.turn);
        }
        self.gap = self.next_gap(heat);
        Some(slot)
    }

    fn backed_off_gap(&self) -> u32 {
        if self.ignored >= IGNORED_BACKOFF {
            self.gap * 2
        } else {
            self.gap
        }
    }

    fn next_gap(&mut self, heat: Heat) -> u32 {
        // xorshift: no extra dependency, and repeatable in tests.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        if heat == Heat::Filthy {
            FILTHY_GAP
        } else {
            GAP_MIN + (self.seed % u64::from(GAP_SPREAD)) as u32
        }
    }

    fn fresh(&self, q: &Quip) -> bool {
        self.used
            .get(q.id.as_str())
            .is_none_or(|t| self.turn - t >= EXAMPLE_COOLDOWN)
    }

    fn pick(&self, trigger: Trigger, heat: Heat) -> Option<Slot> {
        // This heat first, then the next one down so a slot always has company.
        let fits = |q: &&Quip| {
            q.heat == heat || (heat > Heat::Cheeky && q.heat < heat && q.heat >= Heat::Cheeky)
        };
        let mut pool: Vec<&Quip> = bank()
            .iter()
            .filter(fits)
            .filter(|q| self.fresh(q))
            .collect();
        if pool.is_empty() {
            return None;
        }
        pool.sort_by_key(|q| (q.trigger != trigger, q.heat != heat, q.id.clone()));
        let mechanism = pool
            .iter()
            .map(|q| q.mechanism)
            .find(|m| Some(*m) != self.last_mechanism)
            .unwrap_or(pool[0].mechanism);
        let mut chosen: Vec<&Quip> = pool
            .iter()
            .copied()
            .filter(|q| q.mechanism == mechanism)
            .take(1)
            .collect();
        for q in &pool {
            if chosen.len() >= EXAMPLES {
                break;
            }
            if !chosen.iter().any(|c| c.id == q.id) {
                chosen.push(q);
            }
        }
        Some(Slot {
            trigger,
            mechanism,
            examples: chosen.iter().map(|q| q.text.as_str()).collect(),
            ids: chosen.iter().map(|q| q.id.as_str()).collect(),
        })
    }
}

/// The user laughed or played along: "lol", "lmao", "haha", 😂, 💀 or a comeback.
pub fn landed(text: &str) -> bool {
    let low = text.to_lowercase();
    [
        "lol",
        "lmao",
        "lmfao",
        "haha",
        "rofl",
        "😂",
        "🤣",
        "💀",
        "dead 💀",
        "i'm dead",
        "im dead",
    ]
    .iter()
    .any(|w| low.contains(w))
        || Trigger::detect(text, 12) == Trigger::RoastBack
}

/// An in-chat ask to change the heat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nudge {
    Hotter,
    Milder,
}

/// "Roast me harder" turns the heat up one step, "ease off" turns it down.
pub fn nudge(text: &str) -> Option<Nudge> {
    let low = text.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| low.contains(w));
    if any(&[
        "ease off",
        "tone it down",
        "too much",
        "chill out",
        "dial it back",
        "too far",
        "keep it clean",
    ]) {
        Some(Nudge::Milder)
    } else if any(&[
        "roast me harder",
        "go harder",
        "be meaner",
        "dirtier",
        "filthier",
        "turn it up",
        "more raunchy",
        "don't hold back",
    ]) {
        Some(Nudge::Hotter)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adult_guard::{check, Risk};

    fn casual(trigger: Trigger) -> Moment {
        Moment {
            serious: false,
            casual: true,
            trigger,
            landed: false,
        }
    }

    #[test]
    fn the_bank_parses_with_unique_ids_and_every_heat_and_trigger() {
        let all = toml::from_str::<BankFile>(BANK)
            .expect("assets/quips.toml parses")
            .quip;
        assert_eq!(all.len(), bank().len());
        let mut ids = std::collections::BTreeSet::new();
        for q in &all {
            assert!(ids.insert(q.id.clone()), "{} twice", q.id);
            assert!(q.text.len() <= 140, "{} is too long for a quip", q.id);
        }
        for heat in [Heat::Clean, Heat::Cheeky, Heat::Spicy, Heat::Filthy] {
            for t in [
                Trigger::Fail,
                Trigger::Win,
                Trigger::Death,
                Trigger::Grind,
                Trigger::Idle,
                Trigger::LateNight,
                Trigger::SelfOwn,
                Trigger::RoastBack,
                Trigger::Greeting,
            ] {
                assert!(
                    all.iter().any(|q| q.heat == heat && q.trigger == t),
                    "no {heat:?} line for {t:?}"
                );
            }
        }
    }

    #[test]
    fn every_line_passes_the_guard_and_clean_heats_have_no_sexual_words() {
        for q in bank() {
            let risk = check(&q.text);
            assert!(!matches!(risk, Risk::Banned(_)), "{}: {risk:?}", q.id);
            if q.heat <= Heat::Cheeky {
                assert_eq!(risk, Risk::Clean, "{} reads as explicit", q.id);
            }
            if q.heat == Heat::Clean {
                let low = q.text.to_lowercase();
                let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).collect();
                for word in ["fuck", "shit", "ass", "damn", "hell", "hookup", "booty"] {
                    assert!(
                        !words.contains(&word),
                        "{} swears with adults-only off",
                        q.id
                    );
                }
            }
        }
    }

    #[test]
    fn no_line_punches_at_who_someone_is() {
        let mean = [
            "retard",
            "fag",
            "tranny",
            "ugly",
            " fat",
            "fatass",
            "loser",
            "worthless",
            "kill yourself",
            "kys",
            "virgin",
            "teen",
            "kid",
            "child",
            "school",
        ];
        for q in bank() {
            let low = format!(" {}", q.text.to_lowercase());
            for w in mean {
                assert!(!low.contains(w), "{}: {w:?}", q.id);
            }
        }
    }

    #[test]
    fn adults_only_off_quips_only_in_roast_voices_and_only_clean_lines() {
        let mut p = Planner::new(7);
        for _ in 0..30 {
            assert_eq!(p.step(casual(Trigger::Fail), Heat::Clean, false), None);
        }
        let mut p = Planner::new(7);
        let mut slots = 0;
        for _ in 0..30 {
            if let Some(slot) = p.step(casual(Trigger::Fail), Heat::Clean, true) {
                slots += 1;
                for id in &slot.ids {
                    assert!(id.starts_with("c-"), "{id} is not a clean line");
                }
            }
        }
        assert!(slots >= 4, "{slots} slots in 30 turns");
    }

    #[test]
    fn quips_come_every_four_to_six_turns_and_every_three_at_filthy() {
        let mut moment = casual(Trigger::Idle);
        moment.landed = true;
        let mut p = Planner::new(42);
        let turns: Vec<u64> = (1..=60u64)
            .filter(|_| p.step(moment, Heat::Cheeky, false).is_some())
            .collect();
        assert!(turns.len() >= 9, "{turns:?}");
        assert!((4..=6).contains(&turns[0]), "{turns:?}");
        for w in turns.windows(2) {
            assert!((4..=6).contains(&(w[1] - w[0])), "{turns:?}");
        }
        let mut p = Planner::new(42);
        let turns: Vec<u64> = (1..=30u64)
            .filter(|_| p.step(moment, Heat::Filthy, false).is_some())
            .collect();
        for w in turns.windows(2) {
            assert_eq!(w[1] - w[0], 3, "{turns:?}");
        }
    }

    #[test]
    fn serious_moments_and_tasks_never_get_a_quip() {
        let mut p = Planner::new(1);
        let serious = Moment {
            serious: true,
            ..casual(Trigger::Fail)
        };
        let task = Moment {
            casual: false,
            ..casual(Trigger::Fail)
        };
        for _ in 0..40 {
            assert_eq!(p.step(serious, Heat::Filthy, true), None);
            assert_eq!(p.step(task, Heat::Filthy, true), None);
        }
    }

    #[test]
    fn ignored_quips_double_the_gap_and_a_laugh_resets_it() {
        let mut p = Planner::new(3);
        let mut quiet = vec![];
        for turn in 1..=80u64 {
            if p.step(casual(Trigger::Idle), Heat::Spicy, false).is_some() {
                quiet.push(turn);
            }
        }
        let late_gaps: Vec<u64> = quiet.windows(2).skip(2).map(|w| w[1] - w[0]).collect();
        assert!(
            late_gaps.iter().all(|g| *g >= 8),
            "backs off after 2 ignored: {quiet:?}"
        );

        let mut p = Planner::new(3);
        let mut moment = casual(Trigger::Idle);
        moment.landed = true;
        let laughing: Vec<u64> = (1..=40u64)
            .filter(|_| p.step(moment, Heat::Spicy, false).is_some())
            .collect();
        assert!(
            laughing.windows(2).all(|w| w[1] - w[0] <= 6),
            "{laughing:?}"
        );
    }

    #[test]
    fn a_roast_back_gets_a_comeback_right_away() {
        let mut p = Planner::new(9);
        let mut first = None;
        for turn in 1..=10u64 {
            if p.step(casual(Trigger::Idle), Heat::Cheeky, false).is_some() {
                first = Some(turn);
                break;
            }
        }
        assert!(first.is_some());
        let back = p.step(casual(Trigger::RoastBack), Heat::Cheeky, false);
        assert_eq!(back.map(|s| s.trigger), Some(Trigger::RoastBack));
        // Anything else right after still waits.
        assert_eq!(p.step(casual(Trigger::Idle), Heat::Cheeky, false), None);
    }

    #[test]
    fn examples_rest_before_they_repeat_and_mechanisms_rotate() {
        let mut p = Planner::new(11);
        let mut moment = casual(Trigger::Fail);
        moment.landed = true;
        let mut seen: BTreeMap<&str, u64> = BTreeMap::new();
        let mut last_mech = None;
        for turn in 1..=120u64 {
            if let Some(slot) = p.step(moment, Heat::Spicy, false) {
                assert_ne!(Some(slot.mechanism), last_mech, "turn {turn}");
                last_mech = Some(slot.mechanism);
                assert!(!slot.examples.is_empty() && slot.examples.len() <= EXAMPLES);
                for id in slot.ids {
                    if let Some(prev) = seen.insert(id, turn) {
                        assert!(turn - prev >= EXAMPLE_COOLDOWN, "{id} at {prev} and {turn}");
                    }
                }
            }
        }
    }

    #[test]
    fn spicy_slots_never_offer_clean_or_filthy_lines() {
        let mut p = Planner::new(5);
        let mut moment = casual(Trigger::Win);
        moment.landed = true;
        for _ in 0..60 {
            if let Some(slot) = p.step(moment, Heat::Spicy, false) {
                for id in slot.ids {
                    assert!(id.starts_with("s-") || id.starts_with("k-"), "{id}");
                }
            }
        }
    }

    #[test]
    fn the_slot_note_says_riff_never_copy() {
        let mut p = Planner::new(2);
        let slot = (0..10)
            .find_map(|_| p.step(casual(Trigger::Death), Heat::Cheeky, false))
            .unwrap();
        let note = slot.note();
        assert!(note.starts_with("Quip slot: after the answer"), "{note}");
        assert!(note.contains("they just died"));
        assert!(note.contains("never copy them"));
        assert!(note.contains("Skip it if it would feel forced."));
        assert!(note.len() < 520, "{} bytes", note.len());
    }

    #[test]
    fn triggers_landing_and_nudges_read_plain_words() {
        assert_eq!(
            Trigger::detect("ugh I died to the boss again", 20),
            Trigger::Death
        );
        assert_eq!(Trigger::detect("WE WON lets go", 20), Trigger::Win);
        assert_eq!(Trigger::detect("roast me", 20), Trigger::RoastBack);
        assert_eq!(
            Trigger::detect("my bad, I forgot the key", 20),
            Trigger::SelfOwn
        );
        assert_eq!(
            Trigger::detect("been farming mats for hours", 20),
            Trigger::Grind
        );
        assert_eq!(Trigger::detect("yo what's up", 20), Trigger::Greeting);
        assert_eq!(Trigger::detect("can't sleep", 3), Trigger::LateNight);
        assert_eq!(Trigger::detect("what are you up to", 15), Trigger::Idle);
        assert!(landed("lmao ok that's good"));
        assert!(landed("💀"));
        assert!(!landed("ok"));
        assert_eq!(nudge("roast me harder"), Some(Nudge::Hotter));
        assert_eq!(nudge("whoa ease off a bit"), Some(Nudge::Milder));
        assert_eq!(nudge("nice"), None);
    }
}
