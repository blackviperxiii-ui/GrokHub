//! B5–B7 tests: the neighbour blend, the bank on disk and the live turn flow.
//!
//! No real embedding model is on disk here and nothing fetches one, so every
//! test that needs embeddings installs a deterministic fake through
//! [`crate::memory_embed::set_test_model`]: [`toy_model`], a hashed
//! bag-of-words over the labeled sets' own words with a few topic groups.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use grokhub_core::model_registry::store::save_registry;
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Registry, SourceKind};
use grokhub_core::static_embed::StaticModel;

use super::ask::{AskContext, AskKind, AskRead, Stakes, STAKES_LANE};
use super::ask_set::{HELD_OUT, SET, STAKES_HELD_OUT, STAKES_SET};
use super::guard::in_holdout;
use super::knn::*;
use super::live::{self, RouteCall, RouteDone};

// ---------------------------------------------------------------- fake embedder

/// Words that carry no meaning in the fake (as stop words weigh little in the real model).
const STOP: &[&str] = &[
    "a", "an", "the", "to", "of", "in", "on", "for", "and", "or", "is", "it", "my", "me", "i",
    "you", "this", "that", "what", "how", "do", "does", "with", "s", "t", "at", "be", "can", "are",
    "from", "by", "all", "any", "so", "just", "get", "like", "up", "if", "as", "its", "your",
    "one", "after", "when", "should", "give", "make", "write", "some", "about", "m",
];

/// Extra words the live tests type.
const EXTRA: &[&str] = &["nsfw", "sex", "scene", "wrong", "again", "loader", "config"];

/// Topic groups: a word in a group leans toward the group's direction, as
/// words about one topic sit close in a real embedding model. Topics, not
/// the reader's kinds: "fix" sits with errors whether the turn is a debug or not.
const TOPICS: &[&[&str]] = &[
    &[
        "hey",
        "yo",
        "sup",
        "thanks",
        "thx",
        "thank",
        "gg",
        "cool",
        "nice",
        "lol",
        "morning",
        "gn",
        "awesome",
        "appreciate",
        "ok",
        "dude",
        "bro",
        "much",
    ],
    &[
        "python",
        "rust",
        "function",
        "bash",
        "script",
        "regex",
        "sql",
        "query",
        "typescript",
        "fn",
        "snippet",
        "variable",
        "lua",
        "css",
        "json",
        "serde",
        "debounce",
        "vec",
        "string",
        "code",
        "binary",
        "tests",
        "implement",
        "js",
        "rename",
        "reverse",
        "reverses",
        "hash",
        "truncate",
        "addon",
    ],
    &[
        "error",
        "crash",
        "crashes",
        "crashing",
        "panicked",
        "traceback",
        "fails",
        "failed",
        "broken",
        "flaky",
        "deadlock",
        "stuttering",
        "fix",
        "exiting",
        "linker",
        "blue",
        "screens",
        "keyerror",
        "borrow",
        "moved",
        "respond",
        "drops",
        "black",
        "screen",
    ],
    &[
        "refactor", "codebase", "crate", "crates", "repo", "migrate", "modules", "module",
        "across", "files", "port", "project", "whole", "split", "imports", "caller",
    ],
    &[
        "game",
        "games",
        "elden",
        "ring",
        "valorant",
        "apex",
        "warzone",
        "minecraft",
        "steam",
        "ranked",
        "league",
        "cs2",
        "witcher",
        "gta",
        "boss",
        "raid",
        "souls",
        "roguelikes",
        "level",
        "fps",
        "diamond",
    ],
    &[
        "gpu", "cpu", "ddr4", "ddr5", "rtx", "3070", "5070", "4080", "cooler", "aio", "ram",
        "monitor", "setup", "pc", "240hz", "cache",
    ],
    &[
        "prove",
        "probability",
        "integral",
        "solve",
        "calculate",
        "irrational",
        "primes",
        "complexity",
        "quicksort",
        "square",
        "root",
        "dps",
        "arranged",
        "rolling",
    ],
    &[
        "plan",
        "roadmap",
        "step",
        "milestones",
        "approach",
        "week",
        "outline",
        "leveling",
        "weekend",
        "months",
    ],
    &[
        "design",
        "schema",
        "architecture",
        "api",
        "layout",
        "ui",
        "format",
        "rest",
        "sketch",
        "bracket",
        "inventory",
    ],
    &[
        "story",
        "poem",
        "lyrics",
        "haiku",
        "joke",
        "roleplay",
        "backstory",
        "villain",
        "bio",
        "name",
        "gamertag",
        "ideas",
        "funny",
    ],
    &[
        "latest", "news", "patch", "notes", "current", "meta", "today", "price", "reviews",
        "release", "date", "look", "find",
    ],
    &[
        "explain",
        "difference",
        "compare",
        "works",
        "work",
        "pros",
        "cons",
        "summarize",
        "tl",
        "dr",
        "walk",
        "through",
    ],
];

/// A word's row: its own pseudo-random direction, pulled toward its topic's.
fn toy_row(word: &str, dim: usize) -> Vec<f32> {
    let own = unit(row(word, dim));
    let Some(t) = TOPICS.iter().position(|g| g.contains(&word)) else {
        return own;
    };
    let topic = unit(row(&format!("topic#{t}"), dim));
    unit(
        topic
            .iter()
            .zip(&own)
            .map(|(a, b)| 0.85 * a + 0.5 * b)
            .collect(),
    )
}

fn unit(v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.into_iter().map(|x| x / n).collect()
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A pseudo-random row in -1..1, the same for the same word.
fn row(word: &str, dim: usize) -> Vec<f32> {
    let mut x = fnv(word) | 1;
    (0..dim)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        })
        .collect()
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn build(vocab_words: &[String], dim: usize, row_of: fn(&str, usize) -> Vec<f32>) -> StaticModel {
    let mut vocab = String::from("[PAD]\n[UNK]\n");
    let mut data: Vec<u8> = vec![0u8; 2 * dim * 4];
    for w in vocab_words {
        vocab.push_str(w);
        vocab.push('\n');
        data.extend(row_of(w, dim).iter().flat_map(|x| x.to_le_bytes()));
    }
    let n = vocab_words.len() + 2;
    let header = format!(
        "{{\"embeddings\":{{\"dtype\":\"F32\",\"shape\":[{n},{dim}],\"data_offsets\":[0,{}]}}}}",
        data.len()
    );
    let mut st = (header.len() as u64).to_le_bytes().to_vec();
    st.extend_from_slice(header.as_bytes());
    st.extend_from_slice(&data);
    StaticModel::from_parts(&vocab, &st).unwrap()
}

/// The fake: every non-stop word of the labeled sets (and [`EXTRA`]) gets a
/// fixed 64-wide row ([`toy_row`]), so turns sharing words or topics sit close.
pub(crate) fn toy_model() -> Arc<StaticModel> {
    let mut vocab: Vec<String> = SET
        .iter()
        .chain(HELD_OUT)
        .chain(STAKES_SET)
        .chain(STAKES_HELD_OUT)
        .flat_map(|r| words(r.0))
        .chain(EXTRA.iter().map(|w| w.to_string()))
        .filter(|w| !STOP.contains(&w.as_str()))
        .collect();
    vocab.sort();
    vocab.dedup();
    Arc::new(build(&vocab, 64, toy_row))
}

// ---------------------------------------------------------------- the blend

/// A unit vector at cosine `sim` from `[1, 0, 0, 0]`.
fn at(sim: f32) -> Vec<f32> {
    vec![sim, (1.0 - sim * sim).sqrt(), 0.0, 0.0]
}

const Q: [f32; 4] = [1.0, 0.0, 0.0, 0.0];

fn entry(sim: f32, kind: AskKind, effort: usize, ok: bool) -> Entry {
    Entry {
        v: at(sim),
        kind,
        lane: kind.lane(),
        effort,
        ok,
        at: 0,
    }
}

fn many(n: usize, sim: f32, kind: AskKind, effort: usize) -> Vec<Entry> {
    (0..n).map(|_| entry(sim, kind, effort, true)).collect()
}

fn explain_at_medium(confidence: f64) -> AskRead {
    AskRead {
        kind: AskKind::Explain,
        stakes: Stakes::None,
        d: 0.4,
        effort: 3,
        confidence,
        rules: vec!["ask:explain".into(), "ask:effort:medium".into()],
    }
}

fn cx() -> AskContext {
    AskContext::default()
}

#[test]
fn a_thin_bank_leaves_the_rules_alone() {
    let four = many(4, 0.9, AskKind::Explain, 4);
    let r = blend(explain_at_medium(0.2), &Q, &four, &cx());
    assert_eq!(r.effort, 3);
    assert_eq!(
        r.rules,
        vec!["ask:explain", "ask:effort:medium", "knn:thin"]
    );
    // Ten neighbours, all under the similarity floor, are no neighbours.
    let far = many(10, 0.55, AskKind::Explain, 4);
    let r = blend(explain_at_medium(0.2), &Q, &far, &cx());
    assert_eq!(r.effort, 3);
    assert_eq!(r.rules.last().unwrap(), "knn:thin");
}

#[test]
fn strong_agreement_raises_or_lowers_the_rule_effort() {
    let high = many(6, 0.9, AskKind::Explain, 4);
    let r = blend(explain_at_medium(0.2), &Q, &high, &cx());
    assert_eq!(r.effort, 4);
    assert_eq!(r.rules, vec!["ask:explain", "ask:effort:high", "knn:raise"]);
    let low = many(6, 0.9, AskKind::Explain, 2);
    let r = blend(explain_at_medium(0.2), &Q, &low, &cx());
    assert_eq!(r.effort, 2);
    assert_eq!(r.rules, vec!["ask:explain", "ask:effort:low", "knn:lower"]);
    let same = many(6, 0.9, AskKind::Explain, 3);
    let r = blend(explain_at_medium(0.2), &Q, &same, &cx());
    assert_eq!(
        r.rules,
        vec!["ask:explain", "ask:effort:medium", "knn:agree"]
    );
    // A picture is never read at none, whatever the neighbours say.
    let none = many(6, 0.9, AskKind::Explain, 0);
    let image = AskContext {
        needs_image: true,
        ..AskContext::default()
    };
    let r = blend(explain_at_medium(0.2), &Q, &none, &image);
    assert_eq!(r.effort, 2);
    assert_eq!(r.rules.last().unwrap(), "knn:lower");
}

#[test]
fn on_disagreement_the_higher_effort_wins() {
    // Half low, half xhigh: the median is low, below the rules, so the rules stand.
    let mut split = many(3, 0.9, AskKind::Explain, 2);
    split.extend(many(3, 0.9, AskKind::Explain, 5));
    let r = blend(explain_at_medium(0.2), &Q, &split, &cx());
    assert_eq!(r.effort, 3);
    assert_eq!(r.rules.last().unwrap(), "knn:mixed");
    // Two thirds xhigh is under the agreement bar, but the median is above the rules.
    let mut lean = many(2, 0.9, AskKind::Explain, 2);
    lean.extend(many(4, 0.9, AskKind::Explain, 5));
    let r = blend(explain_at_medium(0.2), &Q, &lean, &cx());
    assert_eq!(r.effort, 5);
    assert_eq!(
        r.rules,
        vec!["ask:explain", "ask:effort:xhigh", "knn:raise"]
    );
}

#[test]
fn neighbours_move_the_lane_only_when_the_rules_were_unsure() {
    let debug = many(6, 0.9, AskKind::Debug, 3);
    let r = blend(explain_at_medium(0.2), &Q, &debug, &cx());
    assert_eq!(r.kind, AskKind::Debug);
    assert_eq!(r.lane(), "debug");
    assert_eq!(
        r.rules,
        vec!["ask:debug", "ask:effort:medium", "knn:agree", "knn:lane"]
    );
    let r = blend(explain_at_medium(0.5), &Q, &debug, &cx());
    assert_eq!(r.kind, AskKind::Explain);
    assert_eq!(r.lane(), "chat:default");
    assert_eq!(
        r.rules,
        vec!["ask:explain", "ask:effort:medium", "knn:agree"]
    );
}

#[test]
fn a_stakes_turn_is_never_lowered_or_moved_and_stakes_rows_skip_plain_turns() {
    let stakes = AskRead {
        kind: AskKind::CodeSmall,
        stakes: Stakes::Destructive,
        d: 0.3,
        effort: 4,
        confidence: 0.1,
        rules: vec![
            "ask:code_small".into(),
            "ask:stakes:destructive".into(),
            "ask:effort:high".into(),
        ],
    };
    let chatty = many(6, 0.95, AskKind::Chat, 2);
    let r = blend(stakes.clone(), &Q, &chatty, &cx());
    assert_eq!(r.effort, 4);
    assert_eq!(r.kind, AskKind::CodeSmall);
    assert_eq!(r.lane(), STAKES_LANE);
    assert_eq!(r.rules.last().unwrap(), "knn:hold");
    let hard = many(6, 0.95, AskKind::Chat, 5);
    let r = blend(stakes, &Q, &hard, &cx());
    assert_eq!(r.effort, 5);
    assert_eq!(r.lane(), STAKES_LANE);
    assert_eq!(r.rules.last().unwrap(), "knn:raise");
    // A stakes row's high effort came from words a plain turn doesn't have.
    let rows: Vec<Entry> = (0..6)
        .map(|_| Entry {
            lane: STAKES_LANE,
            ..entry(0.95, AskKind::CodeSmall, 4, true)
        })
        .collect();
    let r = blend(explain_at_medium(0.2), &Q, &rows, &cx());
    assert_eq!(r.effort, 3);
    assert_eq!(r.rules.last().unwrap(), "knn:thin");
}

#[test]
fn a_failed_turn_votes_one_rung_above_and_k_caps_the_vote() {
    assert_eq!(entry(0.9, AskKind::Debug, 3, false).vote(), 4);
    assert_eq!(entry(0.9, AskKind::Debug, 5, false).vote(), 5);
    assert_eq!(entry(0.9, AskKind::Debug, 6, true).vote(), 5);
    let failed: Vec<Entry> = (0..6)
        .map(|_| entry(0.9, AskKind::Explain, 3, false))
        .collect();
    let r = blend(explain_at_medium(0.2), &Q, &failed, &cx());
    assert_eq!(r.effort, 4);
    let lots = many(60, 0.8, AskKind::Explain, 3);
    assert_eq!(neighbours(&Q, &lots, false).len(), 50);
    assert_eq!(K, 50);
}

// ---------------------------------------------------------------- the bank on disk

fn tmp(tag: &str) -> std::path::PathBuf {
    crate::harness::test_dir(&format!("knn-{tag}"))
}

#[test]
fn the_bank_keeps_no_text_caps_its_size_and_writes_atomically() {
    let dir = tmp("cap");
    let model = toy_model();
    let v = model.encode("why is my docker container exiting").unwrap();
    for i in 0..(BANK_CAP + 3) as u64 {
        record(
            &dir,
            Entry {
                v: v.clone(),
                kind: AskKind::Debug,
                lane: "debug",
                effort: 3,
                ok: true,
                at: i,
            },
        );
    }
    let bank = load_bank(&dir, 64);
    assert_eq!(bank.len(), 2_000);
    assert_eq!(bank.front().unwrap().at, 3);
    assert_eq!(bank.back().unwrap().at, 2_002);
    let file = std::fs::read_to_string(bank_path(&dir)).unwrap();
    assert!(file.contains("\"lane\": \"debug\""), "{}", &file[..200]);
    assert!(!file.contains("docker"));
    assert!(!file.contains("container"));
    assert!(!bank_path(&dir).with_extension("json.tmp").exists());
    // Signed bytes keep the direction.
    let back = &bank.front().unwrap().v;
    assert!(grokhub_core::static_embed::cosine(&v, back) > 0.999);
    // A file for another model width is no bank.
    assert_eq!(load_bank(&dir, 256), VecDeque::new());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_seed_covers_both_labeled_sets_and_stays_in_each_range() {
    let model = toy_model();
    let s = seed(&model);
    assert_eq!(s.len(), 148);
    assert_eq!(
        s.iter().filter(|e| e.lane == STAKES_LANE).count(),
        STAKES_SET.len() + STAKES_HELD_OUT.len()
    );
    assert!(s.iter().all(|e| e.ok && e.at == 0));
    let greeting = &s[0];
    assert_eq!(greeting.kind, AskKind::Greeting);
    assert_eq!(greeting.effort, 0);
}

// ---------------------------------------------------------------- live turns

fn read_of(text: &str) -> AskRead {
    super::ask::read(text, &AskContext::default())
}

/// `text` as a turn of `key`, run at `rungs`, ending `ok`.
fn turn(dir: &std::path::Path, key: &str, text: &str, rungs: &[usize], ok: bool) -> AskRead {
    let t = super::ladder::turn_hash(text);
    let r = blend_turn(dir, key, text, t, &cx(), read_of(text), 1_000);
    for rung in rungs {
        note_rung(key, t, *rung);
        note_done(key, ok, false);
    }
    r
}

#[test]
fn without_the_model_the_read_is_the_rules_alone() {
    crate::memory_embed::set_test_model(None);
    let dir = tmp("off");
    let text = "explain how a CPU cache works like I'm five";
    let r = turn(&dir, "knn-off", text, &[3], true);
    assert_eq!(r, read_of(text));
    turn(&dir, "knn-off", "thanks!", &[0], true);
    assert!(!bank_path(&dir).exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_finished_turn_teaches_the_cheapest_rung_that_worked() {
    crate::memory_embed::set_test_model(Some(toy_model()));
    let dir = tmp("learn");
    let r = turn(
        &dir,
        "knn-learn",
        "how does ray tracing work?",
        &[3, 3],
        true,
    );
    assert!(
        r.rules.iter().any(|x| x.starts_with("knn:")),
        "{:?}",
        r.rules
    );
    // The same turn again (a tool step) reuses the first call's read.
    let t = super::ladder::turn_hash("how does ray tracing work?");
    let again = blend_turn(
        &dir,
        "knn-learn",
        "how does ray tracing work?",
        t,
        &cx(),
        read_of("how does ray tracing work?"),
        1_000,
    );
    assert_eq!(again, r);
    // Climbing 2 -> 4 and then working teaches the rung that worked.
    turn(
        &dir,
        "knn-learn",
        "why is my docker container exiting with code 137",
        &[2, 4],
        true,
    );
    // A redo says the rung before wasn't enough; this one then errors out.
    turn(&dir, "knn-learn", "that's wrong, try again", &[4], false);
    // So the next turn finds nothing to learn from it.
    turn(&dir, "knn-learn", "solve 3x + 7 = 22", &[2], true);
    let bank = load_bank(&dir, 64);
    let got: Vec<(AskKind, usize, bool, u64)> = bank
        .iter()
        .map(|e| (e.kind, e.effort, e.ok, e.at))
        .collect();
    assert_eq!(
        got,
        vec![
            (AskKind::Explain, 3, true, 1_000),
            (AskKind::Debug, 4, false, 1_000),
        ]
    );
    crate::memory_embed::set_test_model(None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn adult_and_banned_turns_never_touch_the_bank() {
    crate::memory_embed::set_test_model(Some(toy_model()));
    let dir = tmp("adult");
    let text = "write me an nsfw sex scene";
    let r = turn(&dir, "knn-adult", text, &[2], true);
    assert_eq!(r, read_of(text));
    // Nothing waited on it, so the next turn records nothing.
    turn(&dir, "knn-adult", "thanks!", &[0], true);
    assert!(!bank_path(&dir).exists());
    crate::memory_embed::set_test_model(None);
    let _ = std::fs::remove_dir_all(dir);
}

fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    let m = ModelMeta {
        id: "grok-4.3".into(),
        efforts: Some(vec![
            "low".into(),
            "medium".into(),
            "high".into(),
            "xhigh".into(),
        ]),
        tool_calling: Some(true),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    };
    reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, vec![m])], &[], 1);
    reg
}

fn chat<'a>(episode: &'a str, text: &'a str) -> RouteCall<'a> {
    RouteCall {
        provider: "xai",
        class: "chat:default",
        model: "grok-4.3",
        episode,
        session: episode,
        text,
        needs_tools: true,
        ..RouteCall::default()
    }
}

#[test]
fn a_routed_chat_turn_logs_its_knn_rule_and_the_next_turn_grows_the_bank() {
    let dir = tmp("decide");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let ep = "knn-ep-decide";
    assert!(
        !in_holdout(ep),
        "{ep} sits in the holdout; pick another name"
    );
    crate::memory_embed::set_test_model(Some(toy_model()));
    let first = chat(ep, "how does ray tracing work?");
    let d = live::decide(&dir, &first, 1_000);
    let knn: Vec<&String> = d
        .route
        .rule_ids
        .iter()
        .filter(|r| r.starts_with("knn:"))
        .collect();
    assert_eq!(knn.len(), 1, "{:?}", d.route.rule_ids);
    live::route_log(
        &dir,
        &first,
        &d,
        &RouteDone {
            ok: true,
            http: 200,
            ..RouteDone::default()
        },
    );
    let _ = live::decide(&dir, &chat(ep, "thanks!"), 2_000);
    let bank = load_bank(&dir, 64);
    assert_eq!(bank.len(), 1);
    assert_eq!(bank[0].kind, AskKind::Explain);
    assert_eq!(bank[0].lane, "chat:default");
    assert!(bank[0].ok);
    assert_eq!(bank[0].at, 2_000);
    // Without the model nothing is read or written and no knn rule is logged.
    crate::memory_embed::set_test_model(None);
    let d = live::decide(
        &dir,
        &chat("knn-ep-off", "how does ray tracing work?"),
        3_000,
    );
    assert!(!d.route.rule_ids.iter().any(|r| r.starts_with("knn:")));
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------- timing

/// B5: embed and blend time on a model the size of potion-base-8M (30,522
/// tokens × 256) and a full bank. The real files aren't here, so the table is
/// synthetic; the cost is the same lookups and dot products.
#[test]
fn embed_and_blend_time_on_a_full_size_model() {
    let mut vocab: Vec<String> = SET
        .iter()
        .chain(HELD_OUT)
        .flat_map(|r| words(r.0))
        .collect();
    vocab.sort();
    vocab.dedup();
    let real = vocab.len();
    vocab.extend((real..30_520).map(|i| format!("zz{i}")));
    let model = build(&vocab, 256, row);
    assert_eq!(model.dim(), 256);
    let texts: Vec<&str> = SET.iter().chain(HELD_OUT).map(|r| r.0).collect();
    let started = Instant::now();
    let mut vs = Vec::new();
    for _ in 0..10 {
        vs = texts.iter().filter_map(|t| model.encode(t)).collect();
    }
    let embed_us = started.elapsed().as_secs_f64() * 1e6 / (10 * texts.len()) as f64;
    assert_eq!(vs.len(), texts.len());
    let bank: Vec<Entry> = (0..BANK_CAP)
        .map(|i| Entry {
            v: vs[i % vs.len()].clone(),
            kind: AskKind::Chat,
            lane: "chat:default",
            effort: 3,
            ok: true,
            at: i as u64,
        })
        .collect();
    let started = Instant::now();
    for (t, v) in texts.iter().zip(&vs) {
        let _ = blend(read_of(t), v, &bank, &cx());
    }
    let blend_us = started.elapsed().as_secs_f64() * 1e6 / texts.len() as f64;
    println!(
        "knn timing: embed {embed_us:.1} µs a turn, blend over {} entries {blend_us:.1} µs a turn",
        bank.len()
    );
    // Generous for an unoptimized test build on a busy CI box.
    assert!(embed_us < 2_000.0, "embed {embed_us:.1} µs");
    assert!(blend_us < 50_000.0, "blend {blend_us:.1} µs");
}
