//! Outside links: a web page the UI links to opens in the system browser.
//! The webview itself never navigates away from the app, and links with
//! `target="_blank"` do nothing inside Tauri, so the UI asks for this instead.
//! Not on the web-access allowlist: a paired browser opens links itself.

/// http(s) with a host, and nothing a shell or `cmd` would read specially.
pub fn url_ok(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
        && !host.starts_with(':')
        && !url.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(
                    c,
                    '^' | '|' | '`' | '\\' | '{' | '}' | '\'' | '"' | '<' | '>'
                )
        })
}

/// Hand the URL to the platform's opener as one argument, never a shell string.
pub fn open(url: &str) -> Result<(), String> {
    if !url_ok(url) {
        return Err("That link can't be opened.".into());
    }
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't open the browser: {e}"))?;
    // Reap the opener so it doesn't linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::url_ok;

    #[test]
    fn only_plain_web_pages_pass() {
        assert!(url_ok("https://example.com/a?b=1#c"));
        assert!(url_ok("http://localhost:8080/x"));
        assert!(!url_ok("file:///etc/passwd"));
        assert!(!url_ok("javascript:alert(1)"));
        assert!(!url_ok("https://"));
        assert!(!url_ok("https:///path"));
        assert!(!url_ok("https://example.com/a b"));
        assert!(!url_ok("https://example.com/\"&calc"));
        assert!(!url_ok("https://example.com/|x"));
        assert!(!url_ok("ms-settings:"));
    }
}
