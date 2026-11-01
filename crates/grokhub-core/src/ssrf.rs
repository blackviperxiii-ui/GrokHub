//! Public-address checks for outbound fetches and the resolver dialers share.
//!
//! Callers attach [`public_only_resolve`] so a DNS answer that lands on a blocked
//! range cannot connect even when the URL hostname looked public.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

/// Official MCP registry host for Marketplace catalog refresh.
pub const MCP_REGISTRY_HOST: &str = "registry.modelcontextprotocol.io";

/// Hosts allowed for the pinned on-device engine download (llama.cpp releases).
/// GitHub serves the asset from `github.com/.../releases/download/...` then
/// redirects to `release-assets.githubusercontent.com` (or the older
/// `objects.githubusercontent.com` CDN).
pub const RUNTIME_DOWNLOAD_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// `true` when `host` may serve a runtime engine download.
pub fn runtime_download_host_ok(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    RUNTIME_DOWNLOAD_HOSTS.iter().any(|h| host == *h)
}

/// Refuse non-HTTPS runtime downloads and hosts outside [`RUNTIME_DOWNLOAD_HOSTS`].
pub fn runtime_download_url_ok(url: &str) -> Result<(), String> {
    crate::offline::refuse()?;
    let url = url.trim();
    if !url.starts_with("https://") {
        return Err("engine download must use HTTPS".into());
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "engine download URL is invalid".to_string())?;
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "engine download URL is missing a host".to_string())?;
    if host_name_blocked(host) {
        return Err(format!("engine download host {host} is not allowed"));
    }
    if !runtime_download_host_ok(host) {
        return Err(format!(
            "engine download host {host} is not on the allowlist"
        ));
    }
    Ok(())
}

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

/// Resolver shared by MCP registry refresh, Pulse, `web_fetch`, and media.
pub fn public_only_resolve(netloc: &str) -> io::Result<Vec<std::net::SocketAddr>> {
    // Offline only: this resolver only ever dials public hosts, so none go.
    if crate::offline::enabled() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            *crate::offline::OFFLINE_MSG,
        ));
    }
    let host = netloc
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(netloc)
        .trim_matches(|c| c == '[' || c == ']');
    if host_name_blocked(host) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refused non-public address {host}"),
        ));
    }
    let all: Vec<std::net::SocketAddr> = netloc.to_socket_addrs()?.collect();
    if all.iter().any(|addr| ip_blocked(addr.ip())) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refused non-public address {host}"),
        ));
    }
    if all.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("dns failed for {host}: no addresses"),
        ));
    }
    Ok(all)
}

/// HTTPS URL on the official MCP registry only (hostname + public-address rail).
pub fn guard_mcp_registry_url(url: &str) -> Result<(), String> {
    crate::offline::refuse()?;
    let url = url.trim();
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err("MCP registry fetch requires https".into());
    }
    let rest = url.get("https://".len()..).unwrap_or("");
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(v6) = hostport.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host != MCP_REGISTRY_HOST {
        return Err("MCP registry host is not allowlisted".into());
    }
    if !crate::public_http_url(url) {
        return Err("MCP registry url is not a public address".into());
    }
    Ok(())
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

    #[test]
    fn runtime_download_urls_are_https_and_allowlisted() {
        let ok = "https://github.com/ggml-org/llama.cpp/releases/download/b11602/llama-b11602-bin-ubuntu-x64.tar.gz";
        assert_eq!(runtime_download_url_ok(ok), Ok(()));
        let cdn = "https://objects.githubusercontent.com/github-production-release-asset-2e65be/123/456?X-Amz=1";
        assert_eq!(runtime_download_url_ok(cdn), Ok(()));
        let assets = "https://release-assets.githubusercontent.com/github-production-release-asset-2e65be/123/456?sp=r&sv=2026";
        assert_eq!(runtime_download_url_ok(assets), Ok(()));
        assert!(runtime_download_url_ok("http://github.com/x").is_err());
        assert!(runtime_download_url_ok("https://evil.com/x").is_err());
        assert!(runtime_download_url_ok("https://localhost/x").is_err());
    }

    #[test]
    fn mcp_registry_url_is_https_and_allowlisted() {
        let ok = format!("https://{MCP_REGISTRY_HOST}/v0.1/servers/x/versions");
        assert!(guard_mcp_registry_url(&ok).is_ok());
        assert_eq!(
            guard_mcp_registry_url("http://registry.modelcontextprotocol.io/v0/servers")
                .unwrap_err(),
            "MCP registry fetch requires https"
        );
        assert_eq!(
            guard_mcp_registry_url("https://evil.example/v0/servers").unwrap_err(),
            "MCP registry host is not allowlisted"
        );
        assert_eq!(
            guard_mcp_registry_url("https://127.0.0.1/v0/servers").unwrap_err(),
            "MCP registry host is not allowlisted"
        );
    }

    #[test]
    fn public_only_resolve_refuses_loopback() {
        assert!(public_only_resolve("localhost:443").is_err());
        assert!(public_only_resolve("127.0.0.1:443").is_err());
        assert!(!public_only_resolve("1.1.1.1:443").unwrap().is_empty());
    }
}
