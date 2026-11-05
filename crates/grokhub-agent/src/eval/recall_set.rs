//! Labelled recall set for the pre-model memory router (LO-44 phase 1).
//! One shared notes file, twenty asks, each labelled with what the pack must
//! carry. Keyword, stemmed and recency rows must hit. Paraphrase rows share no
//! word with their note, so word search misses them today: they are the
//! baseline for word search alone; with the local embedding model (HX-12)
//! they hit too. Local files only.

#![cfg(test)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::perm::ConfigGuard;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Keyword,
    Stemmed,
    Recency,
    Paraphrase,
}

struct Row {
    ask: &'static str,
    kind: Kind,
    /// Text the pack must carry for a hit.
    want: &'static str,
    /// An older, replaced note: absent from the pack, or after `want`.
    stale: Option<&'static str>,
}

const NOTES: &[&str] = &[
    "the save folder is D:/Loadout/saves",
    "preferred controller layout is southpaw with inverted look",
    "the build server runs nightly at two in the morning",
    "streaming bitrate is capped at 6000 kbps for the overlay",
    "the mod list loads skyline textures before the lighting pack",
    "voice chat push to talk key is the mouse side button",
    "the benchmark preset uses ultra shadows and medium foliage",
    "backups are zipped weekly into the archive drive",
    "the shader cache gets cleared after every driver update",
    "graphics card is an RTX 4080",
    "the PC has 64 GB of RAM",
    "raid night is on thursdays with the guild",
    "the squad meets at nine",
    "the squad meets at ten",
    "default resolution is 1440p",
    "default resolution is 4k",
];

/// Notes planted 90 days before the rest (three half-lives).
const OLDER: &[&str] = &["the squad meets at nine", "default resolution is 1440p"];

const ROWS: &[Row] = &[
    row(
        "where is the save folder",
        Kind::Keyword,
        "D:/Loadout/saves",
    ),
    row("where are my saves", Kind::Keyword, "D:/Loadout/saves"),
    row("what controller layout do I use", Kind::Keyword, "southpaw"),
    row(
        "when does the build server run",
        Kind::Keyword,
        "nightly at two",
    ),
    row(
        "what is the streaming bitrate cap",
        Kind::Keyword,
        "6000 kbps",
    ),
    row(
        "which push to talk key for voice chat",
        Kind::Keyword,
        "mouse side button",
    ),
    row(
        "what does the benchmark preset use",
        Kind::Keyword,
        "ultra shadows",
    ),
    row(
        "what does the mod list load first",
        Kind::Keyword,
        "skyline textures",
    ),
    row(
        "the loading order of my mods",
        Kind::Stemmed,
        "skyline textures",
    ),
    row(
        "is the shader cache clearing automatic",
        Kind::Stemmed,
        "shader cache gets cleared",
    ),
    row("how are backups zipping up", Kind::Stemmed, "zipped weekly"),
    row("which day are raids", Kind::Stemmed, "raid night"),
    row(
        "after updating drivers what happens",
        Kind::Stemmed,
        "shader cache gets cleared",
    ),
    Row {
        ask: "what time does the squad meet",
        kind: Kind::Recency,
        want: "the squad meets at ten",
        stale: Some("the squad meets at nine"),
    },
    Row {
        ask: "what is the default resolution",
        kind: Kind::Recency,
        want: "default resolution is 4k",
        stale: Some("default resolution is 1440p"),
    },
    row("which GPU do I have", Kind::Paraphrase, "RTX 4080"),
    row(
        "how much memory does my rig have",
        Kind::Paraphrase,
        "64 GB of RAM",
    ),
    row(
        "what evening is the group dungeon",
        Kind::Paraphrase,
        "raid night",
    ),
    row(
        "where are snapshots stored",
        Kind::Paraphrase,
        "zipped weekly",
    ),
    row(
        "what pad scheme do I play with",
        Kind::Paraphrase,
        "southpaw",
    ),
];

/// Asks the notes have nothing to say about: their pack must carry no note
/// (recall precision), so an off-topic turn pays for no memory tokens.
const NEGATIVE: &[&str] = &[
    "how do I boil an egg",
    "translate thank you into spanish",
    "who wrote pride and prejudice",
    "what is the capital of portugal",
    "give me a haiku about autumn leaves",
];

/// What an off-topic ask's pack carried from the notes, if anything.
fn leaked(pack: &str) -> Vec<&'static str> {
    NOTES
        .iter()
        .copied()
        .chain(std::iter::once("the shelf note number"))
        .filter(|note| pack.contains(note))
        .collect()
}

/// Each off-topic ask with its pack, on the index (legacy) backend.
fn run_negative() -> Vec<(&'static str, String)> {
    let (cfg, ws, _guard) = pair();
    seed(&cfg, &ws);
    NEGATIVE
        .iter()
        .map(|ask| {
            let pack = crate::memory::first_turn_injection(&ws, ask).unwrap_or_default();
            (*ask, pack)
        })
        .collect()
}

const fn row(ask: &'static str, kind: Kind, want: &'static str) -> Row {
    Row {
        ask,
        kind,
        want,
        stale: None,
    }
}

fn pair() -> (PathBuf, PathBuf, ConfigGuard) {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("gh-recall-set-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cfg = root.join("cfg");
    let ws = root.join("ws");
    fs::create_dir_all(&cfg).unwrap();
    fs::create_dir_all(&ws).unwrap();
    let guard = ConfigGuard::set(&cfg);
    (cfg, ws, guard)
}

/// Whether `pack` carries `row.want`, with any stale note left out or after it.
fn hit(row: &Row, pack: &str) -> bool {
    let Some(at) = pack.find(row.want) else {
        return false;
    };
    match row.stale.and_then(|stale| pack.find(stale)) {
        Some(old) => old > at,
        None => true,
    }
}

/// The shared notes file, with the [`OLDER`] notes moved 90 days back.
fn seed(cfg: &Path, ws: &Path) {
    fs::write(cfg.join("app.json"), "{}\n").unwrap();
    let mut text = String::new();
    for i in 0..20 {
        text.push_str(&format!("the shelf note number {i} sits by the door\n\n"));
    }
    for note in NOTES {
        text.push_str(note);
        text.push_str("\n\n");
    }
    fs::write(crate::memory::workspace_memory_path(ws), text).unwrap();
    for note in OLDER {
        let moved = crate::memory::backdate_entries(ws, note, 90 * 24 * 60 * 60).unwrap();
        assert_eq!(moved, 1, "{note}");
    }
}

/// Each row's ask with whether its pack hit, on the index (legacy) backend.
fn run() -> Vec<(&'static str, Kind, bool)> {
    let (cfg, ws, _guard) = pair();
    seed(&cfg, &ws);
    ROWS.iter()
        .map(|row| {
            let pack = crate::memory::first_turn_injection(&ws, row.ask).unwrap_or_default();
            (row.ask, row.kind, hit(row, &pack))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keyword_stemmed_and_recency_row_hits() {
        let results = run();
        assert_eq!(results.len(), 20);
        let missed: Vec<&str> = results
            .iter()
            .filter(|(_, kind, hit)| *kind != Kind::Paraphrase && !hit)
            .map(|(ask, _, _)| *ask)
            .collect();
        assert_eq!(missed, Vec::<&str>::new());
        let counts = |kind: Kind| results.iter().filter(|(_, k, _)| *k == kind).count();
        assert_eq!(counts(Kind::Keyword), 8);
        assert_eq!(counts(Kind::Stemmed), 5);
        assert_eq!(counts(Kind::Recency), 2);
        assert_eq!(counts(Kind::Paraphrase), 5);
    }

    /// Every ask's hits and pack, scored at `now`.
    fn recall_all(ws: &Path, now: i64) -> Vec<(Vec<crate::memory::Hit>, String)> {
        ROWS.iter()
            .map(|row| {
                let hits = crate::memory::search_at(ws, row.ask, now).unwrap();
                let pack = crate::memory::first_turn_injection(ws, row.ask).unwrap_or_default();
                (hits, pack)
            })
            .collect()
    }

    /// Edits land one file change at a time (each picked up incrementally by
    /// the next recall); then a from-scratch rebuild must recall the same hits,
    /// in the same order, with the same scores, and pack the same text.
    #[test]
    fn the_incremental_index_recalls_exactly_what_a_full_rebuild_does() {
        let (cfg, ws, _guard) = pair();
        seed(&cfg, &ws);
        let notes = crate::memory::workspace_memory_path(&ws);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let step = |ask: &str| crate::memory::search_at(&ws, ask, now).unwrap();
        // A new note, appended the way an editor would.
        let mut text = fs::read_to_string(&notes).unwrap();
        text.push_str("the shelf by the window holds the spare controller\n\n");
        fs::write(&notes, &text).unwrap();
        assert!(step("spare controller")[0]
            .body
            .contains("spare controller"));
        // A deleted note and an edited one.
        let text = text
            .replace(
                "the build server runs nightly at two in the morning\n\n",
                "",
            )
            .replace(
                "the shelf note number 7 sits",
                "the shelf note number 7 now sits",
            );
        fs::write(&notes, &text).unwrap();
        assert!(step("build server nightly")
            .iter()
            .all(|hit| !hit.body.contains("build server")));
        // A global note in its own file.
        let global = crate::memory::global_memory_path();
        fs::create_dir_all(global.parent().unwrap()).unwrap();
        fs::write(&global, "the shelf in the hall keeps the backup drive\n").unwrap();
        assert_eq!(step("backup drive hall")[0].scope, "global");

        let incremental = recall_all(&ws, now);
        assert!(incremental.iter().any(|(hits, _)| !hits.is_empty()));
        crate::memory::rebuild_keeping_dates(&ws).unwrap();
        let rebuilt = recall_all(&ws, now);
        for ((row, inc), full) in ROWS.iter().zip(&incremental).zip(&rebuilt) {
            assert_eq!(inc, full, "{}", row.ask);
        }
        assert_eq!(incremental.len(), 20);
    }

    /// A toy static model: each concept is one axis, its words point along
    /// it, and nothing else is in the vocab (so filler words count for nothing,
    /// as stop words count for little in the real model).
    fn toy_model() -> std::sync::Arc<grokhub_core::static_embed::StaticModel> {
        const CONCEPTS: &[&[&str]] = &[
            &["gpu", "graphics", "card"],
            &["ram", "memory", "gb", "rig"],
            &["raid", "dungeon", "group", "guild", "evening", "night"],
            &["backups", "snapshots", "archive", "zipped", "stored"],
            &["controller", "pad", "layout", "scheme", "southpaw"],
        ];
        let dim = CONCEPTS.len();
        let mut vocab = String::from("[PAD]\n[UNK]\n");
        let mut rows = vec![vec![0f32; dim]; 2];
        for (axis, words) in CONCEPTS.iter().enumerate() {
            for word in *words {
                vocab.push_str(word);
                vocab.push('\n');
                let mut v = vec![0f32; dim];
                v[axis] = 1.0;
                rows.push(v);
            }
        }
        let data: Vec<u8> = rows
            .iter()
            .flatten()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        let header = format!(
            "{{\"embeddings\":{{\"dtype\":\"F32\",\"shape\":[{},{dim}],\"data_offsets\":[0,{}]}}}}",
            rows.len(),
            data.len()
        );
        let mut st = (header.len() as u64).to_le_bytes().to_vec();
        st.extend_from_slice(header.as_bytes());
        st.extend_from_slice(&data);
        std::sync::Arc::new(
            grokhub_core::static_embed::StaticModel::from_parts(&vocab, &st).unwrap(),
        )
    }

    /// With an embedding model (HX-12), recall finds the five paraphrases by
    /// meaning and every word, stem and recency row still hits.
    #[test]
    fn with_embeddings_every_row_hits_paraphrases_included() {
        crate::memory_embed::set_test_model(Some(toy_model()));
        let results = run();
        crate::memory_embed::set_test_model(None);
        let missed: Vec<&str> = results
            .iter()
            .filter(|(_, _, hit)| !hit)
            .map(|(ask, _, _)| *ask)
            .collect();
        assert_eq!(missed, Vec::<&str>::new());
    }

    /// A note found by meaning alone never crowds a word match out of the
    /// pack, and the pack stays inside its caps.
    #[test]
    fn meaning_matches_add_to_word_matches_and_respect_the_caps() {
        let (cfg, ws, _guard) = pair();
        seed(&cfg, &ws);
        crate::memory_embed::set_test_model(Some(toy_model()));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let hits = crate::memory::search_at(&ws, "what controller layout do I use", now).unwrap();
        crate::memory_embed::set_test_model(None);
        assert!(hits[0].body.contains("southpaw"));
        assert!(hits[0].bm25 < 0.0, "the top hit is still the word match");
        let meaning_only = hits.iter().filter(|h| h.bm25 == 0.0).count();
        assert!(meaning_only <= 2, "{hits:?}");
        let plain = crate::memory::search_at(&ws, "what controller layout do I use", now).unwrap();
        assert!(
            plain.iter().all(|h| h.bm25 < 0.0),
            "no model, no meaning-only hits"
        );
    }

    /// AMR facts are found by meaning too, and a superseded fact stays out.
    #[test]
    fn amr_recall_finds_facts_by_meaning_but_never_a_superseded_one() {
        let (cfg, ws, _guard) = pair();
        fs::write(cfg.join("app.json"), "{\"memory_backend\":\"amr\"}\n").unwrap();
        let store = crate::memory::amr_store();
        store.init().unwrap();
        let fact = |text: &'static str| grokhub_core::amr::LineWrite {
            text,
            source: "user",
            tags: Vec::new(),
            confidence: 0.9,
            revive: false,
        };
        let old = grokhub_core::amr::remember_line(&store, &fact("graphics card is a GTX 1080"), 1)
            .unwrap();
        let new =
            grokhub_core::amr::remember_line(&store, &fact("graphics card is an RTX 4080"), 2)
                .unwrap();
        store
            .link(new.id(), old.id(), grokhub_core::amr::EdgeRel::Supersedes)
            .unwrap();
        let ask = "which GPU do I have";
        let plain = crate::memory::first_turn_injection(&ws, ask).unwrap_or_default();
        assert!(
            !plain.contains("RTX 4080"),
            "words alone can't find it: {plain}"
        );
        crate::memory_embed::set_test_model(Some(toy_model()));
        let pack = crate::memory::first_turn_injection(&ws, ask).unwrap_or_default();
        crate::memory_embed::set_test_model(None);
        assert!(pack.contains("RTX 4080"), "{pack}");
        assert!(!pack.contains("GTX 1080"), "{pack}");
    }

    /// Live check with the real model: put `vocab.txt` and `model.safetensors`
    /// of potion-base-8M in a folder, then
    /// `GROKHUB_EMBED_DIR=<folder> cargo test -p grokhub-agent --lib real_embedding_model -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the 30 MB model on disk (GROKHUB_EMBED_DIR)"]
    fn real_embedding_model_recall_set() {
        let dir = std::env::var_os("GROKHUB_EMBED_DIR").expect("GROKHUB_EMBED_DIR");
        let model = grokhub_core::static_embed::StaticModel::load(
            &crate::memory_embed::MODEL,
            Path::new(&dir),
        )
        .unwrap();
        crate::memory_embed::set_test_model(Some(std::sync::Arc::new(model)));
        let results = run();
        let negative = run_negative();
        crate::memory_embed::set_test_model(None);
        for (ask, kind, hit) in &results {
            println!("{} {kind:?} {ask}", if *hit { "hit " } else { "MISS" });
        }
        let missed: Vec<&str> = results
            .iter()
            .filter(|(_, kind, hit)| *kind != Kind::Paraphrase && !hit)
            .map(|(ask, _, _)| *ask)
            .collect();
        assert_eq!(missed, Vec::<&str>::new(), "word rows must never regress");
        let paraphrase = results
            .iter()
            .filter(|(_, kind, hit)| *kind == Kind::Paraphrase && *hit)
            .count();
        println!("paraphrase rows found by meaning: {paraphrase} of 5");
        for (ask, pack) in &negative {
            println!("off-topic {ask}: {} bytes injected", pack.len());
        }
        assert!(paraphrase >= 3, "{paraphrase} of 5 paraphrases");
    }

    /// An ask the notes don't cover recalls none of them, with word search
    /// alone and with the meaning model on.
    #[test]
    fn off_topic_asks_recall_no_note() {
        for (ask, pack) in run_negative() {
            assert_eq!(leaked(&pack), Vec::<&str>::new(), "{ask}: {pack}");
        }
        crate::memory_embed::set_test_model(Some(toy_model()));
        let with_model = run_negative();
        crate::memory_embed::set_test_model(None);
        for (ask, pack) in with_model {
            assert_eq!(leaked(&pack), Vec::<&str>::new(), "{ask}: {pack}");
        }
    }

    /// Paraphrases share no word with their note. Word search misses all five
    /// today; when a later phase recalls one, move its row out of this baseline.
    #[test]
    fn paraphrase_rows_are_the_known_miss_baseline() {
        let hits: Vec<&str> = run()
            .into_iter()
            .filter(|(_, kind, hit)| *kind == Kind::Paraphrase && *hit)
            .map(|(ask, _, _)| ask)
            .collect();
        assert_eq!(hits, Vec::<&str>::new());
    }
}
