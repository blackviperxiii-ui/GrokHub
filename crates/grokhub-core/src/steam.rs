//! The user's local Steam library as a Pulse source. Reads only what Steam
//! keeps on disk (`libraryfolders.vdf`, `appmanifest_*.acf`, and each
//! account's `localconfig.vdf` for last played and playtime); no account,
//! key or network call is needed to list games. Each game's public news feed
//! then joins Pulse's source check like any RSS feed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pulse_sources::{SourceKind, SourceReq};

/// Where the Steam choices live, beside `app.json` in the config folder.
pub const PREFS_FILE: &str = "pulse-steam.json";
/// News feeds asked for on one source check: the most recently played games.
pub const FEEDS_PER_CHECK: usize = 6;
/// Games listed under Settings → Pulse → Steam.
pub const SETTINGS_LIST_MAX: usize = 12;

/// One node of Valve's KeyValues text format: a string or a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Vdf {
    Str(String),
    Block(Vec<(String, Vdf)>),
}

impl Vdf {
    /// A child by key, ignoring case (Steam writes both `apps` and `Apps`).
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Block(items) => items
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Block(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Block(items) => items,
            Vdf::Str(_) => &[],
        }
    }

    /// Follow a path of keys.
    pub fn at(&self, path: &[&str]) -> Option<&Vdf> {
        path.iter().try_fold(self, |node, key| node.get(key))
    }
}

/// Parse KeyValues text. Quoted and bare tokens, `\\` and `\"` escapes, `//`
/// comments. Unbalanced input returns what was read so far.
pub fn parse_vdf(text: &str) -> Vdf {
    let tokens = tokenize(text);
    let mut pos = 0;
    Vdf::Block(parse_block(&tokens, &mut pos))
}

#[derive(Debug, PartialEq)]
enum Tok {
    Text(String),
    Open,
    Close,
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        other => s.push(other),
                    }
                }
                out.push(Tok::Text(s));
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            c => {
                let mut s = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                out.push(Tok::Text(s));
            }
        }
    }
    out
}

fn parse_block(tokens: &[Tok], pos: &mut usize) -> Vec<(String, Vdf)> {
    let mut items = Vec::new();
    while *pos < tokens.len() {
        match &tokens[*pos] {
            Tok::Close => {
                *pos += 1;
                return items;
            }
            Tok::Open => {
                // A block with no key: skip into it.
                *pos += 1;
                items.extend(parse_block(tokens, pos));
            }
            Tok::Text(key) => {
                *pos += 1;
                match tokens.get(*pos) {
                    Some(Tok::Text(value)) => {
                        *pos += 1;
                        items.push((key.clone(), Vdf::Str(value.clone())));
                    }
                    Some(Tok::Open) => {
                        *pos += 1;
                        let block = parse_block(tokens, pos);
                        items.push((key.clone(), Vdf::Block(block)));
                    }
                    _ => return items,
                }
            }
        }
    }
    items
}

/// One game from the library.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGame {
    pub app_id: u32,
    pub name: String,
    /// Unix seconds, 0 when never played or unknown.
    pub last_played: u64,
    /// Minutes played, 0 when unknown.
    pub playtime_min: u64,
}

impl SteamGame {
    /// The game's public news feed (no key needed).
    pub fn news_feed(&self) -> String {
        format!(
            "https://store.steampowered.com/feeds/news/app/{}/",
            self.app_id
        )
    }
}

/// Is this one of the Steam news feeds [`SteamGame::news_feed`] makes?
pub fn is_news_feed(url: &str) -> bool {
    header_image(url).is_some()
}

/// App id from a [`SteamGame::news_feed`] URL.
pub fn app_id_from_news_feed(url: &str) -> Option<u32> {
    url.strip_prefix("https://store.steampowered.com/feeds/news/app/")?
        .trim_end_matches('/')
        .parse()
        .ok()
}

/// The store header art for a Steam news feed's game.
pub fn header_image(feed_url: &str) -> Option<String> {
    let id: u32 = app_id_from_news_feed(feed_url)?;
    Some(format!(
        "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{id}/header.jpg"
    ))
}

/// Steam's own runtimes and redistributables are not games.
fn is_tool(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    [
        "proton",
        "steam linux runtime",
        "steamworks common",
        "redistributable",
        "steamvr",
    ]
    .iter()
    .any(|w| low.contains(w))
}

/// The library folders `libraryfolders.vdf` names, the root first.
pub fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    for file in [
        root.join("steamapps").join("libraryfolders.vdf"),
        root.join("config").join("libraryfolders.vdf"),
    ] {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let vdf = parse_vdf(&text);
        let Some(folders) = vdf.get("libraryfolders") else {
            continue;
        };
        for (_, entry) in folders.entries() {
            // New format: a block with "path"; old format: the path itself.
            let path = entry
                .get("path")
                .and_then(Vdf::str)
                .or_else(|| entry.str().filter(|s| s.contains('/') || s.contains('\\')));
            if let Some(path) = path {
                let p = PathBuf::from(path);
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        break;
    }
    out
}

/// Installed games from every library folder's `appmanifest_*.acf`.
pub fn installed_games(root: &Path) -> Vec<SteamGame> {
    let mut games: BTreeMap<u32, SteamGame> = BTreeMap::new();
    for folder in library_folders(root) {
        let Ok(dir) = std::fs::read_dir(folder.join("steamapps")) else {
            continue;
        };
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if let Some(game) = manifest_game(&parse_vdf(&text)) {
                games.entry(game.app_id).or_insert(game);
            }
        }
    }
    games.into_values().collect()
}

/// A game from one parsed `appmanifest_*.acf`.
pub fn manifest_game(vdf: &Vdf) -> Option<SteamGame> {
    let state = vdf.get("AppState")?;
    let app_id = state.get("appid")?.str()?.parse().ok()?;
    let name = state.get("name")?.str()?.trim().to_string();
    if name.is_empty() || is_tool(&name) {
        return None;
    }
    let last_played = state
        .get("LastPlayed")
        .and_then(Vdf::str)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Some(SteamGame {
        app_id,
        name,
        last_played,
        playtime_min: 0,
    })
}

/// Last played and playtime per app from each account's `localconfig.vdf`.
/// The newest value wins when there are several accounts.
pub fn play_stats(root: &Path) -> BTreeMap<u32, (u64, u64)> {
    let mut out: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
    let Ok(users) = std::fs::read_dir(root.join("userdata")) else {
        return out;
    };
    for user in users.flatten() {
        let file = user.path().join("config").join("localconfig.vdf");
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        merge_stats(&parse_vdf(&text), &mut out);
    }
    out
}

/// Fold one parsed `localconfig.vdf` into `out`.
pub fn merge_stats(vdf: &Vdf, out: &mut BTreeMap<u32, (u64, u64)>) {
    let Some(apps) = vdf.at(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]) else {
        return;
    };
    for (id, app) in apps.entries() {
        let Ok(id) = id.parse::<u32>() else {
            continue;
        };
        let num = |key: &str| {
            app.get(key)
                .and_then(Vdf::str)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        };
        let (last, minutes) = (num("LastPlayed"), num("Playtime"));
        let slot = out.entry(id).or_default();
        slot.0 = slot.0.max(last);
        slot.1 = slot.1.max(minutes);
    }
}

/// Active games first: most recently played, then most played, then the rest
/// by name. Never-played installs sink to the bottom.
pub fn rank_games(mut games: Vec<SteamGame>) -> Vec<SteamGame> {
    games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then(b.playtime_min.cmp(&a.playtime_min))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    games
}

/// The whole library under one Steam root, ranked.
pub fn read_library(root: &Path) -> Vec<SteamGame> {
    let stats = play_stats(root);
    let games = installed_games(root)
        .into_iter()
        .map(|mut g| {
            if let Some((last, minutes)) = stats.get(&g.app_id) {
                g.last_played = g.last_played.max(*last);
                g.playtime_min = *minutes;
            }
            g
        })
        .collect();
    rank_games(games)
}

/// Where Steam usually lives, for a home folder and (on Windows) the
/// Program Files folders.
pub fn candidate_roots(home: Option<&Path>, program_files: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home {
        out.push(home.join(".steam").join("steam"));
        out.push(home.join(".local").join("share").join("Steam"));
        out.push(
            home.join(".var")
                .join("app")
                .join("com.valvesoftware.Steam")
                .join(".local")
                .join("share")
                .join("Steam"),
        );
        out.push(
            home.join("Library")
                .join("Application Support")
                .join("Steam"),
        );
    }
    for pf in program_files {
        out.push(pf.join("Steam"));
    }
    out
}

/// The first candidate that holds a Steam library.
pub fn find_root(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|root| root.join("steamapps").is_dir())
        .cloned()
}

/// This machine's Steam root, if Steam is installed.
pub fn detect_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let program_files: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    find_root(&candidate_roots(home.as_deref(), &program_files))
}

/// The user's choices: on or off, and the games kept out of Pulse.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamPrefs {
    /// `None` until the user picks: on when Steam is found.
    #[serde(default)]
    pub on: Option<bool>,
    #[serde(default)]
    pub muted: Vec<u32>,
}

impl SteamPrefs {
    pub fn enabled(&self) -> bool {
        self.on.unwrap_or(true)
    }

    pub fn is_muted(&self, app_id: u32) -> bool {
        self.muted.contains(&app_id)
    }

    /// Mute or unmute one game.
    pub fn set_muted(&mut self, app_id: u32, muted: bool) {
        self.muted.retain(|id| *id != app_id);
        if muted {
            self.muted.push(app_id);
            self.muted.sort_unstable();
        }
    }
}

pub fn prefs_path(config_dir: &Path) -> PathBuf {
    config_dir.join(PREFS_FILE)
}

pub fn load_prefs(config_dir: &Path) -> SteamPrefs {
    std::fs::read_to_string(prefs_path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_prefs(config_dir: &Path, prefs: &SteamPrefs) -> Result<(), String> {
    let text = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(prefs_path(config_dir), text).map_err(|e| e.to_string())
}

/// The games that feed Pulse: unmuted, most recently played first.
pub fn feeding<'a>(games: &'a [SteamGame], prefs: &SteamPrefs) -> Vec<&'a SteamGame> {
    if !prefs.enabled() {
        return Vec::new();
    }
    games.iter().filter(|g| !prefs.is_muted(g.app_id)).collect()
}

/// This check's Steam news fetches. Each one carries its game as the topic,
/// so Pulse's interest ranking and feedback apply to it.
pub fn requests(games: &[SteamGame], prefs: &SteamPrefs, max: usize) -> Vec<SourceReq> {
    feeding(games, prefs)
        .into_iter()
        .take(max)
        .map(|g| SourceReq {
            kind: SourceKind::Rss,
            url: g.news_feed(),
            name: format!("{} on Steam", g.name),
            topic: Some(g.name.clone()),
        })
        .collect()
}

/// Is a source post from a muted game? Its source name is "<game> on Steam".
pub fn muted_post(source_name: &str, games: &[SteamGame], prefs: &SteamPrefs) -> bool {
    games
        .iter()
        .filter(|g| prefs.is_muted(g.app_id))
        .any(|g| source_name == format!("{} on Steam", g.name))
}

/// A library game by name, ignoring case and surrounding space.
pub fn find_game<'a>(games: &'a [SteamGame], name: &str) -> Option<&'a SteamGame> {
    let want = name.trim();
    games.iter().find(|g| g.name.eq_ignore_ascii_case(want))
}

/// One line for Settings and the agent: what is feeding Pulse right now.
pub fn status_line(games: &[SteamGame], prefs: &SteamPrefs, found: bool) -> String {
    if !found {
        return "Steam is not installed on this computer.".into();
    }
    if !prefs.enabled() {
        return format!("Steam library: off ({} games found).", games.len());
    }
    let feeding = feeding(games, prefs);
    let muted = games.len() - feeding.len();
    let names: Vec<&str> = feeding
        .iter()
        .take(FEEDS_PER_CHECK)
        .map(|g| g.name.as_str())
        .collect();
    let mut line = match feeding.len() {
        1 => "Steam library: on, 1 game feeds Pulse".to_string(),
        n => format!("Steam library: on, {n} games feed Pulse"),
    };
    if muted > 0 {
        line.push_str(&format!(", {muted} muted"));
    }
    if !names.is_empty() {
        line.push_str(&format!(". Checked first: {}", names.join(", ")));
    }
    line.push('.');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY_LINUX: &str = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/user/.local/share/Steam"
		"label"		""
		"apps"
		{
			"570"		"123"
		}
	}
	"1"
	{
		"path"		"/home/user/games/SteamLibrary"
		"apps"
		{
			"1091500"		"456"
		}
	}
}
"#;

    const LIBRARY_WINDOWS: &str = r#"
"libraryfolders"
{
	"contentstatsid"		"-1"
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;

    fn manifest(id: u32, name: &str, last: u64) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"name\"\t\t\"{name}\"\n\t\"LastPlayed\"\t\t\"{last}\"\n}}\n"
        )
    }

    const LOCALCONFIG: &str = r#"
"UserLocalConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"Apps"
				{
					"570"
					{
						"LastPlayed"		"1700000000"
						"Playtime"		"5400"
					}
					"1091500"
					{
						"LastPlayed"		"1760000000"
						"Playtime"		"900"
					}
				}
			}
		}
	}
}
"#;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grokhub-steam-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("steamapps")).unwrap();
        dir
    }

    #[test]
    fn parses_linux_and_windows_library_folders() {
        let linux = parse_vdf(LIBRARY_LINUX);
        let paths: Vec<&str> = linux
            .get("libraryfolders")
            .unwrap()
            .entries()
            .iter()
            .filter_map(|(_, e)| e.get("path").and_then(Vdf::str))
            .collect();
        assert_eq!(
            paths,
            [
                "/home/user/.local/share/Steam",
                "/home/user/games/SteamLibrary"
            ]
        );
        let windows = parse_vdf(LIBRARY_WINDOWS);
        assert_eq!(
            windows
                .at(&["libraryfolders", "1", "path"])
                .and_then(Vdf::str),
            Some("D:\\SteamLibrary")
        );
        assert_eq!(
            windows
                .at(&["libraryfolders", "0", "path"])
                .and_then(Vdf::str),
            Some("C:\\Program Files (x86)\\Steam")
        );
    }

    #[test]
    fn reads_manifests_and_skips_steam_runtimes() {
        let game = manifest_game(&parse_vdf(&manifest(570, "Dota 2", 1_700_000_000))).unwrap();
        assert_eq!(
            game,
            SteamGame {
                app_id: 570,
                name: "Dota 2".into(),
                last_played: 1_700_000_000,
                playtime_min: 0,
            }
        );
        assert_eq!(
            manifest_game(&parse_vdf(&manifest(1493710, "Proton Experimental", 0))),
            None
        );
        assert_eq!(
            manifest_game(&parse_vdf(&manifest(
                228980,
                "Steamworks Common Redistributables",
                0
            ))),
            None
        );
        assert_eq!(
            manifest_game(&parse_vdf("\"AppState\" { \"name\" \"No id\" }")),
            None
        );
    }

    #[test]
    fn reads_a_library_across_folders_with_play_stats() {
        let root = temp_root("lib");
        let second = root.join("second");
        std::fs::create_dir_all(second.join("steamapps")).unwrap();
        let folders = format!(
            "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} }}",
            root.display().to_string().replace('\\', "\\\\"),
            second.display().to_string().replace('\\', "\\\\"),
        );
        std::fs::write(root.join("steamapps/libraryfolders.vdf"), folders).unwrap();
        std::fs::write(
            root.join("steamapps/appmanifest_570.acf"),
            manifest(570, "Dota 2", 0),
        )
        .unwrap();
        std::fs::write(
            root.join("steamapps/appmanifest_620.acf"),
            manifest(620, "Portal 2", 0),
        )
        .unwrap();
        std::fs::write(
            second.join("steamapps/appmanifest_1091500.acf"),
            manifest(1091500, "Cyberpunk 2077", 0),
        )
        .unwrap();
        let cfg = root.join("userdata/12345/config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("localconfig.vdf"), LOCALCONFIG).unwrap();

        let games = read_library(&root);
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Cyberpunk 2077", "Dota 2", "Portal 2"]);
        assert_eq!(
            (games[0].last_played, games[0].playtime_min),
            (1_760_000_000, 900)
        );
        assert_eq!(
            (games[1].last_played, games[1].playtime_min),
            (1_700_000_000, 5400)
        );
        assert_eq!((games[2].last_played, games[2].playtime_min), (0, 0));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_the_root_only_where_steam_is_installed() {
        let home = std::env::temp_dir().join(format!("grokhub-steam-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let candidates = candidate_roots(Some(&home), &[PathBuf::from("/nowhere")]);
        assert_eq!(candidates[1], home.join(".local/share/Steam"));
        assert_eq!(candidates.last().unwrap(), &PathBuf::from("/nowhere/Steam"));
        assert_eq!(find_root(&candidates), None);
        std::fs::create_dir_all(home.join(".local/share/Steam/steamapps")).unwrap();
        assert_eq!(
            find_root(&candidates),
            Some(home.join(".local/share/Steam"))
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    fn game(id: u32, name: &str, last: u64, minutes: u64) -> SteamGame {
        SteamGame {
            app_id: id,
            name: name.into(),
            last_played: last,
            playtime_min: minutes,
        }
    }

    #[test]
    fn recently_played_games_lead_the_requests_and_muted_ones_drop() {
        let games = rank_games(vec![
            game(1, "Old Favorite", 1_600_000_000, 90_000),
            game(2, "Never Played", 0, 0),
            game(3, "Current Main", 1_760_000_000, 300),
            game(4, "Last Week", 1_759_000_000, 50),
        ]);
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            ["Current Main", "Last Week", "Old Favorite", "Never Played"]
        );

        let mut prefs = SteamPrefs::default();
        let reqs = requests(&games, &prefs, 2);
        assert_eq!(
            reqs,
            vec![
                SourceReq {
                    kind: SourceKind::Rss,
                    url: "https://store.steampowered.com/feeds/news/app/3/".into(),
                    name: "Current Main on Steam".into(),
                    topic: Some("Current Main".into()),
                },
                SourceReq {
                    kind: SourceKind::Rss,
                    url: "https://store.steampowered.com/feeds/news/app/4/".into(),
                    name: "Last Week on Steam".into(),
                    topic: Some("Last Week".into()),
                },
            ]
        );

        prefs.set_muted(3, true);
        let names: Vec<String> = requests(&games, &prefs, 2)
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["Last Week on Steam", "Old Favorite on Steam"]);
        assert!(muted_post("Current Main on Steam", &games, &prefs));
        assert!(!muted_post("Last Week on Steam", &games, &prefs));
        prefs.set_muted(3, false);
        assert!(prefs.muted.is_empty());

        prefs.on = Some(false);
        assert!(requests(&games, &prefs, 6).is_empty());
    }

    #[test]
    fn status_line_names_what_feeds_pulse() {
        let games = vec![
            game(10, "Current Main", 300, 60),
            game(20, "Old Save", 100, 5),
        ];
        let mut prefs = SteamPrefs::default();
        assert_eq!(
            status_line(&games, &prefs, false),
            "Steam is not installed on this computer."
        );
        assert_eq!(
            status_line(&games, &prefs, true),
            "Steam library: on, 2 games feed Pulse. Checked first: Current Main, Old Save."
        );
        prefs.set_muted(20, true);
        assert_eq!(
            status_line(&games, &prefs, true),
            "Steam library: on, 1 game feeds Pulse, 1 muted. Checked first: Current Main."
        );
        prefs.on = Some(false);
        assert_eq!(
            status_line(&games, &prefs, true),
            "Steam library: off (2 games found)."
        );
        assert_eq!(find_game(&games, " old save ").map(|g| g.app_id), Some(20));
        assert_eq!(find_game(&games, "Nope"), None);
    }

    #[test]
    fn steam_news_posts_name_their_game() {
        let req = &requests(
            &[game(10, "Current Main", 300, 60)],
            &SteamPrefs::default(),
            6,
        )[0];
        let rss = r#"<rss><channel><title>Current Main - Steam News</title><item><title>Patch 1.2</title><link>https://store.steampowered.com/news/app/10/view/1</link></item></channel></rss>"#;
        let items = crate::pulse_sources::parse(req, rss);
        assert_eq!(items[0].source_name, "Current Main on Steam");
        assert_eq!(items[0].topic.as_deref(), Some("Current Main"));
        assert_eq!(
            items[0].image.as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/10/header.jpg"
            )
        );
        assert_eq!(header_image("https://example.com/feed.xml"), None);
    }

    #[test]
    fn prefs_round_trip_and_default_on() {
        let dir = std::env::temp_dir().join(format!("grokhub-steam-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_prefs(&dir), SteamPrefs::default());
        assert!(SteamPrefs::default().enabled());
        let prefs = SteamPrefs {
            on: Some(false),
            muted: vec![570],
        };
        save_prefs(&dir, &prefs).unwrap();
        assert_eq!(load_prefs(&dir), prefs);
        assert_eq!(
            std::fs::read_to_string(dir.join(PREFS_FILE)).unwrap(),
            "{\n  \"on\": false,\n  \"muted\": [\n    570\n  ]\n}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vdf_handles_comments_escapes_and_bare_tokens() {
        let v = parse_vdf("// header\n\"a\" { \"quote\" \"say \\\"hi\\\"\" bare 12 }");
        assert_eq!(v.at(&["a", "quote"]).and_then(Vdf::str), Some("say \"hi\""));
        assert_eq!(v.at(&["A", "BARE"]).and_then(Vdf::str), Some("12"));
        assert_eq!(parse_vdf("\"x\" {").get("x"), Some(&Vdf::Block(Vec::new())));
    }
}
