//! Memory index bench: how long recall at Enter takes right after one memory
//! file changed, next to steady-state recall, on a fixture of plain notes.
//! Offline: it writes its own config dir and workspace under `root`.
//! `examples/memory_index_bench.rs` prints the result.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::perm::ConfigGuard;

/// Timings in microseconds, one per round.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryIndexBench {
    pub notes: usize,
    /// First recall on a fresh index (every file indexed).
    pub cold_us: u32,
    /// First recall after one note was appended to the workspace `MEMORY.md`.
    pub after_edit_us: Vec<u32>,
    /// A second recall with no change in between.
    pub steady_us: Vec<u32>,
    /// Whether every appended note was in its own first recall pack.
    pub found_every_edit: bool,
}

/// One note line. Each holds a shared word ("shelf") so every recall scores
/// many rows, and its own number so a note can be asked for by name.
fn note(i: usize) -> String {
    format!("the shelf note number n{i} keeps the build cache warm for crate c{i}")
}

/// Index `notes` notes split across the global and workspace `MEMORY.md`,
/// then run `rounds` rounds of: append one note to the workspace file, time
/// the first recall, time a second recall.
pub fn run_memory_index_bench(root: &Path, notes: usize, rounds: usize) -> MemoryIndexBench {
    let _ = fs::remove_dir_all(root);
    let cfg = root.join("cfg");
    let ws = root.join("ws");
    fs::create_dir_all(cfg.join("memory")).expect("bench config dir");
    fs::create_dir_all(&ws).expect("bench workspace");
    let _guard = ConfigGuard::set(&cfg);
    fs::write(cfg.join("app.json"), "{}\n").expect("bench app.json");
    let global_n = notes / 3;
    let body = |range: std::ops::Range<usize>| range.map(|i| note(i) + "\n\n").collect::<String>();
    fs::write(crate::memory::global_memory_path(), body(0..global_n)).expect("global notes");
    let ws_file = crate::memory::workspace_memory_path(&ws);
    fs::write(&ws_file, body(global_n..notes)).expect("workspace notes");

    let ask = |text: &str| {
        let start = Instant::now();
        let pack = crate::memory::first_turn_injection(&ws, text).unwrap_or_default();
        let us = u32::try_from(start.elapsed().as_micros()).unwrap_or(u32::MAX);
        (us, pack)
    };
    let (cold_us, _) = ask("which shelf note warms the cache");
    let mut after_edit_us = Vec::with_capacity(rounds);
    let mut steady_us = Vec::with_capacity(rounds);
    let mut found_every_edit = true;
    for round in 0..rounds {
        let fresh = notes + round;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&ws_file)
            .expect("open workspace notes");
        file.write_all((note(fresh) + "\n\n").as_bytes())
            .expect("append note");
        drop(file);
        let want = format!("n{fresh} ");
        let (us, pack) = ask(&format!("what does note n{fresh} keep warm"));
        after_edit_us.push(us);
        found_every_edit &= pack.contains(&want);
        let (us, _) = ask(&format!("what does note n{fresh} keep warm"));
        steady_us.push(us);
    }
    MemoryIndexBench {
        notes,
        cold_us,
        after_edit_us,
        steady_us,
        found_every_edit,
    }
}

/// `p50` and `max` of `samples` in milliseconds, as `p50 / max`.
fn p50_max_ms(samples: &[u32]) -> String {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let p50 = sorted.get(sorted.len() / 2).copied().unwrap_or(0);
    let max = sorted.last().copied().unwrap_or(0);
    format!(
        "{:.2} / {:.2}",
        f64::from(p50) / 1000.0,
        f64::from(max) / 1000.0
    )
}

pub fn render_memory_index_bench(bench: &MemoryIndexBench) -> String {
    format!(
        "| notes | rounds | cold ms | after one-file edit ms (p50 / max) | steady ms (p50 / max) | edit found |\n\
         |---|---|---|---|---|---|\n\
         | {} | {} | {:.2} | {} | {} | {} |\n",
        bench.notes,
        bench.after_edit_us.len(),
        f64::from(bench.cold_us) / 1000.0,
        p50_max_ms(&bench.after_edit_us),
        p50_max_ms(&bench.steady_us),
        if bench.found_every_edit { "yes" } else { "NO" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_finds_every_appended_note_and_renders_one_row() {
        let root =
            std::env::temp_dir().join(format!("gh-memory-index-bench-{}", std::process::id()));
        let bench = run_memory_index_bench(&root, 30, 3);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(bench.notes, 30);
        assert_eq!(bench.after_edit_us.len(), 3);
        assert_eq!(bench.steady_us.len(), 3);
        assert!(bench.found_every_edit);
        let table = render_memory_index_bench(&bench);
        assert_eq!(table.lines().count(), 3);
        assert!(table.lines().nth(2).unwrap().starts_with("| 30 | 3 | "));
        assert!(table.trim_end().ends_with("| yes |"));
    }

    #[test]
    fn p50_max_reads_the_middle_and_the_top() {
        assert_eq!(p50_max_ms(&[3_000, 1_000, 2_000]), "2.00 / 3.00");
        assert_eq!(p50_max_ms(&[]), "0.00 / 0.00");
    }
}
