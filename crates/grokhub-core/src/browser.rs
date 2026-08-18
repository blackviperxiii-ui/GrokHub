//! Optional localhost CDP — a hand, not a browser product.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTab {
    pub id: String,
    pub title: String,
    pub url: String,
    pub ws_url: String,
}

pub const CDP_PORTS: &[u16] = &[9222, 9223];

pub const CDP_DOWN: &str =
    "browser hand down — start Firefox or Chromium with --remote-debugging-port=9222, or use act / key ctrl+t or ctrl+w after wait_for";

/// Parse Chrome/Firefox `/json/list` (or `/json`) payload.
pub fn parse_cdp_targets(raw: &str) -> Result<Vec<BrowserTab>, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("cdp list: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| "cdp list: expected array".to_string())?;
    let mut out = Vec::new();
    for item in arr {
        if !keep_cdp_target(item) {
            continue;
        }
        let id = json_str(item, "id");
        if id.is_empty() {
            continue;
        }
        out.push(BrowserTab {
            id,
            title: json_str(item, "title"),
            url: json_str(item, "url"),
            ws_url: json_str(item, "webSocketDebuggerUrl"),
        });
    }
    Ok(out)
}

fn keep_cdp_target(item: &serde_json::Value) -> bool {
    let kind = json_str(item, "type").to_ascii_lowercase();
    if kind == "page" || kind == "tab" {
        return true;
    }
    if !kind.is_empty() {
        return false;
    }
    let url = json_str(item, "url");
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("about:")
        || url.starts_with("file:")
}

fn json_str(item: &serde_json::Value, key: &str) -> String {
    item.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

pub fn match_browser_tabs<'a>(tabs: &'a [BrowserTab], query: &str) -> Vec<&'a BrowserTab> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    tabs.iter()
        .filter(|t| {
            t.title.to_ascii_lowercase().contains(&q)
                || t.url.to_ascii_lowercase().contains(&q)
                || t.id.to_ascii_lowercase() == q
        })
        .collect()
}

fn is_browser_app_query(q: &str) -> bool {
    matches!(
        q,
        "firefox" | "chrome" | "chromium" | "browser" | "brave"
    )
}

fn is_new_tab_query(q: &str) -> bool {
    q == "new tab" || q == "newtab" || q.contains("new tab") || q == "+"
}

fn is_blank_tab(tab: &BrowserTab) -> bool {
    let title = tab.title.trim();
    let url = tab.url.to_ascii_lowercase();
    title.is_empty()
        || title.eq_ignore_ascii_case("new tab")
        || title.eq_ignore_ascii_case("(untitled)")
        || url.contains("about:newtab")
        || url.contains("about:blank")
}

fn soft_pick_browser_tab<'a>(tabs: &'a [BrowserTab], query: &str) -> Option<&'a BrowserTab> {
    let q = query.trim().to_ascii_lowercase();
    if is_browser_app_query(&q) && tabs.len() == 1 {
        return Some(&tabs[0]);
    }
    if is_new_tab_query(&q) {
        let blanks: Vec<_> = tabs.iter().filter(|t| is_blank_tab(t)).collect();
        if blanks.len() == 1 {
            return Some(blanks[0]);
        }
    }
    None
}

/// One exact hit, or an error that lists the candidates.
pub fn pick_browser_tab<'a>(
    tabs: &'a [BrowserTab],
    query: &str,
) -> Result<&'a BrowserTab, String> {
    let hits = match_browser_tabs(tabs, query);
    match hits.len() {
        0 => {
            if let Some(hit) = soft_pick_browser_tab(tabs, query) {
                return Ok(hit);
            }
            Err(format!("no tab matched {query}"))
        }
        1 => Ok(hits[0]),
        _ => Err(format!(
            "ambiguous tab {query} — {} hits:\n{}",
            hits.len(),
            format_tab_list(&hits.iter().map(|t| (*t).clone()).collect::<Vec<_>>())
        )),
    }
}

/// Chrome/Firefox `GET /json/new?<url>`.
pub fn cdp_new_tab_path(url: &str) -> String {
    let url = url.trim();
    let url = if url.is_empty() { "about:blank" } else { url };
    format!("/json/new?{}", encode_cdp_url(url))
}

fn encode_cdp_url(url: &str) -> String {
    let mut out = String::new();
    for b in url.bytes() {
        match b {
            b'?' | b'&' | b'#' | b' ' | b'\n' | b'\r' => out.push_str(&format!("%{b:02X}")),
            _ => out.push(b as char),
        }
    }
    out
}

pub fn format_tab_list(tabs: &[BrowserTab]) -> String {
    if tabs.is_empty() {
        return "no page tabs".into();
    }
    let mut s = String::new();
    for t in tabs {
        if !s.is_empty() {
            s.push('\n');
        }
        let title = if t.title.trim().is_empty() {
            "(untitled)"
        } else {
            t.title.trim()
        };
        s.push_str(&format!("- {title}  {}", t.url));
    }
    s
}

pub fn browser_windshield_line(up: bool, tabs: usize) -> String {
    if up {
        format!("browser: cdp {tabs} tabs\n")
    } else {
        "browser: cdp down\n".into()
    }
}

pub fn cdp_page_close_payload() -> &'static str {
    r#"{"id":1,"method":"Page.close"}"#
}

pub fn cdp_page_focus_payload() -> &'static str {
    r#"{"id":1,"method":"Page.bringToFront"}"#
}

pub fn cdp_activate_payload(target_id: &str) -> String {
    format!(
        r#"{{"id":1,"method":"Target.activateTarget","params":{{"targetId":{}}}}}"#,
        serde_json::to_string(target_id).unwrap_or_else(|_| "\"\"".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"[
      {"id":"t1","type":"page","title":"GitHub","url":"https://github.com/foo","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/t1"},
      {"id":"t2","type":"page","title":"Hacker News","url":"https://news.ycombinator.com/","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/t2"},
      {"id":"sw","type":"service_worker","title":"","url":"https://github.com/sw.js"}
    ]"#;

    #[test]
    fn parse_list_keeps_pages_drops_workers() {
        let tabs = parse_cdp_targets(LIST).unwrap();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].id, "t1");
        assert_eq!(tabs[0].title, "GitHub");
        assert!(tabs[1].url.contains("ycombinator"));
    }

    #[test]
    fn pick_unique_and_refuse_ambiguous() {
        let tabs = parse_cdp_targets(LIST).unwrap();
        let hit = pick_browser_tab(&tabs, "github").unwrap();
        assert_eq!(hit.id, "t1");
        let miss = pick_browser_tab(&tabs, "https").unwrap_err();
        assert!(miss.contains("ambiguous"), "{miss}");
        assert!(miss.contains("GitHub") && miss.contains("Hacker News"), "{miss}");
        assert!(pick_browser_tab(&tabs, "nope").unwrap_err().contains("no tab"));
        let one = parse_cdp_targets(
            r#"[{"id":"t1","type":"page","title":"GitHub","url":"https://github.com/foo","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/t1"}]"#,
        )
        .unwrap();
        let firefox = pick_browser_tab(&one, "Firefox").unwrap();
        assert_eq!(firefox.id, "t1");
        let blank = parse_cdp_targets(
            r#"[{"id":"n1","type":"page","title":"","url":"about:newtab","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/n1"}]"#,
        )
        .unwrap();
        assert_eq!(pick_browser_tab(&blank, "new tab").unwrap().id, "n1");
    }

    #[test]
    fn new_tab_path_defaults_to_blank() {
        assert_eq!(cdp_new_tab_path(""), "/json/new?about:blank");
        assert_eq!(
            cdp_new_tab_path("https://example.com"),
            "/json/new?https://example.com"
        );
        assert_eq!(
            cdp_new_tab_path("https://example.com/search?q=grok&lang=en"),
            "/json/new?https://example.com/search%3Fq=grok%26lang=en"
        );
    }

    #[test]
    fn windshield_line_up_or_down() {
        assert_eq!(browser_windshield_line(true, 3), "browser: cdp 3 tabs\n");
        assert_eq!(browser_windshield_line(false, 0), "browser: cdp down\n");
    }

    #[test]
    fn close_payload_is_page_close() {
        assert!(cdp_page_close_payload().contains("Page.close"));
        assert!(cdp_page_focus_payload().contains("Page.bringToFront"));
        assert!(cdp_activate_payload("t1").contains("t1"));
    }
}
