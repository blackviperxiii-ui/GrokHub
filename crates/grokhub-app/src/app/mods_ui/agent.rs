//! The app side of the chat's mod tools. Research goes through the same
//! client, cache and egress guard as the Mods page. Setting up a mod puts an
//! Install card on the Mods page and nothing more: the download waits for
//! the user's click there, and the files wait for a second click on the
//! sheet that lists them.

use std::path::PathBuf;
use std::sync::Arc;

use grokhub_agent::ModsOps;
use grokhub_core::mods::allowlist::{self, Entry};
use grokhub_core::mods::{Browse, Download, ModFile, ModSummary, Query, Source};

use super::{drawer, install, jobs, ModGame};

/// How many results a search hands the model.
const RESULTS: usize = 10;
/// Descriptions are long; the model gets the start.
const DESCRIPTION: usize = 2_000;
const FILES: usize = 6;

struct Host;

/// Give the chat its mod tools, and mark every allowlisted game folder on
/// this PC so a shell or file write there parks a card.
pub(in crate::app) fn register() {
    grokhub_agent::set_mods_host(Arc::new(Host));
    std::thread::spawn(|| grokhub_agent::harness::set_mod_roots(&game_dirs()));
}

/// The game as the Mods page sees it: WoW with its Battle.net installs.
fn game_for(entry: &'static Entry) -> ModGame {
    let installs = if entry.bnet_products.is_empty() {
        Vec::new()
    } else {
        grokhub_core::battlenet::detect()
            .into_iter()
            .filter(|g| entry.bnet_products.contains(&g.product.as_str()))
            .map(|g| g.install)
            .collect()
    };
    ModGame {
        entry,
        last_played: 0,
        playtime_min: 0,
        installs,
    }
}

fn game_dirs() -> Vec<PathBuf> {
    let steam = grokhub_core::steam::detect_root();
    allowlist::ALLOWLIST
        .iter()
        .filter(|e| e.moddable())
        .filter_map(|e| install::game_dir(&game_for(e), steam.as_deref()))
        .collect()
}

fn entry(game: &str) -> Result<&'static Entry, String> {
    allowlist::by_id(game).ok_or_else(|| allowlist::UNKNOWN_REASON.to_string())
}

fn adult_ok() -> bool {
    let cfg = crate::config::load();
    cfg.explicit_personalities && cfg.adult_confirmed
}

fn short(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}

pub(super) fn result_line(m: &ModSummary) -> String {
    let mut line = format!(
        "- {} (id {}) by {}: {} downloads",
        m.name,
        m.id,
        m.author,
        super::short_count(m.downloads)
    );
    if !m.summary.is_empty() {
        line.push_str(". ");
        line.push_str(&short(&m.summary, 200));
    }
    line
}

/// The file a request means: the one named, else the main one.
pub(super) fn pick_file<'a>(
    files: &'a [ModFile],
    want: Option<&str>,
) -> Result<&'a ModFile, String> {
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
        Download::Browser(_) => "the user downloads it on the site, then Loadout picks it up",
        Download::Steam(_) => "Steam installs it when the user subscribes",
    }
}

impl ModsOps for Host {
    fn search(&self, game: &str, source: Source, query: &str) -> Result<String, String> {
        let entry = entry(game)?;
        let q = if query.trim().is_empty() {
            Query::browse(Browse::Popular)
        } else {
            Query::search(query)
        };
        let page = jobs::shared_client()
            .search(entry.id, source, &q, grokhub_core::now_ms())
            .map_err(|e| e.line())?
            .value;
        let ok = adult_ok();
        let lines: Vec<String> = page
            .mods
            .iter()
            .filter(|m| ok || !m.adult)
            .take(RESULTS)
            .map(result_line)
            .collect();
        if lines.is_empty() {
            return Ok(format!("No mods found on {}.", source.label()));
        }
        Ok(format!("On {}:\n{}", source.label(), lines.join("\n")))
    }

    fn detail(&self, game: &str, source: Source, id: &str) -> Result<String, String> {
        let entry = entry(game)?;
        let c = jobs::shared_client();
        let now = grokhub_core::now_ms();
        let d = c
            .detail(entry.id, source, id, now)
            .map_err(|e| e.line())?
            .value;
        if d.summary.adult && !adult_ok() {
            return Err("That mod is marked 18+, which is off here.".into());
        }
        let mut out = format!(
            "{} by {} on {}\nPage: {}\n{}",
            d.summary.name,
            d.summary.author,
            source.label(),
            d.summary.page_url,
            short(&d.description, DESCRIPTION)
        );
        match c.files(entry.id, source, id, now) {
            Ok(files) => {
                out.push_str("\nFiles:");
                for f in files.value.iter().take(FILES) {
                    out.push_str(&format!(
                        "\n- file {}: {} {}, {}; {}",
                        f.id,
                        f.name,
                        f.version,
                        drawer::size_label(f.size),
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

    fn installed(&self, game: &str) -> Result<String, String> {
        let entry = entry(game)?;
        let g = game_for(entry);
        let mut lines: Vec<String> = install::installed(entry.id)
            .iter()
            .map(|m| format!("- {} {} (installed by Loadout)", m.name, m.version))
            .collect();
        let steam = grokhub_core::steam::detect_root();
        lines.extend(
            super::installed_mods(&g, steam.as_deref())
                .iter()
                .map(|r| format!("- {} ({})", r.name, r.detail)),
        );
        if lines.is_empty() {
            return Ok(format!("No mods found for {}.", entry.name));
        }
        Ok(lines.join("\n"))
    }

    fn prepare(
        &self,
        game: &str,
        source: Source,
        id: &str,
        file: Option<&str>,
    ) -> Result<String, String> {
        let entry = entry(game)?;
        let c = jobs::shared_client();
        let now = grokhub_core::now_ms();
        let d = c
            .detail(entry.id, source, id, now)
            .map_err(|e| e.line())?
            .value;
        if d.summary.adult && !adult_ok() {
            return Err("That mod is marked 18+, which is off here.".into());
        }
        let files = c
            .files(entry.id, source, id, now)
            .map_err(|e| e.line())?
            .value;
        let f = pick_file(&files, file)?.clone();
        let g = game_for(entry);
        let mut text = format!(
            "Install {} {} for {}? File {}, {}, from {}; {}.",
            d.summary.name,
            f.version,
            entry.name,
            f.file_name,
            drawer::size_label(f.size),
            source.label(),
            how(&f)
        );
        if let Some(why) = install::blocked(&g) {
            text.push_str(&format!(" The card says instead: {why}"));
        }
        text.push_str(&format!(" {}", entry.risk));
        install::request(install::Requested {
            game: g,
            source,
            m: d.summary,
            f,
        });
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, primary: bool) -> ModFile {
        ModFile {
            id: id.into(),
            name: "Main".into(),
            file_name: format!("{id}.zip"),
            version: "1".into(),
            size: 1_000,
            hash: None,
            download: Download::Direct("https://cdn.modrinth.com/x".into()),
            game_versions: vec![],
            deps: vec![],
            primary,
            uploaded: 0,
        }
    }

    #[test]
    fn a_request_names_its_file_or_takes_the_main_one() {
        let files = [file("a", false), file("b", true)];
        assert_eq!(pick_file(&files, None).unwrap().id, "b");
        assert_eq!(pick_file(&files, Some("a")).unwrap().id, "a");
        assert!(pick_file(&files, Some("z")).is_err());
        assert!(pick_file(&[], None).is_err());
        let m = ModSummary {
            id: "1".into(),
            name: "Sodium".into(),
            author: "jellysquid".into(),
            downloads: 34_500,
            summary: "  Fast\n rendering ".into(),
            ..Default::default()
        };
        assert_eq!(
            result_line(&m),
            "- Sodium (id 1) by jellysquid: 34K downloads. Fast rendering"
        );
        assert!(entry("apex").is_ok(), "the tools check the allowlist first");
        assert!(entry("nope").is_err());
    }
}
