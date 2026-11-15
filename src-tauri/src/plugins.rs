//! Settings → Labs → Plugins: the installed bundles with their trust and
//! on/off state. A bundle installs off and untrusted; trusting covers the
//! commands it lists, and only then can it be turned on. Fetching a
//! marketplace list installs nothing.

use std::path::Path;

use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::plugins::{self, MarketEntry, PluginInfo};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub version: String,
    /// What the bundle brings: skills, hooks, MCP servers, agents.
    pub summary: String,
    /// The manifest's description, shown once under the title.
    pub description: String,
    /// The summary split into labelled lines, without its title and
    /// description.
    pub parts: Vec<Part>,
    pub trusted: bool,
    pub enabled: bool,
    /// Installed through GrokHub, so Remove can delete the copy.
    pub installed: bool,
}

/// One labelled line of a bundle's summary, such as its skills or hooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    /// Empty when the line has no known label.
    pub label: String,
    pub text: String,
    /// A refused part, or a reason the bundle can't be trusted.
    pub warn: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub rows: Vec<Row>,
    pub market_url: String,
    /// What the last action did, or why it failed.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Market {
    pub entries: Vec<Entry>,
    pub page: Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Act {
    Trust,
    Enable,
    Disable,
    Remove,
}

/// The summary's line prefixes (`grokhub_agent::plugins` `summary_of`) and
/// the label each one shows under.
const LABELS: [(&str, &str, bool); 7] = [
    ("skills: ", "Skills", false),
    ("hooks: ", "Hooks", false),
    ("mcp: ", "MCP servers", false),
    ("agents: ", "Agents", false),
    ("commands: ", "Commands", false),
    ("unusable: ", "Can't be trusted", true),
    ("refused: ", "Refused", true),
];

/// Splits a bundle summary into labelled parts. The summary starts with
/// "{name} {version}" and the description (which may hold newlines), so
/// those are stripped as one block rather than line by line. A line with no
/// known label belongs to the part above it. When the header doesn't match,
/// the whole summary comes back as one unlabelled part, so nothing is lost.
fn parts(summary: &str, name: &str, version: &str, description: &str) -> Vec<Part> {
    let whole = || {
        vec![Part {
            label: String::new(),
            text: summary.to_string(),
            warn: false,
        }]
    };
    let Some(mut rest) = summary.strip_prefix(&format!("{name} {version}")) else {
        return whole();
    };
    if !description.is_empty() {
        match rest.strip_prefix(&format!("\n{description}")) {
            Some(after) => rest = after,
            None => return whole(),
        }
    }
    let rest = match rest.strip_prefix('\n') {
        Some(after) => after,
        None if rest.is_empty() => return Vec::new(),
        None => return whole(),
    };
    let mcp_key = format!("plugin__{name}__");
    let mut out: Vec<Part> = Vec::new();
    for line in rest.lines() {
        let known = LABELS.iter().find_map(|(prefix, label, warn)| {
            line.strip_prefix(prefix).map(|t| (*label, t, *warn))
        });
        match (known, out.last_mut()) {
            (Some((label, text, warn)), _) => {
                let text = if label == "MCP servers" {
                    text.replace(&mcp_key, "")
                } else {
                    text.to_string()
                };
                out.push(Part {
                    label: label.to_string(),
                    text,
                    warn,
                });
            }
            (None, Some(last)) => {
                last.text.push('\n');
                last.text.push_str(line);
            }
            (None, None) => out.push(Part {
                label: String::new(),
                text: line.to_string(),
                warn: false,
            }),
        }
    }
    out
}

fn row(info: PluginInfo) -> Row {
    Row {
        parts: parts(&info.summary, &info.name, &info.version, &info.description),
        name: info.name,
        version: info.version,
        summary: info.summary,
        description: info.description,
        trusted: info.trusted,
        enabled: info.enabled,
        installed: info.installed,
    }
}

fn entry(e: MarketEntry) -> Entry {
    Entry {
        name: e.name,
        version: e.version,
        description: e.description,
        source: e.source,
    }
}

fn page(workspace: &Path, note: String) -> Page {
    Page {
        rows: plugins::list_plugins(workspace)
            .into_iter()
            .map(row)
            .collect(),
        market_url: plugins::marketplace_url(),
        note,
    }
}

pub fn load(config_dir: &Path, workspace: &Path) -> Page {
    let _config = ConfigGuard::set(config_dir);
    page(workspace, String::new())
}

pub fn act(config_dir: &Path, workspace: &Path, name: &str, act: Act) -> Page {
    let _config = ConfigGuard::set(config_dir);
    let done = match act {
        Act::Trust => plugins::trust_plugin(workspace, name)
            .map(|()| format!("Trusted {name}. It stays off until you enable it.")),
        Act::Enable => plugins::enable_plugin(workspace, name).map(|()| format!("Enabled {name}.")),
        Act::Disable => plugins::disable_plugin(name).map(|()| format!("Disabled {name}.")),
        Act::Remove => plugins::remove_plugin(workspace, name).map(|()| format!("Removed {name}.")),
    };
    page(workspace, done.unwrap_or_else(|err| err))
}

/// An https, ssh or file URL, or an absolute folder. Nothing from it runs.
pub fn install(config_dir: &Path, workspace: &Path, source: &str) -> Page {
    let _config = ConfigGuard::set(config_dir);
    let note = match plugins::install_source(source.trim()) {
        Ok(info) => format!("Installed {}. It stays off until you trust it.", info.name),
        Err(err) => err,
    };
    page(workspace, note)
}

/// Saves the index URL and lists its entries. An empty URL clears it.
pub fn fetch_market(config_dir: &Path, workspace: &Path, url: &str) -> Market {
    let _config = ConfigGuard::set(config_dir);
    let url = url.trim();
    let (entries, note) = match plugins::set_marketplace_url(url) {
        Err(err) => (Vec::new(), err),
        Ok(()) if url.is_empty() => (Vec::new(), "Marketplace URL cleared.".to_string()),
        Ok(()) => match plugins::fetch_marketplace(url) {
            Ok(list) => {
                let note = format!("{} entries. Nothing was installed.", list.len());
                (list.into_iter().map(entry).collect(), note)
            }
            Err(err) => (Vec::new(), err),
        },
    };
    Market {
        entries,
        page: page(workspace, note),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gh-shell-plugins-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cfg")).unwrap();
        std::fs::create_dir_all(dir.join("ws").join(".git")).unwrap();
        dir
    }

    fn bundle(root: &Path, name: &str) -> PathBuf {
        let src = root.join("src").join(name);
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("plugin.json"),
            format!(r#"{{"name":"{name}","version":"1.2.0","description":"Demo bundle"}}"#),
        )
        .unwrap();
        src
    }

    /// A bundle with skills, a hook, an MCP server, an agent and a skills
    /// path that leaves the bundle, which is refused.
    fn rich_bundle(root: &Path, name: &str) -> PathBuf {
        let src = root.join("src").join(name);
        let write = |rel: &str, body: &str| {
            let path = src.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        };
        write(
            "plugin.json",
            &serde_json::json!({
                "name": name,
                "version": "1.2.0",
                "description": "Demo bundle",
                "skills": ["skills", "../outside"],
                "hooks": "hooks/hooks.json",
                "mcpServers": ".mcp.json",
                "agents": "agents",
            })
            .to_string(),
        );
        write(
            "skills/ship/SKILL.md",
            "---\nname: ship\ndescription: Ship the change\n---\nShip body.\n",
        );
        write(
            "hooks/hooks.json",
            &serde_json::json!({"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "echo rich-hook"}]}]}})
                .to_string(),
        );
        write(
            ".mcp.json",
            &serde_json::json!({"mcpServers": {"files": {"command": "plugin-mcp", "args": ["--stdio"]}}})
                .to_string(),
        );
        write(
            "agents/reviewer.md",
            "---\nname: reviewer\ndescription: Reviews patches\n---\nReview the diff.\n",
        );
        src
    }

    fn part(label: &str, text: &str, warn: bool) -> Part {
        Part {
            label: label.into(),
            text: text.into(),
            warn,
        }
    }

    #[test]
    fn a_rich_bundle_lists_each_part_on_its_own_line_and_flags_the_refused_one() {
        let root = scratch("rich");
        let (cfg, ws) = (root.join("cfg"), root.join("ws"));
        let src = rich_bundle(&root, "rich-demo");
        let page = install(&cfg, &ws, src.to_str().unwrap());
        let r = find(&page, "rich-demo");
        assert_eq!(r.description, "Demo bundle");
        assert_eq!(
            r.parts,
            vec![
                part("Skills", "skills", false),
                part("Hooks", "echo rich-hook", false),
                part("MCP servers", "files: plugin-mcp --stdio", false),
                part("Agents", "reviewer", false),
                part("Refused", "skills: path escapes the bundle", true),
            ],
            "summary was {:?}",
            r.summary
        );

        // A refused part doesn't block the rest of the bundle.
        act(&cfg, &ws, "rich-demo", Act::Trust);
        let page = act(&cfg, &ws, "rich-demo", Act::Enable);
        assert_eq!(page.note, "Enabled rich-demo.");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_description_with_newlines_is_stripped_whole() {
        assert_eq!(
            parts(
                "kit 2.0\nLine one\nLine two\nskills: s",
                "kit",
                "2.0",
                "Line one\nLine two"
            ),
            vec![part("Skills", "s", false)]
        );
        assert_eq!(
            parts("kit 2.0\nOnly words", "kit", "2.0", "Only words"),
            Vec::new()
        );
    }

    #[test]
    fn an_unlabelled_line_joins_the_part_above_it() {
        assert_eq!(
            parts(
                "kit 2.0\nhooks: echo a\nstill a\nagents: r",
                "kit",
                "2.0",
                ""
            ),
            vec![
                part("Hooks", "echo a\nstill a", false),
                part("Agents", "r", false)
            ]
        );
        assert_eq!(
            parts("kit 2.0\nunusable: hash mismatch", "kit", "2.0", ""),
            vec![part("Can't be trusted", "hash mismatch", true)]
        );
    }

    #[test]
    fn a_summary_in_another_shape_comes_back_whole() {
        assert_eq!(
            parts("x 9\nfoo", "y", "9", ""),
            vec![part("", "x 9\nfoo", false)]
        );
        assert_eq!(
            parts("y 9\nother words\nskills: s", "y", "9", "Demo"),
            vec![part("", "y 9\nother words\nskills: s", false)]
        );
    }

    fn find<'a>(page: &'a Page, name: &str) -> &'a Row {
        page.rows
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("{name} not in {:?}", page.rows))
    }

    #[test]
    fn a_bundle_installs_off_then_trusts_enables_disables_and_removes() {
        let root = scratch("flow");
        let (cfg, ws) = (root.join("cfg"), root.join("ws"));
        let src = bundle(&root, "shell-demo");

        let page = install(&cfg, &ws, src.to_str().unwrap());
        assert_eq!(
            page.note,
            "Installed shell-demo. It stays off until you trust it."
        );
        let r = find(&page, "shell-demo");
        assert_eq!(r.version, "1.2.0");
        assert_eq!((r.trusted, r.enabled, r.installed), (false, false, true));
        assert!(
            cfg.join("plugins").exists(),
            "the copy lives in this config folder"
        );

        let page = act(&cfg, &ws, "shell-demo", Act::Trust);
        assert_eq!(
            page.note,
            "Trusted shell-demo. It stays off until you enable it."
        );
        let r = find(&page, "shell-demo");
        assert_eq!((r.trusted, r.enabled), (true, false));

        let page = act(&cfg, &ws, "shell-demo", Act::Enable);
        assert_eq!(page.note, "Enabled shell-demo.");
        assert!(find(&page, "shell-demo").enabled);

        let page = act(&cfg, &ws, "shell-demo", Act::Disable);
        assert_eq!(page.note, "Disabled shell-demo.");
        assert!(!find(&page, "shell-demo").enabled);

        let page = act(&cfg, &ws, "shell-demo", Act::Remove);
        assert_eq!(page.note, "Removed shell-demo.");
        assert!(page.rows.iter().all(|r| r.name != "shell-demo"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enabling_an_untrusted_bundle_says_why_and_stays_off() {
        let root = scratch("untrusted");
        let (cfg, ws) = (root.join("cfg"), root.join("ws"));
        let src = bundle(&root, "shell-raw");
        install(&cfg, &ws, src.to_str().unwrap());

        let page = act(&cfg, &ws, "shell-raw", Act::Enable);
        assert_eq!(page.note, "trust shell-raw 1.2.0 before enabling");
        assert!(!find(&page, "shell-raw").enabled);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_source_or_url_installs_and_fetches_nothing() {
        let root = scratch("empty");
        let (cfg, ws) = (root.join("cfg"), root.join("ws"));
        let page = install(&cfg, &ws, "  ");
        assert_eq!(
            page.note,
            "marketplace entry is not an https, ssh, or file URL, or an absolute path"
        );
        assert!(!cfg.join("plugins").exists());

        let market = fetch_market(&cfg, &ws, "");
        assert_eq!(market.page.note, "Marketplace URL cleared.");
        assert_eq!(market.entries, Vec::new());
        assert_eq!(market.page.market_url, "");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_json_is_camel_case_and_acts_read_plain_words() {
        let page = Page {
            rows: vec![Row {
                name: "a".into(),
                version: "1".into(),
                summary: "a 1".into(),
                description: String::new(),
                parts: vec![Part {
                    label: "Skills".into(),
                    text: "s".into(),
                    warn: false,
                }],
                trusted: true,
                enabled: false,
                installed: true,
            }],
            market_url: "https://example.com/index.json".into(),
            note: String::new(),
        };
        assert_eq!(
            serde_json::to_value(&page).unwrap(),
            serde_json::json!({
                "rows": [{"name": "a", "version": "1", "summary": "a 1", "description": "", "parts": [{"label": "Skills", "text": "s", "warn": false}], "trusted": true, "enabled": false, "installed": true}],
                "marketUrl": "https://example.com/index.json",
                "note": "",
            })
        );
        let acts: Vec<Act> =
            serde_json::from_str(r#"["trust","enable","disable","remove"]"#).unwrap();
        assert_eq!(
            acts,
            vec![Act::Trust, Act::Enable, Act::Disable, Act::Remove]
        );
    }
}
