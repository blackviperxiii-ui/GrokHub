//! Background file writes whose outcome lands on the status line.
//!
//! `/export`, `/remember` and Memory Save used to set "Wrote <path>" before a
//! worker thread ran and dropped its error. Each write now runs off the UI
//! thread and reports back here: "Wrote <path>" once it is on disk, or
//! "Couldn't write <path>: <error>".

use super::*;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// One finished background write.
pub(super) struct FileWriteDone {
    /// What the status line names on failure (path or memory file name).
    target: String,
    /// `Ok` carries what to name on success (the path actually written).
    result: Result<String, String>,
    /// Autosaves stay quiet on success and only speak up when they fail.
    quiet_ok: bool,
}

impl FileWriteDone {
    /// An autosave's result: silent on success, reported on failure.
    pub(super) fn quiet(target: String, result: Result<String, String>) -> Self {
        Self {
            target,
            result,
            quiet_ok: true,
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
        let (tx, rx) = mpsc::channel();
        self.file_writes.push(rx);
        std::thread::spawn(move || {
            let result = work();
            let _ = tx.send(FileWriteDone {
                target,
                result,
                quiet_ok,
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

/// Write `body` to `dest`, or to `dest` with `-2`, `-3`, … before the
/// extension when that name is taken. Never overwrites. Returns the path used.
/// `make_dir` creates the folder first (our own config folder); a bound
/// project folder that is missing is reported instead.
pub(super) fn write_new_file(dest: &Path, body: &[u8], make_dir: bool) -> std::io::Result<PathBuf> {
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    if make_dir {
        std::fs::create_dir_all(dir)?;
    }
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    for n in 1..=99u32 {
        let path = if n == 1 {
            dest.to_path_buf()
        } else {
            dir.join(format!("{stem}-{n}{ext}"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(body)?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "too many exports with this name",
    ))
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

    #[test]
    fn write_new_file_never_clobbers() {
        let dir = std::env::temp_dir().join(format!("gh-write-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("export-20261031-2005.md");
        let first = write_new_file(&dest, b"first", false).unwrap();
        let second = write_new_file(&dest, b"second", false).unwrap();
        assert_eq!(first, dest);
        assert_eq!(second, dir.join("export-20261031-2005-2.md"));
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "second");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_new_file_reports_a_missing_folder() {
        let dest = std::env::temp_dir()
            .join(format!("gh-no-such-dir-{}", std::process::id()))
            .join("export.md");
        let err = write_new_file(&dest, b"x", false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
