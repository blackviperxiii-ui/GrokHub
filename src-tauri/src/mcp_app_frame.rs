//! The page an MCP App runs in. The chat's own CSP blocks inline script, and
//! a `srcdoc` frame inherits it, so the app gets a page of its own: this shim,
//! served with [`CSP`], takes the app's HTML over postMessage
//! (`ui/notifications/sandbox-resource-ready`) and becomes it. The desktop
//! window loads it from the [`SCHEME`] protocol, a browser from [`PATH`] on
//! the desktop's web server.

/// The shell's protocol for the frame: `mcp-app://localhost/`, or
/// `http://mcp-app.localhost/` on Windows. The window's CSP names both.
pub const SCHEME: &str = "mcp-app";

/// The web server's route for the frame.
pub const PATH: &str = "/mcp-app-frame";

/// `sandbox` keeps the frame at an opaque origin even when it is opened on
/// its own, so the inline script it allows never runs as the desktop's
/// origin. The rest matches the agent's default MCP App CSP: no network.
pub const CSP: &str = "sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

pub const HTML: &str = include_str!("mcp_app_frame.html");

/// The shell protocol's answer, whatever path the window asked for.
pub fn response() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", CSP)
        .body(HTML.as_bytes().to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_protocol_serves_the_frame_with_its_csp() {
        let res = response();
        assert_eq!(res.status(), 200);
        assert_eq!(res.headers()["Content-Security-Policy"], CSP);
        assert_eq!(res.headers()["Content-Type"], "text/html; charset=utf-8");
        assert_eq!(res.body().as_slice(), HTML.as_bytes());
    }

    /// The page finds the frame by these names; they must match.
    #[test]
    fn the_chat_page_loads_the_frame_by_these_names() {
        let host = include_str!("../../ui/src/McpAppHost.tsx");
        assert!(
            host.contains(&format!("convertFileSrc(\"frame\", \"{SCHEME}\")")),
            "McpAppHost.tsx"
        );
        assert!(host.contains(&format!("\"{PATH}\"")), "McpAppHost.tsx");
    }
}
