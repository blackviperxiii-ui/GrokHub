//! Battle.net games on this PC, read from disk only.
//!
//! On Windows the Battle.net app keeps its install list in
//! `C:\ProgramData\Battle.net\Agent\product.db`. On Linux, Battle.net runs
//! inside a Wine prefix (plain Wine, Lutris, Bottles, Heroic, Faugus, or a
//! Steam non-Steam shortcut through Proton), so the same file sits under that
//! prefix's `drive_c`. Each prefix found is read the same way, and the
//! default WoW folders are checked too for an install the database doesn't
//! list (a copied folder, a database that failed to parse).
//!
//! Nothing here starts, opens a handle to, or reads the memory of any game or
//! launcher process; it reads files on disk and nothing else.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A Battle.net game found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BnetGame {
    /// Battle.net's product code (`wow`, `wow_classic`, `fenris`, ...).
    pub product: String,
    /// The install folder on this machine's file system.
    pub install: PathBuf,
    /// The Wine prefix it lives in, on Linux.
    pub prefix: Option<PathBuf>,
}

impl BnetGame {
    /// The game's display name.
    pub fn name(&self) -> String {
        product_name(&self.product)
            .map(str::to_owned)
            .unwrap_or_else(|| self.product.clone())
    }

    /// Whether this is one of the World of Warcraft products.
    pub fn is_wow(&self) -> bool {
        is_wow_product(&self.product)
    }
}

/// The display name for the product codes worth showing.
pub fn product_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "wow" => "World of Warcraft",
        "wow_classic" => "World of Warcraft Classic",
        "wow_classic_era" => "World of Warcraft Classic Era",
        "wow_classic_ptr" | "wowt" | "wowxptr" | "wow_beta" | "wowz" => {
            "World of Warcraft (test realm)"
        }
        "pro" => "Overwatch 2",
        "fenris" => "Diablo IV",
        "osi" => "Diablo II: Resurrected",
        "d3" | "diablo3" => "Diablo III",
        "anbs" => "Diablo Immortal",
        "hs_beta" => "Hearthstone",
        "heroes" => "Heroes of the Storm",
        "s1" => "StarCraft Remastered",
        "s2" => "StarCraft II",
        "w3" => "Warcraft III: Reforged",
        "rtro" => "Blizzard Arcade Collection",
        "wlby" => "Crash Bandicoot 4",
        "viper" => "Call of Duty: Black Ops 4",
        "odin" => "Call of Duty: Modern Warfare",
        "lazr" => "Call of Duty: Modern Warfare II",
        "zeus" => "Call of Duty: Black Ops Cold War",
        "fore" => "Call of Duty: Vanguard",
        "auks" => "Call of Duty",
        "gryphon" => "Warcraft Rumble",
        _ => return None,
    })
}

pub fn is_wow_product(code: &str) -> bool {
    code == "wow" || code.starts_with("wow_") || code.starts_with("wowt") || code == "wowz"
}

/// Battle.net's own entries in the database, not games.
fn is_launcher_product(code: &str) -> bool {
    matches!(code, "agent" | "bna" | "battle.net" | "bts")
}

// ------------------------------------------------------------- product.db

/// One protobuf field as it sits in the bytes.
enum Field<'a> {
    Varint,
    Bytes(&'a [u8]),
    Fixed,
}

fn varint(buf: &[u8], at: &mut usize) -> Option<u64> {
    let mut out = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *buf.get(*at)?;
        *at += 1;
        out |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(out);
        }
    }
    None
}

/// The fields of one protobuf message, in order. `None` when the bytes are
/// not a well-formed message, so a different file never yields junk.
fn fields(buf: &[u8]) -> Option<Vec<(u64, Field<'_>)>> {
    let mut at = 0;
    let mut out = Vec::new();
    while at < buf.len() {
        let key = varint(buf, &mut at)?;
        let (num, wire) = (key >> 3, key & 7);
        if num == 0 {
            return None;
        }
        let field = match wire {
            0 => {
                varint(buf, &mut at)?;
                Field::Varint
            }
            1 => {
                at = at.checked_add(8).filter(|&e| e <= buf.len())?;
                Field::Fixed
            }
            2 => {
                let len = usize::try_from(varint(buf, &mut at)?).ok()?;
                let end = at.checked_add(len).filter(|&e| e <= buf.len())?;
                let bytes = &buf[at..end];
                at = end;
                Field::Bytes(bytes)
            }
            5 => {
                at = at.checked_add(4).filter(|&e| e <= buf.len())?;
                Field::Fixed
            }
            _ => return None,
        };
        out.push((num, field));
    }
    Some(out)
}

fn text(bytes: &[u8]) -> Option<String> {
    std::str::from_utf8(bytes)
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The installs `product.db` lists: (product code, install path as Windows
/// wrote it). The file is the Agent's `Database` message: field 1 repeats a
/// `ProductInstall` (uid = 1, product code = 2, user settings = 3), and the
/// settings' field 1 is the install path.
pub fn parse_product_db(buf: &[u8]) -> Vec<(String, String)> {
    let Some(top) = fields(buf) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (num, f) in top {
        let (1, Field::Bytes(install)) = (num, f) else {
            continue;
        };
        let Some(inner) = fields(install) else {
            continue;
        };
        let mut uid = None;
        let mut code = None;
        let mut path = None;
        for (n, f) in inner {
            match (n, f) {
                (1, Field::Bytes(b)) => uid = text(b),
                (2, Field::Bytes(b)) => code = text(b),
                (3, Field::Bytes(b)) => {
                    path = fields(b).and_then(|s| {
                        s.into_iter().find_map(|(n, f)| match (n, f) {
                            (1, Field::Bytes(b)) => text(b),
                            _ => None,
                        })
                    });
                }
                _ => {}
            }
        }
        if let (Some(code), Some(path)) = (code.or(uid), path) {
            out.push((code, path));
        }
    }
    out
}

/// Where a Windows path from inside a Wine prefix is on this file system.
/// `C:` is the prefix's `drive_c`; other letters go through `dosdevices`.
/// `None` for a path with no drive letter.
pub fn wine_path(prefix: &Path, win: &str) -> Option<PathBuf> {
    let win = win.replace('\\', "/");
    let mut chars = win.chars();
    let letter = chars.next()?.to_ascii_lowercase();
    if !letter.is_ascii_alphabetic() || chars.next() != Some(':') {
        return None;
    }
    let rest = win[2..].trim_start_matches('/');
    let drive = if letter == 'c' {
        prefix.join("drive_c")
    } else {
        // Pushed as text: joining `d:` on Windows would read it as a drive
        // and drop the prefix.
        let mut dir = prefix.join("dosdevices").into_os_string();
        dir.push(format!("/{letter}:"));
        PathBuf::from(dir)
    };
    Some(
        rest.split('/')
            .filter(|s| !s.is_empty())
            .fold(drive, |p, s| p.join(s)),
    )
}

// ---------------------------------------------------------- Wine prefixes

/// Folders that hold Wine prefixes (each child is a prefix), for a home
/// folder.
fn prefix_parents(home: &Path) -> Vec<PathBuf> {
    let share = home.join(".local").join("share");
    let flatpak = |app: &str, rest: &[&str]| {
        rest.iter()
            .fold(home.join(".var").join("app").join(app), |p, s| p.join(s))
    };
    vec![
        // Lutris installs each game in ~/Games/<slug> by default.
        home.join("Games"),
        home.join("Games")
            .join("Heroic")
            .join("Prefixes")
            .join("default"),
        home.join("Games").join("Heroic").join("Prefixes"),
        home.join("Games").join("lutris"),
        share.join("bottles").join("bottles"),
        flatpak("com.usebottles.bottles", &["data", "bottles", "bottles"]),
        flatpak("net.lutris.Lutris", &["data", "lutris", "runners"]),
        home.join("Games").join("faugus"),
        share.join("faugus-launcher").join("prefixes"),
        share.join("wineprefixes"),
        home.join(".PlayOnLinux").join("wineprefix"),
    ]
}

fn is_prefix(dir: &Path) -> bool {
    dir.join("drive_c").is_dir()
}

/// Prefixes the launchers' own game configs name, for prefixes kept outside
/// their default folders: Lutris `games/*.yml` (`prefix: /path`) and Heroic
/// `GamesConfig/*.json` (`"winePrefix": "/path"`). Read as text, line by
/// line; nothing in them is run.
fn configured_prefixes(home: &Path) -> Vec<PathBuf> {
    let config = home.join(".config");
    let dirs = [
        (config.join("lutris").join("games"), "prefix:"),
        (home.join(".local/share/lutris/games"), "prefix:"),
        (config.join("heroic").join("GamesConfig"), "\"winePrefix\":"),
        (
            home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/GamesConfig"),
            "\"winePrefix\":",
        ),
    ];
    let mut out = Vec::new();
    for (dir, key) in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            // Config files are small; a big one isn't a launcher config.
            if e.metadata().map_or(true, |m| m.len() > 256 * 1024) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(e.path()) else {
                continue;
            };
            for line in text.lines() {
                let Some(rest) = line.trim().strip_prefix(key) else {
                    continue;
                };
                let v = rest
                    .trim()
                    .trim_end_matches(',')
                    .trim()
                    .trim_matches(['"', '\'']);
                let v = v
                    .strip_prefix("~/")
                    .map_or_else(|| PathBuf::from(v), |r| home.join(r));
                if v.is_absolute() {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// The Wine prefixes under a home folder, the ones the launchers' configs
/// name, and the Steam library folders' (Proton keeps one per app under
/// `steamapps/compatdata/<id>/pfx`).
pub fn wine_prefixes(home: &Path, steam_libraries: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: BTreeSet<PathBuf> = BTreeSet::new();
    for p in configured_prefixes(home) {
        if is_prefix(&p) {
            out.insert(p);
        }
    }
    for direct in [home.join(".wine"), home.join("Games").join("battlenet")] {
        if is_prefix(&direct) {
            out.insert(direct);
        }
    }
    let mut children = |parent: &Path, pfx: Option<&str>| {
        let Ok(rd) = std::fs::read_dir(parent) else {
            return;
        };
        for e in rd.flatten() {
            let mut dir = e.path();
            if let Some(p) = pfx {
                dir = dir.join(p);
            }
            if is_prefix(&dir) {
                out.insert(dir);
            }
        }
    };
    for parent in prefix_parents(home) {
        children(&parent, None);
    }
    for lib in steam_libraries {
        children(&lib.join("steamapps").join("compatdata"), Some("pfx"));
    }
    out.into_iter().collect()
}

// ------------------------------------------------------------- the finder

/// Default WoW folders, relative to a Windows drive root.
const WOW_DEFAULT_DIRS: [&str; 2] = [
    "Program Files (x86)/World of Warcraft",
    "Program Files/World of Warcraft",
];

fn join_rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, s| p.join(s))
}

/// Whether a folder looks like a WoW install: Blizzard's `.build.info` or a
/// flavor folder with a game exe.
pub fn looks_like_wow(dir: &Path) -> bool {
    dir.join(".build.info").is_file() || !wow_flavors(dir).is_empty()
}

/// The games one `product.db` and the default folders on one Windows drive
/// root give. `map` turns a Windows path into a path on this machine.
fn games_from(
    db: &Path,
    drive_root: &Path,
    prefix: Option<&Path>,
    map: &dyn Fn(&str) -> Option<PathBuf>,
) -> Vec<BnetGame> {
    let mut out = Vec::new();
    if let Ok(buf) = std::fs::read(db) {
        for (code, win) in parse_product_db(&buf) {
            if is_launcher_product(&code) {
                continue;
            }
            let Some(install) = map(&win) else {
                continue;
            };
            if install.is_dir() {
                out.push(BnetGame {
                    product: code,
                    install,
                    prefix: prefix.map(Path::to_path_buf),
                });
            }
        }
    }
    for rel in WOW_DEFAULT_DIRS {
        let dir = join_rel(drive_root, rel);
        if looks_like_wow(&dir) && !out.iter().any(|g| g.install == dir) {
            out.push(BnetGame {
                product: "wow".into(),
                install: dir,
                prefix: prefix.map(Path::to_path_buf),
            });
        }
    }
    out
}

const AGENT_DB: &str = "ProgramData/Battle.net/Agent/product.db";

/// Battle.net games in each Wine prefix.
///
/// Installs are kept by their real path: several prefixes (a Lutris one, a
/// Proton one) often reach one shared game folder through their own drive
/// links or a symlinked `~/Games`, and that folder is one game, not one per
/// prefix.
pub fn find_in_prefixes(prefixes: &[PathBuf]) -> Vec<BnetGame> {
    let mut out = Vec::new();
    for prefix in prefixes {
        let drive_c = prefix.join("drive_c");
        let map = |win: &str| wine_path(prefix, win);
        out.extend(games_from(
            &join_rel(&drive_c, AGENT_DB),
            &drive_c,
            Some(prefix),
            &map,
        ));
    }
    // Wine prefixes are a Linux and macOS thing. On Windows, canonicalize
    // turns a path into a `\\?\` verbatim one, which is no use to show.
    #[cfg(unix)]
    for g in &mut out {
        if let Ok(real) = std::fs::canonicalize(&g.install) {
            g.install = real;
        }
    }
    dedupe(out)
}

/// Battle.net games on a Windows machine whose system drive is `drive_root`
/// (`C:\`).
pub fn find_on_windows(drive_root: &Path) -> Vec<BnetGame> {
    let map = |win: &str| Some(PathBuf::from(win));
    dedupe(games_from(
        &join_rel(drive_root, AGENT_DB),
        drive_root,
        None,
        &map,
    ))
}

/// One entry per install folder; the database's product code wins over the
/// default-folder guess. Every WoW product in one folder (Retail, PTR,
/// Classic) is one game, kept under `wow` when that is one of them; its
/// flavors come from the folder.
fn dedupe(mut games: Vec<BnetGame>) -> Vec<BnetGame> {
    games.sort_by_key(|g| g.product != "wow");
    let mut seen = BTreeSet::new();
    let mut out: Vec<BnetGame> = games
        .into_iter()
        .filter(|g| {
            let product = if g.is_wow() {
                "wow"
            } else {
                g.product.as_str()
            };
            seen.insert((g.install.clone(), product.to_owned()))
        })
        .collect();
    out.sort_by(|a, b| a.name().cmp(&b.name()).then(a.install.cmp(&b.install)));
    out
}

/// This machine's Battle.net games.
pub fn detect() -> Vec<BnetGame> {
    if cfg!(windows) {
        let drive = std::env::var_os("SystemDrive")
            .map(|d| PathBuf::from(format!("{}\\", d.to_string_lossy())))
            .unwrap_or_else(|| PathBuf::from("C:\\"));
        return find_on_windows(&drive);
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let libraries = crate::steam::detect_root()
        .map(|root| crate::steam::library_folders(&root))
        .unwrap_or_default();
    find_in_prefixes(&wine_prefixes(&home, &libraries))
}

// ---------------------------------------------------------------- flavors

/// One WoW game folder inside an install (`_retail_`, `_classic_`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WowFlavor {
    Retail,
    Classic,
    ClassicEra,
    Ptr,
    ClassicPtr,
    Beta,
}

impl WowFlavor {
    pub const ALL: [WowFlavor; 6] = [
        WowFlavor::Retail,
        WowFlavor::Classic,
        WowFlavor::ClassicEra,
        WowFlavor::Ptr,
        WowFlavor::ClassicPtr,
        WowFlavor::Beta,
    ];

    /// The folder under the install root.
    pub fn folder(self) -> &'static str {
        match self {
            WowFlavor::Retail => "_retail_",
            WowFlavor::Classic => "_classic_",
            WowFlavor::ClassicEra => "_classic_era_",
            WowFlavor::Ptr => "_ptr_",
            WowFlavor::ClassicPtr => "_classic_ptr_",
            WowFlavor::Beta => "_beta_",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WowFlavor::Retail => "Retail",
            WowFlavor::Classic => "Classic",
            WowFlavor::ClassicEra => "Classic Era",
            WowFlavor::Ptr => "PTR",
            WowFlavor::ClassicPtr => "Classic PTR",
            WowFlavor::Beta => "Beta",
        }
    }

    /// The game exes this flavor runs as, lowercase.
    pub fn exes(self) -> &'static [&'static str] {
        match self {
            WowFlavor::Retail => &["wow.exe", "wow-64.exe"],
            WowFlavor::Classic | WowFlavor::ClassicEra | WowFlavor::ClassicPtr => {
                &["wowclassic.exe", "wowclassict.exe"]
            }
            WowFlavor::Ptr => &["wowt.exe"],
            WowFlavor::Beta => &["wowb.exe"],
        }
    }

    /// Where this flavor's game folder is under an install root.
    pub fn dir(self, install: &Path) -> PathBuf {
        install.join(self.folder())
    }
}

fn has_exe(dir: &Path, exes: &[&str]) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten().any(|e| {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            exes.contains(&n.as_str())
        })
    })
}

/// The flavors installed under a WoW install root: each flavor folder that
/// holds its game exe, or the `Interface` / `WTF` folders the game makes on
/// first run.
pub fn wow_flavors(install: &Path) -> Vec<WowFlavor> {
    WowFlavor::ALL
        .into_iter()
        .filter(|f| {
            let dir = f.dir(install);
            dir.is_dir()
                && (has_exe(&dir, f.exes())
                    || dir.join("Interface").is_dir()
                    || dir.join("WTF").is_dir())
        })
        .collect()
}

/// Whether a command line runs `exe` (lowercase): the exe is a path segment
/// that ends the program part, as in `C:\...\_retail_\Wow.exe -launch`.
/// Paths hold spaces (`Program Files (x86)`), so the line isn't split on
/// them; the exe has to end the line or be followed by a space.
fn cmdline_runs(cmdline: &str, exe: &str) -> bool {
    let line = cmdline.replace('\0', " ").to_ascii_lowercase();
    line.match_indices(exe).any(|(at, _)| {
        let before_ok = at == 0 || matches!(line.as_bytes()[at - 1], b'\\' | b'/' | b' ' | b'"');
        let after = line.as_bytes().get(at + exe.len());
        before_ok && matches!(after, None | Some(b' ' | b'"'))
    })
}

/// The WoW flavor a process list shows running, from each process's name
/// and command line. Wine and Proton cut a process name to 15 bytes and run
/// the game as `<path>\Wow.exe`, so both are checked. Classic flavors share
/// one exe name and report as Classic; [`running_flavor_dir`] picks the
/// folder.
pub fn wow_running(procs: &[(String, String)]) -> Option<WowFlavor> {
    [
        WowFlavor::Retail,
        WowFlavor::Classic,
        WowFlavor::Ptr,
        WowFlavor::Beta,
    ]
    .into_iter()
    .find(|f| {
        f.exes().iter().any(|exe| {
            procs
                .iter()
                .any(|(comm, cmdline)| comm.eq_ignore_ascii_case(exe) || cmdline_runs(cmdline, exe))
        })
    })
}

/// The installed flavor folder a running exe belongs to. A Classic exe
/// matches Classic, then Classic Era, then Classic PTR, whichever exists.
pub fn running_flavor_dir(install: &Path, running: WowFlavor) -> Option<(WowFlavor, PathBuf)> {
    let installed = wow_flavors(install);
    let order: &[WowFlavor] = match running {
        WowFlavor::Classic | WowFlavor::ClassicEra | WowFlavor::ClassicPtr => &[
            WowFlavor::Classic,
            WowFlavor::ClassicEra,
            WowFlavor::ClassicPtr,
        ],
        WowFlavor::Retail => &[WowFlavor::Retail],
        WowFlavor::Ptr => &[WowFlavor::Ptr],
        WowFlavor::Beta => &[WowFlavor::Beta],
    };
    order
        .iter()
        .find(|f| installed.contains(f))
        .map(|f| (*f, f.dir(install)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loadout-bnet-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    // Minimal protobuf writer for the fixtures.
    fn key(num: u64, wire: u64, out: &mut Vec<u8>) {
        enc(num << 3 | wire, out);
    }
    fn enc(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    fn bytes(num: u64, b: &[u8], out: &mut Vec<u8>) {
        key(num, 2, out);
        enc(b.len() as u64, out);
        out.extend_from_slice(b);
    }
    fn install(uid: &str, code: &str, path: &str) -> Vec<u8> {
        let mut settings = Vec::new();
        bytes(1, path.as_bytes(), &mut settings);
        bytes(2, b"US", &mut settings);
        let mut pi = Vec::new();
        bytes(1, uid.as_bytes(), &mut pi);
        bytes(2, code.as_bytes(), &mut pi);
        bytes(3, &settings, &mut pi);
        // A cached state submessage with a varint, as the real file has.
        let mut state = Vec::new();
        key(1, 0, &mut state);
        enc(1, &mut state);
        bytes(4, &state, &mut pi);
        pi
    }
    fn db(installs: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for i in installs {
            bytes(1, i, &mut out);
        }
        // An active-process field the reader ignores.
        let mut other = Vec::new();
        bytes(1, b"Battle.net.exe", &mut other);
        bytes(2, &other, &mut out);
        out
    }

    #[test]
    fn product_db_lists_each_install_with_its_code_and_path() {
        let buf = db(&[
            install("wow", "wow", "C:/Program Files (x86)/World of Warcraft"),
            install(
                "wow_classic",
                "wow_classic",
                "C:/Program Files (x86)/World of Warcraft",
            ),
            install("agent", "agent", "C:/ProgramData/Battle.net/Agent"),
            install("fenris", "fenris", "D:\\Games\\Diablo IV"),
        ]);
        assert_eq!(
            parse_product_db(&buf),
            vec![
                (
                    "wow".to_string(),
                    "C:/Program Files (x86)/World of Warcraft".to_string()
                ),
                (
                    "wow_classic".to_string(),
                    "C:/Program Files (x86)/World of Warcraft".to_string()
                ),
                (
                    "agent".to_string(),
                    "C:/ProgramData/Battle.net/Agent".to_string()
                ),
                ("fenris".to_string(), "D:\\Games\\Diablo IV".to_string()),
            ]
        );
    }

    #[test]
    fn a_file_that_isnt_a_product_db_gives_nothing() {
        assert!(parse_product_db(b"").is_empty());
        assert!(parse_product_db(b"{\"not\":\"protobuf\"}").is_empty());
        assert!(parse_product_db(&[0x0a, 0xff, 0xff, 0x01]).is_empty());
    }

    #[test]
    fn windows_paths_map_into_the_prefix() {
        let p = Path::new("/home/user/Games/battlenet");
        assert_eq!(
            wine_path(p, "C:\\Program Files (x86)\\World of Warcraft"),
            Some(PathBuf::from(
                "/home/user/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft"
            ))
        );
        assert_eq!(
            wine_path(p, "D:/Games/WoW"),
            Some(PathBuf::from(
                "/home/user/Games/battlenet/dosdevices/d:/Games/WoW"
            ))
        );
        assert_eq!(wine_path(p, "relative/path"), None);
    }

    fn wow_install(root: &Path, flavors: &[(&str, &str)]) {
        put(
            &root.join(".build.info"),
            b"Branch!STRING:0|Product!STRING:0\nus|wow\n",
        );
        for (folder, exe) in flavors {
            put(&root.join(folder).join(exe), b"MZ");
        }
    }

    #[test]
    fn a_lutris_prefix_with_a_product_db_yields_wow_and_its_flavors() {
        let home = tmp("lutris");
        let prefix = home.join("Games").join("battlenet");
        let wow = prefix
            .join("drive_c")
            .join("Program Files (x86)")
            .join("World of Warcraft");
        wow_install(
            &wow,
            &[("_retail_", "Wow.exe"), ("_classic_era_", "WowClassic.exe")],
        );
        put(
            &prefix
                .join("drive_c")
                .join("ProgramData/Battle.net/Agent/product.db"),
            &db(&[
                install("wow", "wow", "C:/Program Files (x86)/World of Warcraft"),
                install(
                    "wow_classic_era",
                    "wow_classic_era",
                    "C:/Program Files (x86)/World of Warcraft",
                ),
                install("agent", "agent", "C:/ProgramData/Battle.net/Agent"),
            ]),
        );
        let prefixes = wine_prefixes(&home, &[]);
        assert_eq!(prefixes, vec![prefix.clone()]);
        let games = find_in_prefixes(&prefixes);
        let found: Vec<(String, &Path)> = games
            .iter()
            .map(|g| (g.name(), g.install.as_path()))
            .collect();
        // Retail and Classic Era share the folder: one game, two flavors.
        assert_eq!(
            found,
            vec![("World of Warcraft".to_string(), wow.as_path())]
        );
        assert!(games
            .iter()
            .all(|g| g.prefix.as_deref() == Some(prefix.as_path())));
        assert_eq!(
            wow_flavors(&wow),
            vec![WowFlavor::Retail, WowFlavor::ClassicEra]
        );
    }

    #[cfg(unix)]
    #[test]
    fn one_wow_folder_reached_from_several_prefixes_is_one_game() {
        let home = tmp("shared");
        let shared = home.join("Library/World of Warcraft");
        wow_install(&shared, &[("_retail_", "Wow.exe"), ("_ptr_", "WowT.exe")]);
        let mut prefixes = Vec::new();
        for name in ["battlenet", "battlenet-2", "lutris-wow"] {
            let prefix = home.join("Games").join(name);
            std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
            std::os::unix::fs::symlink(home.join("Library"), prefix.join("dosdevices/d:")).unwrap();
            put(
                &prefix
                    .join("drive_c")
                    .join("ProgramData/Battle.net/Agent/product.db"),
                &db(&[
                    install("wow", "wow", "D:/World of Warcraft"),
                    install("wow_ptr", "wow_ptr", "D:/World of Warcraft"),
                ]),
            );
            prefixes.push(prefix);
        }
        // A symlinked ~/Games reaching the same prefix a second way.
        std::os::unix::fs::symlink(home.join("Games"), home.join("GamesLink")).unwrap();
        prefixes.push(home.join("GamesLink/battlenet"));
        let games = find_in_prefixes(&prefixes);
        assert_eq!(games.len(), 1, "{games:?}");
        assert_eq!(games[0].product, "wow");
        assert_eq!(games[0].install, std::fs::canonicalize(&shared).unwrap());
        assert_eq!(
            wow_flavors(&games[0].install),
            vec![WowFlavor::Retail, WowFlavor::Ptr]
        );
    }

    #[test]
    fn a_proton_compatdata_prefix_with_no_db_is_found_by_its_default_folder() {
        let home = tmp("proton");
        let lib = home.join(".local/share/Steam");
        let prefix = lib.join("steamapps/compatdata/3141592653/pfx");
        let wow = prefix.join("drive_c/Program Files (x86)/World of Warcraft");
        put(&wow.join("_retail_/Interface/AddOns/.keep"), b"");
        let prefixes = wine_prefixes(&home, &[lib]);
        assert_eq!(prefixes, vec![prefix.clone()]);
        assert_eq!(
            find_in_prefixes(&prefixes),
            vec![BnetGame {
                product: "wow".into(),
                install: wow,
                prefix: Some(prefix),
            }]
        );
    }

    #[test]
    fn bottles_and_plain_wine_prefixes_are_scanned_and_empty_ones_give_nothing() {
        let home = tmp("bottles");
        let bottle = home.join(".local/share/bottles/bottles/Battle-net");
        put(&bottle.join("drive_c/.keep"), b"");
        put(&home.join(".wine/drive_c/.keep"), b"");
        // A folder under ~/Games that isn't a prefix.
        put(&home.join("Games/notes.txt"), b"");
        let prefixes = wine_prefixes(&home, &[]);
        assert_eq!(
            prefixes,
            vec![
                home.join(".local/share/bottles/bottles/Battle-net"),
                home.join(".wine")
            ]
        );
        assert!(find_in_prefixes(&prefixes).is_empty());
    }

    #[test]
    fn prefixes_named_in_lutris_and_heroic_configs_are_scanned() {
        let home = tmp("configs");
        let lutris_pfx = home.join("mnt/ssd/wine/bnet");
        let heroic_pfx = home.join("prefixes/heroic-bnet");
        put(&lutris_pfx.join("drive_c/.keep"), b"");
        put(&heroic_pfx.join("drive_c/.keep"), b"");
        put(
            &home.join(".config/lutris/games/battlenet-1700000000.yml"),
            format!(
                "game:\n  exe: Battle.net.exe\n  prefix: {}\nwine:\n  version: ge\n",
                lutris_pfx.display()
            )
            .as_bytes(),
        );
        put(
            &home.join(".config/heroic/GamesConfig/bnet.json"),
            format!(
                "{{\n  \"bnet\": {{\n    \"winePrefix\": \"{}\",\n  }}\n}}\n",
                heroic_pfx.display()
            )
            .as_bytes(),
        );
        // A prefix line that points at nothing real is skipped.
        put(
            &home.join(".config/lutris/games/gone.yml"),
            b"game:\n  prefix: /nowhere/at/all\n",
        );
        assert_eq!(wine_prefixes(&home, &[]), vec![lutris_pfx, heroic_pfx]);
    }

    #[test]
    fn windows_reads_the_agent_db_and_default_folders_on_the_system_drive() {
        let drive = tmp("windows");
        let wow = drive.join("Program Files (x86)").join("World of Warcraft");
        wow_install(&wow, &[("_retail_", "Wow.exe")]);
        let d4 = drive.join("Games").join("Diablo IV");
        std::fs::create_dir_all(&d4).unwrap();
        put(
            &drive.join("ProgramData/Battle.net/Agent/product.db"),
            &db(&[install("fenris", "fenris", &d4.to_string_lossy())]),
        );
        let names: Vec<String> = find_on_windows(&drive).iter().map(BnetGame::name).collect();
        assert_eq!(
            names,
            vec!["Diablo IV".to_string(), "World of Warcraft".to_string()]
        );
    }

    fn proc(comm: &str, cmdline: &str) -> (String, String) {
        (comm.into(), cmdline.into())
    }

    #[test]
    fn wow_is_seen_running_through_wine_by_name_or_first_argument() {
        assert_eq!(
            wow_running(&[proc("bash", "bash"), proc("Wow.exe", "")]),
            Some(WowFlavor::Retail)
        );
        assert_eq!(
            wow_running(&[proc(
                "wine64-preload",
                "C:\\Program Files (x86)\\World of Warcraft\\_classic_\\WowClassic.exe -launch"
            )]),
            Some(WowFlavor::Classic)
        );
        assert_eq!(
            wow_running(&[proc("Battle.net.exe", "Battle.net.exe")]),
            None
        );
        // A file that only mentions the exe isn't the game running.
        assert_eq!(wow_running(&[proc("vim", "vim /tmp/notwow.exe.txt")]), None);
        assert_eq!(
            wow_running(&[proc(
                "wine64-preload",
                "Z:\\home\\user\\Games\\wow\\_ptr_\\WowT.exe"
            )]),
            Some(WowFlavor::Ptr)
        );
        assert_eq!(wow_running(&[]), None);
    }

    #[test]
    fn a_classic_exe_maps_to_the_classic_folder_that_exists() {
        let root = tmp("flavor");
        wow_install(
            &root,
            &[("_classic_era_", "WowClassic.exe"), ("_retail_", "Wow.exe")],
        );
        assert_eq!(
            running_flavor_dir(&root, WowFlavor::Classic),
            Some((WowFlavor::ClassicEra, root.join("_classic_era_")))
        );
        assert_eq!(
            running_flavor_dir(&root, WowFlavor::Retail),
            Some((WowFlavor::Retail, root.join("_retail_")))
        );
        assert_eq!(running_flavor_dir(&root, WowFlavor::Ptr), None);
    }
}
