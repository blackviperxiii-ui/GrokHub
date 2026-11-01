//! Memory index bench: recall time at Enter right after one memory file
//! changed, beside steady-state recall. Offline. Run it in release:
//! `cargo run --release -p grokhub-agent --example memory_index_bench -- [--notes N] [--rounds N] [--dir DIR]`.
//! The fixture goes under `--dir` (default: the system temp dir), so pass a
//! folder on the disk you want to measure.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let number = |flag: &str, default: usize| match value(flag) {
        None => default,
        Some(raw) => raw.parse().unwrap_or_else(|_| {
            eprintln!("{flag} needs a whole number");
            std::process::exit(1);
        }),
    };
    let notes = number("--notes", 314);
    let rounds = number("--rounds", 21);
    let base = value("--dir").map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let root = base.join(format!("grokhub-memory-index-bench-{}", std::process::id()));
    let bench = grokhub_agent::run_memory_index_bench(&root, notes, rounds);
    let _ = std::fs::remove_dir_all(&root);
    print!("{}", grokhub_agent::render_memory_index_bench(&bench));
}
