//! Print one stored model request exactly as it was sent, so a turn can be
//! read or sent again by hand. Nothing is sent from here.
//!
//! ```text
//! cargo run -p grokhub-agent --example replay_request -- <request hash> > request.json
//! ```
//!
//! The hash is the `request` field of a model-call span's route record, or of
//! a safety event. Requests are kept sealed for 14 days after last use, so
//! this needs the same computer and keyring that stored them.

use std::io::Write;

fn main() {
    let Some(hash) = std::env::args().nth(1) else {
        eprintln!("usage: replay_request <request hash>");
        std::process::exit(2);
    };
    grokhub_agent::harness::use_os_keyring();
    let dir = grokhub_agent::perm::config_dir();
    match grokhub_agent::harness::replay::load_request(&dir, hash.trim()) {
        Ok(bytes) => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&bytes);
            let _ = out.write_all(b"\n");
        }
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    }
}
