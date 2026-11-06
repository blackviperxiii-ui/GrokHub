//! Background file writes whose outcome lands on the status line.
//!
//! `/remember` and Memory Save run off the UI thread and report back here:
//! "Wrote <path>" once it is on disk, or "Couldn't write <path>: <error>".
//! Chat exports ride the same queue but land on the saved-file card
//! (`saved_file.rs`), which says where the file went and links to it.

use super::*;

/// One finished background write.
pub(super) struct FileWriteDone {
    /// What the status line names on failure (path or memory file name).
    target: String,
    /// `Ok` carries what to name on success (the path actually written).
    result: Result<String, String>,
    /// Autosaves stay quiet on success and only speak up when they fail.
    quiet_ok: bool,
    /// A chat export: its result goes to the saved-file card instead.
    export: Option<super::saved_file::ExportRetry>,
}

impl FileWriteDone {
    /// An autosave's result: silent on success, reported on failure.
    pub(super) fn quiet(target: String, result: Result<String, String>) -> Self {
        Self {
            target,
            result,
            quiet_ok: true,
            export: None,
        }
    }
}

impl Cabin {
    /// Run `work` off the UI thread and report its result on the status line.
    /// `work` returns the name to show on success.
    pub(super) fn spawn_file_write(
        &mut self,
        target: String,
        quiet_ok: bool,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        self.spawn_write_done(target, quiet_ok, None, work);
    }

    /// A chat export's write: same worker, reported on the saved-file card.
    pub(super) fn spawn_export_write(
        &mut self,
        retry: super::saved_file::ExportRetry,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        self.spawn_write_done(String::new(), false, Some(retry), work);
    }

    fn spawn_write_done(
        &mut self,
        target: String,
        quiet_ok: bool,
        export: Option<super::saved_file::ExportRetry>,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        self.file_writes.push(rx);
        std::thread::spawn(move || {
            let result = work();
            let _ = tx.send(FileWriteDone {
                target,
                result,
                quiet_ok,
                export,
            });
        });
    }

    /// Drain finished writes into the status line (newest result wins).
    pub(super) fn poll_file_writes(&mut self) {
        if self.file_writes.is_empty() {
            return;
        }
        let mut done = Vec::new();
        self.file_writes.retain(|rx| match rx.try_recv() {
            Ok(d) => {
                done.push(d);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for d in done {
            if let Some(retry) = d.export {
                self.finish_export(d.result, retry);
                continue;
            }
            match d.result {
                Ok(shown) if !d.quiet_ok => self.status = format!("Wrote {shown}"),
                Ok(_) => {}
                Err(e) => self.status = format!("Couldn't write {}: {e}", d.target),
            }
        }
    }

    /// Save a memory file off the UI thread when it differs from disk.
    pub(super) fn save_memory_file(&mut self, name: String, body: String, quiet_ok: bool) {
        let shown = name.clone();
        self.spawn_file_write(name.clone(), quiet_ok, move || {
            if config::read_memory(&name) != body {
                config::write_memory(&name, &body)?;
            }
            Ok(shown)
        });
    }

    /// `/export` file name for now: `export.md` → `export-20261031-2015.md`.
    pub(super) fn stamped_export_name(file: &str) -> String {
        let clock = Self::local_clock();
        stamp_file_name(file, &Self::local_day(), clock.hour, clock.minute)
    }
}

/// `export.md` + `2026-10-31` 20:15 → `export-20261031-2015.md`.
pub(super) fn stamp_file_name(file: &str, day: &str, hour: u32, minute: u32) -> String {
    let ymd: String = day.chars().filter(char::is_ascii_digit).collect();
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, Some(e)),
        _ => (file, None),
    };
    let stamped = format!("{stem}-{ymd}-{hour:02}{minute:02}");
    match ext {
        Some(e) => format!("{stamped}.{e}"),
        None => stamped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_puts_the_local_minute_before_the_extension() {
        assert_eq!(
            stamp_file_name("export.md", "2026-10-31", 20, 5),
            "export-20261031-2005.md"
        );
        assert_eq!(
            stamp_file_name("export.json", "2026-01-02", 0, 0),
            "export-20260102-0000.json"
        );
        assert_eq!(
            stamp_file_name("notes", "2026-10-31", 9, 41),
            "notes-20261031-0941"
        );
    }
}
