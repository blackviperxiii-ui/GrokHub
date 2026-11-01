//! CurseForge (`api.curseforge.com/v1`): search, mod, description and files,
//! all with the build's `x-api-key`. With no key the source stays hidden;
//! the key is a build-time secret and is never committed or logged.
//!
//! A file whose author turned off third-party downloads comes back with
//! `downloadUrl: null`; that file downloads on CurseForge's own site instead.

use serde_json::Value;

use super::allowlist::Source;
use super::ids::GameKey;
use super::source::*;

const API: &str = "https://api.curseforge.com/v1";
/// Minecraft's "Mods" class, so resource packs and worlds stay out.
const MINECRAFT: u32 = 432;
const MINECRAFT_MODS: u32 = 6;

fn game(game: GameKey) -> Result<u32, SourceError> {
    match game {
        GameKey::CurseForge(id) => Ok(id),
        _ => Err(SourceError::Unsupported("Not a CurseForge game.".into())),
    }
}

fn keyed(url: String, key: Option<&str>) -> Result<Request, SourceError> {
    let key = key.ok_or(SourceError::NeedsKey(Source::CurseForge))?;
    Ok(Request::get(Source::CurseForge, url)
        .header("x-api-key", key)
        .header("accept", "application/json"))
}

fn mod_id(id: &str) -> Result<u64, SourceError> {
    id.parse()
        .map_err(|_| SourceError::Unsupported("That isn't a CurseForge mod id.".into()))
}

/// `ModsSearchSortField` for each tab.
fn sort_field(q: &Query) -> u32 {
    match (q.text(), q.browse) {
        (Some(_), _) | (None, Browse::Trending) => 2, // Popularity
        (None, Browse::Popular) => 6,                 // TotalDownloads
        (None, Browse::Updated) => 3,                 // LastUpdated
    }
}

pub fn search_request(
    game_key: GameKey,
    q: &Query,
    key: Option<&str>,
) -> Result<Request, SourceError> {
    let gid = game(game_key)?;
    let class = if gid == MINECRAFT {
        format!("&classId={MINECRAFT_MODS}")
    } else {
        String::new()
    };
    let text = q
        .text()
        .map(|t| format!("&searchFilter={}", enc(t)))
        .unwrap_or_default();
    Ok(keyed(
        format!(
            "{API}/mods/search?gameId={gid}{class}{text}&sortField={}&sortOrder=desc&pageSize={PAGE_SIZE}&index={}",
            sort_field(q),
            q.offset()
        ),
        key,
    )?
    .with_text(q.text().is_some()))
}

fn num(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
        .unwrap_or(0)
}

fn summary(m: &Value) -> Option<ModSummary> {
    let id = num(&m["id"]);
    if id == 0 || m["isAvailable"].as_bool() == Some(false) {
        return None;
    }
    Some(ModSummary {
        source: Some(Source::CurseForge.into()),
        id: id.to_string(),
        name: m["name"].as_str().unwrap_or_default().to_string(),
        author: m["authors"][0]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        summary: plain_text(m["summary"].as_str().unwrap_or_default(), 400),
        downloads: num(&m["downloadCount"]),
        likes: num(&m["thumbsUpCount"]),
        updated: unix_from_iso(m["dateModified"].as_str().unwrap_or_default()),
        adult: false,
        page_url: m["links"]["websiteUrl"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| project_url(id)),
        icon_url: m["logo"]["thumbnailUrl"].as_str().map(str::to_string),
        categories: m["categories"]
            .as_array()
            .map(|c| {
                c.iter()
                    .filter_map(|x| x["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn project_url(id: u64) -> String {
    format!("https://www.curseforge.com/projects/{id}")
}

pub fn parse_search(body: &[u8]) -> Result<Page, SourceError> {
    let v: Value = parse_json(body)?;
    let list = v["data"]
        .as_array()
        .ok_or_else(|| SourceError::Parse("no mods in the answer".into()))?;
    Ok(Page {
        mods: list.iter().filter_map(summary).collect(),
        total: v["pagination"]["totalCount"].as_u64(),
    })
}

pub fn detail_request(id: &str, key: Option<&str>) -> Result<Request, SourceError> {
    keyed(format!("{API}/mods/{}", mod_id(id)?), key)
}

pub fn description_request(id: &str, key: Option<&str>) -> Result<Request, SourceError> {
    keyed(format!("{API}/mods/{}/description", mod_id(id)?), key)
}

/// The mod (and its game, which must be the one asked about), plus the HTML
/// description from its own endpoint.
pub fn parse_detail(
    game_key: GameKey,
    body: &[u8],
    description: &[u8],
) -> Result<ModDetail, SourceError> {
    let gid = game(game_key)?;
    let v: Value = parse_json(body)?;
    let m = &v["data"];
    if num(&m["gameId"]) != u64::from(gid) {
        return Err(SourceError::Unsupported(
            "That CurseForge mod is for another game.".into(),
        ));
    }
    let summary = summary(m).ok_or_else(|| SourceError::Parse("no such mod".into()))?;
    let d: Value = parse_json(description).unwrap_or(Value::Null);
    Ok(ModDetail {
        description: plain_text(d["data"].as_str().unwrap_or_default(), DESCRIPTION_CAP),
        game_versions: m["latestFilesIndexes"]
            .as_array()
            .map(|l| {
                let mut v: Vec<String> = l
                    .iter()
                    .filter_map(|x| x["gameVersion"].as_str().map(str::to_string))
                    .collect();
                v.dedup();
                v
            })
            .unwrap_or_default(),
        summary,
    })
}

pub fn files_request(id: &str, key: Option<&str>) -> Result<Request, SourceError> {
    keyed(format!("{API}/mods/{}/files?pageSize=20", mod_id(id)?), key)
}

pub fn parse_files(id: &str, body: &[u8]) -> Result<Vec<ModFile>, SourceError> {
    let mod_id = mod_id(id)?;
    let v: Value = parse_json(body)?;
    let list = v["data"]
        .as_array()
        .ok_or_else(|| SourceError::Parse("no files in the answer".into()))?;
    let mut files: Vec<ModFile> = list
        .iter()
        .filter(|f| f["isAvailable"].as_bool() != Some(false))
        .filter_map(|f| {
            let fid = num(&f["id"]);
            if fid == 0 {
                return None;
            }
            let hashes = f["hashes"].as_array().cloned().unwrap_or_default();
            let hash_of = |algo: u64| {
                hashes
                    .iter()
                    .find(|h| num(&h["algo"]) == algo)
                    .and_then(|h| h["value"].as_str().map(str::to_string))
            };
            let hash = hash_of(1)
                .map(FileHash::Sha1)
                .or_else(|| hash_of(2).map(FileHash::Md5));
            Some(ModFile {
                id: fid.to_string(),
                name: f["displayName"].as_str().unwrap_or_default().to_string(),
                file_name: f["fileName"].as_str().unwrap_or_default().to_string(),
                version: f["displayName"].as_str().unwrap_or_default().to_string(),
                size: num(&f["fileLength"]),
                hash,
                download: match f["downloadUrl"].as_str() {
                    Some(url) if !url.is_empty() => Download::Direct(url.to_string()),
                    _ => Download::Browser(format!("{}/files/{fid}", project_url(mod_id))),
                },
                game_versions: f["gameVersions"]
                    .as_array()
                    .map(|g| {
                        g.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                deps: f["dependencies"]
                    .as_array()
                    .map(|d| {
                        d.iter()
                            .filter(|x| num(&x["relationType"]) == 3)
                            .map(|x| num(&x["modId"]).to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
                primary: false,
                uploaded: unix_from_iso(f["fileDate"].as_str().unwrap_or_default()),
            })
        })
        .collect();
    files.sort_by(|a, b| b.uploaded.cmp(&a.uploaded).then(a.id.cmp(&b.id)));
    if let Some(first) = files.first_mut() {
        first.primary = true;
    }
    Ok(files)
}

pub struct CurseForge {
    pub key: Option<String>,
}

impl ModSource for CurseForge {
    fn source(&self) -> Source {
        Source::CurseForge
    }

    fn search(&self, net: &dyn Fetch, game: GameKey, q: &Query) -> Result<Page, SourceError> {
        let req = search_request(game, q, self.key.as_deref())?;
        parse_search(&ok_body(net.send(&req)?)?)
    }

    fn detail(&self, net: &dyn Fetch, game: GameKey, id: &str) -> Result<ModDetail, SourceError> {
        let body = ok_body(net.send(&detail_request(id, self.key.as_deref())?)?)?;
        let desc = net
            .send(&description_request(id, self.key.as_deref())?)
            .and_then(ok_body)
            .unwrap_or_default();
        parse_detail(game, &body, &desc)
    }

    fn files(
        &self,
        net: &dyn Fetch,
        _game: GameKey,
        id: &str,
    ) -> Result<Vec<ModFile>, SourceError> {
        parse_files(
            id,
            &ok_body(net.send(&files_request(id, self.key.as_deref())?)?)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::testing::{fixture, Fake};

    const WOW: GameKey = GameKey::CurseForge(1);

    #[test]
    fn no_key_means_no_request() {
        assert_eq!(
            search_request(WOW, &Query::search("details"), None),
            Err(SourceError::NeedsKey(Source::CurseForge))
        );
        let fake = Fake::new();
        assert!(CurseForge { key: None }.files(&fake, WOW, "1").is_err());
        assert!(fake.sent().is_empty());
    }

    #[test]
    fn search_url_names_the_game_class_and_sort() {
        let r = search_request(GameKey::CurseForge(432), &Query::search("jei"), Some("k")).unwrap();
        assert_eq!(
            r.url,
            "https://api.curseforge.com/v1/mods/search?gameId=432&classId=6&searchFilter=jei&sortField=2&sortOrder=desc&pageSize=20&index=0"
        );
        assert!(r.headers.contains(&("x-api-key".into(), "k".into())));
        let r = search_request(WOW, &Query::browse(Browse::Popular), Some("k")).unwrap();
        assert!(r.url.contains("gameId=1&sortField=6"), "{}", r.url);
    }

    #[test]
    fn search_detail_and_files_parse() {
        let page = parse_search(fixture("curseforge_search.json").as_bytes()).unwrap();
        assert_eq!(page.total, Some(500));
        assert_eq!(page.mods[0].name, "Details! Damage Meter");
        assert_eq!(page.mods[0].downloads, 300_000_000);
        assert_eq!(page.mods[0].author, "Terciob");
        assert_eq!(
            page.mods[0].page_url,
            "https://www.curseforge.com/wow/addons/details"
        );
        let fake = Fake::new()
            .on(
                "/mods/61284/description",
                fixture("curseforge_description.json"),
            )
            .on("/mods/61284/files", fixture("curseforge_files.json"))
            .on("/mods/61284", fixture("curseforge_mod.json"));
        let cf = CurseForge {
            key: Some("k".into()),
        };
        let d = cf.detail(&fake, WOW, "61284").unwrap();
        assert_eq!(d.description, "Details! is a damage meter.");
        assert_eq!(d.game_versions, vec!["11.0.5".to_string()]);
        assert!(cf.detail(&fake, GameKey::CurseForge(432), "61284").is_err());
        let files = cf.files(&fake, WOW, "61284").unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].hash, Some(FileHash::Sha1("aaa111".into())));
        assert_eq!(
            files[0].download,
            Download::Direct("https://edge.forgecdn.net/files/5000/1/Details.zip".into())
        );
        assert!(files[0].primary);
        assert_eq!(
            files[1].download,
            Download::Browser("https://www.curseforge.com/projects/61284/files/4000".into()),
            "a null downloadUrl downloads on CurseForge's site"
        );
        assert_eq!(files[1].hash, Some(FileHash::Md5("bbb222".into())));
        assert_eq!(files[0].deps, vec!["999".to_string()]);
    }
}
