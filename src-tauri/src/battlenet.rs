//! Battle.net games on this computer for the Library page.
//!
//! Detection reads the same Agent `product.db` (and Wine prefixes) as the
//! cabin through `grokhub_core::battlenet`. Play hands a checked
//! `battlenet://{product}/` URI to the OS, the same scheme Battle.net's
//! agent registers. Open folder shows the install. API sign-in stays on
//! the credentials card (`battlenet_api`); nothing here talks to Blizzard.

use std::path::Path;

use grokhub_core::battlenet::{self as core, BnetGame};
use serde::Serialize;

/// One Battle.net game tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub product: String,
    pub name: String,
    pub install: String,
    /// Wine prefix on Linux, when the install lives under one.
    pub prefix: Option<String>,
    /// Painted cover colour seed (same hash as the cabin).
    pub seed: u32,
}

/// The Battle.net half of the Library page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// At least one install was found.
    pub found: bool,
    pub games: Vec<Game>,
}

/// Cabin-matching display name: every WoW product is "World of Warcraft".
pub fn title(game: &BnetGame) -> String {
    if game.is_wow() {
        "World of Warcraft".into()
    } else {
        game.name()
    }
}

/// Painted cover seed; matches the cabin's `cover_seed`.
pub fn cover_seed(game: &BnetGame) -> u32 {
    game.product
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
}

/// One tile per install folder; WoW Retail and Classic share a folder.
pub fn library_rows(games: &[BnetGame]) -> Vec<&BnetGame> {
    let mut seen = std::collections::BTreeSet::new();
    games
        .iter()
        .filter(|g| seen.insert((g.is_wow(), g.install.clone())))
        .collect()
}

fn to_game(g: &BnetGame) -> Game {
    Game {
        product: g.product.clone(),
        name: title(g),
        install: g.install.to_string_lossy().into_owned(),
        prefix: g.prefix.as_ref().map(|p| p.to_string_lossy().into_owned()),
        seed: cover_seed(g),
    }
}

/// Detected Battle.net games as Library tiles.
pub fn page_from(games: Vec<BnetGame>) -> Page {
    let games: Vec<Game> = library_rows(&games).into_iter().map(to_game).collect();
    Page {
        found: !games.is_empty(),
        games,
    }
}

/// Live detection on this computer.
pub fn page() -> Page {
    page_from(core::detect())
}

/// A Play URI for a product code already on this computer.
pub fn play_url(product: &str) -> Result<String, String> {
    if !product_ok(product) {
        return Err("That isn't a Battle.net game.".into());
    }
    Ok(format!("battlenet://{product}/"))
}

fn product_ok(product: &str) -> bool {
    !product.is_empty()
        && product.len() <= 32
        && product
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The program and arguments that hand a `battlenet://` URI to the agent.
pub fn launcher(os: &str, url: &str) -> Option<(&'static str, Vec<String>)> {
    let product = url.strip_prefix("battlenet://")?.strip_suffix('/')?;
    if !product_ok(product) {
        return None;
    }
    match os {
        "windows" => Some((
            "cmd",
            vec!["/C".into(), "start".into(), String::new(), url.into()],
        )),
        "macos" => Some(("open", vec![url.into()])),
        "linux" | "freebsd" | "openbsd" | "netbsd" | "dragonfly" => {
            Some(("xdg-open", vec![url.into()]))
        }
        _ => None,
    }
}

fn hand_to_agent(url: &str) -> Result<(), String> {
    let (program, args) =
        launcher(std::env::consts::OS, url).ok_or("Battle.net can't be opened on this device.")?;
    let mut cmd = std::process::Command::new(program);
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't ask Battle.net to start the game: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Play: ask Battle.net to start a product found on this computer.
pub fn play(games: &[BnetGame], product: &str) -> Result<String, String> {
    let game = library_rows(games)
        .into_iter()
        .find(|g| g.product == product)
        .ok_or("That game isn't in this computer's Battle.net library.")?;
    let url = play_url(&game.product)?;
    if launcher(std::env::consts::OS, &url).is_none() {
        return Err("Battle.net games can't be started on this device.".into());
    }
    hand_to_agent(&url)?;
    Ok(format!("Asked Battle.net to start {}.", title(game)))
}

/// Open the install folder for a product found on this computer.
pub fn open_folder(games: &[BnetGame], product: &str) -> Result<String, String> {
    let game = library_rows(games)
        .into_iter()
        .find(|g| g.product == product)
        .ok_or("That game isn't in this computer's Battle.net library.")?;
    crate::skills::open_folder(Path::new(&game.install))?;
    Ok(format!("Opened the folder for {}.", title(game)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn wow_at(root: &Path) -> BnetGame {
        BnetGame {
            product: "wow".into(),
            install: root.to_path_buf(),
            prefix: Some(PathBuf::from("/home/user/Games/battlenet")),
        }
    }

    fn classic_at(root: &Path) -> BnetGame {
        BnetGame {
            product: "wow_classic".into(),
            install: root.to_path_buf(),
            prefix: Some(PathBuf::from("/home/user/Games/battlenet")),
        }
    }

    fn d4_at(root: &Path) -> BnetGame {
        BnetGame {
            product: "fenris".into(),
            install: root.join("d4"),
            prefix: None,
        }
    }

    #[test]
    fn a_fixture_install_gives_tiles_and_dedupes_wow() {
        let root = PathBuf::from("/home/user/Games/World of Warcraft");
        let page = page_from(vec![wow_at(&root), classic_at(&root), d4_at(&root)]);
        assert!(page.found);
        assert_eq!(page.games.len(), 2);
        assert_eq!(page.games[0].name, "World of Warcraft");
        assert_eq!(page.games[0].product, "wow");
        assert_eq!(page.games[1].name, "Diablo IV");
        assert_eq!(page.games[1].product, "fenris");
        assert_eq!(page.games[0].seed, cover_seed(&wow_at(&root)));
    }

    #[test]
    fn play_url_is_the_battlenet_scheme() {
        assert_eq!(play_url("wow").unwrap(), "battlenet://wow/");
        assert_eq!(play_url("fenris").unwrap(), "battlenet://fenris/");
        assert!(play_url("").is_err());
        assert!(play_url("../x").is_err());
        assert!(play_url("WoW").is_err());
    }

    #[test]
    fn launcher_only_accepts_checked_battlenet_uris() {
        assert!(launcher("linux", "battlenet://wow/").is_some());
        assert!(launcher("windows", "battlenet://fenris/").is_some());
        assert!(launcher("linux", "battlenet://../etc/").is_none());
        assert!(launcher("linux", "steam://rungameid/1").is_none());
        assert!(launcher("linux", "battlenet://wow").is_none());
    }

    #[test]
    fn play_refuses_a_product_that_isnt_installed() {
        let root = PathBuf::from("/home/user/Games/World of Warcraft");
        let games = vec![wow_at(&root)];
        assert_eq!(
            play(&games, "fenris"),
            Err("That game isn't in this computer's Battle.net library.".into())
        );
    }
}
