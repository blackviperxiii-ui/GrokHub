//! The chat's mod tools in the desktop app, the same as the cabin's
//! (`grokhub_agent::ModsOps`). Research goes through the Mods page's own
//! client, cache and egress guard. Setting up a mod puts an Install card on
//! the Mods page and nothing more: the download waits for the user's click
//! there, and the files wait for a second click on the sheet that lists
//! them. Desktop only (`capabilities::mods`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use grokhub_agent::ModsOps;
use grokhub_core::mods::allowlist::{self, Entry};
use grokhub_core::mods::install::{self as engine, Manifest};
use grokhub_core::mods::rail::{installed_mods, Installed, ModGame};
use grokhub_core::mods::{Browse, Download, ModClient, ModFile, ModSummary, Query, Source};

use crate::mods_install::{self, size_label, Requested};

/// Tells the Mods page a new Install card is waiting.
pub const REQUESTED_EVENT: &str = "mods://requested";

/// How many results a search hands the model.
const RESULTS: usize = 10;
/// Descriptions are long; the model gets the start.
const DESCRIPTION: usize = 2_000;
const FILES: usize = mods_install::FILES_SHOWN;

/// Shown for an 18+ mod while Adults only is off.
pub const ADULT_OFF: &str = "That mod is marked 18+, which is off here.";

type Flag = Box<dyn Fn() -> bool + Send + Sync>;
type Told = Box<dyn Fn() + Send + Sync>;

/// The app's mod host: the config folder, whether 18+ mods may show, and
/// how the Mods page hears of a new card.
pub struct Host {
    config: PathBuf,
    adult_ok: Flag,
    told: Told,
}

impl Host {
    pub fn new(config: PathBuf, adult_ok: Flag, told: Told) -> Self {
        Self {
            config,
            adult_ok,
            told,
        }
    }

    fn client(&self) -> Arc<ModClient> {
        crate::mods::client(&self.config)
    }
}

/// Give the chat its mod tools, and mark every moddable game folder on
/// this PC so a shell or file write there parks a card.
pub fn register(host: Host) {
    grokhub_agent::set_mods_host(Arc::new(host));
    std::thread::spawn(|| grokhub_agent::harness::set_mod_roots(&game_dirs()));
}

/// The game as the Mods page sees it: from the rail when this PC has it,
/// else the allowlist's entry with no installs.
fn game_for(entry: &'static Entry) -> ModGame {
    crate::mods::rail_now(grokhub_core::now_ms())
        .games
        .into_iter()
        .find(|g| g.entry.id == entry.id)
        .unwrap_or(ModGame {
            entry,
            last_played: 0,
            playtime_min: 0,
            installs: Vec::new(),
        })
}

fn game_dirs() -> Vec<PathBuf> {
    let steam = grokhub_core::steam::detect_root();
    allowlist::ALLOWLIST
        .iter()
        .filter(|e| e.moddable())
        .filter_map(|e| mods_install::game_dir(&game_for(e), steam.as_deref()))
        .collect()
}

fn entry(game: &str) -> Result<&'static Entry, String> {
    allowlist::by_id(game).ok_or_else(|| allowlist::UNKNOWN_REASON.to_string())
}

fn short(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}

/// "34K" or "1.2M": the model needs the size, not the digits.
fn short_count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}K", n / 1_000),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

pub fn result_line(m: &ModSummary) -> String {
    let mut line = format!(
        "- {} (id {}) by {}: {} downloads",
        m.name,
        m.id,
        m.author,
        short_count(m.downloads)
    );
    if !m.summary.is_empty() {
        line.push_str(". ");
        line.push_str(&short(&m.summary, 200));
    }
    line
}

/// The file a request means: the one named, else the main one.
pub fn pick_file<'a>(files: &'a [ModFile], want: Option<&str>) -> Result<&'a ModFile, String> {
    match want {
        Some(id) => files
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| format!("This mod has no file {id}.")),
        None => files
            .iter()
            .find(|f| f.primary)
            .or(files.first())
            .ok_or_else(|| "This mod has no files to install.".into()),
    }
}

fn how(f: &ModFile) -> &'static str {
    match f.download {
        Download::Direct(_) => "Install downloads it after the user's click",
        Download::Browser(_) => "the user downloads it on the site, then the app picks it up",
        Download::Steam(_) => "Steam installs it when the user subscribes",
    }
}

/// Search, or Popular for an empty query, on one source.
pub fn search_text(
    c: &ModClient,
    entry: &Entry,
    source: Source,
    query: &str,
    adult_ok: bool,
    now_ms: u64,
) -> Result<String, String> {
    let q = if query.trim().is_empty() {
        Query::browse(Browse::Popular)
    } else {
        Query::search(query)
    };
    let page = c
        .search(entry.id, source, &q, now_ms)
        .map_err(|e| e.line())?
        .value;
    let lines: Vec<String> = page
        .mods
        .iter()
        .filter(|m| adult_ok || !m.adult)
        .take(RESULTS)
        .map(result_line)
        .collect();
    if lines.is_empty() {
        return Ok(format!("No mods found on {}.", source.label()));
    }
    Ok(format!("On {}:\n{}", source.label(), lines.join("\n")))
}

/// One mod's description, files and what its main file needs.
pub fn detail_text(
    c: &ModClient,
    entry: &Entry,
    source: Source,
    id: &str,
    adult_ok: bool,
    now_ms: u64,
) -> Result<String, String> {
    let d = c
        .detail(entry.id, source, id, now_ms)
        .map_err(|e| e.line())?
        .value;
    if d.summary.adult && !adult_ok {
        return Err(ADULT_OFF.into());
    }
    // Modrinth's project answer names a team, not an author.
    let by = if d.summary.author.is_empty() {
        String::new()
    } else {
        format!(" by {}", d.summary.author)
    };
    let mut out = format!(
        "{}{by} on {}\nPage: {}\n{}",
        d.summary.name,
        source.label(),
        d.summary.page_url,
        short(&d.description, DESCRIPTION)
    );
    match c.files(entry.id, source, id, now_ms) {
        Ok(files) => {
            out.push_str("\nFiles:");
            for f in files.value.iter().take(FILES) {
                out.push_str(&format!(
                    "\n- file {}: {} {}, {}; {}",
                    f.id,
                    f.name,
                    f.version,
                    size_label(f.size),
                    how(f)
                ));
            }
            let deps = pick_file(&files.value, None)
                .map(|f| f.deps.join(", "))
                .unwrap_or_default();
            if !deps.is_empty() {
                out.push_str(&format!("\nNeeds: {deps}"));
            }
        }
        Err(e) => out.push_str(&format!("\nFiles: {}", e.line())),
    }
    Ok(out)
}

/// What the app installed, then what else the game has.
pub fn installed_text(entry: &Entry, ours: &[Manifest], others: &[Installed]) -> String {
    let app = grokhub_core::brand::product_name();
    let mut lines: Vec<String> = ours
        .iter()
        .map(|m| match &m.offline_only {
            Some(note) => format!("- {} {} (installed by {app}). {note}", m.name, m.version),
            None => format!("- {} {} (installed by {app})", m.name, m.version),
        })
        .collect();
    lines.extend(
        others
            .iter()
            .map(|r| format!("- {} ({})", r.name, r.detail)),
    );
    if lines.is_empty() {
        return format!("No mods found for {}.", entry.name);
    }
    lines.join("\n")
}

/// The Install card for one mod's file, and what the model is told about
/// it. Fetches the mod's details and files; downloads and writes nothing.
pub fn prepare_request(
    c: &ModClient,
    game: ModGame,
    source: Source,
    (id, file): (&str, Option<&str>),
    adult_ok: bool,
    blocked: Option<String>,
    now_ms: u64,
) -> Result<(String, Requested), String> {
    let entry = game.entry;
    let d = c
        .detail(entry.id, source, id, now_ms)
        .map_err(|e| e.line())?
        .value;
    if d.summary.adult && !adult_ok {
        return Err(ADULT_OFF.into());
    }
    let files = c
        .files(entry.id, source, id, now_ms)
        .map_err(|e| e.line())?
        .value;
    let f = pick_file(&files, file)?.clone();
    let mut text = format!(
        "Install {} {} for {}? File {}, {}, from {}; {}.",
        d.summary.name,
        f.version,
        entry.name,
        f.file_name,
        size_label(f.size),
        source.label(),
        how(&f)
    );
    if let Some(why) = blocked {
        text.push_str(&format!(" The card says instead: {why}"));
    }
    text.push_str(&format!(" {}", entry.risk));
    Ok((
        text,
        Requested {
            game,
            source,
            m: d.summary,
            f,
        },
    ))
}

fn installed_for(config: &Path, game: &ModGame) -> String {
    let steam = grokhub_core::steam::detect_root();
    installed_text(
        game.entry,
        &engine::installed(config, Some(game.entry.id)),
        &installed_mods(game, steam.as_deref()),
    )
}

impl ModsOps for Host {
    fn search(&self, game: &str, source: Source, query: &str) -> Result<String, String> {
        let entry = entry(game)?;
        search_text(
            &self.client(),
            entry,
            source,
            query,
            (self.adult_ok)(),
            grokhub_core::now_ms(),
        )
    }

    fn detail(&self, game: &str, source: Source, id: &str) -> Result<String, String> {
        let entry = entry(game)?;
        detail_text(
            &self.client(),
            entry,
            source,
            id,
            (self.adult_ok)(),
            grokhub_core::now_ms(),
        )
    }

    fn installed(&self, game: &str) -> Result<String, String> {
        let entry = entry(game)?;
        Ok(installed_for(&self.config, &game_for(entry)))
    }

    fn prepare(
        &self,
        game: &str,
        source: Source,
        id: &str,
        file: Option<&str>,
    ) -> Result<String, String> {
        let g = game_for(entry(game)?);
        let blocked = mods_install::target(&g).err();
        let (text, r) = prepare_request(
            &self.client(),
            g,
            source,
            (id, file),
            (self.adult_ok)(),
            blocked,
            grokhub_core::now_ms(),
        )?;
        mods_install::request(r);
        (self.told)();
        Ok(text)
    }
}

#[cfg(test)]
#[path = "mods_agent_tests.rs"]
mod tests;
