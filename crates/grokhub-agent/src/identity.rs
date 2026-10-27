//! SOUL and USER for the volatile prompt tail.
//!
//! The stable cabin rules stay byte-stable for the prefix cache. These blocks
//! sit after voice and notes and before the safety closer. USER is approved
//! active user-model rows when AMR is on, otherwise USER.md. A `proposed` tag
//! and a superseded row stay out. Every byte is redacted before the cap.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use grokhub_core::amr::{memory_rows, AmrStore};

/// SOUL.md after redaction, about 1.5 KB.
pub const SOUL_BYTE_CAP: usize = 1_536;

/// USER block after redaction, inside the 2–4 KB band.
pub const USER_BYTE_CAP: usize = 3_072;

/// Capped, redacted SOUL and USER blocks. Empty when both files are missing
/// and AMR has no active user-model row.
pub fn identity_blocks(config_dir: &Path) -> String {
    let soul = file_block(config_dir, "SOUL.md", SOUL_BYTE_CAP);
    let user = user_block(config_dir);
    let mut parts = Vec::new();
    if !soul.is_empty() {
        parts.push(format!("<soul>\n{soul}\n</soul>"));
    }
    if !user.is_empty() {
        parts.push(format!("<user>\n{user}\n</user>"));
    }
    parts.join("\n\n")
}

fn user_block(config_dir: &Path) -> String {
    if crate::memory::amr_enabled_at(config_dir) {
        let lines = active_user_lines(config_dir);
        if !lines.is_empty() {
            return prepare(&lines.join("\n"), USER_BYTE_CAP);
        }
    }
    file_block(config_dir, "USER.md", USER_BYTE_CAP)
}

fn active_user_lines(config_dir: &Path) -> Vec<String> {
    let store = user_store(config_dir);
    let (rows, _) = memory_rows(&store);
    rows.into_iter()
        .filter(|row| !row.proposed)
        .map(|row| row.text)
        .filter(|text| !text.trim().is_empty())
        .collect()
}

fn user_store(config_dir: &Path) -> AmrStore {
    crate::memory::amr_store_at(config_dir)
}

fn file_block(config_dir: &Path, name: &str, cap: usize) -> String {
    let path = config_dir.join("memory").join(name);
    let raw = read_capped(&path, cap.saturating_mul(4).max(cap));
    prepare(&raw, cap)
}

fn prepare(text: &str, cap: usize) -> String {
    let (redacted, _) = grokhub_core::redact_recall(text);
    clip_str(redacted.trim(), cap).trim().to_string()
}

fn read_capped(path: &Path, cap: usize) -> String {
    let Ok(file) = File::open(path) else {
        return String::new();
    };
    let mut buf = Vec::new();
    if file.take(cap as u64).read_to_end(&mut buf).is_err() {
        return String::new();
    }
    while !buf.is_empty() && std::str::from_utf8(&buf).is_err() {
        buf.pop();
    }
    String::from_utf8(buf).unwrap_or_default()
}

fn clip_str(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perm::ConfigGuard;
    use grokhub_core::amr::{propose_line, remember_line, EdgeRel, LineWrite};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_cfg(tag: &str) -> (std::path::PathBuf, ConfigGuard) {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("gh-id-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("memory")).unwrap();
        let guard = ConfigGuard::set(&dir);
        (dir, guard)
    }

    fn line<'a>(text: &'a str) -> LineWrite<'a> {
        LineWrite {
            text,
            source: "user",
            tags: Vec::new(),
            confidence: 0.9,
            revive: false,
        }
    }

    #[test]
    fn caps_bound_soul_and_user_and_redact_the_block() {
        let (dir, _guard) = temp_cfg("caps");
        fs::write(dir.join("memory").join("SOUL.md"), "S".repeat(8_000)).unwrap();
        let user = format!(
            "keeps a paper chart and mail harbor.keeper@example.com\n{}",
            "U".repeat(8_000)
        );
        fs::write(dir.join("memory").join("USER.md"), user).unwrap();
        fs::write(dir.join("app.json"), "{}\n").unwrap();
        let block = identity_blocks(&dir);
        let soul = block
            .split("<soul>\n")
            .nth(1)
            .unwrap()
            .split("\n</soul>")
            .next()
            .unwrap();
        let user = block
            .split("<user>\n")
            .nth(1)
            .unwrap()
            .split("\n</user>")
            .next()
            .unwrap();
        assert!(soul.len() <= SOUL_BYTE_CAP, "{}", soul.len());
        assert_eq!(soul.len(), SOUL_BYTE_CAP);
        assert!(user.len() <= USER_BYTE_CAP, "{}", user.len());
        assert!(user.contains("[email]"), "{user}");
        assert!(!user.contains("harbor.keeper@example.com"), "{user}");
    }

    #[test]
    fn proposed_and_superseded_rows_stay_out_and_amr_beats_user_md() {
        let (dir, _guard) = temp_cfg("rows");
        fs::write(dir.join("app.json"), "{\"memory_backend\":\"amr\"}\n").unwrap();
        fs::write(
            dir.join("memory").join("USER.md"),
            "from the markdown file only\n",
        )
        .unwrap();
        fs::write(
            dir.join("memory").join("SOUL.md"),
            "HARBOR_SOUL_MARK speaks plainly\n",
        )
        .unwrap();
        let store = crate::memory::amr_store_at(&dir);
        store.init().unwrap();
        let proposed = propose_line(&store, "prefers secret triangles", "chat:t", 1).unwrap();
        assert!(
            proposed.id().is_some(),
            "the proposed row must be stored: {proposed:?}"
        );
        let old = remember_line(&store, &line("keeps a paper notebook"), 1).unwrap();
        let new = remember_line(&store, &line("keeps a slate board"), 2).unwrap();
        store.link(new.id(), old.id(), EdgeRel::Supersedes).unwrap();
        let block = identity_blocks(&dir);
        assert!(block.contains("HARBOR_SOUL_MARK"), "{block}");
        assert!(block.contains("keeps a slate board"), "{block}");
        assert!(!block.contains("paper notebook"), "{block}");
        assert!(!block.contains("secret triangles"), "{block}");
        assert!(!block.contains("from the markdown file only"), "{block}");
        assert!(!block.contains("waiting for approval"), "{block}");
    }

    #[test]
    fn omitted_backend_uses_user_md_and_writes_off_keeps_an_approved_line() {
        let (dir, _guard) = temp_cfg("legacy-user");
        fs::write(dir.join("app.json"), "{}\n").unwrap();
        fs::write(dir.join("memory").join("USER.md"), "keeps a paper chart\n").unwrap();
        let block = identity_blocks(&dir);
        assert!(block.contains("keeps a paper chart"), "{block}");
        assert!(!block.contains("<soul>"), "{block}");

        fs::write(dir.join("app.json"), "{\"memory_backend\":\"amr\"}\n").unwrap();
        let store = crate::memory::amr_store_at(&dir);
        store.init().unwrap();
        remember_line(&store, &line("keeps a slate board"), 1).unwrap();
        let proposed = propose_line(&store, "prefers secret triangles", "chat:t", 2).unwrap();
        assert!(
            proposed.id().is_some(),
            "the proposed row must be stored: {proposed:?}"
        );
        crate::memory::set_memory_writes(false).unwrap();
        let block = identity_blocks(&dir);
        assert!(block.contains("keeps a slate board"), "{block}");
        assert!(!block.contains("secret triangles"), "{block}");
        assert!(!block.contains("paper chart"), "{block}");
    }

    #[test]
    fn added_identity_tokens_stay_under_1500_at_p95() {
        let (dir, _guard) = temp_cfg("budget");
        fs::write(dir.join("app.json"), "{}\n").unwrap();
        let mut tokens = Vec::new();
        for n in 0..20 {
            let (soul, user) = if n == 0 {
                ("speaks plainly".to_string(), "keeps a chart\n".to_string())
            } else {
                (
                    "S".repeat(8_000),
                    format!("keeps a chart {}\n", "U".repeat(8_000)),
                )
            };
            fs::write(dir.join("memory").join("SOUL.md"), soul).unwrap();
            fs::write(dir.join("memory").join("USER.md"), user).unwrap();
            let block = std::hint::black_box(identity_blocks(&dir));
            tokens.push(crate::tokens::estimate_tokens(&block));
        }
        tokens.sort();
        let p50 = tokens[tokens.len() / 2];
        let p95_idx = ((tokens.len() as f64) * 0.95).ceil() as usize - 1;
        let p95 = tokens[p95_idx];
        let max = *tokens.last().unwrap();
        assert!(
            p95 < 1_500 && max < 1_500,
            "p50 {p50} p95 {p95} max {max} samples {}",
            tokens.len()
        );
        println!(
            "identity_tokens p50 {p50} p95 {p95} max {max} samples {}",
            tokens.len()
        );
    }

    #[test]
    fn identity_read_p50_stays_within_ten_percent_or_the_noise_floor() {
        let (full, _guard) = temp_cfg("lat-full");
        let empty = full
            .parent()
            .unwrap()
            .join(format!("gh-id-lat-empty-{}", std::process::id()));
        fs::create_dir_all(&empty).unwrap();
        fs::write(full.join("app.json"), "{}\n").unwrap();
        fs::write(full.join("memory").join("SOUL.md"), "S".repeat(8_000)).unwrap();
        fs::write(
            full.join("memory").join("USER.md"),
            format!("keeps a chart\n{}", "U".repeat(8_000)),
        )
        .unwrap();
        let grown_check = identity_blocks(&full);
        let base_check = identity_blocks(&empty);
        assert!(grown_check.contains("<soul>"), "{grown_check}");
        assert!(base_check.is_empty(), "{base_check}");
        const SAMPLES: usize = 31;
        let mut base = Vec::with_capacity(SAMPLES);
        let mut grown = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let start = std::time::Instant::now();
            std::hint::black_box(identity_blocks(&empty));
            base.push(start.elapsed().as_secs_f64() * 1000.0);
            let start = std::time::Instant::now();
            std::hint::black_box(identity_blocks(&full));
            grown.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        base.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        grown.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let base_ms = base[SAMPLES / 2];
        let grown_ms = grown[SAMPLES / 2];
        let ratio = if base_ms == 0.0 {
            1.0
        } else {
            grown_ms / base_ms
        };
        let gap = grown_ms - base_ms;
        // Empty-dir baseline does almost no IO, so the +10% ratio is meaningless
        // there. Accept a small absolute gap (Windows CI saw ~2.3 ms).
        assert!(
            ratio <= 1.10 || gap <= 5.0,
            "p50 {grown_ms:.3} ms vs {base_ms:.3} ms ratio {ratio:.3} gap {gap:.3} ms"
        );
        println!(
            "identity_latency p50 {grown_ms:.3} ms vs baseline {base_ms:.3} ms ratio {ratio:.3} gap {gap:.3} ms samples {SAMPLES}"
        );
    }
}
