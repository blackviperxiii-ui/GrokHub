use grokhub_core::{github_api_path, MEDIA_FILE_CAP, TEXT_FILE_CAP};
use std::io::Read;

pub fn run_github_tool(tool: &str, args: &str, token: &str) -> String {
    if token.trim().is_empty() {
        return "No GitHub token. Settings → paste a classic/fine-grained PAT with repo scope."
            .into();
    }
    let path = match github_api_path(tool, args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let url = format!("https://api.github.com{path}");
    let resp = match ureq::get(&url)
        .set("authorization", &format!("Bearer {}", token.trim()))
        .set("user-agent", "GrokHub")
        .set("accept", "application/vnd.github+json")
        .set("x-github-api-version", "2022-11-28")
        .timeout(std::time::Duration::from_secs(30))
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let mut buf = Vec::new();
            let _ = r
                .into_reader()
                .take(TEXT_FILE_CAP as u64)
                .read_to_end(&mut buf);
            let body = String::from_utf8_lossy(&buf);
            return format!("GitHub {code}: {}", body.chars().take(240).collect::<String>());
        }
        Err(e) => return e.to_string(),
    };
    let mut buf = Vec::new();
    if resp
        .into_reader()
        .take(MEDIA_FILE_CAP + 1)
        .read_to_end(&mut buf)
        .is_err()
    {
        return "GitHub response read failed".into();
    }
    if buf.len() as u64 > MEDIA_FILE_CAP {
        return "GitHub response too large".into();
    }
    let v: serde_json::Value = match serde_json::from_slice(&buf) {
        Ok(v) => v,
        Err(e) => return e.to_string(),
    };
    format_github(tool, &v)
}

fn format_github(tool: &str, v: &serde_json::Value) -> String {
    match tool {
        "user" | "me" => {
            let login = v.get("login").and_then(|x| x.as_str()).unwrap_or("?");
            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let repos = v.get("public_repos").and_then(|x| x.as_u64()).unwrap_or(0);
            if name.is_empty() {
                format!("Authenticated as {login} · {repos} public repos")
            } else {
                format!("Authenticated as {login} ({name}) · {repos} public repos")
            }
        }
        "list_repos" | "repos" => v
            .as_array()
            .map(|arr| {
                arr.iter()
                    .take(20)
                    .map(|x| {
                        let n = x.get("full_name").and_then(|s| s.as_str()).unwrap_or("?");
                        let priv_ = x.get("private").and_then(|s| s.as_bool()).unwrap_or(false);
                        let d = x.get("description").and_then(|s| s.as_str()).unwrap_or("");
                        format!("- {n}{} — {d}", if priv_ { " (private)" } else { "" })
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "(no repos)".into()),
        "list_issues" | "issues" => v
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| {
                        let n = i.get("number")?.as_u64()?;
                        let t = i.get("title")?.as_str()?;
                        let u = i
                            .get("user")
                            .and_then(|u| u.get("login"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("?");
                        Some(format!("#{n} {t} (@{u})"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "No open issues".into()),
        "search_code" | "code_search" => v
            .get("items")
            .and_then(|i| i.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|it| {
                        let repo = it
                            .get("repository")
                            .and_then(|r| r.get("full_name"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("?");
                        let path = it.get("path").and_then(|s| s.as_str()).unwrap_or("");
                        let url = it.get("html_url").and_then(|s| s.as_str()).unwrap_or("");
                        format!("- {repo}/{path}\n  {url}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "No code matches".into()),
        "search_issues" => v
            .get("items")
            .and_then(|i| i.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|it| {
                        let t = it.get("title").and_then(|s| s.as_str()).unwrap_or("?");
                        let url = it.get("html_url").and_then(|s| s.as_str()).unwrap_or("");
                        format!("- {t}\n  {url}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "No matches".into()),
        _ => v.to_string().chars().take(800).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_token_is_honest() {
        let s = run_github_tool("user", "", "");
        assert!(s.contains("No GitHub token"), "{s}");
    }

    #[test]
    fn issues_need_repo() {
        let s = run_github_tool("list_issues", "", "dummy");
        assert!(s.contains("Need repo:"), "{s}");
    }

    #[test]
    fn github_http_does_not_slurp_a_huge_body() {
        let src = include_str!("github.rs");
        let run = src
            .split("pub fn run_github_tool(")
            .nth(1)
            .and_then(|s| s.split("fn format_github(").next())
            .expect("run_github_tool");
        assert!(
            run.contains(".take(") && !run.contains("into_string()"),
            "GitHub errors must not slurp a huge error page: {run}"
        );
        assert!(
            !run.contains("into_json()"),
            "GitHub JSON must not slurp an unbounded search body: {run}"
        );
        assert!(
            run.contains("MEDIA_FILE_CAP") || run.contains("TEXT_FILE_CAP"),
            "GitHub reads must stop at a cabin cap: {run}"
        );
    }
}
