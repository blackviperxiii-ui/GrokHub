//! Home's Frame Rate setup in one tap: the same steps as "Copy setup", run
//! behind the OS's own admin prompt (see [`crate::elevate`]), then the
//! counters are opened again to check it worked. The Home tile and the
//! chat's `module_setup` tool both call [`run`], so they always do the same
//! thing and report it the same way.

use crate::elevate::Elevated;

/// How a setup or undo went, in the words the tile and the chat use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Frames can be counted now (setup), or the probe is gone (undo).
    Done,
    /// Windows: the group change lands at the next sign-in.
    SignOut,
    /// The user closed the password or admin prompt.
    Dismissed,
    /// No prompt can show here; the steps have to be pasted by hand.
    NoPrompt,
    /// The steps ran, or tried to, and frames still can't be counted.
    Failed(String),
    /// Frames aren't counted on this OS.
    Unsupported,
}

impl Outcome {
    /// One plain sentence for the chat and the tile.
    pub fn sentence(&self, undo: bool) -> String {
        match (self, undo) {
            (Outcome::Done, false) => "Frame Rate is set up and counting frames.".into(),
            (Outcome::Done, true) => "The Frame Rate setup is undone.".into(),
            (Outcome::SignOut, false) => {
                "Set up. Sign out of Windows and back in, and Frame Rate starts counting.".into()
            }
            (Outcome::SignOut, true) => {
                "Undone. It fully takes effect after you sign out of Windows and back in.".into()
            }
            (Outcome::Dismissed, _) => {
                "The password prompt was closed, so nothing changed.".into()
            }
            (Outcome::NoPrompt, _) => {
                "This desktop can't show a password prompt. Copy setup gives the steps to paste into a terminal.".into()
            }
            (Outcome::Failed(why), _) => format!("The setup didn't finish: {why}"),
            (Outcome::Unsupported, _) => "Frame Rate isn't counted on this computer.".into(),
        }
    }
}

/// Whether the Linux probe is installed (so there is something to undo).
pub fn installed() -> bool {
    cfg!(target_os = "linux") && std::path::Path::new(super::PROBE_PATH).exists()
}

/// The steps as text to paste by hand: a terminal on Linux, an admin
/// PowerShell on Windows.
pub fn copy_text(undo: bool) -> String {
    if cfg!(windows) {
        let account = super::windows_account();
        return if undo {
            super::windows_undo(&account)
        } else {
            super::windows_setup(&account)
        };
    }
    if undo {
        return super::linux_undo();
    }
    let exe = std::env::current_exe()
        .map(|p| super::probe_source(&p).to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/usr/bin/grokhub".into());
    super::linux_setup(&exe)
}

/// Runs the setup (or its undo) behind the OS prompt and reports the result.
/// Blocks until the user answers the prompt.
pub fn run(undo: bool) -> Outcome {
    #[cfg(target_os = "linux")]
    {
        linux(undo)
    }
    #[cfg(windows)]
    {
        windows(undo)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = undo;
        Outcome::Unsupported
    }
}

/// What a finished elevated run means once the counters are tried again.
pub fn after(elevated: Elevated, undo: bool, counting: impl FnOnce() -> bool) -> Outcome {
    match elevated {
        Elevated::Done if undo => Outcome::Done,
        Elevated::Done if counting() => Outcome::Done,
        Elevated::Done => Outcome::Failed(
            "the probe is installed, but the kernel still won't share its frame counter".into(),
        ),
        Elevated::Dismissed => Outcome::Dismissed,
        Elevated::NoPrompt => Outcome::NoPrompt,
        Elevated::Failed(why) => Outcome::Failed(why),
    }
}

#[cfg(target_os = "linux")]
fn linux(undo: bool) -> Outcome {
    use crate::elevate::root_sh;
    if undo {
        let script = format!("rm -f {} {}", super::PROBE_PATH, super::TMPFILES_RULE);
        return after(root_sh(&script), true, || false);
    }
    let Ok(exe) = std::env::current_exe() else {
        return Outcome::Failed("couldn't find this app's own file".into());
    };
    let source = super::probe_source(&exe);
    let copy = match private_copy(&source) {
        Ok(p) => p,
        Err(e) => return Outcome::Failed(format!("couldn't copy the probe ({e})")),
    };
    // SAFETY: getgid has no preconditions.
    let gid = unsafe { libc::getgid() };
    let script = super::linux_root_script(&copy.to_string_lossy(), gid);
    let ran = root_sh(&script);
    let _ = std::fs::remove_file(&copy);
    after(ran, false, || super::FlipCounter::open().is_ok())
}

/// A copy of `source` in a new file only this user can touch, so root reads
/// the copy (an AppImage's mount is closed to root) and nobody can swap it
/// before the install.
#[cfg(target_os = "linux")]
fn private_copy(source: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let path =
        std::env::temp_dir().join(format!("loadout-fps-probe-{}-{nanos}", std::process::id()));
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&path)?;
    let copied = std::fs::File::open(source).and_then(|mut f| std::io::copy(&mut f, &mut out));
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

#[cfg(windows)]
fn windows(undo: bool) -> Outcome {
    let account = super::windows_account();
    let script = if undo {
        super::windows_undo_script(&account)
    } else {
        super::windows_script(&account)
    };
    match crate::elevate::admin_ps(&script) {
        // The new group reaches this sign-in's token only after a sign-out,
        // unless the trace already starts (the user was in the group).
        Elevated::Done if !undo && super::FlipCounter::open().is_ok() => Outcome::Done,
        Elevated::Done => Outcome::SignOut,
        other => after(other, undo, || false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_run_is_checked_by_counting_again() {
        assert_eq!(after(Elevated::Done, false, || true), Outcome::Done);
        assert!(matches!(
            after(Elevated::Done, false, || false),
            Outcome::Failed(why) if why.contains("frame counter")
        ));
        assert_eq!(after(Elevated::Done, true, || false), Outcome::Done);
        assert_eq!(
            after(Elevated::Dismissed, false, || true),
            Outcome::Dismissed
        );
        assert_eq!(after(Elevated::NoPrompt, false, || true), Outcome::NoPrompt);
        assert_eq!(
            after(Elevated::Failed("setcap: not found".into()), false, || true),
            Outcome::Failed("setcap: not found".into())
        );
    }

    #[test]
    fn outcomes_read_as_plain_sentences() {
        assert_eq!(
            Outcome::Done.sentence(false),
            "Frame Rate is set up and counting frames."
        );
        assert_eq!(
            Outcome::Failed("setcap: not found".into()).sentence(false),
            "The setup didn't finish: setcap: not found"
        );
        assert!(Outcome::SignOut
            .sentence(false)
            .starts_with("Set up. Sign out"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_private_copy_is_the_users_alone() {
        use std::os::unix::fs::PermissionsExt;
        let src = std::env::temp_dir().join(format!("fps-copy-src-{}", std::process::id()));
        std::fs::write(&src, b"probe").unwrap();
        let copy = private_copy(&src).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"probe");
        let mode = std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        std::fs::remove_file(&copy).unwrap();
        std::fs::remove_file(&src).unwrap();
    }
}
