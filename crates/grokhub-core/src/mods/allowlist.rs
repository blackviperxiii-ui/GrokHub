//! Which games Loadout may mod, in which modes, and from which sources.
//!
//! The allowlist is the only door: a game that isn't listed is Blocked
//! ("Loadout hasn't checked this game's mod rules yet"), and a listed game is
//! Allowed only in the modes its entry names. A game with a kernel anti-cheat
//! is never Allowed in any mode, and no game is Allowed in competitive play;
//! the table's own tests hold every entry to that.
//!
//! Each entry names the sanctioned sources it may use, a one-line ban-risk
//! note, and where and when its rules were checked, so a stale entry fails a
//! test and gets looked at again.

use crate::game_integrations::Protection;

/// A sanctioned mod source. Nothing else is ever read or downloaded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Nexus,
    Workshop,
    CurseForge,
    Thunderstore,
    Modrinth,
}

impl Source {
    pub const ALL: [Source; 5] = [
        Source::Nexus,
        Source::Workshop,
        Source::CurseForge,
        Source::Thunderstore,
        Source::Modrinth,
    ];

    /// Stable id for files and settings.
    pub fn id(self) -> &'static str {
        match self {
            Source::Nexus => "nexus",
            Source::Workshop => "workshop",
            Source::CurseForge => "curseforge",
            Source::Thunderstore => "thunderstore",
            Source::Modrinth => "modrinth",
        }
    }

    pub fn from_id(id: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The name people know it by.
    pub fn label(self) -> &'static str {
        match self {
            Source::Nexus => "Nexus Mods",
            Source::Workshop => "Steam Workshop",
            Source::CurseForge => "CurseForge",
            Source::Thunderstore => "Thunderstore",
            Source::Modrinth => "Modrinth",
        }
    }

    /// The only hosts Loadout talks to for this source: its API, and the CDN
    /// it serves files from. A redirect anywhere else is refused.
    pub fn hosts(self) -> &'static [&'static str] {
        match self {
            Source::Nexus => &["api.nexusmods.com"],
            Source::Workshop => &["api.steampowered.com"],
            Source::CurseForge => &[
                "api.curseforge.com",
                "edge.forgecdn.net",
                "mediafilez.forgecdn.net",
            ],
            Source::Thunderstore => &["thunderstore.io", "gcdn.thunderstore.io"],
            Source::Modrinth => &["api.modrinth.com", "cdn.modrinth.com"],
        }
    }
}

/// How a game is being played when a mod would load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Offline or solo play.
    Singleplayer,
    /// Co-op with friends who run the same mods, or a server the user runs.
    OwnServers,
    /// Public online play, where the game maker sanctions the mod system
    /// (such as Blizzard's addon API).
    Online,
    /// Ranked, matchmade or tournament play. Never Allowed.
    Competitive,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Singleplayer => "Singleplayer",
            Mode::OwnServers => "your own servers and co-op",
            Mode::Online => "online play through the game's own addon system",
            Mode::Competitive => "competitive play",
        }
    }
}

/// Whether Loadout may mod a game in one mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Blocked(String),
}

/// One game's mod rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub id: &'static str,
    pub name: &'static str,
    pub steam_app_id: Option<u32>,
    /// Battle.net product codes that are this game.
    pub bnet_products: &'static [&'static str],
    pub protection: Protection,
    /// Sanctioned sources, the game's preferred one first. Empty when Blocked.
    pub sources: &'static [Source],
    /// The modes Loadout may mod. Empty means the game is Blocked.
    pub allowed: &'static [Mode],
    /// One line: why every mode not in `allowed` is Blocked.
    pub blocked: &'static str,
    /// One line on ban risk, shown beside the mods.
    pub risk: &'static str,
    /// Where the rules were checked, and when (YYYY-MM-DD).
    pub checked_url: &'static str,
    pub checked_on: &'static str,
}

impl Entry {
    /// True when Loadout may mod this game in at least one mode.
    pub fn moddable(&self) -> bool {
        !self.allowed.is_empty() && !self.sources.is_empty()
    }

    pub fn verdict(&self, mode: Mode) -> Verdict {
        if self.allowed.contains(&mode) {
            Verdict::Allowed
        } else {
            Verdict::Blocked(self.blocked.to_string())
        }
    }

    /// The per-mode note shown under the game name, such as
    /// "Singleplayer and your own servers and co-op only".
    pub fn mode_note(&self) -> String {
        let labels: Vec<&str> = self.allowed.iter().map(|m| m.label()).collect();
        match labels.as_slice() {
            [] => self.blocked.to_string(),
            [one] => format!("{one} only"),
            [rest @ .., last] => format!("{} and {last} only", rest.join(", ")),
        }
    }

    /// Kernel anti-cheat games and competitive modes are never Allowed.
    pub fn follows_the_rules(&self) -> bool {
        let kernel = matches!(self.protection, Protection::Kernel(_));
        !(kernel && self.moddable())
            && !self.allowed.contains(&Mode::Competitive)
            && !self.blocked.trim().is_empty()
            && !self.checked_url.is_empty()
            && (self.moddable() || self.sources.is_empty())
    }
}

/// What the allowlist says about one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Allowed(&'static Entry),
    Blocked(String),
}

impl Check {
    pub fn entry(&self) -> Option<&'static Entry> {
        match self {
            Check::Allowed(e) => Some(e),
            Check::Blocked(_) => None,
        }
    }
}

/// The line for a game the allowlist doesn't list.
pub const UNKNOWN_REASON: &str = "Loadout hasn't checked this game's mod rules yet.";

const SP: &[Mode] = &[Mode::Singleplayer];
const SP_OWN: &[Mode] = &[Mode::Singleplayer, Mode::OwnServers];
const NO_ONLINE: &str =
    "Online and competitive play stay unmodded: Loadout only mods singleplayer and your own games.";
const NO_PUBLIC: &str = "Public servers set their own mod rules, and competitive play stays unmodded: Loadout only mods singleplayer and your own servers.";
const NO_AC: &str = "No anti-cheat; mods run only in your own games.";
const CHECKED: &str = "2026-11-01";

const fn allowed(
    id: &'static str,
    name: &'static str,
    steam_app_id: Option<u32>,
    sources: &'static [Source],
    modes: &'static [Mode],
    checked_url: &'static str,
) -> Entry {
    Entry {
        id,
        name,
        steam_app_id,
        bnet_products: &[],
        protection: Protection::None,
        sources,
        allowed: modes,
        blocked: if modes.len() == 1 {
            NO_ONLINE
        } else {
            NO_PUBLIC
        },
        risk: NO_AC,
        checked_url,
        checked_on: CHECKED,
    }
}

const fn never(
    id: &'static str,
    name: &'static str,
    steam_app_id: Option<u32>,
    protection: Protection,
    blocked: &'static str,
    checked_url: &'static str,
) -> Entry {
    Entry {
        id,
        name,
        steam_app_id,
        bnet_products: &[],
        protection,
        sources: &[],
        allowed: &[],
        blocked,
        risk: "Mods there can get accounts banned.",
        checked_url,
        checked_on: CHECKED,
    }
}

use Source::{CurseForge, Modrinth, Nexus, Thunderstore, Workshop};

/// Every game Loadout has checked, moddable games first.
pub const ALLOWLIST: &[Entry] = &[
    allowed("skyrimse", "The Elder Scrolls V: Skyrim Special Edition", Some(489830), &[Nexus], SP,
        "https://www.nexusmods.com/skyrimspecialedition"),
    allowed("fallout4", "Fallout 4", Some(377160), &[Nexus], SP,
        "https://www.nexusmods.com/fallout4"),
    allowed("starfield", "Starfield", Some(1716740), &[Nexus], SP,
        "https://www.nexusmods.com/starfield"),
    allowed("cyberpunk2077", "Cyberpunk 2077", Some(1091500), &[Nexus], SP,
        "https://www.nexusmods.com/cyberpunk2077"),
    allowed("bg3", "Baldur's Gate 3", Some(1086940), &[Nexus], SP_OWN,
        "https://www.nexusmods.com/baldursgate3"),
    allowed("witcher3", "The Witcher 3: Wild Hunt", Some(292030), &[Nexus], SP,
        "https://www.nexusmods.com/witcher3"),
    allowed("stardew", "Stardew Valley", Some(413150), &[Nexus], SP_OWN,
        "https://www.nexusmods.com/stardewvalley"),
    Entry {
        id: "wow",
        name: "World of Warcraft",
        steam_app_id: None,
        bnet_products: &["wow", "wow_classic", "wow_classic_era"],
        protection: Protection::UserMode("Warden"),
        sources: &[CurseForge],
        allowed: &[Mode::Singleplayer, Mode::Online],
        blocked: "Only addons through Blizzard's own addon API; nothing else touches the game.",
        risk: "Addons run in Blizzard's own sandboxed Lua addon API, which Blizzard allows; Warden is user-mode.",
        checked_url: "https://www.curseforge.com/wow",
        checked_on: CHECKED,
    },
    allowed("minecraft", "Minecraft: Java Edition", None, &[Modrinth, CurseForge], SP_OWN,
        "https://modrinth.com/mods"),
    allowed("valheim", "Valheim", Some(892970), &[Thunderstore], SP_OWN,
        "https://thunderstore.io/c/valheim/"),
    allowed("lethalcompany", "Lethal Company", Some(1966720), &[Thunderstore], SP_OWN,
        "https://thunderstore.io/c/lethal-company/"),
    allowed("ror2", "Risk of Rain 2", Some(632360), &[Thunderstore], SP_OWN,
        "https://thunderstore.io/c/riskofrain2/"),
    allowed("repo", "R.E.P.O.", Some(3241660), &[Thunderstore], SP_OWN,
        "https://thunderstore.io/c/repo/"),
    allowed("rimworld", "RimWorld", Some(294100), &[Workshop], SP,
        "https://steamcommunity.com/app/294100/workshop/"),
    allowed("cities", "Cities: Skylines", Some(255710), &[Workshop], SP,
        "https://steamcommunity.com/app/255710/workshop/"),
    allowed("stellaris", "Stellaris", Some(281990), &[Workshop], SP_OWN,
        "https://steamcommunity.com/app/281990/workshop/"),
    allowed("tmodloader", "tModLoader (Terraria)", Some(1281930), &[Workshop], SP_OWN,
        "https://steamcommunity.com/app/1281930/workshop/"),
    never("league", "League of Legends", None, Protection::Kernel("Riot Vanguard"),
        "League of Legends runs Riot Vanguard, a kernel anti-cheat. Loadout never mods it.",
        "https://www.leagueoflegends.com/"),
    never("valorant", "VALORANT", None, Protection::Kernel("Riot Vanguard"),
        "VALORANT runs Riot Vanguard, a kernel anti-cheat. Loadout never mods it.",
        "https://playvalorant.com/"),
    never("fortnite", "Fortnite", None, Protection::Kernel("Easy Anti-Cheat"),
        "Loadout doesn't mod Fortnite: Easy Anti-Cheat. Mods there can get accounts banned.",
        "https://www.fortnite.com/"),
    never("apex", "Apex Legends", Some(1172470), Protection::Kernel("Easy Anti-Cheat"),
        "Loadout doesn't mod Apex Legends: Easy Anti-Cheat. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/1172470/"),
    never("eldenring", "ELDEN RING", Some(1245620), Protection::Kernel("Easy Anti-Cheat"),
        "Loadout doesn't mod ELDEN RING: Easy Anti-Cheat guards its online play. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/1245620/"),
    never("rust", "Rust", Some(252490), Protection::Kernel("Easy Anti-Cheat"),
        "Loadout doesn't mod Rust: Easy Anti-Cheat. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/252490/"),
    never("halomcc", "Halo: The Master Chief Collection", Some(976730), Protection::Kernel("Easy Anti-Cheat"),
        "Loadout doesn't mod Halo: The Master Chief Collection: Easy Anti-Cheat guards its online play.",
        "https://store.steampowered.com/app/976730/"),
    never("rocketleague", "Rocket League", Some(252950), Protection::Kernel("Easy Anti-Cheat"),
        "Loadout doesn't mod Rocket League: Easy Anti-Cheat. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/252950/"),
    never("pubg", "PUBG: BATTLEGROUNDS", Some(578080), Protection::Kernel("BattlEye"),
        "Loadout doesn't mod PUBG: BattlEye. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/578080/"),
    never("destiny2", "Destiny 2", Some(1085660), Protection::Kernel("BattlEye"),
        "Loadout doesn't mod Destiny 2: BattlEye. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/1085660/"),
    never("gta5", "Grand Theft Auto V", Some(271590), Protection::Kernel("BattlEye"),
        "Loadout doesn't mod Grand Theft Auto V or GTA Online: BattlEye. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/271590/"),
    never("gta5e", "Grand Theft Auto V Enhanced", Some(3240220), Protection::Kernel("BattlEye"),
        "Loadout doesn't mod Grand Theft Auto V or GTA Online: BattlEye. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/3240220/"),
    never("r6siege", "Tom Clancy's Rainbow Six Siege", Some(359550), Protection::Kernel("BattlEye"),
        "Loadout doesn't mod Rainbow Six Siege: BattlEye. Mods there can get accounts banned.",
        "https://store.steampowered.com/app/359550/"),
    Entry {
        bnet_products: &["auks", "viper", "odin", "lazr", "zeus", "fore"],
        ..never("cod", "Call of Duty", Some(1938090), Protection::Kernel("Ricochet"),
            "Loadout doesn't mod Call of Duty: Ricochet, a kernel anti-cheat. Mods there can get accounts banned.",
            "https://www.callofduty.com/ricochet")
    },
    never("cs2", "Counter-Strike 2", Some(730), Protection::UserMode("VAC"),
        "Counter-Strike 2 is competitive and runs VAC (and FACEIT for many players). Loadout only reads Valve's Game State Integration feed and never mods it.",
        "https://store.steampowered.com/app/730/"),
    never("dota2", "Dota 2", Some(570), Protection::UserMode("VAC"),
        "Dota 2 is competitive and runs VAC. Loadout only reads Valve's Game State Integration feed and never mods it.",
        "https://store.steampowered.com/app/570/"),
];

pub fn by_id(id: &str) -> Option<&'static Entry> {
    ALLOWLIST.iter().find(|e| e.id == id)
}

pub fn by_steam_app(app_id: u32) -> Option<&'static Entry> {
    ALLOWLIST.iter().find(|e| e.steam_app_id == Some(app_id))
}

pub fn by_bnet_product(code: &str) -> Option<&'static Entry> {
    ALLOWLIST.iter().find(|e| e.bnet_products.contains(&code))
}

fn check_entry(entry: Option<&'static Entry>) -> Check {
    match entry {
        Some(e) if e.moddable() && e.follows_the_rules() => Check::Allowed(e),
        Some(e) => Check::Blocked(e.blocked.to_string()),
        None => Check::Blocked(UNKNOWN_REASON.to_string()),
    }
}

/// The allowlist's word on one game by Loadout id.
pub fn check(id: &str) -> Check {
    check_entry(by_id(id))
}

/// The allowlist's word on one Steam game.
pub fn check_steam(app_id: u32) -> Check {
    check_entry(by_steam_app(app_id))
}

/// The allowlist's word on one Battle.net product.
pub fn check_bnet(code: &str) -> Check {
    check_entry(by_bnet_product(code))
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` date (proleptic Gregorian).
pub fn days_from_ymd(date: &str) -> Option<i64> {
    let mut it = date.splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// Entries whose rules were last checked more than `max_days` before `today`
/// (days since 1970), or whose date doesn't parse.
pub fn stale(today: i64, max_days: i64) -> Vec<&'static str> {
    ALLOWLIST
        .iter()
        .filter(|e| days_from_ymd(e.checked_on).is_none_or(|d| today - d > max_days))
        .map(|e| e.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_guard::ANTI_CHEATS;
    use crate::game_integrations;

    #[test]
    fn every_entry_follows_the_rules_and_ids_are_unique() {
        for e in ALLOWLIST {
            assert!(e.follows_the_rules(), "{} breaks the rules", e.id);
        }
        let mut ids: Vec<&str> = ALLOWLIST.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALLOWLIST.len(), "ids must be unique");
        let mut apps: Vec<u32> = ALLOWLIST.iter().filter_map(|e| e.steam_app_id).collect();
        let n = apps.len();
        apps.sort_unstable();
        apps.dedup();
        assert_eq!(apps.len(), n, "steam app ids must be unique");
    }

    #[test]
    fn a_kernel_anti_cheat_game_is_never_allowed() {
        for e in ALLOWLIST {
            if matches!(e.protection, Protection::Kernel(_)) {
                assert!(e.allowed.is_empty(), "{} has a kernel anti-cheat", e.id);
                assert!(matches!(check(e.id), Check::Blocked(_)));
            }
        }
        let bad = Entry {
            protection: Protection::Kernel("Some driver"),
            ..*by_id("skyrimse").unwrap()
        };
        assert!(!bad.follows_the_rules());
        let competitive = Entry {
            allowed: &[Mode::Singleplayer, Mode::Competitive],
            ..*by_id("skyrimse").unwrap()
        };
        assert!(!competitive.follows_the_rules());
    }

    #[test]
    fn every_kernel_game_loadout_reads_is_listed_and_blocked() {
        for g in game_integrations::ALL {
            if matches!(g.protection, Protection::Kernel(_)) {
                let e = by_id(g.id).unwrap_or_else(|| panic!("{} missing", g.id));
                assert!(!e.moddable(), "{} must be Blocked", g.id);
                assert_eq!(e.steam_app_id, g.steam_app_id, "{}", g.id);
            }
        }
    }

    #[test]
    fn anti_cheat_names_match_the_guard_list() {
        // VAC, Warden and Ricochet have no stable process name, so the guard
        // doesn't list them; every other name must be one the guard knows.
        let extra = ["VAC", "Warden", "Ricochet"];
        for e in ALLOWLIST {
            let ac = match e.protection {
                Protection::None => continue,
                Protection::UserMode(n) | Protection::Kernel(n) => n,
            };
            assert!(
                extra.contains(&ac) || ANTI_CHEATS.iter().any(|a| ac.contains(a.name)),
                "{}: {ac} isn't in the anti-cheat list",
                e.id
            );
        }
    }

    #[test]
    fn blocked_games_say_why_in_one_line() {
        assert_eq!(
            check("valorant"),
            Check::Blocked(
                "VALORANT runs Riot Vanguard, a kernel anti-cheat. Loadout never mods it.".into()
            )
        );
        assert_eq!(
            check_steam(1172470),
            Check::Blocked(
                "Loadout doesn't mod Apex Legends: Easy Anti-Cheat. Mods there can get accounts banned."
                    .into()
            )
        );
        assert_eq!(check("not-a-game"), Check::Blocked(UNKNOWN_REASON.into()));
        assert_eq!(check_steam(1), Check::Blocked(UNKNOWN_REASON.into()));
        for e in ALLOWLIST {
            assert!(!e.blocked.contains('\n'), "{}", e.id);
        }
        assert!(matches!(check_bnet("lazr"), Check::Blocked(_)));
    }

    #[test]
    fn allowed_games_name_their_modes_and_sources() {
        let skyrim = check_steam(489830).entry().unwrap();
        assert_eq!(skyrim.sources, &[Source::Nexus]);
        assert_eq!(skyrim.mode_note(), "Singleplayer only");
        assert_eq!(skyrim.verdict(Mode::Singleplayer), Verdict::Allowed);
        assert!(matches!(skyrim.verdict(Mode::Online), Verdict::Blocked(_)));
        let mc = check("minecraft").entry().unwrap();
        assert_eq!(
            mc.mode_note(),
            "Singleplayer and your own servers and co-op only"
        );
        assert_eq!(mc.sources[0], Source::Modrinth);
        assert!(matches!(mc.verdict(Mode::Online), Verdict::Blocked(_)));
        let wow = check_bnet("wow_classic").entry().unwrap();
        assert_eq!(wow.id, "wow");
        assert_eq!(wow.sources, &[Source::CurseForge]);
        assert!(matches!(
            wow.verdict(Mode::Competitive),
            Verdict::Blocked(_)
        ));
        for e in ALLOWLIST.iter().filter(|e| e.moddable()) {
            assert!(e.checked_url.starts_with("https://"), "{}", e.id);
            for s in e.sources {
                assert!(!s.hosts().is_empty());
            }
        }
    }

    #[test]
    fn source_ids_round_trip() {
        for s in Source::ALL {
            assert_eq!(Source::from_id(s.id()), Some(s));
        }
        assert_eq!(Source::from_id("ftp"), None);
    }

    #[test]
    fn dates_parse_and_stale_entries_are_named() {
        assert_eq!(days_from_ymd("1970-01-01"), Some(0));
        assert_eq!(days_from_ymd("2026-11-01"), Some(20_758));
        assert_eq!(days_from_ymd("2026-13-01"), None);
        let checked = days_from_ymd(CHECKED).unwrap();
        assert!(stale(checked + 365, 365).is_empty());
        assert_eq!(stale(checked + 366, 365).len(), ALLOWLIST.len());
    }

    #[test]
    fn no_entry_is_older_than_twelve_months() {
        let today = (crate::now_ms() / 86_400_000) as i64;
        assert_eq!(
            stale(today, 365),
            Vec::<&str>::new(),
            "re-check these games' anti-cheat and mod rules, then update checked_on"
        );
    }
}
