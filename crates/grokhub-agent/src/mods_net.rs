//! The only HTTP the mod sources use, for both shells. Each request must be
//! https, to one of its source's own hosts, to a public address, and pass the
//! egress guard (which refuses everything while Offline only is on). A
//! redirect is followed only to another host of the same source, at most
//! three times ([`grokhub_core::mods::fetch`] decides where a request may go).
//! Bodies are read up to a cap.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_core::mods::fetch::{self, cap_for, DOWNLOAD_CAP, KEPT, LIST_CAP};
use grokhub_core::mods::{Fetch, Keys, Reply, Request, Source, SourceError};

use crate::harness::{self as hx, DataClass};

/// What a mod request sends. A search carries typed text: it is checked for
/// recalled memory lines, so a personal line in a query is classed personal.
fn sent_classes(req: &Request, url: &str) -> &'static [DataClass] {
    if !req.carries_text {
        return &[];
    }
    let sent = match &req.body {
        Some(body) => format!("{url}\n{body}"),
        None => url.to_string(),
    };
    hx::model_text_classes(&sent)
}

/// The CurseForge key Loadout ships with, when the build has one.
fn shipped_curseforge_key() -> Option<String> {
    option_env!("CURSEFORGE_API_KEY")
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_owned)
}

/// The keys the sources may use: the user's sealed Nexus and Steam keys and
/// the shipped CurseForge key.
pub fn mod_keys(config_dir: &Path) -> Keys {
    Keys {
        nexus: hx::open_mod_key(config_dir, Source::Nexus.id()),
        steam: hx::open_mod_key(config_dir, Source::Workshop.id()),
        curseforge: shipped_curseforge_key(),
    }
}

/// Dials the mod sources. `live` is false in a shell's own tests, so a test
/// build never reaches a source.
pub struct ModsNet {
    config_dir: PathBuf,
    live: bool,
}

impl ModsNet {
    /// The egress log and its decisions go under `config_dir`. Pass
    /// `!cfg!(test)` for `live` from the shell that builds it.
    pub fn new(config_dir: PathBuf, live: bool) -> Self {
        Self { config_dir, live }
    }

    fn agent(url: &str, to_file: bool) -> grokhub_core::http::Agent {
        let overall = if to_file {
            1800
        } else if cap_for(url) == LIST_CAP {
            180
        } else {
            25
        };
        grokhub_core::http::agent(grokhub_core::http::Opts {
            connect: Duration::from_secs(15),
            overall: Some(Duration::from_secs(overall)),
            // Redirects come back as answers and are checked here.
            redirects: 0,
            public_only: true,
            ..Default::default()
        })
    }

    /// One request. With `sink`, a 2xx body is written there instead of kept.
    fn once(&self, req: &Request, url: &str, sink: Option<&Path>) -> Result<Reply, SourceError> {
        if !self.live || cfg!(test) || !fetch::allowed(req, url) {
            return Err(fetch::not_allowed(req, url));
        }
        grokhub_core::offline::refuse().map_err(|_| SourceError::Offline)?;
        let data = sent_classes(req, url);
        hx::guard_quiet(&self.config_dir, &hx::EgressReq::new(url, data)).map_err(|why| {
            if grokhub_core::offline::is_refusal(&why) {
                SourceError::Offline
            } else {
                SourceError::Net(why)
            }
        })?;
        let agent = Self::agent(url, sink.is_some());
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
        if let Some(path) = sink.filter(|_| (200..300).contains(&status)) {
            let net = |e: std::io::Error| SourceError::Net(e.to_string());
            let mut out = std::fs::File::create(path).map_err(net)?;
            let n = std::io::copy(&mut resp.into_reader().take(DOWNLOAD_CAP + 1), &mut out)
                .map_err(net)?;
            if n > DOWNLOAD_CAP {
                let _ = std::fs::remove_file(path);
                return Err(SourceError::Net("the file is too large".into()));
            }
            return Ok(Reply {
                status,
                body: Vec::new(),
                headers,
            });
        }
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

    /// Download one mod file to `dest`, after the user's click. Same rules
    /// as every other request: the source's own hosts, the egress guard,
    /// redirects only within the source.
    pub fn download(&self, req: &Request, dest: &Path) -> Result<(), SourceError> {
        let reply = fetch::follow(req, |url| self.once(req, url, Some(dest)))?;
        if (200..300).contains(&reply.status) {
            Ok(())
        } else {
            Err(SourceError::Http(reply.status))
        }
    }
}

impl Fetch for ModsNet {
    fn send(&self, req: &Request) -> Result<Reply, SourceError> {
        fetch::follow(req, |url| self.once(req, url, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mod_search_carrying_a_recalled_line_is_classed_personal() {
        use crate::harness::RecallScope;
        let line = "my sister lives on maple street";
        let _recall = RecallScope::enter(vec![line.to_string()]);
        let url = format!(
            "https://api.modrinth.com/v2/search?query={}",
            line.replace(' ', "%20")
        );
        let search = Request::get(Source::Modrinth, url.clone()).with_text(true);
        assert!(sent_classes(&search, &url).contains(&DataClass::Personal));
        let plain = "https://api.modrinth.com/v2/search?query=shaders";
        let other = Request::get(Source::Modrinth, plain.into()).with_text(true);
        assert_eq!(sent_classes(&other, plain), &[DataClass::Chat]);
        let listing = Request::get(Source::Modrinth, url.clone());
        assert!(sent_classes(&listing, &url).is_empty());
    }

    #[test]
    fn tests_never_dial() {
        let req = Request::get(
            Source::Modrinth,
            "https://api.modrinth.com/v2/search".into(),
        );
        let dir = std::env::temp_dir().join(format!("mods-net-{}", std::process::id()));
        for live in [true, false] {
            let net = ModsNet::new(dir.clone(), live);
            assert!(matches!(net.send(&req), Err(SourceError::Net(_))));
            let dest = dir.join(format!("never-{live}"));
            assert!(net.download(&req, &dest).is_err());
            assert!(!dest.exists());
        }
    }
}
