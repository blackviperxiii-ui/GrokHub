//! Which games Loadout can read, and through which door.
//!
//! Every integration is read-only and goes through something the game's
//! maker built for it: a file the game itself writes, a local API the maker
//! documents, or an addon or mod the game's own sandboxed addon or mod
//! system loads. None reads game memory, injects, hooks, attaches to the
//! process or sends the game input.
//!
//! A game with kernel-level anti-cheat gets no addon or mod from Loadout,
//! only files it writes and its maker's local API, so nothing Loadout adds
//! runs alongside that anti-cheat. The table's own test holds every entry
//! to that.

/// How a game is protected against cheats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    /// No client anti-cheat, or checks only on the game's servers.
    None,
    /// A client anti-cheat that runs in user mode.
    UserMode(&'static str),
    /// A kernel-level anti-cheat driver.
    Kernel(&'static str),
}

/// One read-only way in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// A file the game itself writes, such as a log or saved data.
    GameFile(&'static str),
    /// A local, read-only API the game's maker documents.
    OfficialLocalApi(&'static str),
    /// Loadout's own addon or mod, loaded by the game's official addon or
    /// mod system, which only writes a file Loadout reads.
    OfficialAddon(&'static str),
    /// The game maker's own read-only web API, with a key the user makes.
    /// Nothing runs on the user's computer beside the game.
    OfficialWebApi(&'static str),
}

impl Channel {
    pub fn describe(self) -> &'static str {
        match self {
            Channel::GameFile(s)
            | Channel::OfficialLocalApi(s)
            | Channel::OfficialAddon(s)
            | Channel::OfficialWebApi(s) => s,
        }
    }

    pub fn is_addon(self) -> bool {
        matches!(self, Channel::OfficialAddon(_))
    }
}

/// Whether Loadout reads a game today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Built,
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Integration {
    pub id: &'static str,
    pub name: &'static str,
    pub steam_app_id: Option<u32>,
    pub protection: Protection,
    pub channels: &'static [Channel],
    pub status: Status,
}

impl Integration {
    /// Kernel anti-cheat games take no addon or mod from Loadout.
    pub fn follows_the_rules(&self) -> bool {
        !self.channels.is_empty()
            && !(matches!(self.protection, Protection::Kernel(_))
                && self.channels.iter().any(|c| c.is_addon()))
    }

    /// One line for Settings: how Loadout reads it.
    pub fn how(&self) -> String {
        let ways: Vec<&str> = self.channels.iter().map(|c| c.describe()).collect();
        let mut line = ways.join("; ");
        match self.protection {
            Protection::Kernel(ac) => {
                line.push_str(&format!(". Has {ac}, so Loadout adds nothing to the game"))
            }
            Protection::UserMode(ac) => line.push_str(&format!(". Read-only alongside {ac}")),
            Protection::None => {}
        }
        if self.status == Status::Planned {
            line.push_str(". Planned");
        }
        line
    }
}

/// Every game, built first.
pub const ALL: &[Integration] = &[
    Integration {
        id: "wow",
        name: "World of Warcraft",
        steam_app_id: None,
        protection: Protection::UserMode("Warden"),
        channels: &[
            Channel::OfficialAddon(
                "Loadout's WoW addon saves your characters, gold, item level and boss kills through Blizzard's addon API",
            ),
            Channel::GameFile("WoW's own combat log after /combatlog"),
        ],
        status: Status::Built,
    },
    Integration {
        id: "cs2",
        name: "Counter-Strike 2",
        steam_app_id: Some(730),
        protection: Protection::UserMode("VAC"),
        channels: &[Channel::OfficialLocalApi(
            "Valve's Game State Integration feed",
        )],
        status: Status::Built,
    },
    Integration {
        id: "elite",
        name: "Elite Dangerous",
        steam_app_id: Some(359320),
        protection: Protection::None,
        channels: &[Channel::GameFile(
            "the game's own Player Journal: jumps, dockings, earnings and deaths, with a session recap in Pulse",
        )],
        status: Status::Built,
    },
    Integration {
        id: "gw2",
        name: "Guild Wars 2",
        steam_app_id: Some(1284210),
        protection: Protection::None,
        channels: &[Channel::OfficialWebApi(
            "ArenaNet's official API with your read-only key; nothing added to the game",
        )],
        status: Status::Built,
    },
    Integration {
        id: "stardew",
        name: "Stardew Valley",
        steam_app_id: Some(413150),
        protection: Protection::None,
        channels: &[Channel::GameFile(
            "the farm saves the game writes: day, gold earned and skills, with a session recap in Pulse",
        )],
        status: Status::Built,
    },
    Integration {
        id: "destiny2",
        name: "Destiny 2",
        steam_app_id: Some(1085660),
        protection: Protection::Kernel("BattlEye"),
        channels: &[Channel::OfficialWebApi(
            "Bungie.net's official API with your own key, public profile only",
        )],
        status: Status::Built,
    },
    Integration {
        id: "osrs",
        name: "Old School RuneScape",
        steam_app_id: Some(1343370),
        protection: Protection::None,
        channels: &[Channel::OfficialWebApi(
            "Jagex's official hiscores by your display name; no plugin or client",
        )],
        status: Status::Built,
    },
    Integration {
        id: "dota2",
        name: "Dota 2",
        steam_app_id: Some(570),
        protection: Protection::UserMode("VAC"),
        channels: &[Channel::OfficialLocalApi(
            "Valve's Game State Integration feed",
        )],
        status: Status::Built,
    },
    Integration {
        id: "league",
        name: "League of Legends",
        steam_app_id: None,
        protection: Protection::Kernel("Riot Vanguard"),
        channels: &[Channel::OfficialLocalApi(
            "Riot's Live Client Data API on this computer",
        )],
        status: Status::Built,
    },
    Integration {
        id: "poe",
        name: "Path of Exile",
        steam_app_id: Some(238960),
        protection: Protection::None,
        channels: &[Channel::GameFile(
            "the game's own Client.txt log: areas, level-ups and deaths, with a session recap in Pulse",
        )],
        status: Status::Built,
    },
    Integration {
        id: "poe2",
        name: "Path of Exile 2",
        steam_app_id: Some(2694490),
        protection: Protection::None,
        channels: &[Channel::GameFile(
            "the game's own Client.txt log: areas, level-ups and deaths, with a session recap in Pulse",
        )],
        status: Status::Built,
    },
    Integration {
        id: "minecraft",
        name: "Minecraft: Java Edition",
        steam_app_id: None,
        protection: Protection::None,
        channels: &[Channel::GameFile(
            "the game's own logs/latest.log, vanilla launcher and Prism or MultiMC instances: deaths and advancements, with a session recap in Pulse",
        )],
        status: Status::Built,
    },
    Integration {
        id: "eso",
        name: "The Elder Scrolls Online",
        steam_app_id: Some(306130),
        protection: Protection::None,
        channels: &[Channel::OfficialAddon(
            "a Loadout addon through ZeniMax's Lua addon API and saved variables",
        )],
        status: Status::Planned,
    },
];

pub fn by_id(id: &str) -> Option<&'static Integration> {
    ALL.iter().find(|g| g.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_follows_the_read_only_rules() {
        for g in ALL {
            assert!(g.follows_the_rules(), "{} breaks the rules", g.name);
        }
        let mut ids: Vec<&str> = ALL.iter().map(|g| g.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALL.len(), "ids must be unique");
    }

    #[test]
    fn a_kernel_anti_cheat_game_with_an_addon_is_refused() {
        let bad = Integration {
            id: "x",
            name: "X",
            steam_app_id: None,
            protection: Protection::Kernel("Some driver"),
            channels: &[Channel::OfficialAddon("an addon")],
            status: Status::Planned,
        };
        assert!(!bad.follows_the_rules());
        let ok = Integration {
            channels: &[Channel::GameFile("a log")],
            ..bad
        };
        assert!(ok.follows_the_rules());
    }

    #[test]
    fn built_games_come_first_and_read_plainly() {
        let first_planned = ALL.iter().position(|g| g.status == Status::Planned);
        if let Some(i) = first_planned {
            assert!(ALL[i..].iter().all(|g| g.status == Status::Planned));
        }
        assert_eq!(
            by_id("league").unwrap().how(),
            "Riot's Live Client Data API on this computer. Has Riot Vanguard, so Loadout adds nothing to the game"
        );
        assert!(by_id("eso").unwrap().how().ends_with(". Planned"));
        assert_eq!(by_id("poe2").unwrap().steam_app_id, Some(2694490));
        assert_eq!(by_id("dota2").unwrap().steam_app_id, Some(570));
        let d2 = by_id("destiny2").unwrap();
        assert!(d2.follows_the_rules());
        assert!(d2.how().contains("so Loadout adds nothing to the game"));
        assert_eq!(by_id("osrs").unwrap().steam_app_id, Some(1343370));
        let gw2 = by_id("gw2").unwrap();
        assert!(!gw2.channels[0].is_addon());
        assert!(gw2.how().contains("read-only key"));
        assert_eq!(by_id("elite").unwrap().steam_app_id, Some(359320));
        assert_eq!(by_id("stardew").unwrap().steam_app_id, Some(413150));
    }
}
