//! Steam Workshop: item details keyless (`ISteamRemoteStorage/
//! GetPublishedFileDetails`), search and browse only with the user's own
//! Steam Web API key (`IPublishedFileService/QueryFiles`). Without a key the
//! page offers the Workshop itself in Steam. Steam downloads what the user
//! subscribes to; Loadout only reads what Steam installed.

use serde_json::Value;

use super::allowlist::Source;
use super::ids::GameKey;
use super::source::*;

const API: &str = "https://api.steampowered.com";

fn app(game: GameKey) -> Result<u32, SourceError> {
    match game {
        GameKey::Workshop(app) => Ok(app),
        _ => Err(SourceError::Unsupported(
            "Not a Steam Workshop game.".into(),
        )),
    }
}

/// The Workshop page in Steam for a game, the fallback when there's no key.
pub fn workshop_url(app_id: u32) -> String {
    format!("steam://url/SteamWorkshopPage/{app_id}")
}

/// One item's page in Steam.
pub fn item_url(id: &str) -> String {
    format!("steam://url/CommunityFilePage/{id}")
}

fn web_url(id: &str) -> String {
    format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")
}

fn item_id(id: &str) -> Result<u64, SourceError> {
    id.parse()
        .map_err(|_| SourceError::Unsupported("That isn't a Workshop item id.".into()))
}

/// `EPublishedFileQueryType` for each tab.
fn query_type(q: &Query) -> u32 {
    match (q.text(), q.browse) {
        (Some(_), _) => 12,            // RankedByTextSearch
        (None, Browse::Popular) => 9,  // RankedByTotalUniqueSubscriptions
        (None, Browse::Trending) => 3, // RankedByTrend
        (None, Browse::Updated) => 21, // RankedByLastUpdatedDate
    }
}

pub fn search_request(game: GameKey, q: &Query, key: Option<&str>) -> Result<Request, SourceError> {
    let app = app(game)?;
    let Some(key) = key else {
        return Err(SourceError::Unsupported(format!(
            "Workshop search needs a Steam Web API key in Settings. Until then, open the Workshop in Steam: {}",
            workshop_url(app)
        )));
    };
    let text = q
        .text()
        .map(|t| format!("&search_text={}", enc(t)))
        .unwrap_or_default();
    Ok(Request::get(
        Source::Workshop,
        format!(
            "{API}/IPublishedFileService/QueryFiles/v1/?key={}&appid={app}&query_type={}&page={}&numperpage={PAGE_SIZE}&return_metadata=true&return_tags=true&return_short_description=true{text}",
            enc(key),
            query_type(q),
            q.page + 1,
        ),
    )
    .with_text(q.text().is_some()))
}

fn num(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

fn summary(app: u32, d: &Value) -> Option<ModSummary> {
    let id = d["publishedfileid"].as_str()?.to_string();
    if num(&d["consumer_app_id"]) != 0 && num(&d["consumer_app_id"]) != u64::from(app) {
        return None;
    }
    if num(&d["banned"]) != 0 || d["result"].as_u64().is_some_and(|r| r != 1) {
        return None;
    }
    let blurb = d["short_description"]
        .as_str()
        .or(d["file_description"].as_str())
        .or(d["description"].as_str())
        .unwrap_or_default();
    Some(ModSummary {
        source: Some(Source::Workshop.into()),
        page_url: web_url(&id),
        id,
        name: d["title"].as_str().unwrap_or_default().to_string(),
        author: String::new(),
        summary: plain_text(blurb, 400),
        downloads: num(&d["subscriptions"]),
        likes: num(&d["favorited"]),
        updated: num(&d["time_updated"]),
        adult: d["maybe_inappropriate_sex"].as_bool().unwrap_or(false),
        icon_url: d["preview_url"]
            .as_str()
            .filter(|u| !u.is_empty())
            .map(str::to_string),
        categories: d["tags"]
            .as_array()
            .map(|t| {
                t.iter()
                    .filter_map(|x| x["tag"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub fn parse_search(game: GameKey, body: &[u8]) -> Result<Page, SourceError> {
    let app = app(game)?;
    let v: Value = parse_json(body)?;
    let r = &v["response"];
    Ok(Page {
        total: r["total"].as_u64(),
        mods: r["publishedfiledetails"]
            .as_array()
            .map(|l| l.iter().filter_map(|d| summary(app, d)).collect())
            .unwrap_or_default(),
    })
}

/// Keyless details for one item (a form POST).
pub fn detail_request(id: &str) -> Result<Request, SourceError> {
    let id = item_id(id)?;
    let mut r = Request::get(
        Source::Workshop,
        format!("{API}/ISteamRemoteStorage/GetPublishedFileDetails/v1/"),
    )
    .header("content-type", "application/x-www-form-urlencoded");
    r.body = Some(format!("itemcount=1&publishedfileids%5B0%5D={id}"));
    Ok(r)
}

fn detail_item(game: GameKey, body: &[u8]) -> Result<Value, SourceError> {
    let app = app(game)?;
    let v: Value = parse_json(body)?;
    let d = v["response"]["publishedfiledetails"][0].clone();
    if summary(app, &d).is_none() {
        return Err(SourceError::Unsupported(
            "That Workshop item isn't for this game or isn't available.".into(),
        ));
    }
    Ok(d)
}

pub fn parse_detail(game: GameKey, body: &[u8]) -> Result<ModDetail, SourceError> {
    let app = app(game)?;
    let d = detail_item(game, body)?;
    let summary = summary(app, &d).ok_or_else(|| SourceError::Parse("no item".into()))?;
    Ok(ModDetail {
        description: plain_text(
            d["description"].as_str().unwrap_or_default(),
            DESCRIPTION_CAP,
        ),
        game_versions: Vec::new(),
        summary,
    })
}

/// A Workshop item is one file, downloaded by Steam when the user subscribes.
pub fn parse_files(game: GameKey, body: &[u8]) -> Result<Vec<ModFile>, SourceError> {
    let d = detail_item(game, body)?;
    let id = d["publishedfileid"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Ok(vec![ModFile {
        download: Download::Steam(item_id(&id)?),
        name: d["title"].as_str().unwrap_or_default().to_string(),
        file_name: d["filename"].as_str().unwrap_or_default().to_string(),
        version: String::new(),
        size: num(&d["file_size"]),
        hash: None,
        game_versions: Vec::new(),
        deps: Vec::new(),
        primary: true,
        uploaded: num(&d["time_updated"]),
        id,
    }])
}

pub struct Workshop {
    pub key: Option<String>,
}

impl ModSource for Workshop {
    fn source(&self) -> Source {
        Source::Workshop
    }

    fn search(&self, net: &dyn Fetch, game: GameKey, q: &Query) -> Result<Page, SourceError> {
        let req = search_request(game, q, self.key.as_deref())?;
        parse_search(game, &ok_body(net.send(&req)?)?)
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

    const RIM: GameKey = GameKey::Workshop(294100);

    #[test]
    fn search_needs_a_key_and_says_where_to_go_instead() {
        let err = search_request(RIM, &Query::search("hugslib"), None).unwrap_err();
        assert_eq!(
            err.line(),
            "Workshop search needs a Steam Web API key in Settings. Until then, open the Workshop in Steam: steam://url/SteamWorkshopPage/294100"
        );
        let r = search_request(RIM, &Query::search("hugs lib"), Some("KEY")).unwrap();
        assert!(r.url.starts_with("https://api.steampowered.com/IPublishedFileService/QueryFiles/v1/?key=KEY&appid=294100&query_type=12&page=1"), "{}", r.url);
        assert!(r.url.ends_with("&search_text=hugs%20lib"));
        let r = search_request(RIM, &Query::browse(Browse::Updated), Some("KEY")).unwrap();
        assert!(r.url.contains("query_type=21"));
    }

    #[test]
    fn search_detail_and_files_parse() {
        let page = parse_search(RIM, fixture("workshop_query.json").as_bytes()).unwrap();
        assert_eq!(page.total, Some(3));
        assert_eq!(page.mods.len(), 2, "another game's item is dropped");
        assert_eq!(page.mods[0].name, "HugsLib");
        assert_eq!(page.mods[0].downloads, 1_500_000);
        assert_eq!(page.mods[0].categories, vec!["Mod".to_string()]);
        assert!(page.mods[1].adult);
        let fake = Fake::new().on("GetPublishedFileDetails", fixture("workshop_details.json"));
        let w = Workshop { key: None };
        let d = w.detail(&fake, RIM, "818773962").unwrap();
        assert_eq!(d.description, "Library\nfor mods.");
        assert_eq!(
            d.summary.page_url,
            "https://steamcommunity.com/sharedfiles/filedetails/?id=818773962"
        );
        let files = w.files(&fake, RIM, "818773962").unwrap();
        assert_eq!(files[0].download, Download::Steam(818773962));
        assert_eq!(files[0].size, 204_800);
        let sent = fake.sent();
        assert_eq!(
            sent[0].body.as_deref(),
            Some("itemcount=1&publishedfileids%5B0%5D=818773962")
        );
        assert!(w
            .detail(&fake, GameKey::Workshop(255710), "818773962")
            .is_err());
        assert_eq!(item_url("5"), "steam://url/CommunityFilePage/5");
    }
}
