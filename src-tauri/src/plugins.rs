//! Settings → Labs → Plugins (shell parity): the same bundles, trust and
//! on/off state as the cabin's Labs page. A bundle installs off and
//! untrusted; trusting covers the commands it lists, and only then can it be
//! turned on. Fetching a marketplace list installs nothing.

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
    pub trusted: bool,
    pub enabled: bool,
    /// Installed through GrokHub, so Remove can delete the copy.
    pub installed: bool,
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

fn row(info: PluginInfo) -> Row {
    Row {
        name: info.name,
        version: info.version,
        summary: info.summary,
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
                "rows": [{"name": "a", "version": "1", "summary": "a 1", "trusted": true, "enabled": false, "installed": true}],
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
