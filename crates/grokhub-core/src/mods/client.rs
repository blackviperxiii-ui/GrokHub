//! One place the Mods page and the agent ask for mods: it checks the
//! allowlist, serves from the cache while fresh, keeps each source under its
//! rate, and only then asks the source through [`Fetch`].

use std::path::Path;
use std::sync::Mutex;

use super::allowlist::{self, Source};
use super::cache::{Buckets, Cache, DETAIL_TTL_SECS, LIST_TTL_SECS};
use super::ids::{self, GameKey};
use super::source::*;
use super::{curseforge, modrinth, nexus, thunderstore, workshop};

/// The source adapter for one key.
pub fn adapter(source: Source, keys: &Keys) -> Box<dyn ModSource> {
    match source {
        Source::Nexus => Box::new(nexus::Nexus {
            key: keys.nexus.clone(),
        }),
        Source::Workshop => Box::new(workshop::Workshop {
            key: keys.steam.clone(),
        }),
        Source::CurseForge => Box::new(curseforge::CurseForge {
            key: keys.curseforge.clone(),
        }),
        Source::Thunderstore => Box::new(thunderstore::Thunderstore),
        Source::Modrinth => Box::new(modrinth::Modrinth),
    }
}

/// Whether a source can serve a game's search right now, and if not, why.
pub fn search_ready(source: Source, keys: &Keys) -> Result<(), SourceError> {
    match source {
        Source::CurseForge if keys.curseforge.is_none() => Err(SourceError::NeedsKey(source)),
        Source::Workshop if keys.steam.is_none() => Err(SourceError::NeedsKey(source)),
        _ => Ok(()),
    }
}

/// A result and whether it came from the network just now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Got<T> {
    pub value: T,
    pub fresh: bool,
}

pub struct ModClient {
    net: Box<dyn Fetch>,
    cache: Cache,
    keys: Keys,
    buckets: Mutex<Buckets>,
}

impl ModClient {
    pub fn new(net: Box<dyn Fetch>, config_dir: &Path, keys: Keys) -> Self {
        Self {
            net,
            cache: Cache::new(config_dir),
            keys,
            buckets: Mutex::new(Buckets::default()),
        }
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    /// The key for one game on one source, only when the allowlist allows it.
    fn key(game_id: &str, source: Source) -> Result<GameKey, SourceError> {
        match allowlist::check(game_id) {
            allowlist::Check::Blocked(why) => Err(SourceError::Unsupported(why)),
            allowlist::Check::Allowed(_) => ids::key(game_id, source).ok_or_else(|| {
                SourceError::Unsupported(format!(
                    "{} isn't a source Loadout uses for this game.",
                    source.label()
                ))
            }),
        }
    }

    fn take(&self, source: Source, now_ms: u64) -> Result<(), SourceError> {
        self.buckets
            .lock()
            .map_err(|_| SourceError::Net("busy".into()))?
            .take(source, now_ms)
            .map_err(SourceError::RateLimited)
    }

    fn note_limit(&self, source: Source, err: &SourceError, now_ms: u64) {
        if let SourceError::RateLimited(secs) = err {
            if let Ok(mut b) = self.buckets.lock() {
                b.pause(source, now_ms + secs * 1000, now_ms);
            }
        }
    }

    fn cached_or<T, F>(
        &self,
        key: &str,
        ttl: u64,
        source: Source,
        now_ms: u64,
        ask: F,
    ) -> Result<Got<T>, SourceError>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
        F: FnOnce() -> Result<T, SourceError>,
    {
        let now = now_ms / 1000;
        if let Some(value) = self.cache.get::<T>(key, ttl, now) {
            return Ok(Got {
                value,
                fresh: false,
            });
        }
        self.take(source, now_ms)?;
        match ask() {
            Ok(value) => {
                let _ = self.cache.put(key, &value, now);
                Ok(Got { value, fresh: true })
            }
            Err(e) => {
                self.note_limit(source, &e, now_ms);
                Err(e)
            }
        }
    }

    fn query_key(game: GameKey, q: &Query) -> String {
        format!(
            "search|{}|{}|{:?}|{}|{}",
            game.source().id(),
            game.slug(),
            q.browse,
            q.page,
            q.text().unwrap_or_default().to_lowercase()
        )
    }

    /// A page from the cache however old, so the page shows something at once.
    pub fn cached_search(&self, game_id: &str, source: Source, q: &Query) -> Option<Page> {
        let game = Self::key(game_id, source).ok()?;
        if source == Source::Thunderstore {
            let (list, _) = self
                .cache
                .get_any::<Vec<thunderstore::Package>>(&format!("ts-list|{}", game.slug()))?;
            return Some(thunderstore::search_list(&list, q));
        }
        self.cache
            .get_any(&Self::query_key(game, q))
            .map(|(p, _)| p)
    }

    pub fn search(
        &self,
        game_id: &str,
        source: Source,
        q: &Query,
        now_ms: u64,
    ) -> Result<Got<Page>, SourceError> {
        let game = Self::key(game_id, source)?;
        search_ready(source, &self.keys).or_else(|e| match source {
            // Workshop search without a key says where to go instead.
            Source::Workshop => workshop::search_request(game, q, None).map(|_| ()),
            _ => Err(e),
        })?;
        if source == Source::Thunderstore {
            let list = self.cached_or(
                &format!("ts-list|{}", game.slug()),
                LIST_TTL_SECS,
                source,
                now_ms,
                || {
                    thunderstore::parse_list(&ok_body(
                        self.net.send(&thunderstore::list_request(game)?)?,
                    )?)
                },
            )?;
            return Ok(Got {
                value: thunderstore::search_list(&list.value, q),
                fresh: list.fresh,
            });
        }
        let src = adapter(source, &self.keys);
        self.cached_or(
            &Self::query_key(game, q),
            LIST_TTL_SECS,
            source,
            now_ms,
            || src.search(self.net.as_ref(), game, q),
        )
    }

    pub fn detail(
        &self,
        game_id: &str,
        source: Source,
        id: &str,
        now_ms: u64,
    ) -> Result<Got<ModDetail>, SourceError> {
        let game = Self::key(game_id, source)?;
        let src = adapter(source, &self.keys);
        let key = format!("detail|{}|{}|{id}", source.id(), game.slug());
        self.cached_or(&key, DETAIL_TTL_SECS, source, now_ms, || {
            src.detail(self.net.as_ref(), game, id)
        })
    }

    pub fn files(
        &self,
        game_id: &str,
        source: Source,
        id: &str,
        now_ms: u64,
    ) -> Result<Got<Vec<ModFile>>, SourceError> {
        let game = Self::key(game_id, source)?;
        let src = adapter(source, &self.keys);
        let key = format!("files|{}|{}|{id}", source.id(), game.slug());
        self.cached_or(&key, DETAIL_TTL_SECS, source, now_ms, || {
            src.files(self.net.as_ref(), game, id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::testing::{fixture, temp_config, Fake};

    #[test]
    fn a_blocked_game_never_reaches_the_network() {
        let fake = Fake::new();
        let sent = fake.log();
        let c = ModClient::new(Box::new(fake), &temp_config("blocked"), Keys::default());
        let err = c
            .search("apex", Source::Nexus, &Query::browse(Browse::Popular), 0)
            .unwrap_err();
        assert_eq!(
            err.line(),
            "Loadout doesn't mod Apex Legends: Easy Anti-Cheat. Mods there can get accounts banned."
        );
        assert!(c
            .search("unknown", Source::Nexus, &Query::browse(Browse::Popular), 0)
            .is_err());
        let err = c
            .search(
                "skyrimse",
                Source::Modrinth,
                &Query::browse(Browse::Popular),
                0,
            )
            .unwrap_err();
        assert_eq!(
            err.line(),
            "Modrinth isn't a source Loadout uses for this game."
        );
        assert!(sent.lock().unwrap().is_empty());
    }

    #[test]
    fn a_fresh_page_is_served_from_the_cache() {
        let fake = Fake::new().on("/v2/search", fixture("modrinth_search.json"));
        let sent = fake.log();
        let dir = temp_config("cache");
        let c = ModClient::new(Box::new(fake), &dir, Keys::default());
        let q = Query::browse(Browse::Popular);
        let first = c
            .search("minecraft", Source::Modrinth, &q, 1_000_000)
            .unwrap();
        assert!(first.fresh);
        let again = c
            .search("minecraft", Source::Modrinth, &q, 2_000_000)
            .unwrap();
        assert!(!again.fresh);
        assert_eq!(again.value, first.value);
        assert_eq!(sent.lock().unwrap().len(), 1);
        let later = 1_000_000 + (LIST_TTL_SECS + 1) * 1000;
        assert!(
            c.search("minecraft", Source::Modrinth, &q, later)
                .unwrap()
                .fresh
        );
        assert_eq!(sent.lock().unwrap().len(), 2);
        assert_eq!(
            c.cached_search("minecraft", Source::Modrinth, &q),
            Some(first.value)
        );
    }

    #[test]
    fn the_bucket_holds_back_a_burst() {
        let fake = Fake::new().on("/v2/search", fixture("modrinth_search.json"));
        let c = ModClient::new(Box::new(fake), &temp_config("burst"), Keys::default());
        let mut limited = 0;
        for page in 0..12 {
            let mut q = Query::browse(Browse::Updated);
            q.page = page;
            if let Err(SourceError::RateLimited(_)) = c.search("minecraft", Source::Modrinth, &q, 0)
            {
                limited += 1;
            }
        }
        assert_eq!(limited, 4, "8 at once, then wait");
    }

    #[test]
    fn keyed_sources_say_they_need_a_key() {
        let c = ModClient::new(Box::new(Fake::new()), &temp_config("keys"), Keys::default());
        assert_eq!(
            c.search("wow", Source::CurseForge, &Query::search("details"), 0),
            Err(SourceError::NeedsKey(Source::CurseForge))
        );
        let err = c
            .search(
                "rimworld",
                Source::Workshop,
                &Query::browse(Browse::Popular),
                0,
            )
            .unwrap_err();
        assert!(
            err.line().contains("steam://url/SteamWorkshopPage/294100"),
            "{}",
            err.line()
        );
    }

    #[test]
    fn thunderstore_keeps_one_list_and_searches_it() {
        let fake = Fake::new().on(
            "/c/lethal-company/api/v1/package/",
            fixture("thunderstore_list.json"),
        );
        let sent = fake.log();
        let c = ModClient::new(Box::new(fake), &temp_config("ts"), Keys::default());
        let a = c
            .search(
                "lethalcompany",
                Source::Thunderstore,
                &Query::browse(Browse::Popular),
                0,
            )
            .unwrap();
        assert_eq!(a.value.mods[0].id, "BepInEx-BepInExPack");
        let b = c
            .search(
                "lethalcompany",
                Source::Thunderstore,
                &Query::search("suits"),
                1000,
            )
            .unwrap();
        assert_eq!(b.value.mods[0].id, "x753-More_Suits");
        assert!(!b.fresh);
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert!(c
            .cached_search(
                "lethalcompany",
                Source::Thunderstore,
                &Query::search("bepinex")
            )
            .is_some());
    }

    #[test]
    fn a_rate_limited_answer_pauses_the_source() {
        let mut limited = Reply::ok("");
        limited.status = 429;
        limited.headers.push(("retry-after".into(), "30".into()));
        let fake = Fake::new().reply("/v2/search", limited);
        let sent = fake.log();
        let c = ModClient::new(Box::new(fake), &temp_config("429"), Keys::default());
        let q = Query::browse(Browse::Popular);
        assert_eq!(
            c.search("minecraft", Source::Modrinth, &q, 0),
            Err(SourceError::RateLimited(30))
        );
        let mut q2 = q.clone();
        q2.page = 1;
        assert!(matches!(
            c.search("minecraft", Source::Modrinth, &q2, 10_000),
            Err(SourceError::RateLimited(_))
        ));
        assert_eq!(sent.lock().unwrap().len(), 1, "no request while paused");
    }
}
