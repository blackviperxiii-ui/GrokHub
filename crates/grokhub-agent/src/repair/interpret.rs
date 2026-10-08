//! Probe output to plain findings. Pure parsers: no I/O, fixture-tested.

use super::probes::ProbeId;

/// Disk this full (percent used) is a warning.
pub const DISK_WARN_PCT: u64 = 90;
/// Disk this full is critical: updates and saves start failing.
pub const DISK_CRIT_PCT: u64 = 97;
/// Less available memory than this (percent of total) is a warning.
pub const MEM_WARN_PCT: u64 = 10;
/// This many log errors since boot, or in 24 h, is a warning, not a note.
pub const LOG_WARN_LINES: usize = 10;
/// Names a finding lists before it says "and N more".
const NAMES_SHOWN: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Ok,
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

/// One thing a probe found, in words a person reads first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// Short sentences, no command names, no unexplained jargon.
    pub plain: String,
    /// The redacted lines behind it, for "show details".
    pub detail: String,
    pub probe: ProbeId,
}

impl Finding {
    fn new(probe: ProbeId, severity: Severity, plain: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            severity,
            plain: plain.into(),
            detail: detail.into(),
            probe,
        }
    }
}

/// What a finished probe printed (already redacted), and which program ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    pub program: String,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Findings for one probe's output.
pub fn interpret(probe: ProbeId, out: &ProbeOutput) -> Vec<Finding> {
    match probe {
        ProbeId::DiskUsage => disk_usage(out),
        ProbeId::Volumes => volumes(out),
        ProbeId::Memory => memory(out),
        ProbeId::FailedServices => failed_services(out),
        ProbeId::ServicesStoppedAuto => stopped_auto(out),
        ProbeId::BootErrors | ProbeId::EventErrors24h => log_errors(probe, out),
        ProbeId::NetworkLinks => network_links(out),
        ProbeId::Dns => dns(out),
        ProbeId::NetTest => net_test(out),
        ProbeId::PackageHealth => package_health(out),
        ProbeId::PendingUpdates => pending_updates(out),
        ProbeId::WingetUpgrades => winget_upgrades(out),
        ProbeId::WuPending => wu_pending(out),
    }
}

fn names(list: &[String]) -> String {
    let shown: Vec<&str> = list.iter().take(NAMES_SHOWN).map(String::as_str).collect();
    let more = list.len().saturating_sub(NAMES_SHOWN);
    if more == 0 {
        shown.join(", ")
    } else {
        format!("{} and {more} more", shown.join(", "))
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn disk_finding(probe: ProbeId, place: &str, pct: u64, line: &str) -> Option<Finding> {
    if pct >= DISK_CRIT_PCT {
        Some(Finding::new(
            probe,
            Severity::Critical,
            format!("{place} is almost full ({pct}% used), so updates can't install and apps may fail to save files."),
            line,
        ))
    } else if pct >= DISK_WARN_PCT {
        Some(Finding::new(
            probe,
            Severity::Warning,
            format!("{place} is getting full ({pct}% used). Freeing some space will keep updates working."),
            line,
        ))
    } else {
        None
    }
}

/// `df -P -k`: Filesystem, 1024-blocks, Used, Available, Capacity, Mounted on.
fn disk_usage(out: &ProbeOutput) -> Vec<Finding> {
    const SKIP_FS: &[&str] = &["tmpfs", "devtmpfs", "overlay", "squashfs", "efivarfs", "udev", "none"];
    const SKIP_MOUNT: &[&str] = &["/snap", "/run", "/dev", "/sys", "/proc"];
    let mut found = Vec::new();
    let mut seen = 0;
    for line in out.stdout.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 {
            continue;
        }
        let mount = cols[5..].join(" ");
        if SKIP_FS.contains(&cols[0]) || SKIP_MOUNT.iter().any(|m| mount == *m || mount.starts_with(&format!("{m}/"))) {
            continue;
        }
        let (Ok(used), Ok(avail)) = (cols[2].parse::<u64>(), cols[3].parse::<u64>()) else {
            continue;
        };
        if used + avail == 0 {
            continue;
        }
        seen += 1;
        let pct = cols[4].trim_end_matches('%').parse::<u64>().unwrap_or(used * 100 / (used + avail));
        let place = if mount == "/" { "Your main disk".to_string() } else { format!("The disk at {mount}") };
        found.extend(disk_finding(ProbeId::DiskUsage, &place, pct, line.trim()));
    }
    if found.is_empty() && seen > 0 {
        found.push(Finding::new(ProbeId::DiskUsage, Severity::Ok, "Your disks have enough free space.", ""));
    }
    found
}

/// Tab lines: drive letter, label, size bytes, free bytes.
fn volumes(out: &ProbeOutput) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut seen = 0;
    for line in out.stdout.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 4 || cols[0].trim().is_empty() {
            continue;
        }
        let (Ok(size), Ok(free)) = (cols[2].trim().parse::<u64>(), cols[3].trim().parse::<u64>()) else {
            continue;
        };
        if size == 0 {
            continue;
        }
        seen += 1;
        let pct = (size - free.min(size)) * 100 / size;
        let place = format!("Drive {}:", cols[0].trim());
        found.extend(disk_finding(ProbeId::Volumes, &place, pct, line.trim()));
    }
    if found.is_empty() && seen > 0 {
        found.push(Finding::new(ProbeId::Volumes, Severity::Ok, "Your drives have enough free space.", ""));
    }
    found
}

/// `free -b`: the `Mem:` row, total and available (last column).
fn memory(out: &ProbeOutput) -> Vec<Finding> {
    let Some(line) = out.stdout.lines().find(|l| l.starts_with("Mem:")) else {
        return Vec::new();
    };
    let cols: Vec<u64> = line.split_whitespace().skip(1).filter_map(|c| c.parse().ok()).collect();
    let (Some(&total), Some(&avail)) = (cols.first(), cols.last()) else {
        return Vec::new();
    };
    if total == 0 || cols.len() < 6 {
        return Vec::new();
    }
    let pct = avail * 100 / total;
    let finding = if pct < MEM_WARN_PCT {
        Finding::new(
            ProbeId::Memory,
            Severity::Warning,
            format!("Your computer is low on memory (only {pct}% free), which can make it slow. Closing a few apps or browser tabs will help."),
            line.trim(),
        )
    } else {
        Finding::new(ProbeId::Memory, Severity::Ok, format!("Memory is fine ({pct}% free)."), "")
    };
    vec![finding]
}

/// `systemctl --failed --plain --no-legend`: one unit per line, name first.
fn failed_services(out: &ProbeOutput) -> Vec<Finding> {
    // No systemd, or it didn't answer: nothing to say about services.
    if out.code != Some(0) && out.stdout.trim().is_empty() {
        return Vec::new();
    }
    let units: Vec<String> = out
        .stdout
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(|u| u.trim_end_matches(".service").to_string())
        .collect();
    services_finding(
        ProbeId::FailedServices,
        &units,
        "A background service has stopped working: {names}.",
        "{n} background services have stopped working: {names}.",
        out.stdout.trim(),
    )
}

/// Tab lines: service name, display name. Auto-start services that aren't running.
fn stopped_auto(out: &ProbeOutput) -> Vec<Finding> {
    let shown: Vec<String> = out
        .stdout
        .lines()
        .filter_map(|l| {
            let mut cols = l.split('\t');
            let name = cols.next()?.trim();
            let display = cols.next().map(str::trim).filter(|d| !d.is_empty()).unwrap_or(name);
            (!name.is_empty()).then(|| display.to_string())
        })
        .collect();
    services_finding(
        ProbeId::ServicesStoppedAuto,
        &shown,
        "A background service should start on its own but isn't running: {names}.",
        "{n} background services should start on their own but aren't running: {names}.",
        out.stdout.trim(),
    )
}

/// `one` and `many` are templates: `{names}` is the list, `{n}` the count.
fn services_finding(probe: ProbeId, list: &[String], one: &str, many: &str, detail: &str) -> Vec<Finding> {
    if list.is_empty() {
        return vec![Finding::new(probe, Severity::Ok, "All background services are running.", "")];
    }
    let tpl = if list.len() == 1 { one } else { many };
    let plain = tpl.replace("{n}", &list.len().to_string()).replace("{names}", &names(list));
    vec![Finding::new(probe, Severity::Warning, plain, detail)]
}

/// Journal or Event Log error lines (already capped and redacted).
fn log_errors(probe: ProbeId, out: &ProbeOutput) -> Vec<Finding> {
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("-- No entries --") && !l.starts_with("Hint:"))
        .collect();
    let when = if probe == ProbeId::BootErrors { "since you last started the computer" } else { "in the last day" };
    let mut found = Vec::new();
    let combined = format!("{}\n{}", out.stdout, out.stderr).to_ascii_lowercase();
    if combined.contains("insufficient permissions") || combined.contains("not seeing messages from other users") {
        found.push(Finding::new(
            probe,
            Severity::Info,
            "I could only read your own logs. The system's logs need admin rights, so I skipped them.",
            "",
        ));
    }
    if lines.is_empty() {
        if out.code == Some(0) {
            found.push(Finding::new(probe, Severity::Ok, format!("No errors were logged {when}."), ""));
        }
        return found;
    }
    let severity = if lines.len() >= LOG_WARN_LINES { Severity::Warning } else { Severity::Info };
    found.push(Finding::new(
        probe,
        severity,
        format!("{} logged {when}. Most are harmless, but they can point to what broke.", plural(lines.len(), "error was", "errors were")),
        lines.join("\n"),
    ));
    found
}

/// `ip -brief address`: name, state, addresses.
fn network_links(out: &ProbeOutput) -> Vec<Finding> {
    const VIRTUAL: &[&str] = &["lo", "docker", "veth", "br-", "virbr", "vmnet", "vboxnet", "tailscale", "zt"];
    let mut up_with_addr = Vec::new();
    let mut up_no_addr = Vec::new();
    for line in out.stdout.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let Some(name) = cols.first() else { continue };
        let name = name.split('@').next().unwrap_or(name);
        if VIRTUAL.iter().any(|v| name == *v || (v.len() > 2 && name.starts_with(v))) {
            continue;
        }
        let state = cols.get(1).copied().unwrap_or("");
        let addrs = cols.iter().skip(2).filter(|a| !a.starts_with("fe80:")).count();
        if state == "UP" && addrs > 0 {
            up_with_addr.push(name.to_string());
        } else if state == "UP" {
            up_no_addr.push(name.to_string());
        }
    }
    let kind = |n: &str| if n.starts_with("wl") { "Wi-Fi" } else { "a cable" };
    if let Some(name) = up_with_addr.first() {
        return vec![Finding::new(
            ProbeId::NetworkLinks,
            Severity::Ok,
            format!("You're connected to a network over {} ({name}).", kind(name)),
            "",
        )];
    }
    if let Some(name) = up_no_addr.first() {
        return vec![Finding::new(
            ProbeId::NetworkLinks,
            Severity::Warning,
            format!("Your {} is on but didn't get an address from the router. Turning it off and on again, or restarting the router, often fixes this.", kind(name)),
            out.stdout.trim(),
        )];
    }
    vec![Finding::new(
        ProbeId::NetworkLinks,
        Severity::Warning,
        "You're not connected to any network. Wi-Fi may be off, or the cable may be unplugged.",
        out.stdout.trim(),
    )]
}

const DNS_BROKEN: &str = "Your computer can't look up website names (DNS), so websites won't load even when you're connected.";

/// `getent hosts example.com`: an address line on success, nothing and exit 2 on failure.
fn dns(out: &ProbeOutput) -> Vec<Finding> {
    if out.code == Some(0) && !out.stdout.trim().is_empty() {
        vec![Finding::new(ProbeId::Dns, Severity::Ok, "Looking up website names works.", "")]
    } else {
        vec![Finding::new(ProbeId::Dns, Severity::Warning, DNS_BROKEN, out.stdout.trim())]
    }
}

/// Tab line: internet reachable, name lookup worked.
fn net_test(out: &ProbeOutput) -> Vec<Finding> {
    let line = out.stdout.lines().find(|l| l.contains('\t')).unwrap_or("");
    let mut cols = line.split('\t').map(|c| c.trim().eq_ignore_ascii_case("true"));
    let (online, dns_ok) = (cols.next().unwrap_or(false), cols.next().unwrap_or(false));
    let (severity, plain) = match (online, dns_ok) {
        (true, true) => (Severity::Ok, "Your internet connection works."),
        (true, false) => (Severity::Warning, DNS_BROKEN),
        (false, true) => (Severity::Warning, "Your computer can look up website names but can't reach the internet. The router or the provider may be down."),
        (false, false) => (Severity::Warning, "You're not connected to the internet. Wi-Fi may be off, or the cable may be unplugged."),
    };
    vec![Finding::new(ProbeId::NetTest, severity, plain, line.trim())]
}

/// `dpkg --audit`, `dnf check`, `pacman -Qkq`: silence means healthy.
fn package_health(out: &ProbeOutput) -> Vec<Finding> {
    let lines: Vec<&str> = out.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return vec![Finding::new(ProbeId::PackageHealth, Severity::Ok, "Your installed packages look healthy.", "")];
    }
    let plain = if out.program == "pacman" {
        let pkgs: std::collections::BTreeSet<&str> = lines.iter().filter_map(|l| l.split_whitespace().next()).collect();
        format!("{} missing files, which can make apps crash or updates fail.", plural(pkgs.len(), "installed package is", "installed packages are"))
    } else {
        "Some packages are half-installed or broken, which can stop updates from installing.".to_string()
    };
    vec![Finding::new(ProbeId::PackageHealth, Severity::Warning, plain, lines.join("\n"))]
}

/// Pending updates from the package lists already on disk.
fn pending_updates(out: &ProbeOutput) -> Vec<Finding> {
    let lines = out.stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let n = match out.program.as_str() {
        "apt" => lines.filter(|l| l.contains("[upgradable from:")).count(),
        "pacman" => lines.filter(|l| l.contains(" -> ") || l.split_whitespace().count() == 2).count(),
        "zypper" => lines.filter(|l| l.starts_with("v ")).count(),
        // dnf check-update: `name.arch  version  repo` rows.
        _ => lines.filter(|l| l.split_whitespace().count() == 3 && l.contains('.')).count(),
    };
    updates_finding(ProbeId::PendingUpdates, n, "updates are", out.stdout.trim())
}

fn updates_finding(probe: ProbeId, n: usize, many: &str, detail: &str) -> Vec<Finding> {
    if n == 0 {
        return vec![Finding::new(probe, Severity::Ok, "Your apps are up to date, as of the last update check.", "")];
    }
    let one = many.replace("updates are", "update is");
    vec![Finding::new(
        probe,
        Severity::Info,
        format!("{} waiting to install. Installing them often fixes bugs.", plural(n, &one, many)),
        detail,
    )]
}

/// `winget upgrade`: a table, then "N upgrades available."
fn winget_upgrades(out: &ProbeOutput) -> Vec<Finding> {
    let n = out
        .stdout
        .lines()
        .find_map(|l| {
            let l = l.trim();
            let (num, rest) = l.split_once(' ')?;
            (rest.starts_with("upgrade") && rest.contains("available")).then(|| num.parse::<usize>().ok())?
        })
        .unwrap_or(0);
    updates_finding(ProbeId::WingetUpgrades, n, "app updates are", out.stdout.trim())
}

/// Tab line: reboot pending, number of updates waiting.
fn wu_pending(out: &ProbeOutput) -> Vec<Finding> {
    let line = out.stdout.lines().find(|l| l.contains('\t')).unwrap_or("");
    let mut cols = line.split('\t').map(str::trim);
    let reboot = cols.next().is_some_and(|c| c.eq_ignore_ascii_case("true"));
    let count = cols.next().and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
    let mut found = Vec::new();
    if reboot {
        found.push(Finding::new(
            ProbeId::WuPending,
            Severity::Warning,
            "Windows needs a restart to finish installing updates.",
            line.trim(),
        ));
    }
    if count > 0 {
        found.extend(updates_finding(ProbeId::WuPending, count, "Windows updates are", line.trim()));
    } else if !reboot {
        found.push(Finding::new(ProbeId::WuPending, Severity::Ok, "Windows is up to date.", ""));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(program: &str, code: i32, stdout: &str) -> ProbeOutput {
        ProbeOutput {
            program: program.into(),
            code: Some(code),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    fn plains(probe: ProbeId, o: &ProbeOutput) -> Vec<(Severity, String)> {
        interpret(probe, o).into_iter().map(|f| (f.severity, f.plain)).collect()
    }

    const DF: &str = "Filesystem     1024-blocks      Used Available Capacity Mounted on
/dev/nvme0n1p2   488281250 478515625   4882812      99% /
tmpfs              8000000   7900000    100000      99% /run
/dev/nvme0n1p3   976562500 888671875  87890625      92% /home
/dev/sdb1        976562500 100000000 876562500      11% /mnt/backup
";

    #[test]
    fn disk_fixture_flags_critical_and_warning_and_skips_tmpfs() {
        assert_eq!(
            plains(ProbeId::DiskUsage, &out("df", 0, DF)),
            vec![
                (Severity::Critical, "Your main disk is almost full (99% used), so updates can't install and apps may fail to save files.".into()),
                (Severity::Warning, "The disk at /home is getting full (92% used). Freeing some space will keep updates working.".into()),
            ]
        );
        let roomy = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 1000 500 500 50% /\n";
        assert_eq!(plains(ProbeId::DiskUsage, &out("df", 0, roomy)), vec![(Severity::Ok, "Your disks have enough free space.".into())]);
    }

    #[test]
    fn disk_thresholds_sit_at_ninety_and_ninety_seven() {
        let at = |pct: u64| disk_finding(ProbeId::DiskUsage, "Your main disk", pct, "").map(|f| f.severity);
        assert_eq!(at(89), None);
        assert_eq!(at(90), Some(Severity::Warning));
        assert_eq!(at(96), Some(Severity::Warning));
        assert_eq!(at(97), Some(Severity::Critical));
    }

    #[test]
    fn volumes_fixture_flags_a_full_drive() {
        let text = "C\tOS\t255000000000\t5000000000\n\tRecovery\t600000000\t100000000\nD\tData\t1000000000000\t500000000000\n";
        assert_eq!(
            plains(ProbeId::Volumes, &out("powershell", 0, text)),
            vec![(Severity::Critical, "Drive C: is almost full (98% used), so updates can't install and apps may fail to save files.".into())]
        );
    }

    #[test]
    fn memory_fixture_warns_below_ten_percent() {
        let low = "               total        used        free      shared  buff/cache   available
Mem:     16000000000 15000000000   200000000   100000000   800000000   960000000
Swap:     2000000000  1900000000   100000000
";
        assert_eq!(
            plains(ProbeId::Memory, &out("free", 0, low)),
            vec![(Severity::Warning, "Your computer is low on memory (only 6% free), which can make it slow. Closing a few apps or browser tabs will help.".into())]
        );
        let fine = "Mem: 16000000000 4000000000 8000000000 100000000 4000000000 11000000000\n";
        assert_eq!(plains(ProbeId::Memory, &out("free", 0, fine)), vec![(Severity::Ok, "Memory is fine (68% free).".into())]);
    }

    #[test]
    fn failed_units_and_stopped_auto_services_are_warnings() {
        let failed = "nginx.service loaded failed failed A high performance web server\ncups.service loaded failed failed CUPS Scheduler\n";
        assert_eq!(
            plains(ProbeId::FailedServices, &out("systemctl", 0, failed)),
            vec![(Severity::Warning, "2 background services have stopped working: nginx, cups.".into())]
        );
        assert_eq!(
            plains(ProbeId::FailedServices, &out("systemctl", 0, "")),
            vec![(Severity::Ok, "All background services are running.".into())]
        );
        let stopped = "Spooler\tPrint Spooler\n";
        assert_eq!(
            plains(ProbeId::ServicesStoppedAuto, &out("powershell", 0, stopped)),
            vec![(Severity::Warning, "A background service should start on its own but isn't running: Print Spooler.".into())]
        );
    }

    #[test]
    fn a_probe_that_fails_with_no_output_says_nothing() {
        let mut no_systemd = out("systemctl", 1, "");
        no_systemd.stderr = "System has not been booted with systemd as init system (PID 1). Can't operate.\n".into();
        assert_eq!(plains(ProbeId::FailedServices, &no_systemd), vec![]);
        assert_eq!(plains(ProbeId::BootErrors, &out("journalctl", 1, "")), vec![]);
    }

    #[test]
    fn journal_and_event_log_errors_are_counted() {
        let journal = "Oct 07 09:00:01 box kernel: ACPI Error: AE_NOT_FOUND\nOct 07 09:00:02 box bluetoothd[700]: Failed to set mode\n";
        assert_eq!(
            plains(ProbeId::BootErrors, &out("journalctl", 0, journal)),
            vec![(Severity::Info, "2 errors were logged since you last started the computer. Most are harmless, but they can point to what broke.".into())]
        );
        let mut perms = out("journalctl", 0, "-- No entries --\n");
        perms.stderr = "Hint: You are currently not seeing messages from other users and the system.\n".into();
        assert_eq!(
            plains(ProbeId::BootErrors, &perms),
            vec![
                (Severity::Info, "I could only read your own logs. The system's logs need admin rights, so I skipped them.".into()),
                (Severity::Ok, "No errors were logged since you last started the computer.".into()),
            ]
        );
        let events: String = (0..12).map(|i| format!("2026-10-07T0{}:00:00\tDisk\tThe device has a bad block.\n", i % 10)).collect();
        assert_eq!(
            plains(ProbeId::EventErrors24h, &out("powershell", 0, &events)),
            vec![(Severity::Warning, "12 errors were logged in the last day. Most are harmless, but they can point to what broke.".into())]
        );
    }

    #[test]
    fn network_fixtures() {
        let wifi = "lo               UNKNOWN        127.0.0.1/8 ::1/128\nwlp2s0           UP             192.168.1.23/24 fe80::1/64\ndocker0          DOWN           172.17.0.1/16\n";
        assert_eq!(
            plains(ProbeId::NetworkLinks, &out("ip", 0, wifi)),
            vec![(Severity::Ok, "You're connected to a network over Wi-Fi (wlp2s0).".into())]
        );
        let no_addr = "lo UNKNOWN 127.0.0.1/8\nwlp2s0 UP fe80::1/64\n";
        assert_eq!(
            plains(ProbeId::NetworkLinks, &out("ip", 0, no_addr)),
            vec![(Severity::Warning, "Your Wi-Fi is on but didn't get an address from the router. Turning it off and on again, or restarting the router, often fixes this.".into())]
        );
        let down = "lo UNKNOWN 127.0.0.1/8\nenp3s0 DOWN\nwlp2s0 DOWN\n";
        assert_eq!(
            plains(ProbeId::NetworkLinks, &out("ip", 0, down)),
            vec![(Severity::Warning, "You're not connected to any network. Wi-Fi may be off, or the cable may be unplugged.".into())]
        );
        assert_eq!(plains(ProbeId::Dns, &out("getent", 2, "")), vec![(Severity::Warning, DNS_BROKEN.into())]);
        assert_eq!(
            plains(ProbeId::Dns, &out("getent", 0, "93.184.215.14   example.com\n")),
            vec![(Severity::Ok, "Looking up website names works.".into())]
        );
        assert_eq!(
            plains(ProbeId::NetTest, &out("powershell", 0, "False\tTrue\n")),
            vec![(Severity::Warning, "Your computer can look up website names but can't reach the internet. The router or the provider may be down.".into())]
        );
        assert_eq!(plains(ProbeId::NetTest, &out("powershell", 0, "True\tTrue\n")), vec![(Severity::Ok, "Your internet connection works.".into())]);
    }

    #[test]
    fn package_fixtures() {
        assert_eq!(
            plains(ProbeId::PackageHealth, &out("dpkg", 0, "")),
            vec![(Severity::Ok, "Your installed packages look healthy.".into())]
        );
        let audit = "The following packages are only half configured:\n libc-bin   GNU C Library\n";
        assert_eq!(
            plains(ProbeId::PackageHealth, &out("dpkg", 0, audit)),
            vec![(Severity::Warning, "Some packages are half-installed or broken, which can stop updates from installing.".into())]
        );
        let qk = "glibc /usr/lib/libm.so.6\nglibc /usr/lib/libc.so.6\nfirefox /usr/lib/firefox/libxul.so\n";
        assert_eq!(
            plains(ProbeId::PackageHealth, &out("pacman", 1, qk)),
            vec![(Severity::Warning, "2 installed packages are missing files, which can make apps crash or updates fail.".into())]
        );
        let apt = "Listing...\nfirefox/jammy-updates 131.0 amd64 [upgradable from: 130.0]\nlibc6/jammy-updates 2.35-0ubuntu3.9 amd64 [upgradable from: 2.35-0ubuntu3.8]\n";
        assert_eq!(
            plains(ProbeId::PendingUpdates, &out("apt", 0, apt)),
            vec![(Severity::Info, "2 updates are waiting to install. Installing them often fixes bugs.".into())]
        );
        assert_eq!(
            plains(ProbeId::PendingUpdates, &out("pacman", 0, "linux 6.11.1-1 -> 6.11.2-1\n")),
            vec![(Severity::Info, "1 update is waiting to install. Installing them often fixes bugs.".into())]
        );
        let dnf = "\nfirefox.x86_64   131.0-1.fc40   updates\nkernel.x86_64   6.11.2-300.fc40   updates\n";
        assert_eq!(
            plains(ProbeId::PendingUpdates, &out("dnf", 100, dnf)),
            vec![(Severity::Info, "2 updates are waiting to install. Installing them often fixes bugs.".into())]
        );
        assert_eq!(
            plains(ProbeId::PendingUpdates, &out("apt", 0, "Listing...\n")),
            vec![(Severity::Ok, "Your apps are up to date, as of the last update check.".into())]
        );
    }

    #[test]
    fn windows_update_fixtures() {
        let winget = "Name      Id               Version  Available Source\n------------------------------------------------\nFirefox   Mozilla.Firefox  130.0    131.0     winget\nGit       Git.Git          2.45     2.46      winget\n2 upgrades available.\n";
        assert_eq!(
            plains(ProbeId::WingetUpgrades, &out("winget", 0, winget)),
            vec![(Severity::Info, "2 app updates are waiting to install. Installing them often fixes bugs.".into())]
        );
        assert_eq!(
            plains(ProbeId::WingetUpgrades, &out("winget", 0, "No installed package found matching input criteria.\n")),
            vec![(Severity::Ok, "Your apps are up to date, as of the last update check.".into())]
        );
        assert_eq!(
            plains(ProbeId::WuPending, &out("powershell", 0, "True\t3\n")),
            vec![
                (Severity::Warning, "Windows needs a restart to finish installing updates.".into()),
                (Severity::Info, "3 Windows updates are waiting to install. Installing them often fixes bugs.".into()),
            ]
        );
        assert_eq!(plains(ProbeId::WuPending, &out("powershell", 0, "False\t0\n")), vec![(Severity::Ok, "Windows is up to date.".into())]);
    }
}
