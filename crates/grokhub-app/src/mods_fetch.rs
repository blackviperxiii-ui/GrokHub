//! The only HTTP the mod sources use. Each request must be https, to one of
//! its source's own hosts, to a public address, and pass the egress guard
//! (which refuses everything while Offline only is on). A redirect is
//! followed only to another host of the same source, at most three times.
//! Bodies are read up to a cap.

use std::io::Read;
use std::time::Duration;

use grokhub_core::mods::{Fetch, Reply, Request, SourceError};

/// Largest body read for a source's package list (Thunderstore's is big).
const LIST_CAP: u64 = 96 * 1024 * 1024;
/// Largest body read for anything else.
const BODY_CAP: u64 = 8 * 1024 * 1024;
const HOPS: usize = 3;
/// Headers the sources answer with that the adapters read.
const KEPT: &[&str] = &[
    "retry-after",
    "x-rl-hourly-remaining",
    "x-rl-daily-remaining",
    "location",
];

/// The host of an `https://` URL, lower-case; `None` for anything else.
pub(crate) fn https_host(url: &str) -> Option<String> {
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
pub(crate) fn allowed(req: &Request, url: &str) -> bool {
    https_host(url).is_some_and(|h| req.source.hosts().contains(&h.as_str()))
        && grokhub_core::public_http_url(url)
}

/// Resolve a redirect's `Location` against the URL it came from.
pub(crate) fn redirect_target(from: &str, location: &str) -> Option<String> {
    if location.starts_with("https://") {
        return Some(location.to_string());
    }
    if location.starts_with('/') && !location.starts_with("//") {
        let host = https_host(from)?;
        return Some(format!("https://{host}{location}"));
    }
    None
}

fn cap_for(url: &str) -> u64 {
    if url.contains("/api/v1/package/") {
        LIST_CAP
    } else {
        BODY_CAP
    }
}

pub(crate) struct ModsNet;

impl ModsNet {
    fn agent(url: &str) -> grokhub_core::http::Agent {
        let overall = if cap_for(url) == LIST_CAP { 180 } else { 25 };
        grokhub_core::http::agent(grokhub_core::http::Opts {
            connect: Duration::from_secs(15),
            overall: Some(Duration::from_secs(overall)),
            // Redirects come back as answers and are checked here.
            redirects: 0,
            public_only: true,
            ..Default::default()
        })
    }

    fn once(req: &Request, url: &str) -> Result<Reply, SourceError> {
        if cfg!(test) || !allowed(req, url) {
            return Err(SourceError::Net(format!(
                "{} isn't one of {}'s own addresses",
                https_host(url).unwrap_or_default(),
                req.source.label()
            )));
        }
        grokhub_core::offline::refuse().map_err(|_| SourceError::Offline)?;
        let data: &[grokhub_agent::harness::DataClass] = if req.carries_text {
            &[grokhub_agent::harness::DataClass::Chat]
        } else {
            &[]
        };
        crate::xai::egress_ok(url, data).map_err(|why| {
            if grokhub_core::offline::is_refusal(&why) {
                SourceError::Offline
            } else {
                SourceError::Net(why)
            }
        })?;
        let agent = Self::agent(url);
        let mut call = match &req.body {
            Some(_) => agent.post(url),
            None => agent.get(url),
        };
        for (name, value) in &req.headers {
            call = call.header(name, value);
        }
        let sent = match &req.body {
            Some(body) => call.send_string(body),
            None => call.call(),
        };
        let resp = match sent {
            Ok(resp) => resp,
            Err(grokhub_core::http::Error::Status {
                code, retry_after, ..
            }) => {
                let mut reply = Reply {
                    status: code,
                    ..Reply::default()
                };
                if let Some(r) = retry_after {
                    reply.headers.push(("retry-after".into(), r));
                }
                return Ok(reply);
            }
            Err(e) => return Err(SourceError::Net(e.to_string())),
        };
        let status = resp.status();
        let headers = KEPT
            .iter()
            .filter_map(|n| resp.header(n).map(|v| (n.to_string(), v.to_string())))
            .collect();
        let cap = cap_for(url);
        let mut body = Vec::new();
        resp.into_reader()
            .take(cap + 1)
            .read_to_end(&mut body)
            .map_err(|e| SourceError::Net(e.to_string()))?;
        if body.len() as u64 > cap {
            return Err(SourceError::Net("the answer was too large".into()));
        }
        Ok(Reply {
            status,
            body,
            headers,
        })
    }
}

impl Fetch for ModsNet {
    fn send(&self, req: &Request) -> Result<Reply, SourceError> {
        let mut url = req.url.clone();
        for _ in 0..=HOPS {
            let reply = Self::once(req, &url)?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::mods::Source;

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
    fn tests_never_dial() {
        let req = Request::get(
            Source::Modrinth,
            "https://api.modrinth.com/v2/search".into(),
        );
        assert!(matches!(ModsNet.send(&req), Err(SourceError::Net(_))));
    }
}
