//! MEMORY.md keeps long-term notes only. The Pulse ledger lives in its own
//! `memory/pulse-ledger.md`; lines an older build wrote into MEMORY.md move
//! over the first time the ledger is read. Lines the old reflect copied
//! straight from chat are offered for removal once.

use grokhub_core::learn_notes::{raw_chat_lines, without_lines};
use grokhub_core::pulse::{merge_ledger, split_ledger, LEDGER_FILE};

use super::persist::pin_scheduled_dir;
use super::*;
use crate::config;

/// A memory file bigger than `read_memory` reads: rewriting it from that
/// read would drop its tail, so it is left as it is.
fn too_big_to_rewrite(name: &str) -> bool {
    std::fs::metadata(config::memory_dir().join(name))
        .is_ok_and(|m| m.len() > config::MEMORY_FILE_CAP as u64)
}

/// `text` cut from the front, at a line, to fit the memory file cap: the
/// newest ledger lines are the ones kept.
fn newest_that_fit(mut text: String) -> String {
    while text.len() > config::MEMORY_FILE_CAP {
        match text.find('\n') {
            Some(i) => {
                text.drain(..=i);
            }
            None => text.clear(),
        }
    }
    text
}

/// Move any ledger lines still in MEMORY.md into the ledger file, oldest
/// first. MEMORY.md keeps a `.prev` copy of what it held (`write_memory`).
/// A MEMORY.md over the cap is never rewritten; its lines stay readable there.
pub(super) fn migrate_pulse_ledger() -> Result<(), String> {
    if too_big_to_rewrite("MEMORY.md") || too_big_to_rewrite(LEDGER_FILE) {
        return Ok(());
    }
    let memory = config::read_memory("MEMORY.md");
    if !memory.contains("pulse:") {
        return Ok(());
    }
    let (kept, moved) = split_ledger(&memory);
    if moved.is_empty() {
        return Ok(());
    }
    let ledger = config::read_memory(LEDGER_FILE);
    config::write_memory(LEDGER_FILE, &newest_that_fit(merge_ledger(&ledger, &moved)))?;
    config::write_memory("MEMORY.md", &kept)
}

/// The Pulse ledger text, after moving any lines MEMORY.md still holds.
/// Lines a too-big MEMORY.md still holds are read in front of the file's.
pub(super) fn read_pulse_ledger() -> String {
    let _ = migrate_pulse_ledger();
    let own = config::read_memory(LEDGER_FILE);
    let (_, left) = split_ledger(&config::read_memory("MEMORY.md"));
    merge_ledger(&own, &left)
}

/// One new ledger line, after the move so older lines stay in front. A full
/// ledger drops its oldest lines, never the new one.
pub(super) fn append_pulse_ledger(line: &str) -> Result<(), String> {
    migrate_pulse_ledger()?;
    if too_big_to_rewrite(LEDGER_FILE) {
        return Err("The Pulse ledger is too large to add to. Nothing was changed.".into());
    }
    let mut body = config::read_memory(LEDGER_FILE);
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(line.trim());
    body.push('\n');
    config::write_memory(LEDGER_FILE, &newest_that_fit(body))
}

impl Cabin {
    /// Old chat lines in the open MEMORY.md, worked out once per open.
    fn memory_raw_lines(&mut self) -> &[String] {
        if self.mem_raw_lines.is_none() {
            let lines = raw_chat_lines(&self.mem_body, config::MEMORY_SEED);
            if lines.is_empty() && !self.cfg.memory_raw_tidy_done {
                // Nothing from the old reflect: never offer it later for lines
                // the user writes by hand.
                self.cfg.memory_raw_tidy_done = true;
                self.persist_cfg();
            }
            self.mem_raw_lines = Some(lines);
        }
        self.mem_raw_lines.as_deref().unwrap_or_default()
    }

    /// Memory page, MEMORY.md open: the one-time offer to drop old chat lines.
    pub(super) fn ui_memory_raw_tidy(&mut self, ui: &mut egui::Ui) {
        if self.mem_name != "MEMORY.md" || self.cfg.memory_raw_tidy_done || self.scratch() {
            return;
        }
        let lines = self.memory_raw_lines().to_vec();
        if lines.is_empty() {
            return;
        }
        ui.add_space(8.0);
        crate::cards::settings_note(
            ui,
            &format!(
                "{} line{} here came straight from old chats, copied by an earlier reflect. Remove them? MEMORY.md.prev keeps a copy.",
                lines.len(),
                if lines.len() == 1 { "" } else { "s" }
            ),
        );
        for line in lines.iter().take(5) {
            ui.label(
                RichText::new(line)
                    .monospace()
                    .size(12.0)
                    .color(crate::theme::muted()),
            );
        }
        if lines.len() > 5 {
            ui.label(
                RichText::new(format!("and {} more", lines.len() - 5))
                    .size(12.0)
                    .color(crate::theme::subtle()),
            );
        }
        ui.horizontal(|ui| {
            if crate::cards::white_pill(ui, "Remove them") {
                self.tidy_memory_raw_lines();
            }
            if crate::cards::ghost_pill(ui, "Keep them") {
                self.cfg.memory_raw_tidy_done = true;
                self.persist_cfg();
            }
        });
    }

    /// Drop the old chat lines from the open MEMORY.md and save it.
    pub(super) fn tidy_memory_raw_lines(&mut self) {
        let lines = self.memory_raw_lines().to_vec();
        if too_big_to_rewrite("MEMORY.md") {
            self.status = "MEMORY.md is too large to tidy here. Nothing was changed.".into();
            return;
        }
        self.cfg.memory_raw_tidy_done = true;
        self.persist_cfg();
        if lines.is_empty() {
            return;
        }
        let next = without_lines(&self.mem_body, &lines);
        self.mem_body = next.clone();
        if let Some(i) = Self::mem_file_idx("MEMORY.md") {
            self.mem_cache_body[i] = next.clone();
        }
        let dir = config::config_dir();
        self.mem_leave_saves.retain(|s| !s.is_finished());
        self.mem_leave_saves.push(std::thread::spawn(move || {
            let _pin = pin_scheduled_dir(dir);
            let _ = config::write_memory("MEMORY.md", &next);
        }));
        self.status = format!(
            "Removed {} old chat line{}. MEMORY.md.prev keeps a copy.",
            lines.len(),
            if lines.len() == 1 { "" } else { "s" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        std::fs::create_dir_all(&root).unwrap();
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    #[test]
    fn old_ledger_lines_move_out_of_memory_md_and_new_ones_land_after_them() {
        let (_pin, root) = pinned("ledger-move");
        let memory = "# Memory\n\nLong-term notes.\n- [2026-10-04] pulse: dismissed \"Standup\" reason=dismiss\n- 2026-10-05: the spare key is under the planter\n";
        config::write_memory("MEMORY.md", memory).unwrap();
        let mut cabin = Cabin::quiet_for_test();
        cabin.pulse_note(
            "NFL scores",
            grokhub_core::pulse::LedgerReason::Used,
            "2026-10-28",
        );
        assert_eq!(
            config::read_memory("MEMORY.md"),
            "# Memory\n\nLong-term notes.\n- 2026-10-05: the spare key is under the planter\n"
        );
        assert_eq!(
            config::read_memory("MEMORY.md.prev"),
            memory,
            "the move keeps a copy"
        );
        assert_eq!(
            config::read_memory(LEDGER_FILE),
            "- [2026-10-04] pulse: dismissed \"Standup\" reason=dismiss\n- [2026-10-28] pulse: used \"NFL scores\" reason=used\n"
        );
        let titles: Vec<String> = grokhub_core::pulse::parse_ledger(&read_pulse_ledger())
            .into_iter()
            .map(|e| e.title)
            .collect();
        assert_eq!(titles, ["Standup", "NFL scores"]);
        drop(cabin);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_memory_md_over_the_cap_is_never_rewritten_and_its_lines_still_read() {
        let (_pin, root) = pinned("ledger-big");
        let old = "- [2026-10-04] pulse: dismissed \"Standup\" reason=dismiss\n";
        let big = format!("{old}{}", "x".repeat(config::MEMORY_FILE_CAP));
        std::fs::create_dir_all(config::memory_dir()).unwrap();
        std::fs::write(config::memory_dir().join("MEMORY.md"), &big).unwrap();
        let mut cabin = Cabin::quiet_for_test();
        cabin.pulse_note(
            "NFL scores",
            grokhub_core::pulse::LedgerReason::Used,
            "2026-10-28",
        );
        assert_eq!(
            std::fs::read_to_string(config::memory_dir().join("MEMORY.md")).unwrap(),
            big,
            "nothing past the read cap is lost"
        );
        assert_eq!(
            config::read_memory(LEDGER_FILE),
            "- [2026-10-28] pulse: used \"NFL scores\" reason=used\n"
        );
        let titles: Vec<String> = grokhub_core::pulse::parse_ledger(&read_pulse_ledger())
            .into_iter()
            .map(|e| e.title)
            .collect();
        assert_eq!(titles, ["Standup", "NFL scores"]);
        cabin.mem_name = "MEMORY.md".into();
        cabin.mem_body = config::read_memory("MEMORY.md");
        cabin.tidy_memory_raw_lines();
        assert_eq!(
            cabin.status,
            "MEMORY.md is too large to tidy here. Nothing was changed."
        );
        drop(cabin);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_full_ledger_drops_its_oldest_lines_never_the_new_one() {
        let (_pin, root) = pinned("ledger-full");
        let line = "- [2026-10-01] pulse: liked \"Filler\" reason=liked\n";
        let full = line.repeat(config::MEMORY_FILE_CAP / line.len());
        config::write_memory(LEDGER_FILE, &full).unwrap();
        append_pulse_ledger("- [2026-10-28] pulse: used \"NFL scores\" reason=used").unwrap();
        let after = config::read_memory(LEDGER_FILE);
        assert!(after.len() <= config::MEMORY_FILE_CAP);
        assert!(after.ends_with("- [2026-10-28] pulse: used \"NFL scores\" reason=used\n"));
        assert!(after.starts_with(line), "it cuts at a line");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn old_chat_lines_are_offered_once_and_removed_with_a_copy_kept() {
        let (_pin, root) = pinned("memory-raw-tidy");
        let memory = "# Memory\n\nLong-term notes.\nI prefer the harbor light on at night\n- 2026-10-05: the spare key is under the planter\n";
        config::write_memory("MEMORY.md", memory).unwrap();
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.memory_raw_tidy_done = false;
        cabin.mem_name = "MEMORY.md".into();
        cabin.mem_body = memory.into();
        assert_eq!(
            cabin.memory_raw_lines(),
            ["I prefer the harbor light on at night"]
        );
        cabin.tidy_memory_raw_lines();
        for save in cabin.mem_leave_saves.drain(..) {
            save.join().unwrap();
        }
        let tidied =
            "# Memory\n\nLong-term notes.\n- 2026-10-05: the spare key is under the planter\n";
        assert_eq!(cabin.mem_body, tidied);
        assert_eq!(config::read_memory("MEMORY.md"), tidied);
        assert_eq!(config::read_memory("MEMORY.md.prev"), memory);
        assert!(cabin.cfg.memory_raw_tidy_done, "never offered again");
        assert_eq!(
            cabin.status,
            "Removed 1 old chat line. MEMORY.md.prev keeps a copy."
        );
        drop(cabin);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_memory_md_with_no_old_chat_lines_never_offers_later() {
        let (_pin, root) = pinned("memory-raw-none");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.memory_raw_tidy_done = false;
        cabin.mem_name = "MEMORY.md".into();
        cabin.mem_body = "# Memory\n\nLong-term notes.\n- 2026-10-05: a dated note\n".into();
        assert!(cabin.memory_raw_lines().is_empty());
        assert!(
            cabin.cfg.memory_raw_tidy_done,
            "a plain line typed later is the user's own, not an old dump"
        );
        drop(cabin);
        let _ = std::fs::remove_dir_all(root);
    }
}
