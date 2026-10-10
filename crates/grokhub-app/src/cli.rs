#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    Cabin,
    Agent,
    Hub,
    Version,
    Doctor,
    Update,
    Oauth,
    Help,
    McpDesktop,
    McpCua,
    McpSelf,
}

/// `grokhub --askpass <socket> [prompt]`: `sudo` runs this through the helper script.
pub fn askpass_socket(args: &[String]) -> Option<&str> {
    match args {
        [_, flag, socket, ..] if flag == "--askpass" => Some(socket),
        _ => None,
    }
}

pub fn parse_args(args: &[String]) -> Launch {
    let mut out = Launch::Cabin;
    for a in args.iter().skip(1) {
        match a.as_str() {
            "--hub" => out = Launch::Hub,
            "--agent" | "--tray" => out = Launch::Agent,
            "--version" | "-V" => return Launch::Version,
            "--doctor" => return Launch::Doctor,
            "--update" => return Launch::Update,
            "--oauth" => return Launch::Oauth,
            "--mcp-desktop" => return Launch::McpDesktop,
            "--mcp-cua" => return Launch::McpCua,
            "--mcp-self" => return Launch::McpSelf,
            "-h" | "--help" => return Launch::Help,
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn askpass_takes_the_socket_and_only_as_the_first_flag() {
        assert_eq!(askpass_socket(&args(&["grokhub", "--askpass", "/run/user/1000/grokhub-askpass-9/s", "[sudo] password for j: "])), Some("/run/user/1000/grokhub-askpass-9/s"));
        assert_eq!(askpass_socket(&args(&["grokhub", "--askpass"])), None);
        assert_eq!(askpass_socket(&args(&["grokhub", "--hub", "--askpass", "/x"])), None);
        assert_eq!(parse_args(&args(&["grokhub", "--askpass", "/x"])), Launch::Cabin);
    }

    #[test]
    fn flags() {
        assert_eq!(parse_args(&args(&["grokhub"])), Launch::Cabin);
        assert_eq!(parse_args(&args(&["grokhub", "--hub"])), Launch::Hub);
        assert_eq!(parse_args(&args(&["grokhub", "--agent"])), Launch::Agent);
        assert_eq!(parse_args(&args(&["grokhub", "--tray"])), Launch::Agent);
        assert_eq!(parse_args(&args(&["grokhub", "--doctor"])), Launch::Doctor);
        assert_eq!(parse_args(&args(&["grokhub", "--update"])), Launch::Update);
        assert_eq!(parse_args(&args(&["grokhub", "--oauth"])), Launch::Oauth);
        assert_eq!(
            parse_args(&args(&["grokhub", "--mcp-desktop"])),
            Launch::McpDesktop
        );
        assert_eq!(
            parse_args(&args(&["grokhub", "--hub", "--mcp-desktop"])),
            Launch::McpDesktop
        );
        let sign_in_fn = include_str!("main.rs")
            .split("fn run_oauth_cli(")
            .nth(1)
            .and_then(|s| s.split("fn probe_hub_health_body(").next())
            .expect("run_oauth_cli");
        assert!(
            sign_in_fn.contains("secrets::save(&s)")
                && !sign_in_fn.contains("write_cli_auth_if_needed")
                && !sign_in_fn.contains("grokhub_acp"),
            "grokhub --oauth keeps the sign-in to GrokHub and never writes the CLI's login (see run_oauth_cli in main.rs)"
        );
        assert_eq!(parse_args(&args(&["grokhub", "-V"])), Launch::Version);
        assert_eq!(parse_args(&args(&["grokhub", "--version"])), Launch::Version);
    }

    #[test]
    fn desktop_mcp_launch_flag() {
        assert_eq!(
            parse_args(&args(&["grokhub", "--mcp-desktop"])),
            Launch::McpDesktop
        );
        assert_eq!(
            parse_args(&args(&["grokhub", "--version", "--mcp-desktop"])),
            Launch::Version,
            "--version still returns before later flags"
        );
        let main = include_str!("main.rs");
        assert!(
            main.contains("Launch::McpDesktop") && main.contains("desktop_mcp::run_stdio"),
            "grokhub --mcp-desktop must run the stdio server and skip the cabin window"
        );
        assert!(
            !main.contains("attach_cli_console") || main.contains("Launch::Cabin | Launch::Agent | Launch::McpDesktop"),
            "the desktop server must not attach a console"
        );
    }

    #[test]
    fn cua_gate_launch_flag() {
        assert_eq!(parse_args(&args(&["grokhub", "--mcp-cua"])), Launch::McpCua);
        assert_eq!(parse_args(&args(&["grokhub", "--hub", "--mcp-cua"])), Launch::McpCua);
        assert_eq!(parse_args(&args(&["grokhub", "--version", "--mcp-cua"])), Launch::Version);
    }

    #[test]
    fn cabin_reports_version() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "2.12.1");
    }

    #[test]
    fn doctor_probes_live_hub_kind() {
        let main = include_str!("main.rs");
        let doctor = main
            .split("fn probe_hub_health_body()")
            .nth(1)
            .and_then(|s| s.split("fn run_update_cli()").next())
            .expect("doctor probe");
        assert!(
            !doctor.contains("doctor_lines(authed, mem_ok, HUB_KIND)"),
            "grokhub --doctor must not stamp hub kind as the compile-time constant: {doctor}"
        );
        assert!(
            (doctor.contains("/v1/health") || doctor.contains("desktop::probe_hub_health_body"))
                && doctor.contains("hub_kind_from_health"),
            "doctor must read kind from live /v1/health: {doctor}"
        );
        assert!(
            doctor.contains("doctor_cabin_line") || doctor.contains("cabin_running"),
            "grokhub --doctor must report whether the cabin process is alive: {doctor}"
        );
        assert!(
            !doctor.contains("doctor_grok_line") && !doctor.contains("find_grok"),
            "grokhub --doctor checks GrokHub alone, never the Grok Build CLI: {doctor}"
        );
    }
}
