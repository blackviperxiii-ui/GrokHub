//! Public-address checks for outbound fetches. Pure classification only — no HTTP.
//!
//! Callers that dial (Pulse, `web_fetch`, media) also attach
//! `grokhub_agent::public_only_resolve` so a DNS answer that lands on a blocked
//! range cannot connect even when the URL hostname looked public.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// True when a hostname is loopback-by-name (no DNS).
pub fn host_name_blocked(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host == "localhost.localdomain" || host.ends_with(".localhost")
}

/// True when an IP must not be fetched (loopback, private, CGNAT, link-local, …).
pub fn ip_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_blocked(v4),
        IpAddr::V6(v6) => ipv6_blocked(v6),
    }
}

fn ipv4_blocked(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        // 0.0.0.0/8 "this network"
        || a == 0
        // 100.64.0.0/10 carrier-grade NAT (Tailscale and similar overlays)
        || (a == 100 && (64..=127).contains(&b))
        // 192.0.0.0/24 IETF protocol assignments
        || (a == 192 && b == 0 && c == 0)
        // 198.18.0.0/15 benchmarking
        || (a == 198 && (b == 18 || b == 19))
        // 240.0.0.0/4 reserved
        || a >= 240
}

fn ipv6_blocked(v6: Ipv6Addr) -> bool {
    if let Some(v4) = v6.to_ipv4_mapped() {
        return ipv4_blocked(v4);
    }
    let seg = v6.segments();
    // IPv4-compatible (::a.b.c.d), NAT64 (64:ff9b::/96) and 6to4 (2002::/16)
    // carry an IPv4 address that must pass the same check.
    if seg[..6] == [0, 0, 0, 0, 0, 0] && !v6.is_loopback() && !v6.is_unspecified() {
        return ipv4_blocked(Ipv4Addr::new(
            (seg[6] >> 8) as u8,
            seg[6] as u8,
            (seg[7] >> 8) as u8,
            seg[7] as u8,
        ));
    }
    if seg[0] == 0x64 && seg[1] == 0xff9b && seg[2..6] == [0, 0, 0, 0] {
        return ipv4_blocked(Ipv4Addr::new(
            (seg[6] >> 8) as u8,
            seg[6] as u8,
            (seg[7] >> 8) as u8,
            seg[7] as u8,
        ));
    }
    if seg[0] == 0x2002 {
        return ipv4_blocked(Ipv4Addr::new(
            (seg[1] >> 8) as u8,
            seg[1] as u8,
            (seg[2] >> 8) as u8,
            seg[2] as u8,
        ));
    }
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        || v6.is_unique_local()
        || v6.is_unicast_link_local()
        // fec0::/10 site-local (deprecated, still routed on some LANs)
        || (seg[0] & 0xffc0) == 0xfec0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn blocked_ranges_cover_private_cgnat_and_mapped() {
        assert!(ip_blocked(IpAddr::from([127, 0, 0, 2])));
        assert!(ip_blocked(IpAddr::from([10, 0, 0, 1])));
        assert!(ip_blocked(IpAddr::from([192, 168, 0, 1])));
        assert!(ip_blocked(IpAddr::from([172, 16, 5, 1])));
        assert!(ip_blocked(IpAddr::from([169, 254, 169, 254])));
        assert!(ip_blocked(IpAddr::from([100, 64, 0, 1])));
        assert!(ip_blocked(IpAddr::from([100, 101, 7, 9])));
        assert!(!ip_blocked(IpAddr::from([100, 128, 0, 1])));
        assert!(ip_blocked(IpAddr::from([0, 1, 2, 3])));
        assert!(ip_blocked(IpAddr::from([192, 0, 0, 8])));
        assert!(ip_blocked(IpAddr::from([198, 18, 0, 1])));
        assert!(ip_blocked(IpAddr::from([240, 0, 0, 1])));
        assert!(!ip_blocked(IpAddr::from([203, 0, 113, 5])));
        assert!(!ip_blocked(IpAddr::from([1, 1, 1, 1])));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(ip_blocked(IpAddr::V6(
            Ipv4Addr::new(127, 0, 0, 1).to_ipv6_mapped()
        )));
        assert!(!ip_blocked(IpAddr::V6(
            Ipv4Addr::new(1, 1, 1, 1).to_ipv6_mapped()
        )));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0x7f00, 1
        ))));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0x64, 0xff9b, 0, 0, 0, 0, 0xc0a8, 0x0101
        ))));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0x2002, 0xc0a8, 0x0101, 0, 0, 0, 0, 1
        ))));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0xfec0, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(!ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0x2002, 0x0101, 0x0101, 0, 0, 0, 0, 1
        ))));
    }

    #[test]
    fn localhost_names_are_blocked() {
        assert!(host_name_blocked("localhost"));
        assert!(host_name_blocked("LOCALHOST."));
        assert!(host_name_blocked("foo.localhost"));
        assert!(!host_name_blocked("example.com"));
    }
}
