// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Glob, prefix, path, and domain matching.
//! `*` does not cross `/` on paths. `**` does. `..` is collapsed before the match.

use std::path::Path;

use globset::GlobBuilder;

use crate::perm::rules::{PatMode, Rule, Tool};

pub fn normalize_domain(raw: &str) -> String {
    let s = raw.trim().trim_end_matches('/').trim_end_matches('.');
    let s = s.strip_prefix("www.").unwrap_or(s);
    s.to_lowercase()
}

pub fn domain_matches(pattern: &str, url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    let domain = normalize_domain(host);
    let pattern = normalize_domain(pattern);
    if pattern.is_empty() {
        return false;
    }
    domain == pattern || domain.ends_with(&format!(".{pattern}"))
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.trim();
    let rest = rest.split_once("://").map(|(_, rest)| rest).unwrap_or(rest);
    let rest = rest.split('/').next().unwrap_or(rest);
    let rest = rest.split('@').next_back().unwrap_or(rest);
    let host = if let Some(stripped) = rest.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("")
    } else {
        rest.split(':').next().unwrap_or("")
    };
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

pub fn glob_match(pattern: &str, text: &str, path_mode: bool) -> bool {
    let Ok(glob) = GlobBuilder::new(pattern)
        .literal_separator(path_mode)
        .backslash_escape(true)
        .build()
    else {
        return false;
    };
    glob.compile_matcher().is_match(text)
}

/// Word-boundary prefix: `git` matches `git` and `git status`, not `gitleaks`.
pub fn word_prefix(command: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    command == pattern
        || (command.starts_with(pattern) && command.as_bytes().get(pattern.len()) == Some(&b' '))
}

pub fn bash_matches(command: &str, pattern: &str, wide_prefix: bool) -> bool {
    let command = command.trim_start();
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }
    if word_prefix(command, pattern) || glob_match(pattern, command, false) {
        return true;
    }
    wide_prefix && command.starts_with(pattern)
}

pub fn rule_matches_text(rule: &Rule, text: &str, wide_prefix: bool) -> bool {
    match rule.pattern.as_deref() {
        None => true,
        Some("*") => true,
        Some(pattern) => bash_matches(text, pattern, wide_prefix),
    }
}

/// `None` when `raw` leaves the workspace after `.` and `..` collapse, or starts with `~`.
pub fn path_forms(workspace: &Path, raw: &str) -> Option<Vec<String>> {
    let raw = raw.trim().replace('\\', "/");
    if raw.is_empty() || raw == "." {
        let abs = workspace_string(workspace);
        return Some(vec![abs, ".".into(), "./".into()]);
    }
    if raw.starts_with('~') {
        return None;
    }
    let ws = workspace_string(workspace);
    if is_absolute(&raw) {
        let abs = lexical_absolute(&raw)?;
        let ws_abs = lexical_absolute(&ws)?;
        return forms_if_inside(&ws_abs, &abs);
    }
    let (abs, rel) = collapse_relative(&ws, &raw)?;
    if rel.is_empty() {
        Some(vec![abs, ".".into(), "./".into()])
    } else {
        Some(vec![abs, format!("./{rel}"), rel])
    }
}

fn forms_if_inside(ws_abs: &str, abs: &str) -> Option<Vec<String>> {
    let trimmed = ws_abs.trim_end_matches('/');
    let ws = if trimmed.is_empty() { "/" } else { trimmed };
    let inside = if ws == "/" {
        abs.starts_with('/')
    } else {
        abs == ws || abs.starts_with(&format!("{ws}/"))
    };
    if !inside {
        return None;
    }
    let rel = if ws == "/" {
        abs.trim_start_matches('/').to_string()
    } else {
        abs.strip_prefix(ws)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };
    if rel.is_empty() {
        Some(vec![abs.to_string(), ".".into(), "./".into()])
    } else {
        Some(vec![abs.to_string(), rel.clone(), format!("./{rel}")])
    }
}

/// Collapse `raw` against `ws`. `..` that climbs out of `ws` is an escape.
fn collapse_relative(ws: &str, raw: &str) -> Option<(String, String)> {
    let (drive, ws_rest, ws_abs) = split_root(ws);
    let mut base = Vec::new();
    for comp in ws_rest.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            base.pop()?;
            continue;
        }
        base.push(comp.to_string());
    }
    let base_len = base.len();
    let mut stack = base;
    for comp in raw.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            if stack.len() <= base_len {
                return None;
            }
            stack.pop();
            continue;
        }
        stack.push(comp.to_string());
    }
    let rel = stack[base_len..].join("/");
    let body = stack.join("/");
    let abs = if let Some(drive) = drive {
        if body.is_empty() {
            drive
        } else {
            format!("{drive}/{body}")
        }
    } else if ws_abs {
        if body.is_empty() {
            "/".to_string()
        } else {
            format!("/{body}")
        }
    } else {
        body
    };
    Some((abs, rel))
}

pub fn path_escapes(workspace: &Path, raw: &str) -> bool {
    let raw = raw.trim();
    if raw.is_empty() || raw == "." {
        return false;
    }
    path_forms(workspace, raw).is_none()
}

pub fn path_rule_matches(rule: &Rule, workspace: &Path, raw: &str) -> bool {
    if path_escapes(workspace, raw) {
        return false;
    }
    let Some(pattern) = rule.pattern.as_deref() else {
        return true;
    };
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    let Some(forms) = path_forms(workspace, raw) else {
        return false;
    };
    if rule.mode == PatMode::Domain {
        return false;
    }
    forms.iter().any(|form| glob_match(pattern, form, true))
}

pub fn tool_reaches(rule_tool: Tool, kind: Tool) -> bool {
    match rule_tool {
        Tool::Any => true,
        Tool::Read => matches!(kind, Tool::Read | Tool::Grep),
        other => other == kind,
    }
}

fn workspace_string(workspace: &Path) -> String {
    workspace.to_string_lossy().replace('\\', "/")
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
        || (path.len() >= 3
            && path.as_bytes().get(1) == Some(&b':')
            && path
                .as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_alphabetic())
            && matches!(path.as_bytes().get(2), Some(b'/' | b'\\')))
}

fn lexical_absolute(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let (drive, rest, absolute) = split_root(&path);
    if !absolute && drive.is_none() {
        return None;
    }
    let mut stack = Vec::new();
    for comp in rest.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            if stack.pop().is_none() && drive.is_none() {
                continue;
            }
            continue;
        }
        stack.push(comp);
    }
    let body = stack.join("/");
    let prefix = if let Some(drive) = drive {
        format!("{drive}/")
    } else {
        "/".to_string()
    };
    if body.is_empty() {
        Some(prefix.trim_end_matches('/').to_string())
    } else {
        Some(format!("{prefix}{body}"))
    }
}

fn split_root(path: &str) -> (Option<String>, &str, bool) {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let drive = path[..2].to_string();
        let rest = path[2..].trim_start_matches('/');
        return (Some(drive), rest, true);
    }
    if let Some(rest) = path.strip_prefix('/') {
        return (None, rest, true);
    }
    (None, path, false)
}
