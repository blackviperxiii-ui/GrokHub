//! Offline routing eval for the ask reader ([`super::ask`]).
//!
//! A hand-labeled set of chat turns, each with the kind a careful engineer
//! would file it under and the effort range that serves it (lowest rung that
//! does the job, highest that is still worth paying for). The test reports:
//!
//! - kind accuracy (exact kind, and "same lane" which is what routing uses);
//! - effort in range;
//! - under-effort rate (quality risk; weighted heavier) and over-effort rate
//!   (wasted tokens);
//! - expected reasoning spend against always sending High.
//!
//! Run with `cargo test -p grokhub-agent --lib route::ask_eval -- --nocapture`
//! to see the table and every miss.

use super::ask::{read, AskContext, AskKind};
use AskKind::*;

/// (turn, kind, lowest ok effort, highest ok effort)
pub(crate) const SET: &[(&str, AskKind, &str, &str)] = &[
    // Greetings and acknowledgements
    ("hey", Greeting, "none", "low"),
    ("yo what's up", Greeting, "none", "low"),
    ("thanks!", Greeting, "none", "low"),
    ("gg", Greeting, "none", "low"),
    ("ok cool", Greeting, "none", "low"),
    ("lol nice", Greeting, "none", "low"),
    ("good morning", Greeting, "none", "low"),
    ("thx bro appreciate it", Greeting, "none", "low"),
    // Quick facts
    ("What is the capital of Australia?", QuickFact, "none", "low"),
    ("how many ounces in a pound", QuickFact, "none", "low"),
    ("who won the 2018 world cup?", QuickFact, "none", "low"),
    ("what does GPU stand for?", QuickFact, "none", "low"),
    ("convert 72 F to celsius", QuickFact, "none", "low"),
    ("translate 'good game' to spanish", QuickFact, "none", "low"),
    ("define latency", QuickFact, "none", "low"),
    ("how tall is mount everest", QuickFact, "none", "low"),
    // Everyday chat
    ("I'm bored, any ideas for tonight?", Chat, "low", "medium"),
    ("I just hit level 50 in Elden Ring finally", Chat, "low", "medium"),
    ("my cat keeps knocking stuff off my desk, any tips?", Chat, "low", "medium"),
    ("give me a pep talk, rough day at work", Chat, "low", "medium"),
    ("what should I name my new pc build", Creative, "low", "medium"),
    ("I can't decide what game to play next, I like souls-likes and roguelikes", Chat, "low", "medium"),
    // Creative
    ("Write a short story about a dragon who loves coffee", Creative, "low", "medium"),
    ("write me a poem about losing a ranked match", Creative, "low", "medium"),
    ("give me 10 gamertag ideas with a viper theme", Creative, "low", "medium"),
    ("tell me a joke about creepers", Creative, "none", "low"),
    ("write lyrics for a lofi song about late night coding", Creative, "low", "medium"),
    ("roleplay as a grumpy blacksmith in a fantasy tavern", Creative, "low", "medium"),
    ("write a backstory for my D&D tiefling warlock", Creative, "low", "medium"),
    // Explain
    ("Explain the difference between TCP and UDP for game netcode", Explain, "low", "high"),
    ("how does ray tracing work?", Explain, "low", "high"),
    ("explain how a CPU cache works like I'm five", Explain, "low", "medium"),
    ("compare DDR4 vs DDR5 for gaming", Explain, "low", "high"),
    ("what's the difference between a mutex and a semaphore", Explain, "low", "high"),
    ("walk me through how DLSS frame generation works", Explain, "low", "high"),
    ("pros and cons of an AIO vs air cooler", Explain, "low", "medium"),
    ("should I upgrade from a 3070 to a 5070?", Explain, "low", "high"),
    ("summarize the plot of the witcher 3", Explain, "low", "medium"),
    // Research
    ("What are the patch notes for today's update?", Research, "low", "high"),
    ("what's the current meta in apex legends this season", Research, "low", "high"),
    ("latest news on the GTA 6 release date", Research, "low", "high"),
    ("best build for a bleed katana in elden ring right now", Research, "low", "high"),
    ("find me reviews of the new steam deck", Research, "low", "high"),
    ("price of an rtx 5090 today", Research, "low", "medium"),
    // Small code
    ("Write a Python function that reverses a string", CodeSmall, "low", "medium"),
    ("how do I reverse a vec in rust", CodeSmall, "low", "medium"),
    ("write a bash one-liner to count lines in all .rs files", CodeSmall, "low", "medium"),
    ("regex to match an email address", CodeSmall, "low", "medium"),
    ("convert this to typescript:\n```js\nfunction add(a,b){return a+b}\n```", CodeSmall, "low", "medium"),
    ("add a doc comment to this fn:\n```rust\nfn area(w: f64, h: f64) -> f64 { w * h }\n```", CodeSmall, "low", "medium"),
    ("write a SQL query that finds the top 5 players by kills", CodeSmall, "low", "medium"),
    ("rename the variable x to player_count in this snippet: `let x = 4;`", CodeSmall, "low", "medium"),
    ("Implement a binary search in Rust with tests", CodeSmall, "low", "high"),
    ("write a lua script for a WoW addon that prints my gold on login", CodeSmall, "low", "high"),
    // Multi-file code
    ("Refactor the auth flow across the codebase: move src/auth.rs, src/session.rs and src/api/login.rs into a new crate and update every caller", CodeMulti, "high", "xhigh"),
    ("build me an app in Tauri and React that tracks my game backlog with a sqlite database", CodeMulti, "medium", "xhigh"),
    ("migrate the whole repo from serde_yaml to toml across all crates", CodeMulti, "high", "xhigh"),
    ("add a settings page to the app: new route in ui/src/App.tsx, a Settings.tsx component and a tauri command in src-tauri/src/lib.rs", CodeMulti, "medium", "xhigh"),
    ("port this python project to rust, it has 6 modules and a CLI", CodeMulti, "high", "xhigh"),
    // Debug
    ("I get this error:\n```\nthread 'main' panicked at src/main.rs:10:5\nindex out of bounds\n```\nwhy?", Debug, "medium", "high"),
    ("my game crashes on launch with a black screen, how do I fix it", Debug, "medium", "high"),
    ("why does my rust code say cannot borrow `x` as mutable more than once", Debug, "medium", "high"),
    ("Traceback (most recent call last):\n  File \"bot.py\", line 12, in <module>\n    KeyError: 'token'\nhelp", Debug, "medium", "high"),
    ("my fps drops to 20 in warzone every few minutes, stuttering like crazy", Debug, "medium", "high"),
    ("Debug a deadlock: two async tasks lock a Mutex in src/a.rs and src/b.rs in opposite order under load. It's subtle and only in production. Make sure the fix is lock-free and handles edge cases, then add tests.", Debug, "high", "xhigh"),
    ("npm install fails with ERESOLVE unable to resolve dependency tree", Debug, "medium", "high"),
    ("the tests are flaky on CI but pass locally, what's wrong", Debug, "medium", "xhigh"),
    ("my discord bot doesn't respond to commands anymore", Debug, "medium", "high"),
    // Plan
    ("Make me a plan to get to Diamond in ranked this season, step by step", Plan, "medium", "high"),
    ("what's the best way to learn rust in 3 months? give me a roadmap", Plan, "medium", "high"),
    ("plan a weekend LAN party for 8 people", Plan, "medium", "high"),
    ("outline the milestones for launching my indie game on steam", Plan, "medium", "high"),
    ("how should I approach rewriting our backend without downtime", Plan, "high", "xhigh"),
    // Design
    ("Design the database schema for a mod manager", Design, "medium", "high"),
    ("design the architecture for a multiplayer game server that scales to 10k players", Design, "high", "xhigh"),
    ("sketch a UI layout for a settings page with tabs", Design, "medium", "high"),
    ("design a REST API for a game inventory system", Design, "medium", "high"),
    // Math and logic
    ("Prove that the square root of 2 is irrational.", Math, "medium", "high"),
    ("solve 3x + 7 = 22", Math, "low", "medium"),
    ("what's the probability of rolling at least one 6 in four rolls of a die", Math, "medium", "high"),
    ("calculate my dps if I hit for 1200 with a 1.5 attack speed and 30% crit at 2x", Math, "low", "high"),
    ("find the integral of x^2 * e^x dx", Math, "medium", "high"),
    ("what's the time complexity of quicksort and why", Math, "medium", "high"),
    ("logic puzzle: three boxes are all mislabeled, one has apples, one oranges, one both. how many picks to fix the labels?", Math, "medium", "high"),
];

fn rung(e: &str) -> usize {
    grokhub_core::model_registry::EFFORT_LADDER
        .iter()
        .position(|r| *r == e)
        .expect("effort on the ladder")
}

/// Rough reasoning tokens per rung, for the spend comparison only.
fn reasoning_cost(r: usize) -> f64 {
    [0.0, 100.0, 400.0, 1_200.0, 3_500.0, 8_000.0, 16_000.0][r.min(6)]
}

pub(crate) struct Score {
    pub n: usize,
    pub kind_exact: usize,
    pub lane_same: usize,
    pub effort_ok: usize,
    pub under: usize,
    pub over: usize,
    pub spend: f64,
    pub spend_high: f64,
    pub misses: Vec<String>,
}


/// Held out: written after the reader was tuned on [`SET`], never tuned on.
/// Its numbers are the honest ones.
pub(crate) const HELD_OUT: &[(&str, AskKind, &str, &str)] = &[
    ("sup dude", Greeting, "none", "low"),
    ("awesome thank you so much", Greeting, "none", "low"),
    ("gn", Greeting, "none", "low"),
    ("who is the CEO of valve?", QuickFact, "none", "low"),
    ("how much is 15% of 80", QuickFact, "none", "low"),
    ("when did minecraft come out", QuickFact, "none", "low"),
    ("how far is the moon", QuickFact, "none", "low"),
    ("I think I'm finally getting good at valorant", Chat, "low", "medium"),
    ("what do you think about open world games getting so big", Chat, "low", "medium"),
    ("rate my setup: 7800x3d, 4080 super, 32gb ram", Chat, "low", "medium"),
    ("write a haiku about lag", Creative, "none", "medium"),
    ("make up a villain for my minecraft server lore", Creative, "low", "medium"),
    ("write a funny bio for my twitch channel", Creative, "low", "medium"),
    ("explain how anti-cheat software detects cheats", Explain, "low", "high"),
    ("what's the difference between g-sync and freesync", Explain, "low", "high"),
    ("how do vaccines train the immune system", Explain, "low", "high"),
    ("tl;dr of the rust ownership model", Explain, "low", "medium"),
    ("what's the latest on the next nintendo console", Research, "low", "high"),
    ("best settings for cs2 on a 240hz monitor", Research, "low", "high"),
    ("look up the release date for hollow knight silksong", Research, "low", "high"),
    ("write a python script that renames all my screenshots by date", CodeSmall, "low", "medium"),
    ("how to read a json file in rust with serde", CodeSmall, "low", "medium"),
    ("give me a css snippet for a glowing button", CodeSmall, "low", "medium"),
    ("write a function in typescript that debounces input", CodeSmall, "low", "medium"),
    ("refactor the whole codebase to use async across all modules", CodeMulti, "high", "xhigh"),
    ("build a game from scratch in godot with menus, saves and a level editor", CodeMulti, "medium", "xhigh"),
    ("split src/app.rs into src/app/ui.rs, src/app/state.rs and src/app/net.rs and fix the imports", CodeMulti, "medium", "xhigh"),
    ("my pc blue screens with IRQL_NOT_LESS_OR_EQUAL when gaming", Debug, "medium", "high"),
    ("error[E0382]: borrow of moved value: `cfg`\n --> src/main.rs:14:20\nhow do I fix", Debug, "medium", "high"),
    ("why is my docker container exiting with code 137", Debug, "medium", "high"),
    ("cargo build fails with linker cc not found", Debug, "medium", "high"),
    ("steam keeps crashing when I open my library", Debug, "medium", "high"),
    ("give me a step by step leveling plan for a new league account", Plan, "medium", "high"),
    ("plan my week so I can study for exams and still raid on thursday", Plan, "medium", "high"),
    ("what's the best approach to moving my homelab to kubernetes", Plan, "medium", "xhigh"),
    ("design a save file format for my game that supports versioning", Design, "medium", "high"),
    ("design the data model for a tournament bracket app", Design, "medium", "high"),
    ("how many ways can 5 players be arranged in a line", Math, "low", "high"),
    ("prove there are infinitely many primes", Math, "medium", "high"),
    ("solve for x: 2x^2 - 8 = 0", Math, "low", "medium"),
];

pub(crate) fn score_set(set: &[(&str, AskKind, &str, &str)]) -> Score {
    let mut s = Score {
        n: set.len(),
        kind_exact: 0,
        lane_same: 0,
        effort_ok: 0,
        under: 0,
        over: 0,
        spend: 0.0,
        spend_high: 0.0,
        misses: Vec::new(),
    };
    for (text, kind, lo, hi) in set {
        let r = read(text, &AskContext::default());
        let (lo, hi) = (rung(lo), rung(hi));
        if r.kind == *kind {
            s.kind_exact += 1;
        }
        if r.kind.lane() == kind.lane() {
            s.lane_same += 1;
        }
        if r.effort < lo {
            s.under += 1;
        } else if r.effort > hi {
            s.over += 1;
        } else {
            s.effort_ok += 1;
        }
        s.spend += reasoning_cost(r.effort);
        s.spend_high += reasoning_cost(rung("high"));
        if r.kind != *kind || r.effort < lo || r.effort > hi {
            let short: String = text.chars().take(60).collect();
            s.misses.push(format!(
                "{short:?}: got {} {} (d {:.2}), want {} {}..{}",
                r.kind.id(),
                r.effort_name(),
                r.d,
                kind.id(),
                grokhub_core::model_registry::EFFORT_LADDER[lo],
                grokhub_core::model_registry::EFFORT_LADDER[hi],
            ));
        }
    }
    s
}

fn report(name: &str, s: &Score) {
    let pct = |x: usize| 100.0 * x as f64 / s.n as f64;
    println!(
        "ask eval {name}: {} turns · kind {:.1}% · lane {:.1}% · effort in range {:.1}% · under {:.1}% · over {:.1}% · reasoning spend {:.0}% of always-High",
        s.n,
        pct(s.kind_exact),
        pct(s.lane_same),
        pct(s.effort_ok),
        pct(s.under),
        pct(s.over),
        100.0 * s.spend / s.spend_high,
    );
    for m in &s.misses {
        println!("  miss {m}");
    }
}

#[test]
fn ask_reader_routes_the_labeled_set() {
    let s = score_set(SET);
    report("tuning", &s);
    let pct = |x: usize| 100.0 * x as f64 / s.n as f64;
    assert!(s.n >= 80, "set too small: {}", s.n);
    assert!(pct(s.lane_same) >= 90.0, "lane accuracy {:.1}%", pct(s.lane_same));
    assert!(pct(s.effort_ok) >= 90.0, "effort in range {:.1}%", pct(s.effort_ok));
    // Under-thinking costs quality; keep it rare.
    assert!(pct(s.under) <= 5.0, "under-effort {:.1}%", pct(s.under));
    // And the whole point: far less reasoning than always-High.
    assert!(s.spend < 0.6 * s.spend_high, "spend {:.0}", s.spend);
}

#[test]
fn ask_reader_holds_up_on_turns_it_was_not_tuned_on() {
    let s = score_set(HELD_OUT);
    report("held-out", &s);
    let pct = |x: usize| 100.0 * x as f64 / s.n as f64;
    assert!(s.n >= 40, "held-out too small: {}", s.n);
    assert!(pct(s.lane_same) >= 80.0, "lane accuracy {:.1}%", pct(s.lane_same));
    assert!(pct(s.under) <= 10.0, "under-effort {:.1}%", pct(s.under));
}

#[test]
fn every_kind_has_labeled_turns() {
    for k in AskKind::ALL {
        let n = SET.iter().filter(|(_, kind, _, _)| *kind == k).count();
        assert!(n >= 4, "{} has {n} labeled turns", k.id());
    }
}
