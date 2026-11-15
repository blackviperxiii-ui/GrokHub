//! Connectors → GitHub: a personal access token in the OS keyring and two
//! read-only calls, Who am I and List repos. Nothing is ever written to
//! GitHub, and the token is never returned to the page or logged.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use grokhub_agent::harness as hx;
use grokhub_agent::route::providers::{named_key, set_named_key, NamedKey};
use serde::{Deserialize, Serialize};

/// The token's keyring entry, which no AI provider can share.
const TOKEN: NamedKey = NamedKey::GithubToken;
const API: &str = "https://api.github.com";
const TIMEOUT: Duration = Duration::from_secs(20);
/// GitHub's answers here are a few KB; anything this big is not one.
const ANSWER_CAP: u64 = 2 * 1024 * 1024;
/// A server's error message is shown, cut to this many characters.
const MESSAGE_CAP: usize = 160;

pub const NOT_A_TOKEN: &str =
    "A GitHub token is letters, digits and underscores, with no spaces. Paste it again.";
pub const SAVE_FIRST: &str = "Save a GitHub token first.";
const SAVED: &str = "GitHub token saved.";
const REMOVED: &str = "GitHub token removed.";

/// The two read-only calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    User,
    ListRepos,
}

impl Tool {
    fn path(self) -> &'static str {
        match self {
            Self::User => "/user",
            Self::ListRepos => "/user/repos?per_page=20&sort=updated",
        }
    }
}

/// What the GitHub section shows. Whether a token is saved, never the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub token_set: bool,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRow {
    /// `owner/name`.
    pub name: String,
    pub private: bool,
    pub description: String,
}

/// One call's answer: a line to show and, for List repos, the repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ran {
    pub summary: String,
    pub repos: Vec<RepoRow>,
}

/// Whether a token is saved. A keyring that can't be read counts as none.
pub fn token_present(config_dir: &Path) -> bool {
    named_key(config_dir, TOKEN).ok().flatten().is_some()
}

pub fn view(config_dir: &Path) -> View {
    View {
        token_set: token_present(config_dir),
        status: String::new(),
    }
}

/// Save the token; an empty one removes the saved token.
pub fn save_token(config_dir: &Path, token: &str) -> Result<View, String> {
    let token = token.trim();
    // Every GitHub token is visible ASCII; anything else could never be
    // sent as a header, and would read later as "Couldn't reach GitHub."
    if !token.chars().all(|c| c.is_ascii_graphic()) {
        return Err(NOT_A_TOKEN.into());
    }
    set_named_key(config_dir, TOKEN, token)?;
    Ok(View {
        token_set: token_present(config_dir),
        status: if token.is_empty() { REMOVED } else { SAVED }.into(),
    })
}

/// Run one call against api.github.com with the saved token.
pub fn run(config_dir: &Path, tool: Tool) -> Result<Ran, String> {
    run_at(config_dir, API, tool, true)
}

/// [`run`] against `base`. Tests point it at a server on this computer, so
/// they turn off the public-only resolver; [`run`] always keeps it on.
pub(crate) fn run_at(
    config_dir: &Path,
    base: &str,
    tool: Tool,
    public_only: bool,
) -> Result<Ran, String> {
    let token = named_key(config_dir, TOKEN)?.ok_or(SAVE_FIRST)?;
    let url = format!("{base}{}", tool.path());
    // The request carries the token and nothing from a chat.
    hx::guard_quiet(config_dir, &hx::EgressReq::new(&url, &[]))?;
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        overall: Some(TIMEOUT),
        redirects: 0,
        public_only,
        ..Default::default()
    });
    let resp = agent
        .get(&url)
        .header("authorization", &format!("Bearer {}", token.as_str()))
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2022-11-28")
        .header("user-agent", grokhub_agent::USER_AGENT)
        .call()
        .map_err(refused)?;
    // A redirect is not followed: the token goes to api.github.com only.
    if resp.status() != 200 {
        return Err(format!("GitHub answered {}.", resp.status()));
    }
    let mut raw = Vec::new();
    resp.into_reader()
        .take(ANSWER_CAP + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "Couldn't read GitHub's answer.".to_string())?;
    if raw.len() as u64 > ANSWER_CAP {
        return Err("GitHub's answer was too large to read.".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|_| "Couldn't read GitHub's answer.".to_string())?;
    Ok(match tool {
        Tool::User => who_am_i(&value),
        Tool::ListRepos => repos(&value),
    })
}

fn refused(err: grokhub_core::http::Error) -> String {
    match err {
        grokhub_core::http::Error::Status {
            code: 401, body, ..
        } => {
            format!(
                "GitHub didn't accept this token ({}). Paste a new one.",
                with_message(401, &body)
            )
        }
        grokhub_core::http::Error::Status { code, body, .. } => {
            format!("GitHub refused this ({}).", with_message(code, &body))
        }
        grokhub_core::http::Error::Transport { .. } => "Couldn't reach GitHub.".into(),
    }
}

/// `401 Bad credentials`: the code and GitHub's own message, if it sent one.
fn with_message(code: u16, body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message")?.as_str().map(str::to_string))
        .map(|m| m.chars().take(MESSAGE_CAP).collect::<String>())
        .unwrap_or_default();
    if message.trim().is_empty() {
        code.to_string()
    } else {
        format!("{code} {}", message.trim())
    }
}

fn text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("").trim()
}

fn who_am_i(user: &serde_json::Value) -> Ran {
    let login = text(user, "login");
    let name = text(user, "name");
    let count = user
        .get("public_repos")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let who = if name.is_empty() || name == login {
        login.to_string()
    } else {
        format!("{login} ({name})")
    };
    let repos = if count == 1 { "repo" } else { "repos" };
    Ran {
        summary: format!("Signed in as {who} · {count} public {repos}"),
        repos: Vec::new(),
    }
}

fn repos(list: &serde_json::Value) -> Ran {
    let repos: Vec<RepoRow> = list
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|r| !text(r, "full_name").is_empty())
                .map(|r| RepoRow {
                    name: text(r, "full_name").to_string(),
                    private: r.get("private").and_then(|v| v.as_bool()).unwrap_or(false),
                    description: text(r, "description").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let summary = match repos.len() {
        0 => "No repositories on this account yet.".to_string(),
        1 => "1 repository.".to_string(),
        n => format!("{n} repositories, most recently updated first."),
    };
    Ran { summary, repos }
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
