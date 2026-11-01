//! Labelled recall set for the pre-model memory router (LO-44 phase 1).
//! One shared notes file, twenty asks, each labelled with what the pack must
//! carry. Keyword, stemmed and recency rows must hit. Paraphrase rows share no
//! word with their note, so word search misses them today: they are the
//! baseline a later embeddings phase should flip. Local files only.

#![cfg(test)]

use std::fs;
use std::path::PathBuf;
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

/// Each row's ask with whether its pack hit, on the index (legacy) backend.
fn run() -> Vec<(&'static str, Kind, bool)> {
    let (cfg, ws, _guard) = pair();
    fs::write(cfg.join("app.json"), "{}\n").unwrap();
    let mut text = String::new();
    for i in 0..20 {
        text.push_str(&format!("the shelf note number {i} sits by the door\n\n"));
    }
    for note in NOTES {
        text.push_str(note);
        text.push_str("\n\n");
    }
    fs::write(crate::memory::workspace_memory_path(&ws), text).unwrap();
    for note in OLDER {
        let moved = crate::memory::backdate_entries(&ws, note, 90 * 24 * 60 * 60).unwrap();
        assert_eq!(moved, 1, "{note}");
    }
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
