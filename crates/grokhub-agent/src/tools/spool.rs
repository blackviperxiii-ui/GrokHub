//! Whole shell output, kept when the answer has to cut its middle: one file
//! per cut output under `{config}/tool-output/<workspace hash>/`, the newest
//! [`SPOOL_KEEP`] kept across all workspaces. The marker in the answer names
//! the file, and `read_file` reads it back, but only from its own
//! workspace's folder ([`find`]). Nothing is written in the workspace, so a
//! helper's sealed work and the project's git status never see these files.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use grokhub_core::autotune::device::fnv1a;

const SPOOL_DIR: &str = "tool-output";
/// Saved outputs kept, newest first, across every workspace's folder. A file
/// is at most about 385 KiB (the shell keeps 96 KiB head and tail per
/// stream), so about 7.5 MiB at worst.
pub(crate) const SPOOL_KEEP: usize = 20;
const PREFIX: &str = "shell-";
const SUFFIX: &str = ".txt";
/// Tags the output of a command that fetched from outside: it reads back fenced.
const FETCHED: &str = "-fetched";

static SEQ: AtomicU32 = AtomicU32::new(0);

/// What to keep a long output for.
pub(crate) struct Keep<'a> {
    /// The command's workspace: its folder is the only place this output
    /// reads back from.
    pub workspace: &'a Path,
    /// The chat, whose held secrets are taken out of the saved copy.
    pub session: Option<&'a str>,
    /// The output may hold fetched outside text.
    pub outside: bool,
}

fn root() -> PathBuf {
    crate::perm::config_dir().join(SPOOL_DIR)
}

fn spool_dir(workspace: &Path) -> PathBuf {
    let key = crate::perm::project_key(workspace);
    root().join(format!("{:016x}", fnv1a(key.as_bytes())))
}

fn is_spool_name(name: &str) -> bool {
    name.starts_with(PREFIX) && name.ends_with(SUFFIX)
}

/// Save `text` whole, secrets taken out (line for line, so the numbers a
/// marker names still hold), owner-only on unix. The path, with `/`
/// separators, or `None` when it could not be written.
pub(crate) fn save(keep: &Keep<'_>, text: &str) -> Option<String> {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) % 10_000;
    let tag = if keep.outside { FETCHED } else { "" };
    let name = format!(
        "{PREFIX}{:013}-{}-{seq:04}{tag}{SUFFIX}",
        grokhub_core::now_ms(),
        std::process::id()
    );
    let path = spool_dir(keep.workspace).join(name);
    let mut body = grokhub_core::redact_secrets(text);
    if let Some(session) = keep.session.filter(|s| !s.is_empty()) {
        body = crate::held::redact(session, &body);
    }
    crate::harness::private_write(&path, body.as_bytes()).ok()?;
    prune(&root());
    Some(path.to_string_lossy().replace('\\', "/"))
}

/// Keep the newest [`SPOOL_KEEP`] files across every workspace's folder; the
/// zero-padded time in each name makes name order time order. Errors are
/// ignored: Windows can't remove a file that is open.
fn prune(root: &Path) {
    let Ok(folders) = std::fs::read_dir(root) else {
        return;
    };
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for folder in folders.flatten() {
        let Ok(entries) = std::fs::read_dir(folder.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_spool_name(&name) {
                files.push((name, entry.path()));
            }
        }
    }
    if files.len() <= SPOOL_KEEP {
        return;
    }
    files.sort();
    let old = files.len() - SPOOL_KEEP;
    for (_, path) in files.into_iter().take(old) {
        let _ = std::fs::remove_file(&path);
        if let Some(folder) = path.parent() {
            // Only an empty folder goes.
            let _ = std::fs::remove_dir(folder);
        }
    }
}

/// The saved output `raw` names, when it is one of `workspace`'s: the real
/// path, and whether it was fetched from outside. `raw` must be absolute and
/// name a `shell-*.txt` file whose real parent is this workspace's folder,
/// so a link out of the folder, another project's outputs and every other
/// config file stay out of reach.
pub(crate) fn find(workspace: &Path, raw: &str) -> Option<(PathBuf, bool)> {
    let path = Path::new(raw.trim());
    if !path.is_absolute() || !is_spool_name(path.file_name()?.to_str()?) {
        return None;
    }
    let real = path.canonicalize().ok()?;
    let folder = spool_dir(workspace).canonicalize().ok()?;
    if real.parent()? != folder {
        return None;
    }
    let name = real.file_name()?.to_str()?;
    if !is_spool_name(name) {
        return None;
    }
    let fetched = name.ends_with(&format!("{FETCHED}{SUFFIX}"));
    Some((real, fetched))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keep(workspace: &Path, outside: bool) -> Keep<'_> {
        Keep {
            workspace,
            session: None,
            outside,
        }
    }

    fn saved_files(cfg: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for folder in std::fs::read_dir(cfg.join(SPOOL_DIR)).unwrap().flatten() {
            for entry in std::fs::read_dir(folder.path()).unwrap().flatten() {
                out.push(entry.path());
            }
        }
        out
    }

    #[test]
    fn the_newest_outputs_are_kept_across_workspaces() {
        let root = crate::harness::test_dir("spool-keep");
        let cfg = root.join("cfg");
        let _cfg = crate::perm::ConfigGuard::set(&cfg);
        let (one, two) = (root.join("one"), root.join("two"));
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut paths = Vec::new();
        for i in 0..21 {
            let ws = if i % 2 == 0 { &one } else { &two };
            paths.push(save(&keep(ws, false), &format!("output {i}")).expect("saved"));
        }
        let files = saved_files(&cfg);
        assert_eq!(files.len(), SPOOL_KEEP, "{files:?}");
        assert!(!Path::new(&paths[0]).exists(), "{}", paths[0]);
        assert!(Path::new(&paths[1]).exists(), "{}", paths[1]);
        assert_eq!(std::fs::read_to_string(&paths[20]).unwrap(), "output 20");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_fetched_output_reads_back_fenced_and_secrets_never_reach_the_file() {
        let root = crate::harness::test_dir("spool-fetched");
        let _cfg = crate::perm::ConfigGuard::set(root.join("cfg"));
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let fetched = save(&keep(&ws, true), "x").expect("saved");
        assert!(fetched.ends_with("-fetched.txt"), "{fetched}");
        let (real, outside) = find(&ws, &fetched).expect("found");
        assert!(outside);
        assert_eq!(real, Path::new(&fetched).canonicalize().unwrap());
        crate::tools::fence::reset_run_nonce_for_tests();
        let read = crate::tools::execute(
            &ws,
            "s",
            "read_file",
            &serde_json::json!({"target_file": fetched}).to_string(),
        );
        assert!(!read.failed, "{}", read.text);
        assert!(
            crate::tools::fence::looks_fenced(&read.text),
            "{}",
            read.text
        );
        assert!(read.text.contains("read_file:shell"), "{}", read.text);
        assert!(read.text.contains("1|x"), "{}", read.text);
        crate::tools::fence::reset_run_nonce_for_tests();

        let session = format!("spool-held-{}", std::process::id());
        crate::held::hold_secret(&session, "held-value-4321");
        let held = Keep {
            workspace: &ws,
            session: Some(&session),
            outside: false,
        };
        let plain = save(
            &held,
            "line one\nOPENAI_KEY sk-abcdefghijklmnopqrstuv\ntyped held-value-4321\nlast",
        )
        .expect("saved");
        assert_eq!(
            std::fs::read_to_string(&plain).unwrap(),
            "line one\nOPENAI_KEY [redacted]\ntyped [redacted]\nlast"
        );
        assert_eq!(find(&ws, &plain).map(|(_, outside)| outside), Some(false));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&plain).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Not absolute, not a saved output's name, or another workspace's.
        let name = Path::new(&plain).file_name().unwrap().to_str().unwrap();
        assert_eq!(find(&ws, name), None);
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(find(&other, &plain), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_folder_is_not_found() {
        let root = crate::harness::test_dir("spool-link");
        let _cfg = crate::perm::ConfigGuard::set(root.join("cfg"));
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let saved = save(&keep(&ws, false), "x").expect("saved");
        let outside = root.join("secret.txt");
        std::fs::write(&outside, "not an output").unwrap();
        let link = Path::new(&saved).with_file_name("shell-1-1-0001.txt");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert_eq!(find(&ws, &link.display().to_string()), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
