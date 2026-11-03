//! Linux Landlock, by raw syscall (no new crate). The ruleset is built in the
//! parent, where a failure can be handled; the child only turns on
//! `no_new_privs` and calls `landlock_restrict_self`, both async-signal-safe.
//!
//! Only write rights are handled, so reads and execs stay open everywhere and
//! writes, removes, renames and new files are allowed only beneath the roots.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const SYS_CREATE_RULESET: libc::c_long = 444;
const SYS_ADD_RULE: libc::c_long = 445;
const SYS_RESTRICT_SELF: libc::c_long = 446;
const CREATE_RULESET_VERSION: u32 = 1;
const RULE_PATH_BENEATH: u32 = 1;

const WRITE_FILE: u64 = 1 << 1;
const REMOVE_DIR: u64 = 1 << 4;
const REMOVE_FILE: u64 = 1 << 5;
const MAKE_CHAR: u64 = 1 << 6;
const MAKE_DIR: u64 = 1 << 7;
const MAKE_REG: u64 = 1 << 8;
const MAKE_SOCK: u64 = 1 << 9;
const MAKE_FIFO: u64 = 1 << 10;
const MAKE_BLOCK: u64 = 1 << 11;
const MAKE_SYM: u64 = 1 << 12;
/// ABI 2: rename and link across folders.
const REFER: u64 = 1 << 13;
/// ABI 3: `truncate` and `O_TRUNC`.
const TRUNCATE: u64 = 1 << 14;

/// The rights a file (not a folder) can be given.
const FILE_RIGHTS: u64 = WRITE_FILE | TRUNCATE;

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
}

#[repr(C, packed)]
struct PathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

/// The kernel's Landlock ABI, or None when Landlock is off or missing.
pub fn abi() -> Option<i32> {
    // SAFETY: a null attr with size 0 and the VERSION flag only asks for the ABI.
    let v = unsafe {
        libc::syscall(
            SYS_CREATE_RULESET,
            std::ptr::null::<RulesetAttr>(),
            0usize,
            CREATE_RULESET_VERSION,
        )
    };
    (v >= 1).then_some(v as i32)
}

fn handled(abi: i32) -> u64 {
    let mut rights = WRITE_FILE
        | REMOVE_DIR
        | REMOVE_FILE
        | MAKE_CHAR
        | MAKE_DIR
        | MAKE_REG
        | MAKE_SOCK
        | MAKE_FIFO
        | MAKE_BLOCK
        | MAKE_SYM;
    if abi >= 2 {
        rights |= REFER;
    }
    if abi >= 3 {
        rights |= TRUNCATE;
    }
    rights
}

/// A ready ruleset: write access beneath `roots` only. Roots that are missing
/// or can't be opened are skipped. Err when Landlock is not available.
pub fn ruleset(roots: &[std::path::PathBuf]) -> Result<OwnedFd, String> {
    let abi = abi().ok_or("Landlock is not available on this kernel")?;
    let rights = handled(abi);
    let attr = RulesetAttr {
        handled_access_fs: rights,
    };
    // SAFETY: attr is a valid ruleset_attr of the size passed; flags 0.
    let fd = unsafe {
        libc::syscall(
            SYS_CREATE_RULESET,
            &attr as *const RulesetAttr,
            std::mem::size_of::<RulesetAttr>(),
            0u32,
        )
    };
    if fd < 0 {
        return Err(format!(
            "landlock_create_ruleset: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: the syscall returned a new fd that nothing else owns.
    let ruleset = unsafe { OwnedFd::from_raw_fd(fd as RawFd) };
    for root in roots {
        let _ = add_root(&ruleset, root, rights);
    }
    Ok(ruleset)
}

fn add_root(ruleset: &OwnedFd, root: &Path, rights: u64) -> Result<(), String> {
    let c = CString::new(root.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // SAFETY: c is a valid C string; O_PATH opens without read rights.
    let raw = unsafe { libc::open(c.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: open returned a new fd that nothing else owns.
    let dir = unsafe { OwnedFd::from_raw_fd(raw) };
    let allowed = if root.is_dir() {
        rights
    } else {
        rights & FILE_RIGHTS
    };
    let attr = PathBeneathAttr {
        allowed_access: allowed,
        parent_fd: dir.as_raw_fd(),
    };
    // SAFETY: attr is a valid path_beneath_attr; both fds are open.
    let rc = unsafe {
        libc::syscall(
            SYS_ADD_RULE,
            ruleset.as_raw_fd(),
            RULE_PATH_BENEATH,
            &attr as *const PathBeneathAttr,
            0u32,
        )
    };
    if rc < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

/// In the child, after fork and before exec: no new privileges (Landlock
/// needs it without CAP_SYS_ADMIN), then the ruleset. Async-signal-safe.
pub fn restrict_self(ruleset: RawFd) -> std::io::Result<()> {
    // SAFETY: prctl and the Landlock syscall only change this process.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::syscall(SYS_RESTRICT_SELF, ruleset, 0u32) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}
