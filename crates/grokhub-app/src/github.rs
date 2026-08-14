use grokhub_core::github_api_path;

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
            let body = r.into_string().unwrap_or_default();
            return format!("GitHub {code}: {}", body.chars().take(240).collect::<String>());
        }
        Err(e) => return e.to_string(),
    };
    let v: serde_json::Value = match resp.into_json() {
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
}
