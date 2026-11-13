//! Round 3 item 38: a package cooldown for everything the agent installs.
//!
//! Most poisoned npm and PyPI releases are caught and pulled within days, so
//! every agent shell command and every MCP server the app starts gets its
//! package managers told to skip versions younger than [`DEFAULT_DAYS`]:
//!
//! - npm 11.10+: `NPM_CONFIG_MIN_RELEASE_AGE` (days).
//! - uv: `UV_EXCLUDE_NEWER`, and pip 26+: `PIP_UPLOADED_PRIOR_TO`, both as a
//!   timestamp so older releases that only read timestamps still take it.
//! - pipx 1.17+: `PIPX_COOLDOWN`; Poetry 2.4+: `POETRY_SOLVER_MIN_RELEASE_AGE`
//!   (days).
//!
//! Older releases of each ignore the setting. A value already in the app's
//! own environment is the user's choice and stays. `days = 0` in
//! [`CONFIG`] turns the cooldown off.
//!
//! A plain `npm install` (or `pnpm install`, `yarn install`, `uv sync`) in a
//! folder with a lockfile also gets one note pointing at the locked form,
//! which installs exactly what the lockfile pins.
//!
//! Source: Cognition, "A practical guide to security", 2026-11-03, and
//! <https://cooldowns.dev/> for each tool's setting.

use std::path::Path;

pub const CONFIG: &str = "package_age.toml";
/// The cooldown when the file sets none.
pub const DEFAULT_DAYS: u64 = 7;
/// The longest cooldown the file can ask for.
const MAX_DAYS: u64 = 90;

/// The cooldown in days from `config_dir`'s [`CONFIG`].
pub fn days(config_dir: &Path) -> u64 {
    std::fs::read_to_string(config_dir.join(CONFIG))
        .map(|text| parse_days(&text))
        .unwrap_or(DEFAULT_DAYS)
}

fn parse_days(text: &str) -> u64 {
    crate::config_file::parse(CONFIG, text, &["days"])
        .count("days", 0, DEFAULT_DAYS as usize)
        .min(MAX_DAYS as usize) as u64
}

/// The cooldown settings for `days` at `now_ms`, before the user's own
/// environment is taken into account. Empty when `days` is 0.
pub fn env_for(days: u64, now_ms: u64) -> Vec<(String, String)> {
    if days == 0 {
        return Vec::new();
    }
    let cutoff = grokhub_core::net_log::iso_utc(now_ms.saturating_sub(days * 86_400_000));
    let n = days.to_string();
    [
        ("NPM_CONFIG_MIN_RELEASE_AGE", n.clone()),
        ("UV_EXCLUDE_NEWER", cutoff.clone()),
        ("PIP_UPLOADED_PRIOR_TO", cutoff),
        ("PIPX_COOLDOWN", n.clone()),
        ("POETRY_SOLVER_MIN_RELEASE_AGE", n),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// What an agent command or MCP server gets: [`env_for`] with the user's
/// configured days, minus any setting already in the app's environment.
pub fn env() -> Vec<(String, String)> {
    let days = days(&crate::perm::config_dir());
    env_for(days, grokhub_core::now_ms())
        .into_iter()
        .filter(|(k, _)| {
            std::env::var_os(k).is_none() && std::env::var_os(k.to_lowercase()).is_none()
        })
        .collect()
}

/// One note when `command` installs from a manifest in `cwd` while a
/// lockfile sits beside it.
pub fn lockfile_hint(command: &str, cwd: &Path) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let plain = |tool: &str, verbs: &[&str]| {
        words.len() == 2 && words[0] == tool && verbs.contains(&words[1])
    };
    let (lock, better) = if plain("npm", &["install", "i"]) {
        ("package-lock.json", "npm ci")
    } else if plain("pnpm", &["install", "i"]) {
        ("pnpm-lock.yaml", "pnpm install --frozen-lockfile")
    } else if plain("yarn", &["install"]) || words == ["yarn"] {
        ("yarn.lock", "yarn install --immutable")
    } else if plain("uv", &["sync"]) {
        ("uv.lock", "uv sync --locked")
    } else {
        return None;
    };
    cwd.join(lock)
        .is_file()
        .then(|| format!("Note: this folder has {lock}; `{better}` installs exactly what it pins."))
}

#[cfg(test)]
#[path = "package_age_tests.rs"]
mod tests;
