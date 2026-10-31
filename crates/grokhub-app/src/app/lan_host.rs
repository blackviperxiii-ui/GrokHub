//! The LAN address the Devices page shows for pairing, read off the UI thread.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_core::{hub_pair_url, parse_hostname_i, pick_lan_ipv4};

struct LanHostCache {
    at: Instant,
    out: String,
    inflight: bool,
}

static LAN_HOST: Mutex<Option<LanHostCache>> = Mutex::new(None);

fn hostname_i() -> String {
    if let Ok(g) = LAN_HOST.lock() {
        if let Some(c) = g.as_ref() {
            let hit = c.out.clone();
            let fresh = c.at.elapsed().as_secs() < 30;
            let busy = c.inflight;
            drop(g);
            if !fresh && !busy {
                kick_hostname();
            }
            return hit;
        }
    }
    let out = hostname_i_now();
    if let Ok(mut g) = LAN_HOST.lock() {
        *g = Some(LanHostCache {
            at: Instant::now(),
            out: out.clone(),
            inflight: false,
        });
    }
    out
}

pub(super) fn hostname_i_now() -> String {
    let out = lan_candidates_now();
    let addrs = parse_hostname_i(&out);
    let refs: Vec<&str> = addrs.iter().map(|s| s.as_str()).collect();
    if pick_lan_ipv4(&refs).is_some() {
        return out;
    }
    // No candidate (macOS has no `hostname -I`): the address outbound traffic leaves from.
    routed_ipv4().unwrap_or(out)
}

/// Space-separated local IPv4 addresses, in `hostname -I`'s format.
#[cfg(not(windows))]
fn lan_candidates_now() -> String {
    let mut cmd = std::process::Command::new("hostname");
    cmd.arg("-I");
    crate::desktop::run_limited(cmd, Duration::from_millis(400))
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Windows has no `hostname -I` (its `hostname` treats `-I` as an error), so read the
/// adapters directly instead of spawning a process that always fails.
#[cfg(windows)]
fn lan_candidates_now() -> String {
    let ips: Vec<String> = crate::win_native::adapter_ipv4s()
        .iter()
        .map(|ip| ip.to_string())
        .collect();
    ips.join(" ")
}

/// The local address the OS would route outbound traffic from. A UDP connect sends nothing;
/// 192.0.2.1 is TEST-NET-1 (RFC 5737).
pub(super) fn routed_ipv4() -> Option<String> {
    let sock = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    sock.connect(("192.0.2.1", 9)).ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

fn kick_hostname() {
    if let Ok(mut g) = LAN_HOST.lock() {
        if let Some(c) = g.as_mut() {
            if c.inflight {
                return;
            }
            c.inflight = true;
        }
    }
    std::thread::spawn(|| {
        let out = hostname_i_now();
        if let Ok(mut g) = LAN_HOST.lock() {
            *g = Some(LanHostCache {
                at: Instant::now(),
                out,
                inflight: false,
            });
        }
    });
}

pub(super) fn discover_hub_pair_url(port: u16) -> String {
    let out = hostname_i();
    let addrs = parse_hostname_i(&out);
    let refs: Vec<&str> = addrs.iter().map(|s| s.as_str()).collect();
    hub_pair_url(port, pick_lan_ipv4(&refs).as_deref())
}
