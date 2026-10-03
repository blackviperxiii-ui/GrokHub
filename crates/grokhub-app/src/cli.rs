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
        let oauth = include_str!("main.rs")
            .split("fn run_oauth_cli(")
            .nth(1)
            .and_then(|s| s.split("fn probe_hub_health_body(").next())
            .expect("run_oauth_cli");
        assert!(
            oauth.contains("write_cli_auth_if_needed"),
            "grokhub --oauth must sign in the CLI: {oauth}"
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
    fn cabin_reports_version() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "2.10.67");
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
            doctor.contains("doctor_grok_line_blocking") && doctor.contains("find_grok"),
            "grokhub --doctor must report grok CLI using the same locate path as the cabin: {doctor}"
        );
    }
}
