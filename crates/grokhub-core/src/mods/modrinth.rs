//! Modrinth (`api.modrinth.com/v2`): keyless search, project and versions.
//! Every request carries a unique User-Agent, as Modrinth asks.

use serde::Deserialize;

use super::allowlist::Source;
use super::ids::GameKey;
use super::source::*;

const API: &str = "https://api.modrinth.com/v2";

/// The User-Agent Modrinth asks every client to send.
pub fn user_agent() -> String {
    format!(
        "Loadout/{} (github.com/blackviperxiii-ui/GrokHub)",
        env!("CARGO_PKG_VERSION")
    )
}

fn project_type(game: GameKey) -> Result<&'static str, SourceError> {
    match game {
        GameKey::Modrinth(t) => Ok(t),
        _ => Err(SourceError::Unsupported("Not a Modrinth game.".into())),
    }
}

fn req(url: String) -> Request {
    Request::get(Source::Modrinth, url).header("user-agent", &user_agent())
}

pub fn search_request(game: GameKey, q: &Query) -> Result<Request, SourceError> {
    let kind = project_type(game)?;
    let index = match (q.text(), q.browse) {
        (Some(_), _) => "relevance",
        (None, Browse::Popular) => "downloads",
        (None, Browse::Trending) => "follows",
        (None, Browse::Updated) => "updated",
    };
    let facets = enc(&format!("[[\"project_type:{kind}\"]]"));
    let text = q
        .text()
        .map(|t| format!("query={}&", enc(t)))
        .unwrap_or_default();
    Ok(req(format!(
        "{API}/search?{text}facets={facets}&index={index}&offset={}&limit={PAGE_SIZE}",
        q.offset()
    ))
    .with_text(q.text().is_some()))
}

#[derive(Deserialize)]
struct SearchBody {
    hits: Vec<Hit>,
    #[serde(default)]
    total_hits: Option<u64>,
}

#[derive(Deserialize)]
struct Hit {
    project_id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    follows: u64,
    #[serde(default)]
    date_modified: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    icon_url: Option<String>,
}

fn page_url(slug: &str) -> String {
    format!("https://modrinth.com/mod/{slug}")
}

pub fn parse_search(body: &[u8]) -> Result<Page, SourceError> {
    let b: SearchBody = parse_json(body)?;
    Ok(Page {
        total: b.total_hits,
        mods: b
            .hits
            .into_iter()
            .map(|h| ModSummary {
                source: Some(Source::Modrinth.into()),
                page_url: page_url(&h.slug),
                id: h.project_id,
                name: h.title,
                author: h.author,
                summary: h.description,
                downloads: h.downloads,
                likes: h.follows,
                updated: unix_from_iso(&h.date_modified),
                adult: false,
                icon_url: h.icon_url.filter(|u| !u.is_empty()),
                categories: h.categories,
            })
            .collect(),
    })
}

fn safe_id(id: &str) -> Result<&str, SourceError> {
    if !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(id)
    } else {
        Err(SourceError::Unsupported(
            "That isn't a Modrinth project id.".into(),
        ))
    }
}

pub fn detail_request(id: &str) -> Result<Request, SourceError> {
    Ok(req(format!("{API}/project/{}", safe_id(id)?)))
}

#[derive(Deserialize)]
struct Project {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    followers: u64,
    #[serde(default)]
    updated: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    icon_url: Option<String>,
}

pub fn parse_detail(body: &[u8]) -> Result<ModDetail, SourceError> {
    let p: Project = parse_json(body)?;
    Ok(ModDetail {
        description: plain_text(&p.body, DESCRIPTION_CAP),
        game_versions: p.game_versions,
        summary: ModSummary {
            source: Some(Source::Modrinth.into()),
            page_url: page_url(&p.slug),
            id: p.id,
            name: p.title,
            author: String::new(),
            summary: p.description,
            downloads: p.downloads,
            likes: p.followers,
            updated: unix_from_iso(&p.updated),
            adult: false,
            icon_url: p.icon_url.filter(|u| !u.is_empty()),
            categories: p.categories,
        },
    })
}

pub fn files_request(id: &str) -> Result<Request, SourceError> {
    Ok(req(format!("{API}/project/{}/version", safe_id(id)?)))
}

#[derive(Deserialize)]
struct Version {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    date_published: String,
    #[serde(default)]
    files: Vec<VFile>,
    #[serde(default)]
    dependencies: Vec<Dep>,
}

#[derive(Deserialize)]
struct VFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    hashes: Hashes,
}

#[derive(Deserialize, Default)]
struct Hashes {
    sha512: Option<String>,
    sha1: Option<String>,
}

#[derive(Deserialize)]
struct Dep {
    project_id: Option<String>,
    #[serde(default)]
    dependency_type: String,
}

/// Newest versions first, one file each (the primary one), at most ten.
pub fn parse_files(body: &[u8]) -> Result<Vec<ModFile>, SourceError> {
    let versions: Vec<Version> = parse_json(body)?;
    Ok(versions
        .into_iter()
        .take(10)
        .enumerate()
        .filter_map(|(i, v)| {
            let file = v
                .files
                .iter()
                .position(|f| f.primary)
                .or(if v.files.is_empty() { None } else { Some(0) })?;
            let f = &v.files[file];
            let hash = f
                .hashes
                .sha512
                .clone()
                .map(FileHash::Sha512)
                .or_else(|| f.hashes.sha1.clone().map(FileHash::Sha1));
            Some(ModFile {
                id: v.id,
                name: v.name,
                file_name: f.filename.clone(),
                version: v.version_number,
                size: f.size,
                hash,
                download: Download::Direct(f.url.clone()),
                game_versions: v.game_versions,
                deps: v
                    .dependencies
                    .into_iter()
                    .filter(|d| d.dependency_type == "required")
                    .filter_map(|d| d.project_id)
                    .collect(),
                primary: i == 0,
                uploaded: unix_from_iso(&v.date_published),
            })
        })
        .collect())
}

pub struct Modrinth;

impl ModSource for Modrinth {
    fn source(&self) -> Source {
        Source::Modrinth
    }

    fn search(&self, net: &dyn Fetch, game: GameKey, q: &Query) -> Result<Page, SourceError> {
        parse_search(&ok_body(net.send(&search_request(game, q)?)?)?)
    }

    fn detail(&self, net: &dyn Fetch, _game: GameKey, id: &str) -> Result<ModDetail, SourceError> {
        parse_detail(&ok_body(net.send(&detail_request(id)?)?)?)
    }

    fn files(
        &self,
        net: &dyn Fetch,
        _game: GameKey,
        id: &str,
    ) -> Result<Vec<ModFile>, SourceError> {
        parse_files(&ok_body(net.send(&files_request(id)?)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::testing::{fixture, Fake};

    const MC: GameKey = GameKey::Modrinth("mod");

    #[test]
    fn search_builds_a_facetted_url_with_the_user_agent() {
        let r = search_request(MC, &Query::search("sodium mod")).unwrap();
        assert_eq!(
            r.url,
            "https://api.modrinth.com/v2/search?query=sodium%20mod&facets=%5B%5B%22project_type%3Amod%22%5D%5D&index=relevance&offset=0&limit=20"
        );
        assert!(r.carries_text);
        assert!(r
            .headers
            .iter()
            .any(|(n, v)| n == "user-agent" && v.starts_with("Loadout/")));
        let mut q = Query::browse(Browse::Updated);
        q.page = 2;
        let r = search_request(MC, &q).unwrap();
        assert!(r.url.contains("index=updated&offset=40"), "{}", r.url);
        assert!(!r.carries_text);
        assert!(search_request(GameKey::CurseForge(432), &q).is_err());
    }

    #[test]
    fn search_detail_and_files_parse_the_fixtures() {
        let fake = Fake::new()
            .on("/v2/search", fixture("modrinth_search.json"))
            .on(
                "/v2/project/AANobbMI/version",
                fixture("modrinth_versions.json"),
            )
            .on("/v2/project/AANobbMI", fixture("modrinth_project.json"));
        let page = Modrinth
            .search(&fake, MC, &Query::search("sodium"))
            .unwrap();
        assert_eq!(page.total, Some(2));
        assert_eq!(page.mods[0].name, "Sodium");
        assert_eq!(page.mods[0].author, "jellysquid3");
        assert_eq!(page.mods[0].downloads, 90_000_000);
        assert_eq!(page.mods[0].page_url, "https://modrinth.com/mod/sodium");
        assert_eq!(page.mods[0].updated, unix_from_iso("2026-10-01T12:00:00Z"));
        let d = Modrinth.detail(&fake, MC, "AANobbMI").unwrap();
        assert_eq!(d.summary.likes, 30_000);
        assert_eq!(d.description, "Sodium\n\nA modern rendering engine.");
        let files = Modrinth.files(&fake, MC, "AANobbMI").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "sodium-fabric-0.6.0.jar");
        assert_eq!(files[0].hash, Some(FileHash::Sha512("abc512".into())));
        assert_eq!(
            files[0].download,
            Download::Direct(
                "https://cdn.modrinth.com/data/AANobbMI/versions/x/sodium-fabric-0.6.0.jar".into()
            )
        );
        assert_eq!(files[0].deps, vec!["P7dR8mSH".to_string()]);
        assert!(Modrinth.detail(&fake, MC, "../etc").is_err());
    }
}
