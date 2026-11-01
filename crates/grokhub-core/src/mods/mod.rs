//! Mod support: which games Loadout may mod ([`allowlist`]), how each
//! sanctioned source names them ([`ids`]), and read-only adapters for Nexus
//! Mods, Steam Workshop, CurseForge, Thunderstore and Modrinth behind one
//! trait ([`source::ModSource`]), with a cache and per-source rate limits
//! ([`client::ModClient`]).
//!
//! Installing ([`install`]) takes a download the app fetched after the
//! user's click, checks it ([`archive`]), and writes it into the game with
//! a journal and backups so uninstall puts the folder back exactly. A game
//! another mod manager looks after ([`managers`]) is left to it.
//!
//! Nothing here dials the network: the app hands in a [`source::Fetch`]
//! that runs every request through the egress guard (refused while Offline
//! only is on, public addresses only, no redirect off the source's own hosts).

pub mod allowlist;
pub mod archive;
pub mod cache;
pub mod client;
pub mod curseforge;
pub mod ids;
pub mod install;
pub mod managers;
pub mod modrinth;
pub mod nexus;
pub mod pickup;
pub mod source;
pub mod suggest;
pub mod thunderstore;
pub mod workshop;

pub use allowlist::{Check, Entry, Mode, Source, Verdict};
pub use client::{Got, ModClient};
pub use ids::GameKey;
pub use source::{
    Browse, Download, Fetch, FileHash, Keys, ModDetail, ModFile, ModSummary, Page, Query, Reply,
    Request, SourceError,
};

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::source::{Fetch, Reply, Request, SourceError};

    /// A fixture under `tests/fixtures/mods/`.
    pub fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mods")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// A fresh config folder for one test.
    pub fn temp_config(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mods-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Serves replies by pattern: `url-part` or `url-part#body-part`, first
    /// match wins. Unmatched requests get a 404. Every request is logged.
    #[derive(Default)]
    pub struct Fake {
        routes: Vec<(String, Reply)>,
        sent: Arc<Mutex<Vec<Request>>>,
    }

    impl Fake {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn on(self, pattern: &str, body: String) -> Self {
            self.reply(pattern, Reply::ok(&body))
        }

        pub fn reply(mut self, pattern: &str, reply: Reply) -> Self {
            self.routes.push((pattern.to_string(), reply));
            self
        }

        pub fn log(&self) -> Arc<Mutex<Vec<Request>>> {
            self.sent.clone()
        }

        pub fn sent(&self) -> Vec<Request> {
            self.sent.lock().unwrap().clone()
        }
    }

    impl Fetch for Fake {
        fn send(&self, req: &Request) -> Result<Reply, SourceError> {
            self.sent.lock().unwrap().push(req.clone());
            for (pattern, reply) in &self.routes {
                let (url, body) = pattern.split_once('#').unwrap_or((pattern, ""));
                let body_ok =
                    body.is_empty() || req.body.as_deref().is_some_and(|b| b.contains(body));
                if req.url.contains(url) && body_ok {
                    return Ok(reply.clone());
                }
            }
            Ok(Reply {
                status: 404,
                ..Reply::default()
            })
        }
    }
}
