use crate::fill_random;

pub const CODE_ALPH: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const PAIR_TTL_MS: u64 = 15 * 60 * 1000;

pub fn normalize_code(c: &str) -> String {
    c.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect()
}

pub fn make_pair_code() -> String {
    let mut bytes = [0u8; 6];
    fill_random(&mut bytes);
    let alph = CODE_ALPH.as_bytes();
    let mut s = String::with_capacity(7);
    for (i, b) in bytes.iter().enumerate() {
        if i == 3 {
            s.push('-');
        }
        s.push(alph[(*b as usize) % alph.len()] as char);
    }
    s
}

pub fn parse_hostname_i(stdout: &str) -> Vec<String> {
    stdout.split_whitespace().map(|s| s.to_string()).collect()
}

pub fn pick_lan_ipv4(candidates: &[&str]) -> Option<String> {
    let mut best: Option<(u8, String)> = None;
    for a in candidates {
        let Ok(ip) = a.parse::<std::net::Ipv4Addr>() else {
            continue;
        };
        let Some(rank) = lan_rank(ip) else {
            continue;
        };
        match &best {
            None => best = Some((rank, ip.to_string())),
            Some((r, _)) if rank < *r => best = Some((rank, ip.to_string())),
            _ => {}
        }
    }
    best.map(|(_, s)| s)
}

fn lan_rank(ip: std::net::Ipv4Addr) -> Option<u8> {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_link_local() || ip.is_multicast() {
        return None;
    }
    let o = ip.octets();
    if o[0] == 192 && o[1] == 168 {
        return Some(0);
    }
    if o[0] == 10 {
        return Some(1);
    }
    if o[0] == 172 && (16..=31).contains(&o[1]) {
        if o[1] == 17 {
            return Some(4);
        }
        return Some(2);
    }
    Some(3)
}

pub fn hub_pair_url(port: u16, lan_ip: Option<&str>) -> String {
    match lan_ip.filter(|s| !s.is_empty()) {
        Some(ip) => format!("http://{ip}:{port}"),
        None => format!("http://127.0.0.1:{port}"),
    }
}

/// Same inclusive TTL as `HubState::pair_with`.
pub fn pair_code_is_live(expires_at: u64, now_ms: u64) -> bool {
    expires_at >= now_ms
}

/// Start share must mint when there is no code or the stored one is dead.
pub fn start_hub_rotates_pair(expires_at: Option<u64>, now_ms: u64) -> bool {
    match expires_at {
        None => true,
        Some(exp) => !pair_code_is_live(exp, now_ms),
    }
}

/// A pair tile is only useful while the hub is actually listening.
pub fn devices_shows_pair_code(sharing: bool, code_live: bool) -> bool {
    sharing && code_live
}

/// tiny-http / std bind errors when grokhub-hub.service already owns :18766.
pub fn lan_bind_in_use(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("address already in use") || e.contains("os error 98") || e.contains("eaddrinuse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_and_upcases() {
        assert_eq!(normalize_code("abc-234"), "ABC234");
        assert_eq!(normalize_code("ab c-23 4"), "ABC234");
        assert_eq!(normalize_code(""), "");
    }

    #[test]
    fn make_pair_code_has_dash_and_alphabet() {
        let code = make_pair_code();
        let b = code.as_bytes();
        assert_eq!(b.len(), 7);
        assert_eq!(b[3], b'-');
        assert!(code.chars().filter(|c| *c != '-').all(|c| CODE_ALPH.contains(c)));
        assert_eq!(PAIR_TTL_MS, 15 * 60 * 1000);
    }

    #[test]
    fn pair_url_uses_lan_ip_not_placeholder() {
        assert_eq!(
            pick_lan_ipv4(&["127.0.0.1", "192.168.1.40", "10.0.0.8"]),
            Some("192.168.1.40".into())
        );
        assert_eq!(
            pick_lan_ipv4(&["172.17.0.1", "192.168.1.40"]),
            Some("192.168.1.40".into()),
            "Docker bridge must not beat a Wi-Fi LAN address"
        );
        assert_eq!(
            pick_lan_ipv4(&["172.17.0.1"]),
            Some("172.17.0.1".into()),
            "docker-only hosts still need a pair URL"
        );
        assert_eq!(pick_lan_ipv4(&["127.0.0.1", "0.0.0.0"]), None);
        assert_eq!(
            hub_pair_url(18766, Some("192.168.1.40")),
            "http://192.168.1.40:18766"
        );
        assert_eq!(hub_pair_url(18766, None), "http://127.0.0.1:18766");
        assert!(!hub_pair_url(18766, pick_lan_ipv4(&["192.168.1.40"]).as_deref()).contains("<lan>"));
        assert_eq!(parse_hostname_i("192.168.1.40 10.0.0.8\n"), vec!["192.168.1.40", "10.0.0.8"]);
    }

    #[test]
    fn expired_pair_code_is_not_live() {
        assert!(pair_code_is_live(2_000, 1_000));
        assert!(
            pair_code_is_live(2_000, 2_000),
            "pair_with treats expiry as inclusive"
        );
        assert!(
            !pair_code_is_live(1_000, 2_000),
            "Devices must not keep showing a dead code"
        );
        assert!(start_hub_rotates_pair(None, 1_000));
        assert!(
            start_hub_rotates_pair(Some(500), 1_000),
            "an expired leftover must rotate on Start share"
        );
        assert!(!start_hub_rotates_pair(Some(2_000), 1_000));
        assert!(
            !devices_shows_pair_code(false, true),
            "a leftover code must not paint while the hub is off"
        );
        assert!(devices_shows_pair_code(true, true));
        assert!(!devices_shows_pair_code(true, false));
        assert!(lan_bind_in_use("Address already in use (os error 98)"));
        assert!(lan_bind_in_use("EADDRINUSE"));
        assert!(!lan_bind_in_use("permission denied"));
    }
}
