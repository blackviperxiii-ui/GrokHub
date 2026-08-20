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
        assert_eq!(parse_args(&args(&["grokhub", "-V"])), Launch::Version);
        assert_eq!(parse_args(&args(&["grokhub", "--version"])), Launch::Version);
    }

    #[test]
    fn cabin_reports_version() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "2.6.42");
    }
}
