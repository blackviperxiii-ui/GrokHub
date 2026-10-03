//! Phase 16 parity harness. Dry-run is the default. `--live` is refused here.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match grokhub_agent::parse_args(&args) {
        Ok(opts) => opts,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    if let Err(err) = grokhub_agent::reject_live(&opts) {
        eprintln!("{err}");
        std::process::exit(1);
    }
    let items = grokhub_agent::run_suite(&opts);
    for item in &items {
        eprintln!(
            "eval item={} engine={} wall_ms={} success={} turns={} status={}",
            item.item, item.engine, item.wall_ms, item.success, item.turns, item.status
        );
    }
    let report = grokhub_agent::render_report(&items);
    if let Some(path) = &opts.out {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    eprintln!("could not create report directory: {err}");
                    std::process::exit(1);
                }
            }
        }
        if let Err(err) = std::fs::write(path, report.as_bytes()) {
            eprintln!("could not write report: {err}");
            std::process::exit(1);
        }
    }
    print!("{report}");
}
