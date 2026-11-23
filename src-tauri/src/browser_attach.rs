//! Settings → General → Your browser: the agent may work in a browser the
//! user already runs, signed in, but only on the sites they allow. The
//! attach and its checks live in `grokhub_agent::mcp::BrowserAttach`; this
//! is the page's side.

use std::path::Path;

use grokhub_agent::mcp::{loopback_endpoint, BrowserAttach};
use serde::Serialize;

/// The endpoint a Chromium browser serves with `--remote-debugging-port=9222`.
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:9222";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachView {
    pub endpoint: String,
    /// The sites the agent may open there.
    pub origins: Vec<String>,
    /// The agent's browser tool joins this browser.
    pub active: bool,
    /// Why it doesn't, when the user has started setting it up.
    pub problem: Option<String>,
}

pub fn view(attach: &BrowserAttach) -> AttachView {
    let endpoint = if attach.endpoint.trim().is_empty() {
        DEFAULT_ENDPOINT.to_string()
    } else {
        attach.endpoint.trim().to_string()
    };
    let problem = (!loopback_endpoint(&endpoint)).then(|| {
        "The browser must be on this computer: 127.0.0.1, localhost or [::1], with a port."
            .to_string()
    });
    AttachView {
        active: problem.is_none() && !attach.origins.is_empty(),
        endpoint,
        origins: attach.origins.clone(),
        problem,
    }
}

/// Saves the endpoint and the allowed sites (URLs or origins; each is kept
/// as its origin). A site that isn't http(s) is refused and nothing changes.
pub fn save(config_dir: &Path, endpoint: &str, sites: &[String]) -> Result<AttachView, String> {
    let mut attach = BrowserAttach {
        endpoint: endpoint.trim().to_string(),
        origins: Vec::new(),
    };
    for site in sites {
        attach.allow(site)?;
    }
    attach.save(config_dir)?;
    Ok(view(&attach))
}

pub fn load(config_dir: &Path) -> AttachView {
    view(&BrowserAttach::load(config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grokhub-attach-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn nothing_attaches_until_a_site_is_allowed() {
        let dir = scratch("fresh");
        assert_eq!(
            load(&dir),
            AttachView {
                endpoint: DEFAULT_ENDPOINT.into(),
                origins: vec![],
                active: false,
                problem: None,
            }
        );
        let saved = save(
            &dir,
            DEFAULT_ENDPOINT,
            &[
                "https://github.com/user/my-app".into(),
                "https://GitHub.com".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            saved,
            AttachView {
                endpoint: DEFAULT_ENDPOINT.into(),
                origins: vec!["https://github.com".into()],
                active: true,
                problem: None,
            }
        );
        assert_eq!(load(&dir), saved);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_far_browser_or_a_bad_site_is_refused() {
        let dir = scratch("refused");
        let far = save(
            &dir,
            "http://192.168.1.20:9222",
            &["https://github.com".into()],
        )
        .unwrap();
        assert!(!far.active);
        assert_eq!(
            far.problem.as_deref(),
            Some(
                "The browser must be on this computer: 127.0.0.1, localhost or [::1], with a port."
            )
        );
        assert_eq!(
            save(&dir, DEFAULT_ENDPOINT, &["file:///etc/passwd".into()]),
            Err("\"file:///etc/passwd\" isn't an http(s) site".to_string())
        );
        // The refused save left the last one as it was.
        assert_eq!(load(&dir).endpoint, "http://192.168.1.20:9222");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
