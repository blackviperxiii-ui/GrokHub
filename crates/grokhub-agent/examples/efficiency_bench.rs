//! LO-30 efficiency bench: prints what each fixed scenario would send to a
//! model (requests, bytes, est. tokens, stable prefix, recall, time, RSS).
//! Offline and scripted: no keys, no network. `--out FILE` also writes the table.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rows = grokhub_agent::run_bench();
    let table = grokhub_agent::render_bench(&rows);
    print!("{table}");
    if let Some(i) = args.iter().position(|a| a == "--out") {
        let Some(path) = args.get(i + 1) else {
            eprintln!("--out needs a file path");
            std::process::exit(1);
        };
        if let Err(err) = std::fs::write(path, &table) {
            eprintln!("could not write {path}: {err}");
            std::process::exit(1);
        }
    }
}
