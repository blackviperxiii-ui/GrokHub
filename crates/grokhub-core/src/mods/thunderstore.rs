//! Thunderstore: the community's package list (`/c/{community}/api/v1/
//! package/`), kept as a compact on-disk list and searched locally, and one
//! package's detail from the experimental package endpoint. Keyless. The full
//! list is fetched off the UI thread and cached, never per keystroke.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::allowlist::Source;
use super::ids::GameKey;
use super::source::*;

const SITE: &str = "https://thunderstore.io";

fn community(game: GameKey) -> Result<&'static str, SourceError> {
    match game {
        GameKey::Thunderstore(c) => Ok(c),
        _ => Err(SourceError::Unsupported("Not a Thunderstore game.".into())),
    }
}

/// One package, as much as a result card and the suggestions need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub full_name: String,
    pub name: String,
    pub owner: String,
    pub description: String,
    pub downloads: u64,
    pub rating: u64,
    pub updated: u64,
    pub nsfw: bool,
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub page_url: String,
}

impl Package {
    fn summary(&self) -> ModSummary {
        ModSummary {
            source: Some(Source::Thunderstore.into()),
            id: self.full_name.clone(),
            name: self.name.replace('_', " "),
            author: self.owner.clone(),
            summary: self.description.clone(),
            downloads: self.downloads,
            likes: self.rating,
            updated: self.updated,
            adult: self.nsfw,
            page_url: self.page_url.clone(),
            icon_url: self.icon.clone(),
            categories: self.categories.clone(),
        }
    }
}

pub fn list_request(game: GameKey) -> Result<Request, SourceError> {
    Ok(Request::get(
        Source::Thunderstore,
        format!("{SITE}/c/{}/api/v1/package/", community(game)?),
    ))
}

#[derive(Deserialize)]
struct V1Package {
    name: String,
    full_name: String,
    owner: String,
    #[serde(default)]
    package_url: String,
    #[serde(default)]
    date_updated: String,
    #[serde(default)]
    rating_score: u64,
    #[serde(default)]
    is_deprecated: bool,
    #[serde(default)]
    has_nsfw_content: bool,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    versions: Vec<V1Version>,
}

#[derive(Deserialize)]
struct V1Version {
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    downloads: u64,
}

/// The compact list: deprecated packages dropped.
pub fn parse_list(body: &[u8]) -> Result<Vec<Package>, SourceError> {
    let list: Vec<V1Package> = parse_json(body)?;
    Ok(list
        .into_iter()
        .filter(|p| !p.is_deprecated)
        .map(|p| {
            let latest = p.versions.first();
            Package {
                description: latest.map(|v| v.description.clone()).unwrap_or_default(),
                icon: latest.and_then(|v| v.icon.clone()),
                downloads: p.versions.iter().map(|v| v.downloads).sum(),
                rating: p.rating_score,
                updated: unix_from_iso(&p.date_updated),
                nsfw: p.has_nsfw_content,
                categories: p.categories,
                page_url: p.package_url,
                full_name: p.full_name,
                name: p.name,
                owner: p.owner,
            }
        })
        .collect())
}

/// One page of the list for a tab or a search. Text matches the name, owner
/// or description, ignoring case; results keep the tab's order.
pub fn search_list(list: &[Package], q: &Query) -> Page {
    let needle = q.text().map(str::to_lowercase);
    let mut hits: Vec<&Package> = list
        .iter()
        .filter(|p| match &needle {
            None => true,
            Some(n) => {
                p.full_name.to_lowercase().contains(n)
                    || p.name.replace('_', " ").to_lowercase().contains(n)
                    || p.description.to_lowercase().contains(n)
            }
        })
        .collect();
    match (needle.is_some(), q.browse) {
        (true, _) | (false, Browse::Popular) => hits.sort_by(|a, b| {
            b.downloads
                .cmp(&a.downloads)
                .then(a.full_name.cmp(&b.full_name))
        }),
        (false, Browse::Trending) => {
            hits.sort_by(|a, b| b.rating.cmp(&a.rating).then(a.full_name.cmp(&b.full_name)))
        }
        (false, Browse::Updated) => hits.sort_by(|a, b| {
            b.updated
                .cmp(&a.updated)
                .then(a.full_name.cmp(&b.full_name))
        }),
    }
    let total = hits.len() as u64;
    Page {
        mods: hits
            .into_iter()
            .skip(q.offset() as usize)
            .take(PAGE_SIZE as usize)
            .map(Package::summary)
            .collect(),
        total: Some(total),
    }
}

/// `Owner-Name` split on the first dash (owners have no dashes).
fn split_id(id: &str) -> Result<(&str, &str), SourceError> {
    let bad = || SourceError::Unsupported("That isn't a Thunderstore package.".into());
    let (owner, name) = id.split_once('-').ok_or_else(bad)?;
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok(owner) && ok(name) {
        Ok((owner, name))
    } else {
        Err(bad())
    }
}

pub fn detail_request(id: &str) -> Result<Request, SourceError> {
    let (owner, name) = split_id(id)?;
    Ok(Request::get(
        Source::Thunderstore,
        format!("{SITE}/api/experimental/package/{owner}/{name}/"),
    ))
}

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// The package (it must be listed in this game's community).
fn package(game: GameKey, body: &[u8]) -> Result<Value, SourceError> {
    let c = community(game)?;
    let v: Value = parse_json(body)?;
    let listed = v["community_listings"]
        .as_array()
        .is_some_and(|l| l.iter().any(|x| x["community"].as_str() == Some(c)));
    if !listed {
        return Err(SourceError::Unsupported(
            "That package isn't listed for this game.".into(),
        ));
    }
    Ok(v)
}

pub fn parse_detail(game: GameKey, body: &[u8]) -> Result<ModDetail, SourceError> {
    let c = community(game)?;
    let v = package(game, body)?;
    let latest = &v["latest"];
    let listing = v["community_listings"]
        .as_array()
        .and_then(|l| l.iter().find(|x| x["community"].as_str() == Some(c)))
        .cloned()
        .unwrap_or(Value::Null);
    let full = v["full_name"].as_str().unwrap_or_default().to_string();
    let name = v["name"].as_str().unwrap_or_default().replace('_', " ");
    let description = latest["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Ok(ModDetail {
        summary: ModSummary {
            source: Some(Source::Thunderstore.into()),
            id: full.clone(),
            name,
            author: v["owner"].as_str().unwrap_or_default().to_string(),
            summary: description.clone(),
            downloads: num(&v["total_downloads"]),
            likes: num(&v["rating_score"]),
            updated: unix_from_iso(v["date_updated"].as_str().unwrap_or_default()),
            adult: listing["has_nsfw_content"].as_bool().unwrap_or(false),
            page_url: v["package_url"].as_str().unwrap_or_default().to_string(),
            icon_url: latest["icon"].as_str().map(str::to_string),
            categories: listing["categories"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        },
        description: plain_text(&description, DESCRIPTION_CAP),
        game_versions: Vec::new(),
    })
}

/// The latest version's archive. Dependencies are `Owner-Name-1.2.3`;
/// they come back as package ids (`Owner-Name`).
pub fn parse_files(game: GameKey, body: &[u8]) -> Result<Vec<ModFile>, SourceError> {
    let v = package(game, body)?;
    let latest = &v["latest"];
    let url = latest["download_url"]
        .as_str()
        .ok_or_else(|| SourceError::Parse("no download".into()))?;
    let full = latest["full_name"].as_str().unwrap_or_default();
    Ok(vec![ModFile {
        id: full.to_string(),
        name: v["name"].as_str().unwrap_or_default().replace('_', " "),
        file_name: format!("{full}.zip"),
        version: latest["version_number"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        size: num(&latest["file_size"]),
        hash: None,
        download: Download::Direct(url.to_string()),
        game_versions: Vec::new(),
        deps: latest["dependencies"]
            .as_array()
            .map(|d| {
                d.iter()
                    .filter_map(|x| x.as_str())
                    .filter_map(|s| s.rsplit_once('-').map(|(id, _)| id.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        primary: true,
        uploaded: unix_from_iso(latest["date_created"].as_str().unwrap_or_default()),
    }])
}

pub struct Thunderstore;

impl ModSource for Thunderstore {
    fn source(&self) -> Source {
        Source::Thunderstore
    }

    /// Uncached: [`super::client::ModClient`] keeps the list on disk and
    /// calls [`search_list`] itself.
    fn search(&self, net: &dyn Fetch, game: GameKey, q: &Query) -> Result<Page, SourceError> {
        let list = parse_list(&ok_body(net.send(&list_request(game)?)?)?)?;
        Ok(search_list(&list, q))
    }

    fn detail(&self, net: &dyn Fetch, game: GameKey, id: &str) -> Result<ModDetail, SourceError> {
        parse_detail(game, &ok_body(net.send(&detail_request(id)?)?)?)
    }

    fn files(&self, net: &dyn Fetch, game: GameKey, id: &str) -> Result<Vec<ModFile>, SourceError> {
        parse_files(game, &ok_body(net.send(&detail_request(id)?)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::testing::{fixture, Fake};

    const LC: GameKey = GameKey::Thunderstore("lethal-company");

    #[test]
    fn the_list_is_compacted_and_searched_locally() {
        let list = parse_list(fixture("thunderstore_list.json").as_bytes()).unwrap();
        assert_eq!(list.len(), 3, "deprecated packages are dropped");
        assert_eq!(list[0].downloads, 1_500);
        let page = search_list(&list, &Query::browse(Browse::Popular));
        let names: Vec<&str> = page.mods.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            names,
            ["BepInEx-BepInExPack", "x753-More_Suits", "Lewd-Thing"]
        );
        let page = search_list(&list, &Query::browse(Browse::Trending));
        assert_eq!(page.mods[0].id, "x753-More_Suits");
        let page = search_list(&list, &Query::search("more suits"));
        assert_eq!(page.total, Some(1));
        assert_eq!(page.mods[0].name, "More Suits");
        assert!(search_list(&list, &Query::search("lewd")).mods[0].adult);
        let mut q = Query::browse(Browse::Popular);
        q.page = 1;
        assert!(search_list(&list, &q).mods.is_empty());
    }

    #[test]
    fn detail_and_files_come_from_the_package_endpoint() {
        let fake = Fake::new().on(
            "/api/experimental/package/x753/More_Suits/",
            fixture("thunderstore_package.json"),
        );
        let d = Thunderstore.detail(&fake, LC, "x753-More_Suits").unwrap();
        assert_eq!(d.summary.downloads, 9_000_000);
        assert_eq!(d.summary.categories, vec!["Suits".to_string()]);
        let files = Thunderstore.files(&fake, LC, "x753-More_Suits").unwrap();
        assert_eq!(
            files[0].download,
            Download::Direct(
                "https://thunderstore.io/package/download/x753/More_Suits/1.5.2/".into()
            )
        );
        assert_eq!(files[0].deps, vec!["BepInEx-BepInExPack".to_string()]);
        assert_eq!(files[0].version, "1.5.2");
        assert!(Thunderstore
            .detail(&fake, GameKey::Thunderstore("valheim"), "x753-More_Suits")
            .is_err());
        assert!(detail_request("../x").is_err());
        assert!(detail_request("a-b/c").is_err());
    }
}
