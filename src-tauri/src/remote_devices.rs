//! The Devices page: the browsers paired with Settings → Web access right
//! now. `remote.rs` keeps them with the live pairing code, in memory only,
//! so they go when the code changes, web access turns off or the app quits.
//! Nothing here is written to disk.

use std::collections::HashSet;

use serde::Serialize;

/// Paired browsers kept at once; past this the least recently used goes.
const MAX_BROWSERS: usize = 32;
/// The bytes of a User-Agent kept to tell browsers apart.
const MAX_AGENT: usize = 512;

/// One browser using the live pairing code, told apart by its User-Agent.
/// Two devices with exactly the same browser and system show as one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    agent: String,
    /// Unix ms.
    first_seen: u64,
    /// Unix ms.
    last_call: u64,
    /// The chats it sent a message in.
    chats: HashSet<String>,
}

/// The browsers paired with one code.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Browsers(Vec<Browser>);

impl Browsers {
    /// A paired call or stream from `agent` at `at` (unix ms).
    pub fn saw(&mut self, agent: &str, at: u64) {
        let agent = clip(agent);
        if let Some(known) = self.0.iter_mut().find(|b| b.agent == agent) {
            known.last_call = known.last_call.max(at);
            return;
        }
        if self.0.len() >= MAX_BROWSERS {
            if let Some(oldest) = (0..self.0.len()).min_by_key(|&i| self.0[i].last_call) {
                self.0.swap_remove(oldest);
            }
        }
        self.0.push(Browser {
            agent: agent.to_string(),
            first_seen: at,
            last_call: at,
            chats: HashSet::new(),
        });
    }

    /// `agent` sent a message in `chat`, or (`kept` false) its message there
    /// was refused, so the chat isn't its own.
    pub fn chat(&mut self, agent: &str, chat: &str, kept: bool) {
        let agent = clip(agent);
        if let Some(known) = self.0.iter_mut().find(|b| b.agent == agent) {
            if kept {
                known.chats.insert(chat.to_string());
            } else {
                known.chats.remove(chat);
            }
        }
    }

    /// The list the page shows, the most recently used first.
    pub fn views(&self) -> Vec<BrowserView> {
        let mut out: Vec<BrowserView> = self
            .0
            .iter()
            .map(|b| BrowserView {
                name: family(&b.agent),
                first_seen: b.first_seen,
                last_call: b.last_call,
                chats: b.chats.len(),
            })
            .collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.last_call));
        out
    }
}

fn clip(agent: &str) -> &str {
    let mut end = agent.len().min(MAX_AGENT);
    while !agent.is_char_boundary(end) {
        end -= 1;
    }
    agent[..end].trim()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserView {
    /// The browser and its system, like "Firefox on Android".
    pub name: String,
    /// Unix ms.
    pub first_seen: u64,
    /// Unix ms.
    pub last_call: u64,
    /// Chats it sent a message in.
    pub chats: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesView {
    /// This computer's name.
    pub computer: String,
    /// Whether web access is on.
    pub on: bool,
    /// The web app's address on the LAN, without the code; empty while off
    /// or with no network.
    pub lan: String,
    pub browsers: Vec<BrowserView>,
}

/// "Firefox on Android" from a User-Agent; the version and the rest stay out.
pub fn family(agent: &str) -> String {
    let has = |part: &str| agent.contains(part);
    let browser = if has("Edg/") || has("EdgA/") || has("EdgiOS/") {
        "Edge"
    } else if has("OPR/") || has("OPiOS/") {
        "Opera"
    } else if has("SamsungBrowser/") {
        "Samsung Internet"
    } else if has("Firefox/") || has("FxiOS/") {
        "Firefox"
    } else if has("Chrome/") || has("CriOS/") || has("Chromium/") {
        "Chrome"
    } else if has("Safari/") && has("Version/") {
        "Safari"
    } else {
        "A browser"
    };
    let system = if has("Android") {
        "Android"
    } else if has("iPhone") {
        "iPhone"
    } else if has("iPad") {
        "iPad"
    } else if has("CrOS") {
        "ChromeOS"
    } else if has("Windows") {
        "Windows"
    } else if has("Macintosh") || has("Mac OS X") {
        "macOS"
    } else if has("Linux") {
        "Linux"
    } else {
        return browser.to_string();
    };
    format!("{browser} on {system}")
}

/// This computer's name, as the system reports it.
pub fn computer_name() -> String {
    first_name([
        std::env::var("COMPUTERNAME").ok(),
        std::env::var("HOSTNAME").ok(),
        std::fs::read_to_string("/proc/sys/kernel/hostname").ok(),
        std::fs::read_to_string("/etc/hostname").ok(),
    ])
}

fn first_name(candidates: impl IntoIterator<Item = Option<String>>) -> String {
    candidates
        .into_iter()
        .flatten()
        .map(|name| name.trim().to_string())
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "This computer".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX_ANDROID: &str =
        "Mozilla/5.0 (Android 14; Mobile; rv:131.0) Gecko/131.0 Firefox/131.0";
    const SAFARI_IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1";

    #[test]
    fn names_the_browser_and_its_system() {
        assert_eq!(family(FIREFOX_ANDROID), "Firefox on Android");
        assert_eq!(family(SAFARI_IPHONE), "Safari on iPhone");
        assert_eq!(
            family("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0"),
            "Edge on Windows"
        );
        assert_eq!(
            family("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"),
            "Chrome on Linux"
        );
        assert_eq!(
            family("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15"),
            "Safari on macOS"
        );
        assert_eq!(
            family("Mozilla/5.0 (Linux; Android 14; SM-S921B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36"),
            "Samsung Internet on Android"
        );
        assert_eq!(family("curl/8.5.0"), "A browser");
        assert_eq!(family(""), "A browser");
    }

    #[test]
    fn a_browser_is_kept_once_with_its_first_and_last_use_and_its_chats() {
        let mut browsers = Browsers::default();
        browsers.saw(FIREFOX_ANDROID, 1_000);
        browsers.saw(SAFARI_IPHONE, 2_000);
        browsers.saw(FIREFOX_ANDROID, 3_000);
        browsers.chat(FIREFOX_ANDROID, "c1", true);
        browsers.chat(FIREFOX_ANDROID, "c2", true);
        browsers.chat(FIREFOX_ANDROID, "c1", true);
        // A message refused in the window's chat doesn't make it the browser's.
        browsers.chat(SAFARI_IPHONE, "window-1", true);
        browsers.chat(SAFARI_IPHONE, "window-1", false);
        // A browser not seen with this code gets nothing.
        browsers.chat("Unknown/1.0", "c3", true);
        assert_eq!(
            browsers.views(),
            vec![
                BrowserView {
                    name: "Firefox on Android".into(),
                    first_seen: 1_000,
                    last_call: 3_000,
                    chats: 2,
                },
                BrowserView {
                    name: "Safari on iPhone".into(),
                    first_seen: 2_000,
                    last_call: 2_000,
                    chats: 0,
                },
            ]
        );
    }

    #[test]
    fn past_the_cap_the_least_recently_used_browser_goes() {
        let mut browsers = Browsers::default();
        for n in 0..MAX_BROWSERS as u64 {
            browsers.saw(&format!("Agent/{n}"), 100 + n);
        }
        // Agent/0 is used again, so Agent/1 is now the stalest.
        browsers.saw("Agent/0", 500);
        browsers.saw("Agent/new", 600);
        assert_eq!(browsers.0.len(), MAX_BROWSERS);
        let agents: Vec<&str> = browsers.0.iter().map(|b| b.agent.as_str()).collect();
        assert!(agents.contains(&"Agent/0"));
        assert!(agents.contains(&"Agent/new"));
        assert!(!agents.contains(&"Agent/1"));
    }

    #[test]
    fn a_long_user_agent_is_clipped_on_a_character_boundary() {
        let long = "é".repeat(400);
        assert_eq!(clip(&long).len(), 512);
        assert_eq!(clip("  Agent/1  "), "Agent/1");
    }

    #[test]
    fn the_computer_name_is_the_first_one_given() {
        assert_eq!(
            first_name([None, Some(" \n".into()), Some("desk-1\n".into())]),
            "desk-1"
        );
        assert_eq!(first_name([None, None]), "This computer");
    }
}
