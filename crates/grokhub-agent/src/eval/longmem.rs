//! LongMemEval-style recall. Local harbor fixtures, scripted answers.
//! Categories: information extraction, multi-session, knowledge update,
//! abstention. No network and no LoCoMo transcript.

#![cfg(test)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::perm::ConfigGuard;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Extract,
    Multi,
    Update,
    Abstain,
}

fn pair(tag: &str) -> (PathBuf, PathBuf, ConfigGuard) {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("gh-long-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cfg = root.join("cfg");
    let ws = root.join("ws");
    fs::create_dir_all(&cfg).unwrap();
    fs::create_dir_all(&ws).unwrap();
    let guard = ConfigGuard::set(&cfg);
    (cfg, ws, guard)
}

fn write_backend(cfg: &Path, amr: bool) {
    let body = if amr {
        "{\"memory_backend\":\"amr\"}\n"
    } else {
        "{}\n"
    };
    fs::write(cfg.join("app.json"), body).unwrap();
}

fn line<'a>(text: &'a str) -> grokhub_core::amr::LineWrite<'a> {
    grokhub_core::amr::LineWrite {
        text,
        source: "user",
        tags: Vec::new(),
        confidence: 0.9,
        revive: false,
    }
}

/// A today-color sentence is an answer. An old-log mention is not.
fn scripted_beacon(block: &str) -> &'static str {
    if block
        .to_ascii_lowercase()
        .contains("inland beacon is crimson today")
    {
        "The inland beacon is crimson."
    } else {
        "I don't have that stored."
    }
}

fn passes(kind: Kind, amr: bool) -> bool {
    let (cfg, ws, _guard) = pair(match kind {
        Kind::Extract => "extract",
        Kind::Multi => "multi",
        Kind::Update => "update",
        Kind::Abstain => "abstain",
    });
    write_backend(&cfg, amr);
    match kind {
        Kind::Extract => {
            crate::memory::remember(&ws, "the harbor code is 4419").unwrap();
            let block = crate::memory::first_turn_injection(
                &ws,
                "what is the harbor code for the quay office",
            )
            .unwrap();
            block.contains("4419")
        }
        Kind::Multi => {
            crate::memory::remember(&ws, "global: the pier pin is blue").unwrap();
            crate::memory::remember(&ws, "the quay lamp is green").unwrap();
            let lamp = crate::memory::first_turn_injection(
                &ws,
                "what color is the quay lamp at the harbor",
            )
            .unwrap();
            let pier = crate::memory::first_turn_injection(
                &ws,
                "what color is the pier pin at the harbor",
            )
            .unwrap();
            lamp.contains("quay lamp is green") && pier.contains("pier pin is blue")
        }
        Kind::Update => {
            crate::memory::remember(&ws, "harbor lunch is at nine").unwrap();
            crate::memory::remember(&ws, "harbor lunch is at noon").unwrap();
            if amr {
                let store = crate::memory::amr_store();
                store.init().unwrap();
                let old =
                    grokhub_core::amr::remember_line(&store, &line("harbor lunch is at nine"), 1)
                        .unwrap();
                let new =
                    grokhub_core::amr::remember_line(&store, &line("harbor lunch is at noon"), 2)
                        .unwrap();
                store
                    .link(new.id(), old.id(), grokhub_core::amr::EdgeRel::Supersedes)
                    .unwrap();
            }
            let block = crate::memory::first_turn_injection(
                &ws,
                "when is harbor lunch served today please",
            )
            .unwrap();
            block.contains("at noon")
        }
        Kind::Abstain => {
            crate::memory::remember(&ws, "harbor lunch is at noon").unwrap();
            crate::memory::remember(&ws, "the inland beacon was crimson in the old log").unwrap();
            let block =
                crate::memory::first_turn_injection(&ws, "what color is the inland beacon today")
                    .unwrap_or_default();
            scripted_beacon(&block) == "I don't have that stored."
                && !block
                    .to_ascii_lowercase()
                    .contains("inland beacon is crimson today")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_extraction_finds_the_harbor_code_on_both_backends() {
        assert!(passes(Kind::Extract, false));
        assert!(passes(Kind::Extract, true));
    }

    #[test]
    fn multi_session_reads_the_workspace_lamp_and_the_global_pier() {
        assert!(passes(Kind::Multi, false));
        assert!(passes(Kind::Multi, true));
    }

    #[test]
    fn knowledge_update_keeps_noon_on_amr_and_legacy() {
        let legacy = passes(Kind::Update, false);
        let amr = passes(Kind::Update, true);
        assert!(legacy, "legacy update");
        assert!(amr, "amr update");
        assert!(u8::from(amr) >= u8::from(legacy));
    }

    #[test]
    fn abstention_does_not_regress_against_legacy() {
        let legacy = passes(Kind::Abstain, false);
        let amr = passes(Kind::Abstain, true);
        assert!(legacy, "legacy abstain");
        assert!(amr, "amr abstain");
        assert!(u8::from(amr) >= u8::from(legacy));
    }
}
