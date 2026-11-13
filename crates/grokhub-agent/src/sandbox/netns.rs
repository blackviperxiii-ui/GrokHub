//! Linux: a sandboxed command gets its own user, network and mount
//! namespaces, so the proxy variables are no longer the only thing between
//! it and the internet.
//!
//! - Network: the new namespace has only its own loopback. A small relay
//!   inside it listens on `127.0.0.1:<proxy port>` and carries each
//!   connection over a Unix socket to the egress proxy outside, so the proxy
//!   variables still work and nothing else gets out: no `curl --noproxy`, no
//!   raw sockets, no DNS. `localhost` inside is the namespace's own, so a
//!   test that starts a server and talks to it still works.
//! - Reads: secret folders and files (keys, cloud and registry credentials,
//!   browser profiles, the sign-in folder, Loadout's config folder) are
//!   covered by an empty read-only folder or `/dev/null`.
//!
//! Everything here runs in the child between fork and exec, so it uses raw
//! syscalls and no allocation. The relay is a grandchild that leaves the
//! command's process tree (so a shell's `wait` never waits on it) and exits
//! when the command does.
//!
//! When the kernel or the distribution forbids unprivileged user namespaces
//! (some Ubuntu releases do, through AppArmor), [`available`] is false and the
//! sandbox falls back to the proxy variables, and [`super::status`] says so.

use std::ffi::{c_int, CString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// What the child sets up before exec. Built in the parent.
pub struct Setup {
    uid_map: Vec<u8>,
    gid_map: Vec<u8>,
    hide_dirs: Vec<CString>,
    hide_files: Vec<CString>,
    bridge: Option<(libc::sockaddr_un, u16)>,
}

impl Setup {
    /// `bridge` is the Unix socket of the egress proxy and the port the
    /// command's proxy variables name.
    pub fn new(hide: &[PathBuf], bridge: Option<(&Path, u16)>) -> Result<Self, String> {
        // SAFETY: getuid and getgid can't fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let mut hide_dirs = Vec::new();
        let mut hide_files = Vec::new();
        for path in hide {
            let c = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
            if path.is_dir() {
                hide_dirs.push(c);
            } else {
                hide_files.push(c);
            }
        }
        let bridge = match bridge {
            Some((sock, port)) => Some((sockaddr(sock)?, port)),
            None => None,
        };
        Ok(Self {
            uid_map: format!("{uid} {uid} 1").into_bytes(),
            gid_map: format!("{gid} {gid} 1").into_bytes(),
            hide_dirs,
            hide_files,
            bridge,
        })
    }
}

fn sockaddr(path: &Path) -> Result<libc::sockaddr_un, String> {
    // SAFETY: sockaddr_un is plain data; all zeros is a valid value.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() >= addr.sun_path.len() {
        return Err(format!("socket path too long: {}", path.display()));
    }
    for (dst, src) in addr.sun_path.iter_mut().zip(bytes) {
        *dst = *src as libc::c_char;
    }
    Ok(addr)
}

/// Whether this machine lets an unprivileged process make the namespaces and
/// bring its loopback up. Probed once, in a throwaway child.
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        if std::env::var_os("GROKHUB_SANDBOX_NO_NETNS").is_some() {
            return false;
        }
        let Ok(setup) = Setup::new(&[], None) else {
            return false;
        };
        let Some(true_bin) = ["/bin/true", "/usr/bin/true"]
            .into_iter()
            .find(|p| Path::new(p).exists())
        else {
            return false;
        };
        let mut cmd = std::process::Command::new(true_bin);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // SAFETY: enter only makes async-signal-safe syscalls.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(move || enter(&setup).and_then(|()| pidfd_self().map(close)));
        }
        cmd.status().is_ok_and(|s| s.success())
    })
}

fn last() -> io::Error {
    io::Error::last_os_error()
}

fn close(fd: c_int) {
    // SAFETY: closing an fd this code opened.
    unsafe {
        libc::close(fd);
    }
}

fn pidfd_self() -> io::Result<c_int> {
    // SAFETY: pidfd_open on our own pid; returns a new fd or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, libc::getpid(), 0) };
    if fd < 0 {
        return Err(last());
    }
    let fd = fd as c_int;
    // SAFETY: fcntl on the fd just opened.
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    Ok(fd)
}

fn write_file(path: &std::ffi::CStr, data: &[u8]) -> io::Result<()> {
    // SAFETY: path is NUL-terminated; data outlives the call.
    unsafe {
        let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
        if fd < 0 {
            return Err(last());
        }
        let n = libc::write(fd, data.as_ptr().cast(), data.len());
        let err = last();
        libc::close(fd);
        if n != data.len() as isize {
            return Err(err);
        }
    }
    Ok(())
}

fn loopback_up() -> io::Result<()> {
    // SAFETY: a throwaway datagram socket and two interface ioctls.
    unsafe {
        let s = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if s < 0 {
            return Err(last());
        }
        let mut req: libc::ifreq = std::mem::zeroed();
        for (dst, src) in req.ifr_name.iter_mut().zip(b"lo") {
            *dst = *src as libc::c_char;
        }
        let mut rc = libc::ioctl(s, libc::SIOCGIFFLAGS as _, &mut req);
        if rc == 0 {
            req.ifr_ifru.ifru_flags |= (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_short;
            rc = libc::ioctl(s, libc::SIOCSIFFLAGS as _, &req);
        }
        let err = last();
        libc::close(s);
        if rc != 0 {
            return Err(err);
        }
    }
    Ok(())
}

/// In the child, between fork and exec: the namespaces, the hidden paths and
/// the relay. Async-signal-safe.
pub fn enter(setup: &Setup) -> io::Result<()> {
    // SAFETY: each call below only changes this (forked) process.
    unsafe {
        if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET | libc::CLONE_NEWNS) != 0 {
            return Err(last());
        }
    }
    write_file(c"/proc/self/setgroups", b"deny")?;
    write_file(c"/proc/self/uid_map", &setup.uid_map)?;
    write_file(c"/proc/self/gid_map", &setup.gid_map)?;
    loopback_up()?;
    // SAFETY: mount calls on NUL-terminated paths in our own mount namespace.
    unsafe {
        if libc::mount(
            std::ptr::null(),
            c"/".as_ptr(),
            std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            std::ptr::null(),
        ) != 0
        {
            return Err(last());
        }
        // A path that can't be covered (an odd filesystem) is left as it is
        // rather than failing every command.
        for dir in &setup.hide_dirs {
            libc::mount(
                c"tmpfs".as_ptr(),
                dir.as_ptr(),
                c"tmpfs".as_ptr(),
                libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
                c"size=4k,mode=0555".as_ptr().cast(),
            );
        }
        for file in &setup.hide_files {
            libc::mount(
                c"/dev/null".as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND,
                std::ptr::null(),
            );
        }
    }
    if let Some((addr, port)) = &setup.bridge {
        start_relay(addr, *port)?;
    }
    Ok(())
}

/// Fork the relay twice so it leaves the command's tree, and wait until it
/// listens, so the command's first request finds it.
fn start_relay(upstream: &libc::sockaddr_un, port: u16) -> io::Result<()> {
    let pidfd = pidfd_self()?;
    let mut fds = [0 as c_int; 2];
    // SAFETY: fork, pipe2, waitpid, read and _exit are async-signal-safe.
    unsafe {
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            close(pidfd);
            return Err(last());
        }
        let first = libc::fork();
        if first < 0 {
            let err = last();
            close(fds[0]);
            close(fds[1]);
            close(pidfd);
            return Err(err);
        }
        if first == 0 {
            if libc::fork() == 0 {
                relay(upstream, port, pidfd, fds[1]);
            }
            libc::_exit(0);
        }
        let mut status = 0;
        libc::waitpid(first, &mut status, 0);
        close(fds[1]);
        let mut byte = 0u8;
        let n = libc::read(fds[0], (&mut byte as *mut u8).cast(), 1);
        close(fds[0]);
        close(pidfd);
        if n != 1 {
            return Err(io::Error::other("the sandbox network relay did not start"));
        }
    }
    Ok(())
}

const MAX_CONNS: usize = 64;
const BUF: usize = 16 * 1024;

#[derive(Clone, Copy)]
struct Half {
    len: usize,
    off: usize,
    eof: bool,
    shut: bool,
}

#[derive(Clone, Copy)]
struct Conn {
    client: c_int,
    upstream: c_int,
    /// [0]: client to upstream, [1]: upstream to client.
    half: [Half; 2],
}

const IDLE: Half = Half {
    len: 0,
    off: 0,
    eof: false,
    shut: false,
};

/// The relay: accept on the namespace's loopback, carry bytes to and from
/// the proxy's Unix socket, exit when the command does. Never returns.
fn relay(upstream: &libc::sockaddr_un, port: u16, pidfd: c_int, ready: c_int) -> ! {
    // SAFETY: only async-signal-safe syscalls; memory comes from mmap.
    unsafe {
        for fd in 0..1024 {
            if fd != pidfd && fd != ready {
                libc::close(fd);
            }
        }
        let listener = libc::socket(
            libc::AF_INET,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        );
        if listener < 0 {
            libc::_exit(1);
        }
        let one: c_int = 1;
        libc::setsockopt(
            listener,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            (&one as *const c_int).cast(),
            std::mem::size_of::<c_int>() as u32,
        );
        let mut sin: libc::sockaddr_in = std::mem::zeroed();
        sin.sin_family = libc::AF_INET as libc::sa_family_t;
        sin.sin_port = port.to_be();
        sin.sin_addr.s_addr = u32::from_be_bytes([127, 0, 0, 1]).to_be();
        if libc::bind(
            listener,
            (&sin as *const libc::sockaddr_in).cast(),
            std::mem::size_of::<libc::sockaddr_in>() as u32,
        ) != 0
            || libc::listen(listener, 128) != 0
        {
            libc::_exit(1);
        }
        let byte = 1u8;
        libc::write(ready, (&byte as *const u8).cast(), 1);
        libc::close(ready);

        let mem = libc::mmap(
            std::ptr::null_mut(),
            MAX_CONNS * 2 * BUF,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        if mem == libc::MAP_FAILED {
            libc::_exit(1);
        }
        let mem = mem.cast::<u8>();
        let buf = |slot: usize, dir: usize| mem.add((slot * 2 + dir) * BUF);

        let mut conns: [Option<Conn>; MAX_CONNS] = [None; MAX_CONNS];
        let mut polls: [libc::pollfd; 2 + 2 * MAX_CONNS] = [libc::pollfd {
            fd: -1,
            events: 0,
            revents: 0,
        }; 2 + 2 * MAX_CONNS];
        loop {
            polls[0] = libc::pollfd {
                fd: pidfd,
                events: libc::POLLIN,
                revents: 0,
            };
            let free = conns.iter().any(Option::is_none);
            polls[1] = libc::pollfd {
                fd: if free { listener } else { -1 },
                events: libc::POLLIN,
                revents: 0,
            };
            for (slot, conn) in conns.iter().enumerate() {
                let (c, u) = match conn {
                    Some(conn) => (want(conn, 0), want(conn, 1)),
                    None => ((-1, 0), (-1, 0)),
                };
                polls[2 + slot * 2] = libc::pollfd {
                    fd: c.0,
                    events: c.1,
                    revents: 0,
                };
                polls[3 + slot * 2] = libc::pollfd {
                    fd: u.0,
                    events: u.1,
                    revents: 0,
                };
            }
            let n = libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, -1);
            if n < 0 {
                if *libc::__errno_location() == libc::EINTR {
                    continue;
                }
                libc::_exit(1);
            }
            if polls[0].revents != 0 {
                libc::_exit(0);
            }
            if polls[1].revents & libc::POLLIN != 0 {
                accept_one(listener, upstream, &mut conns);
            }
            for slot in 0..MAX_CONNS {
                let Some(mut conn) = conns[slot] else {
                    continue;
                };
                let rc = polls[2 + slot * 2].revents;
                let ru = polls[3 + slot * 2].revents;
                let ok =
                    step(&mut conn, 0, rc, buf(slot, 0)) && step(&mut conn, 1, ru, buf(slot, 1));
                let done = conn.half.iter().all(|h| h.eof && h.len == h.off);
                if !ok || done {
                    libc::close(conn.client);
                    libc::close(conn.upstream);
                    conns[slot] = None;
                } else {
                    conns[slot] = Some(conn);
                }
            }
        }
    }
}

/// The fd of side `side` (0 client, 1 upstream) and the events it waits for.
fn want(conn: &Conn, side: usize) -> (c_int, libc::c_short) {
    let fd = if side == 0 {
        conn.client
    } else {
        conn.upstream
    };
    // Reading `side` feeds half `side`; writing `side` drains the other half.
    let mut events = 0;
    let feed = conn.half[side];
    if !feed.eof && feed.len == feed.off {
        events |= libc::POLLIN;
    }
    let drain = conn.half[1 - side];
    if drain.len > drain.off {
        events |= libc::POLLOUT;
    }
    (fd, events)
}

/// Move half `dir` along: read from its source when `src_ev` says it is
/// ready, then write what is pending to its sink (both are non-blocking).
/// False on an error.
unsafe fn step(conn: &mut Conn, dir: usize, src_ev: i16, buf: *mut u8) -> bool {
    let (src, dst) = if dir == 0 {
        (conn.client, conn.upstream)
    } else {
        (conn.upstream, conn.client)
    };
    let half = &mut conn.half[dir];
    // SAFETY: buf is this half's own BUF-sized slice of the mmap'd region.
    unsafe {
        if src_ev & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0
            && !half.eof
            && half.len == half.off
        {
            let n = libc::read(src, buf.cast(), BUF);
            if n > 0 {
                half.len = n as usize;
                half.off = 0;
            } else if n == 0 {
                half.eof = true;
            } else if *libc::__errno_location() != libc::EAGAIN {
                return false;
            }
        }
        if half.len > half.off {
            // MSG_NOSIGNAL: a closed peer is an error here, not a SIGPIPE.
            let n = libc::send(
                dst,
                buf.add(half.off).cast(),
                half.len - half.off,
                libc::MSG_NOSIGNAL,
            );
            if n > 0 {
                half.off += n as usize;
            } else if n < 0 && *libc::__errno_location() != libc::EAGAIN {
                return false;
            }
        }
        if half.len == half.off {
            half.len = 0;
            half.off = 0;
            if half.eof && !half.shut {
                libc::shutdown(dst, libc::SHUT_WR);
                half.shut = true;
            }
        }
    }
    true
}

unsafe fn accept_one(listener: c_int, upstream: &libc::sockaddr_un, conns: &mut [Option<Conn>]) {
    // SAFETY: plain socket syscalls on fds this relay owns.
    unsafe {
        let client = libc::accept4(
            listener,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
        );
        if client < 0 {
            return;
        }
        let up = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if up < 0
            || libc::connect(
                up,
                (upstream as *const libc::sockaddr_un).cast(),
                std::mem::size_of::<libc::sockaddr_un>() as u32,
            ) != 0
        {
            if up >= 0 {
                libc::close(up);
            }
            libc::close(client);
            return;
        }
        let flags = libc::fcntl(up, libc::F_GETFL);
        libc::fcntl(up, libc::F_SETFL, flags | libc::O_NONBLOCK);
        let Some(slot) = conns.iter_mut().find(|c| c.is_none()) else {
            libc::close(up);
            libc::close(client);
            return;
        };
        *slot = Some(Conn {
            client,
            upstream: up,
            half: [IDLE; 2],
        });
    }
}
