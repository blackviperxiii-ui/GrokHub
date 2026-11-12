//! Where a mod source's HTTP may go, shared by both shells' dialer
//! (`grokhub_agent::mods_net`): https only, one of the source's own hosts, a
//! public name, redirects followed only to another host of the same source
//! (at most [`HOPS`] times), and bodies read up to a cap. Dialing and the
//! egress guard live with the dialer; this module only decides.

use super::{Reply, Request, SourceError};

/// Largest body read for a source's package list (Thunderstore's is big).
pub const LIST_CAP: u64 = 96 * 1024 * 1024;
/// Largest body read for anything else.
pub const BODY_CAP: u64 = 8 * 1024 * 1024;
/// Largest mod file Loadout downloads (the unpack limit is the same).
pub const DOWNLOAD_CAP: u64 = 2 << 30;
/// Redirects followed at most.
pub const HOPS: usize = 3;
/// Headers the sources answer with that the adapters read.
pub const KEPT: &[&str] = &[
    "retry-after",
    "x-rl-hourly-remaining",
    "x-rl-daily-remaining",
    "location",
];

/// The host of an `https://` URL, lower-case; `None` for anything else.
pub fn https_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') {
        return None;
    }
    let host = authority.split(':').next()?.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Whether a URL may be asked of this source: https, one of its own hosts,
/// and a public name.
pub fn allowed(req: &Request, url: &str) -> bool {
    https_host(url).is_some_and(|h| req.source.hosts().contains(&h.as_str()))
        && crate::public_http_url(url)
}

/// Resolve a redirect's `Location` against the URL it came from.
pub fn redirect_target(from: &str, location: &str) -> Option<String> {
    if location.starts_with("https://") {
        return Some(location.to_string());
    }
    if location.starts_with('/') && !location.starts_with("//") {
        let host = https_host(from)?;
        return Some(format!("https://{host}{location}"));
    }
    None
}

/// The body cap for a URL: a package list may be large.
pub fn cap_for(url: &str) -> u64 {
    if url.contains("/api/v1/package/") {
        LIST_CAP
    } else {
        BODY_CAP
    }
}

/// The refusal for a URL that isn't one of the source's own addresses.
pub fn not_allowed(req: &Request, url: &str) -> SourceError {
    SourceError::Net(format!(
        "{} isn't one of {}'s own addresses",
        https_host(url).unwrap_or_default(),
        req.source.label()
    ))
}

/// Ask `once` for the request's URL, then for each redirect while it stays
/// on the source's own hosts, at most [`HOPS`] times.
pub fn follow(
    req: &Request,
    mut once: impl FnMut(&str) -> Result<Reply, SourceError>,
) -> Result<Reply, SourceError> {
    let mut url = req.url.clone();
    for _ in 0..=HOPS {
        let reply = once(&url)?;
        if !(300..400).contains(&reply.status) {
            return Ok(reply);
        }
        url = reply
            .header("location")
            .and_then(|l| redirect_target(&url, l))
            .filter(|next| allowed(req, next))
            .ok_or_else(|| {
                SourceError::Net(format!(
                    "{} sent Loadout somewhere else, so it stopped",
                    req.source.label()
                ))
            })?;
    }
    Err(SourceError::Net("too many redirects".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::Source;

    #[test]
    fn only_a_sources_own_https_hosts_are_allowed() {
        let req = Request::get(Source::Modrinth, String::new());
        assert!(allowed(&req, "https://api.modrinth.com/v2/search"));
        assert!(allowed(&req, "https://cdn.modrinth.com/data/x/y.jar"));
        assert!(!allowed(&req, "http://api.modrinth.com/v2/search"));
        assert!(!allowed(&req, "https://api.nexusmods.com/v2/graphql"));
        assert!(!allowed(&req, "https://api.modrinth.com.evil.example/x"));
        assert!(!allowed(&req, "https://user@api.modrinth.com/x"));
        let ts = Request::get(Source::Thunderstore, String::new());
        assert!(allowed(&ts, "https://gcdn.thunderstore.io/live/x.zip"));
    }

    #[test]
    fn redirects_resolve_and_stay_on_the_source() {
        assert_eq!(
            redirect_target("https://thunderstore.io/package/download/a/b/1/", "/x/y"),
            Some("https://thunderstore.io/x/y".into())
        );
        assert_eq!(
            redirect_target(
                "https://thunderstore.io/a",
                "https://gcdn.thunderstore.io/z.zip"
            ),
            Some("https://gcdn.thunderstore.io/z.zip".into())
        );
        assert_eq!(
            redirect_target("https://thunderstore.io/a", "//evil.example/x"),
            None
        );
        assert_eq!(
            redirect_target("https://thunderstore.io/a", "http://thunderstore.io/x"),
            None
        );
    }

    #[test]
    fn a_redirect_off_the_source_stops_and_one_on_it_is_followed() {
        let req = Request::get(
            Source::Thunderstore,
            "https://thunderstore.io/package/download/a/b/1/".into(),
        );
        let mut asked = Vec::new();
        let got = follow(&req, |url| {
            asked.push(url.to_string());
            Ok(if url.contains("gcdn") {
                Reply::ok("zip")
            } else {
                Reply {
                    status: 302,
                    headers: vec![(
                        "location".into(),
                        "https://gcdn.thunderstore.io/z.zip".into(),
                    )],
                    ..Reply::default()
                }
            })
        })
        .unwrap();
        assert_eq!(got.status, 200);
        assert_eq!(
            asked,
            vec![
                "https://thunderstore.io/package/download/a/b/1/".to_string(),
                "https://gcdn.thunderstore.io/z.zip".to_string(),
            ]
        );
        let off = follow(&req, |_| {
            Ok(Reply {
                status: 302,
                headers: vec![("location".into(), "https://evil.example/z".into())],
                ..Reply::default()
            })
        });
        assert!(matches!(off, Err(SourceError::Net(why)) if why.contains("somewhere else")));
    }

    #[test]
    fn a_package_list_gets_the_large_cap() {
        assert_eq!(
            cap_for("https://thunderstore.io/c/x/api/v1/package/"),
            LIST_CAP
        );
        assert_eq!(cap_for("https://api.modrinth.com/v2/search"), BODY_CAP);
    }
}
