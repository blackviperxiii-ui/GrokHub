use getrandom::getrandom;

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
    let _ = getrandom(&mut bytes);
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
    candidates.iter().find_map(|a| {
        let ip: std::net::Ipv4Addr = a.parse().ok()?;
        if ip.is_loopback() || ip.is_unspecified() || ip.is_link_local() || ip.is_multicast() {
            None
        } else {
            Some(ip.to_string())
        }
    })
}

pub fn hub_pair_url(port: u16, lan_ip: Option<&str>) -> String {
    match lan_ip.filter(|s| !s.is_empty()) {
        Some(ip) => format!("http://{ip}:{port}"),
        None => format!("http://127.0.0.1:{port}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_url_uses_lan_ip_not_placeholder() {
        assert_eq!(
            pick_lan_ipv4(&["127.0.0.1", "192.168.1.40", "10.0.0.8"]),
            Some("192.168.1.40".into())
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
}
