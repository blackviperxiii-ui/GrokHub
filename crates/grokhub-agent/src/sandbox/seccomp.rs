//! Linux: a seccomp filter for commands in their own network namespace.
//!
//! It shuts the doors the namespace leaves open: a new `AF_UNIX` socket
//! could connect to a host socket on the shared filesystem (the Docker
//! socket, the session bus) and `AF_VSOCK` reaches the host of a VM, so both
//! are refused. `socketpair` stays allowed: an anonymous pair can't reach
//! anything outside, and many tools use one for their own children.
//! `io_uring` is refused too, since its requests skip this filter.
//!
//! Only these calls are checked, so connected sockets, pipes and everything
//! else behave as usual. Installed after the relay is running (the relay
//! itself needs a Unix socket) and before Landlock.

use std::io;

const SECCOMP_DATA_NR: u32 = 0;
const SECCOMP_DATA_ARCH: u32 = 4;
const SECCOMP_DATA_ARG0: u32 = 16;
const RET_ALLOW: u32 = 0x7fff_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const AF_VSOCK: u32 = 40;

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xc000_003e;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xc000_00b7;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const AUDIT_ARCH: u32 = 0;

const fn stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

const fn jeq(k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter {
        code: (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
        jt,
        jf,
        k,
    }
}

const LD: u16 = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
const RET: u16 = (libc::BPF_RET | libc::BPF_K) as u16;

/// The filter program. Unknown architectures get none (the namespace still
/// applies).
pub fn program() -> Vec<libc::sock_filter> {
    if AUDIT_ARCH == 0 {
        return Vec::new();
    }
    let eperm = RET_ERRNO | libc::EPERM as u32;
    vec![
        // 0: another architecture's syscall numbers mean something else.
        stmt(LD, SECCOMP_DATA_ARCH),
        jeq(AUDIT_ARCH, 1, 0),
        stmt(RET, RET_ALLOW),
        // 3
        stmt(LD, SECCOMP_DATA_NR),
        jeq(libc::SYS_socket as u32, 4, 0),
        jeq(libc::SYS_io_uring_setup as u32, 7, 0),
        jeq(libc::SYS_io_uring_enter as u32, 6, 0),
        jeq(libc::SYS_io_uring_register as u32, 5, 0),
        stmt(RET, RET_ALLOW),
        // 9: socket(domain, ...): the low word of the first argument.
        stmt(LD, SECCOMP_DATA_ARG0),
        jeq(libc::AF_UNIX as u32, 2, 0),
        jeq(AF_VSOCK, 1, 0),
        stmt(RET, RET_ALLOW),
        // 13
        stmt(RET, eperm),
    ]
}

/// In the child, before exec: no new privileges, then the filter.
/// Async-signal-safe.
pub fn install(program: &[libc::sock_filter]) -> io::Result<()> {
    if program.is_empty() {
        return Ok(());
    }
    let prog = libc::sock_fprog {
        len: program.len() as u16,
        filter: program.as_ptr() as *mut libc::sock_filter,
    };
    // SAFETY: prctl only changes this process; prog points at a live program.
    unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &prog as *const libc::sock_fprog,
        ) != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
