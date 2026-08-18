use std::process::Command;
use std::time::Duration;

pub fn allow_ping(quiet_hours: bool) -> bool {
    !quiet_hours
}

pub fn should_ping_long(elapsed: Duration) -> bool {
    elapsed >= Duration::from_secs(30)
}

pub fn ping_args<'a>(title: &'a str, body: &'a str) -> Vec<&'a str> {
    vec![
        "-a",
        "GrokHub",
        "-u",
        "low",
        "-t",
        "4000",
        "-h",
        "string:desktop-entry:grokhub",
        "-h",
        "string:x-canonical-private-synchronous:grokhub",
        title,
        body,
    ]
}

pub fn ping(title: &str, body: &str) {
    let _ = Command::new("notify-send").args(ping_args(title, body)).spawn();
}

pub fn ping_if_long_quiet(elapsed: Duration, quiet_hours: bool, title: &str, body: &str) {
    if should_ping_long(elapsed) && allow_ping(quiet_hours) {
        ping(title, body);
    }
}

pub fn inhibit_sleep() -> Option<std::process::Child> {
    Command::new("systemd-inhibit")
        .args([
            "--what=idle:sleep",
            "--who=GrokHub",
            "--why=host-job",
            "--mode=block",
            "sleep",
            "inf",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

pub fn release_inhibit(child: &mut Option<std::process::Child>) {
    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_replaces_and_expires() {
        let args = ping_args("GrokHub", "Still running in the tray");
        assert!(args.contains(&"-a"));
        assert!(args.contains(&"GrokHub"));
        assert!(args.contains(&"-u"));
        assert!(args.contains(&"low"));
        assert!(args.contains(&"-t"));
        assert!(args.iter().any(|a| a.contains("desktop-entry:grokhub")));
        assert!(args
            .iter()
            .any(|a| a.contains("x-canonical-private-synchronous:grokhub")));
        assert_eq!(args[args.len() - 2], "GrokHub");
        assert_eq!(args[args.len() - 1], "Still running in the tray");
    }

    #[test]
    fn quiet_hours_suppress_desktop_ping() {
        assert!(!allow_ping(true));
        assert!(allow_ping(false));
    }

    #[test]
    fn long_job_ping_respects_the_30s_floor() {
        assert!(!should_ping_long(Duration::from_secs(29)));
        assert!(should_ping_long(Duration::from_secs(30)));
    }
}
