//! How each source names a game: the Nexus domain and game id, the
//! CurseForge game id, the Thunderstore community, the Modrinth project type
//! and the Steam Workshop app id. Keyed by the allowlist id, so a game only
//! reaches a source the allowlist names for it.

use super::allowlist::{self, Source};

/// One game as one source knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameKey {
    Nexus { domain: &'static str, game_id: u32 },
    CurseForge(u32),
    Thunderstore(&'static str),
    Modrinth(&'static str),
    Workshop(u32),
}

impl GameKey {
    pub fn source(self) -> Source {
        match self {
            GameKey::Nexus { .. } => Source::Nexus,
            GameKey::CurseForge(_) => Source::CurseForge,
            GameKey::Thunderstore(_) => Source::Thunderstore,
            GameKey::Modrinth(_) => Source::Modrinth,
            GameKey::Workshop(_) => Source::Workshop,
        }
    }

    /// A short stable string for cache keys.
    pub fn slug(self) -> String {
        match self {
            GameKey::Nexus { domain, .. } => domain.to_string(),
            GameKey::CurseForge(id) | GameKey::Workshop(id) => id.to_string(),
            GameKey::Thunderstore(c) | GameKey::Modrinth(c) => c.to_string(),
        }
    }
}

/// `(allowlist id, key)` for every source a game maps to.
const KEYS: &[(&str, GameKey)] = &[
    (
        "skyrimse",
        GameKey::Nexus {
            domain: "skyrimspecialedition",
            game_id: 1704,
        },
    ),
    (
        "fallout4",
        GameKey::Nexus {
            domain: "fallout4",
            game_id: 1151,
        },
    ),
    (
        "starfield",
        GameKey::Nexus {
            domain: "starfield",
            game_id: 4187,
        },
    ),
    (
        "cyberpunk2077",
        GameKey::Nexus {
            domain: "cyberpunk2077",
            game_id: 3333,
        },
    ),
    (
        "bg3",
        GameKey::Nexus {
            domain: "baldursgate3",
            game_id: 3474,
        },
    ),
    (
        "witcher3",
        GameKey::Nexus {
            domain: "witcher3",
            game_id: 952,
        },
    ),
    (
        "stardew",
        GameKey::Nexus {
            domain: "stardewvalley",
            game_id: 1303,
        },
    ),
    ("wow", GameKey::CurseForge(1)),
    ("minecraft", GameKey::Modrinth("mod")),
    ("minecraft", GameKey::CurseForge(432)),
    ("valheim", GameKey::Thunderstore("valheim")),
    ("lethalcompany", GameKey::Thunderstore("lethal-company")),
    ("ror2", GameKey::Thunderstore("riskofrain2")),
    ("repo", GameKey::Thunderstore("repo")),
    ("rimworld", GameKey::Workshop(294100)),
    ("cities", GameKey::Workshop(255710)),
    ("stellaris", GameKey::Workshop(281990)),
    ("tmodloader", GameKey::Workshop(1281930)),
];

/// The key one source uses for a game, only when the allowlist lets that
/// game use that source.
pub fn key(game_id: &str, source: Source) -> Option<GameKey> {
    let entry = allowlist::by_id(game_id)?;
    if !entry.moddable() || !entry.sources.contains(&source) {
        return None;
    }
    KEYS.iter()
        .find(|(id, k)| *id == game_id && k.source() == source)
        .map(|(_, k)| *k)
}

/// Every key a game may use, in the allowlist's source order.
pub fn keys(game_id: &str) -> Vec<GameKey> {
    allowlist::by_id(game_id)
        .map(|e| e.sources.iter().filter_map(|s| key(game_id, *s)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_allowed_source_has_a_key_and_no_blocked_game_has_one() {
        for e in allowlist::ALLOWLIST {
            for s in e.sources {
                assert!(key(e.id, *s).is_some(), "{} has no {} key", e.id, s.id());
            }
            if !e.moddable() {
                assert!(keys(e.id).is_empty(), "{} is Blocked", e.id);
            }
        }
        for (id, k) in KEYS {
            let e = allowlist::by_id(id).unwrap_or_else(|| panic!("{id} not allowlisted"));
            assert!(e.sources.contains(&k.source()), "{id}: {:?}", k.source());
        }
    }

    #[test]
    fn keys_are_unique_per_source() {
        let mut seen = std::collections::HashSet::new();
        for (id, k) in KEYS {
            assert!(seen.insert((k.source(), k.slug())), "{id}: duplicate key");
        }
    }

    #[test]
    fn a_game_only_reaches_the_sources_the_allowlist_names() {
        assert_eq!(
            key("skyrimse", Source::Nexus),
            Some(GameKey::Nexus {
                domain: "skyrimspecialedition",
                game_id: 1704
            })
        );
        assert_eq!(key("skyrimse", Source::Modrinth), None);
        assert_eq!(key("apex", Source::Nexus), None);
        assert_eq!(
            keys("minecraft"),
            vec![GameKey::Modrinth("mod"), GameKey::CurseForge(432)]
        );
        assert_eq!(
            GameKey::Thunderstore("lethal-company").slug(),
            "lethal-company"
        );
    }
}
