use std::process::Command;
use std::time::Duration;

pub fn ping(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "GrokHub", title, body])
        .spawn();
}

pub fn ping_if_long(elapsed: Duration, title: &str, body: &str) {
    if elapsed >= Duration::from_secs(30) {
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
