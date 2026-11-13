//! Nexus Mods: keyless search, browse, detail and file lists through the v2
//! GraphQL API (`api.nexusmods.com/v2/graphql`), and the v1 REST trending list
//! when the user saved a personal key. Every request names the app
//! (`Application-Name` / `Application-Version`), and the `x-rl-*` limits are
//! honored: at zero, Loadout stops asking until the window resets.
//!
//! Loadout never scrapes Nexus pages (the site's terms forbid it): only the
//! documented APIs.

use serde_json::{json, Value};

use super::allowlist::Source;
use super::ids::GameKey;
use super::source::*;

const GRAPHQL: &str = "https://api.nexusmods.com/v2/graphql";
const V1: &str = "https://api.nexusmods.com/v1";

const MOD_FIELDS: &str =
    "modId name summary author downloads endorsements updatedAt adultContent thumbnailUrl category";

fn game(game: GameKey) -> Result<(&'static str, u32), SourceError> {
    match game {
        GameKey::Nexus { domain, game_id } => Ok((domain, game_id)),
        _ => Err(SourceError::Unsupported("Not a Nexus Mods game.".into())),
    }
}

fn named(req: Request, key: Option<&str>) -> Request {
    let req = req
        .header("application-name", "Loadout")
        .header("application-version", env!("CARGO_PKG_VERSION"));
    match key {
        Some(k) => req.header("apikey", k),
        None => req,
    }
}

fn graphql(query: &str, variables: Value, key: Option<&str>) -> Request {
    let mut r = Request::get(Source::Nexus, GRAPHQL.into());
    r.body = Some(json!({ "query": query, "variables": variables }).to_string());
    named(r.header("content-type", "application/json"), key)
}

/// A mod id is a plain number.
fn mod_id(id: &str) -> Result<u64, SourceError> {
    id.parse()
        .map_err(|_| SourceError::Unsupported("That isn't a Nexus Mods mod id.".into()))
}

/// The request for one page. With a key, the Trending tab uses the v1 list.
pub fn search_request(
    game_key: GameKey,
    q: &Query,
    key: Option<&str>,
) -> Result<Request, SourceError> {
    let (domain, _) = game(game_key)?;
    if q.text().is_none() && q.browse == Browse::Trending {
        if let Some(k) = key {
            return Ok(named(
                Request::get(
                    Source::Nexus,
                    format!("{V1}/games/{domain}/mods/trending.json"),
                ),
                Some(k),
            ));
        }
    }
    let mut filter = json!({ "gameDomainName": [{ "value": domain, "op": "EQUALS" }] });
    let sort = match (q.text(), q.browse) {
        (Some(text), _) => {
            filter["name"] = json!([{ "value": format!("*{text}*"), "op": "WILDCARD" }]);
            json!([{ "downloads": { "direction": "DESC" } }])
        }
        (None, Browse::Popular) => json!([{ "downloads": { "direction": "DESC" } }]),
        (None, Browse::Trending) => json!([{ "endorsements": { "direction": "DESC" } }]),
        (None, Browse::Updated) => json!([{ "updatedAt": { "direction": "DESC" } }]),
    };
    let query = format!(
        "query Mods($filter: ModsFilter, $sort: [ModsSort!], $offset: Int, $count: Int) \
         {{ mods(filter: $filter, sort: $sort, offset: $offset, count: $count) \
         {{ totalCount nodes {{ {MOD_FIELDS} }} }} }}"
    );
    let vars = json!({ "filter": filter, "sort": sort, "offset": q.offset(), "count": PAGE_SIZE });
    Ok(graphql(&query, vars, key).with_text(q.text().is_some()))
}

fn page_url(domain: &str, id: u64) -> String {
    format!("https://www.nexusmods.com/{domain}/mods/{id}")
}

fn num(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn graph_summary(domain: &str, n: &Value) -> Option<ModSummary> {
    let id = num(&n["modId"]);
    if id == 0 {
        return None;
    }
    Some(ModSummary {
        source: Some(Source::Nexus.into()),
        id: id.to_string(),
        name: text(&n["name"]),
        author: text(&n["author"]),
        summary: plain_text(n["summary"].as_str().unwrap_or_default(), 400),
        downloads: num(&n["downloads"]),
        likes: num(&n["endorsements"]),
        updated: unix_from_iso(n["updatedAt"].as_str().unwrap_or_default()),
        adult: n["adultContent"].as_bool().unwrap_or(false),
        page_url: page_url(domain, id),
        icon_url: n["thumbnailUrl"].as_str().map(str::to_string),
        categories: n["category"]
            .as_str()
            .map(|c| vec![c.to_string()])
            .unwrap_or_default(),
    })
}

fn v1_summary(domain: &str, m: &Value) -> Option<ModSummary> {
    let id = num(&m["mod_id"]);
    if id == 0 || m["available"].as_bool() == Some(false) {
        return None;
    }
    Some(ModSummary {
        source: Some(Source::Nexus.into()),
        id: id.to_string(),
        name: text(&m["name"]),
        author: text(&m["author"]),
        summary: plain_text(m["summary"].as_str().unwrap_or_default(), 400),
        downloads: num(&m["mod_downloads"]),
        likes: num(&m["endorsement_count"]),
        updated: num(&m["updated_timestamp"]),
        adult: m["contains_adult_content"].as_bool().unwrap_or(false),
        page_url: page_url(domain, id),
        icon_url: m["picture_url"].as_str().map(str::to_string),
        categories: Vec::new(),
    })
}

/// GraphQL errors come back with a 200; turn them into an error.
fn data(body: &[u8]) -> Result<Value, SourceError> {
    let v: Value = parse_json(body)?;
    if let Some(msg) = v["errors"][0]["message"].as_str() {
        return Err(SourceError::Parse(msg.chars().take(200).collect()));
    }
    Ok(v)
}

pub fn parse_search(game_key: GameKey, body: &[u8]) -> Result<Page, SourceError> {
    let (domain, _) = game(game_key)?;
    let v = data(body)?;
    if let Some(list) = v.as_array() {
        // v1 trending: a plain array of mods.
        return Ok(Page {
            mods: list.iter().filter_map(|m| v1_summary(domain, m)).collect(),
            total: Some(list.len() as u64),
        });
    }
    let mods = &v["data"]["mods"];
    let nodes = mods["nodes"]
        .as_array()
        .ok_or_else(|| SourceError::Parse("no mods in the answer".into()))?;
    Ok(Page {
        mods: nodes
            .iter()
            .filter_map(|n| graph_summary(domain, n))
            .collect(),
        total: mods["totalCount"].as_u64(),
    })
}

pub fn detail_request(
    game_key: GameKey,
    id: &str,
    key: Option<&str>,
) -> Result<Request, SourceError> {
    let (domain, _) = game(game_key)?;
    let id = mod_id(id)?;
    let query = format!(
        "query Mod($filter: ModsFilter) {{ mods(filter: $filter, count: 1) \
         {{ nodes {{ {MOD_FIELDS} description }} }} }}"
    );
    let filter = json!({
        "gameDomainName": [{ "value": domain, "op": "EQUALS" }],
        "modId": [{ "value": id.to_string(), "op": "EQUALS" }],
    });
    Ok(graphql(&query, json!({ "filter": filter }), key))
}

pub fn parse_detail(game_key: GameKey, body: &[u8]) -> Result<ModDetail, SourceError> {
    let (domain, _) = game(game_key)?;
    let v = data(body)?;
    let n = &v["data"]["mods"]["nodes"][0];
    let summary =
        graph_summary(domain, n).ok_or_else(|| SourceError::Parse("no such mod".into()))?;
    Ok(ModDetail {
        description: plain_text(
            n["description"].as_str().unwrap_or_default(),
            DESCRIPTION_CAP,
        ),
        game_versions: Vec::new(),
        summary,
    })
}

pub fn files_request(
    game_key: GameKey,
    id: &str,
    key: Option<&str>,
) -> Result<Request, SourceError> {
    let (_, game_id) = game(game_key)?;
    let id = mod_id(id)?;
    let query =
        "query Files($modId: ID!, $gameId: ID!) { modFiles(modId: $modId, gameId: $gameId) \
                 { fileId name version sizeInBytes date category primary uri } }";
    Ok(graphql(
        query,
        json!({ "modId": id.to_string(), "gameId": game_id.to_string() }),
        key,
    ))
}

/// Current files only (main, optional, update, misc), newest first. Each
/// downloads in the browser from the mod's Files tab; Premium direct links
/// come later from the user's own key.
pub fn parse_files(game_key: GameKey, id: &str, body: &[u8]) -> Result<Vec<ModFile>, SourceError> {
    let (domain, _) = game(game_key)?;
    let mod_id = mod_id(id)?;
    let v = data(body)?;
    let list = v["data"]["modFiles"]
        .as_array()
        .ok_or_else(|| SourceError::Parse("no files in the answer".into()))?;
    let mut files: Vec<ModFile> = list
        .iter()
        .filter(|f| {
            !matches!(
                f["category"].as_str().unwrap_or_default(),
                "ARCHIVED" | "OLD_VERSION" | "REMOVED"
            )
        })
        .filter_map(|f| {
            let fid = num(&f["fileId"]);
            (fid != 0).then(|| ModFile {
                id: fid.to_string(),
                name: text(&f["name"]),
                file_name: text(&f["uri"]),
                version: text(&f["version"]),
                size: num(&f["sizeInBytes"]),
                hash: None,
                download: Download::Browser(format!(
                    "{}?tab=files&file_id={fid}",
                    page_url(domain, mod_id)
                )),
                game_versions: Vec::new(),
                deps: Vec::new(),
                primary: f["primary"].as_bool().unwrap_or(num(&f["primary"]) == 1)
                    || f["category"].as_str() == Some("MAIN"),
                uploaded: num(&f["date"]),
            })
        })
        .collect();
    files.sort_by(|a, b| b.uploaded.cmp(&a.uploaded).then(a.id.cmp(&b.id)));
    Ok(files)
}

/// Seconds to wait when Nexus says a limit is used up, from its `x-rl-*`
/// headers. `None` while either window still has requests left.
pub fn rate_wait(reply: &Reply) -> Option<u64> {
    let left = |name: &str| {
        reply
            .header(name)
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    let hourly = left("x-rl-hourly-remaining");
    let daily = left("x-rl-daily-remaining");
    if daily == Some(0) {
        Some(3600)
    } else if hourly == Some(0) {
        Some(600)
    } else {
        None
    }
}

fn send(net: &dyn Fetch, req: &Request) -> Result<Vec<u8>, SourceError> {
    let reply = net.send(req)?;
    if reply.status == 429 {
        return Err(SourceError::RateLimited(rate_wait(&reply).unwrap_or(60)));
    }
    ok_body(reply)
}

pub struct Nexus {
    pub key: Option<String>,
}

impl ModSource for Nexus {
    fn source(&self) -> Source {
        Source::Nexus
    }

    fn search(&self, net: &dyn Fetch, game: GameKey, q: &Query) -> Result<Page, SourceError> {
        let body = send(net, &search_request(game, q, self.key.as_deref())?)?;
        parse_search(game, &body)
    }

    fn detail(&self, net: &dyn Fetch, game: GameKey, id: &str) -> Result<ModDetail, SourceError> {
        let body = send(net, &detail_request(game, id, self.key.as_deref())?)?;
        parse_detail(game, &body)
    }

    fn files(&self, net: &dyn Fetch, game: GameKey, id: &str) -> Result<Vec<ModFile>, SourceError> {
        let body = send(net, &files_request(game, id, self.key.as_deref())?)?;
        parse_files(game, id, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::testing::{fixture, Fake};

    const SKY: GameKey = GameKey::Nexus {
        domain: "skyrimspecialedition",
        game_id: 1704,
    };

    #[test]
    fn search_posts_graphql_and_names_the_app() {
        let r = search_request(SKY, &Query::search("sky ui"), None).unwrap();
        assert_eq!(r.url, "https://api.nexusmods.com/v2/graphql");
        let body: Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["variables"]["filter"]["name"][0]["value"], "*sky ui*");
        assert_eq!(
            body["variables"]["filter"]["gameDomainName"][0]["value"],
            "skyrimspecialedition"
        );
        assert!(r.carries_text);
        assert!(r
            .headers
            .contains(&("application-name".into(), "Loadout".into())));
        assert!(!r.headers.iter().any(|(n, _)| n == "apikey"));
        let t = search_request(SKY, &Query::browse(Browse::Trending), Some("k")).unwrap();
        assert_eq!(
            t.url,
            "https://api.nexusmods.com/v1/games/skyrimspecialedition/mods/trending.json"
        );
        assert!(t.body.is_none());
        assert!(t.headers.contains(&("apikey".into(), "k".into())));
        let t = search_request(SKY, &Query::browse(Browse::Trending), None).unwrap();
        assert!(t.body.unwrap().contains("endorsements"));
    }

    #[test]
    fn graphql_and_v1_pages_parse() {
        let page = parse_search(SKY, fixture("nexus_search.json").as_bytes()).unwrap();
        assert_eq!(page.total, Some(1234));
        assert_eq!(page.mods.len(), 2);
        assert_eq!(page.mods[0].id, "12604");
        assert_eq!(page.mods[0].name, "SkyUI");
        assert_eq!(page.mods[0].likes, 300_000);
        assert_eq!(
            page.mods[0].page_url,
            "https://www.nexusmods.com/skyrimspecialedition/mods/12604"
        );
        assert!(page.mods[1].adult);
        let v1 = parse_search(SKY, fixture("nexus_trending_v1.json").as_bytes()).unwrap();
        assert_eq!(v1.mods.len(), 1, "unavailable mods are dropped");
        assert_eq!(v1.mods[0].downloads, 5000);
        assert!(matches!(
            parse_search(SKY, br#"{"errors":[{"message":"bad filter"}]}"#),
            Err(SourceError::Parse(m)) if m == "bad filter"
        ));
    }

    #[test]
    fn detail_and_files_parse_through_the_source() {
        let fake = Fake::new()
            .on("graphql#Files", fixture("nexus_files.json"))
            .on("graphql#Mod(", fixture("nexus_detail.json"));
        let d = Nexus { key: None }.detail(&fake, SKY, "12604").unwrap();
        assert_eq!(d.summary.name, "SkyUI");
        assert_eq!(
            d.description,
            "Elegant, PC-friendly interface mod.\nRequires SKSE."
        );
        let files = Nexus { key: None }.files(&fake, SKY, "12604").unwrap();
        assert_eq!(files.len(), 2, "archived files are dropped");
        assert_eq!(files[0].id, "200");
        assert_eq!(files[0].size, 2_500_000);
        assert_eq!(
            files[0].download,
            Download::Browser(
                "https://www.nexusmods.com/skyrimspecialedition/mods/12604?tab=files&file_id=200"
                    .into()
            )
        );
        assert!(files[1].primary);
        assert!(Nexus { key: None }.files(&fake, SKY, "abc").is_err());
    }

    #[test]
    fn rate_limit_headers_stop_requests() {
        let mut r = Reply::ok("");
        r.headers.push(("x-rl-hourly-remaining".into(), "0".into()));
        r.headers.push(("x-rl-daily-remaining".into(), "10".into()));
        assert_eq!(rate_wait(&r), Some(600));
        r.headers[1].1 = "0".into();
        assert_eq!(rate_wait(&r), Some(3600));
        assert_eq!(rate_wait(&Reply::ok("")), None);
        let mut limited = Reply::ok("");
        limited.status = 429;
        limited.headers = r.headers.clone();
        let fake = Fake::new().reply("graphql", limited);
        assert_eq!(
            Nexus { key: None }.search(&fake, SKY, &Query::browse(Browse::Popular)),
            Err(SourceError::RateLimited(3600))
        );
    }
}
