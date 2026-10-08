//! Diagnose probes: ids, not commands. Each id maps to one fixed program and
//! argv per OS, run with no shell. The model can pick an id; it can never
//! pass a command line or a flag.

use std::time::Duration;

/// Every probe gets this long before it is killed.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Output kept per probe (stdout plus stderr); the rest is dropped.
pub const OUTPUT_CAP: usize = 64 * 1024;
/// Journal and Event Log lines kept per probe.
pub const LOG_LINE_CAP: usize = 20;

/// The OS a probe list is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// One read-only check. Linux and Windows have their own ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProbeId {
    DiskUsage,
    Memory,
    FailedServices,
    BootErrors,
    NetworkLinks,
    Dns,
    PackageHealth,
    PendingUpdates,
    Volumes,
    ServicesStoppedAuto,
    EventErrors24h,
    NetTest,
    WingetUpgrades,
    WuPending,
}

pub const LINUX_PROBES: &[ProbeId] = &[
    ProbeId::DiskUsage,
    ProbeId::Memory,
    ProbeId::FailedServices,
    ProbeId::BootErrors,
    ProbeId::NetworkLinks,
    ProbeId::Dns,
    ProbeId::PackageHealth,
    ProbeId::PendingUpdates,
];

pub const WINDOWS_PROBES: &[ProbeId] = &[
    ProbeId::Volumes,
    ProbeId::ServicesStoppedAuto,
    ProbeId::EventErrors24h,
    ProbeId::NetTest,
    ProbeId::WingetUpgrades,
    ProbeId::WuPending,
];

impl ProbeId {
    pub fn all(os: Os) -> &'static [ProbeId] {
        match os {
            Os::Linux => LINUX_PROBES,
            Os::Windows => WINDOWS_PROBES,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::DiskUsage => "disk_usage",
            Self::Memory => "memory",
            Self::FailedServices => "failed_services",
            Self::BootErrors => "boot_errors",
            Self::NetworkLinks => "network_links",
            Self::Dns => "dns",
            Self::PackageHealth => "package_health",
            Self::PendingUpdates => "pending_updates",
            Self::Volumes => "volumes",
            Self::ServicesStoppedAuto => "services_stopped_auto",
            Self::EventErrors24h => "event_errors_24h",
            Self::NetTest => "net_test",
            Self::WingetUpgrades => "winget_upgrades",
            Self::WuPending => "wu_pending",
        }
    }

    /// An id for this OS, or `None`. Anything else the model sends is ignored.
    pub fn parse(key: &str, os: Os) -> Option<Self> {
        let key = key.trim().to_ascii_lowercase();
        Self::all(os).iter().copied().find(|p| p.key() == key)
    }

    /// What the probe looks at, for the user ("checked your disk space").
    pub fn label(self) -> &'static str {
        match self {
            Self::DiskUsage | Self::Volumes => "disk space",
            Self::Memory => "memory",
            Self::FailedServices | Self::ServicesStoppedAuto => "background services",
            Self::BootErrors | Self::EventErrors24h => "recent system errors",
            Self::NetworkLinks => "network connections",
            Self::Dns => "name lookups (DNS)",
            Self::NetTest => "internet connection",
            Self::PackageHealth => "installed packages",
            Self::PendingUpdates | Self::WingetUpgrades => "app updates",
            Self::WuPending => "Windows Update",
        }
    }

    /// Journal and Event Log probes: output may hold user text, so it is
    /// capped at [`LOG_LINE_CAP`] lines.
    pub fn is_log(self) -> bool {
        matches!(self, Self::BootErrors | Self::EventErrors24h)
    }
}

/// The package manager a Linux box uses, found by which binary is on PATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
}

impl PackageManager {
    /// First match wins, in this order: apt, dnf, pacman, zypper.
    pub fn detect(has_bin: &dyn Fn(&str) -> bool) -> Option<Self> {
        if has_bin("apt-get") {
            Some(Self::Apt)
        } else if has_bin("dnf") {
            Some(Self::Dnf)
        } else if has_bin("pacman") {
            Some(Self::Pacman)
        } else if has_bin("zypper") {
            Some(Self::Zypper)
        } else {
            None
        }
    }
}

/// A fixed program and argv. No shell, no user text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSpec {
    pub id: ProbeId,
    pub program: String,
    pub argv: Vec<String>,
    pub timeout: Duration,
    /// Runs only as root or admin. Diagnose never elevates, so it is skipped.
    pub needs_admin: bool,
}

impl ProbeSpec {
    fn new(id: ProbeId, program: &str, argv: &[&str]) -> Self {
        Self {
            id,
            program: program.into(),
            argv: argv.iter().map(|a| a.to_string()).collect(),
            timeout: PROBE_TIMEOUT,
            needs_admin: false,
        }
    }

    fn admin(mut self) -> Self {
        self.needs_admin = true;
        self
    }

    /// Program and argv on one line, for spans and the gate's shell check.
    pub fn command_line(&self) -> String {
        let mut out = self.program.clone();
        for a in &self.argv {
            out.push(' ');
            out.push_str(a);
        }
        out
    }
}

/// PowerShell runs a fixed script string, never a profile or a prompt (D3).
fn powershell(id: ProbeId, script: &str) -> ProbeSpec {
    ProbeSpec::new(id, "powershell", &["-NoProfile", "-NonInteractive", "-Command", script])
}

/// The spec for `id` on `os`, looking up package managers on PATH.
pub fn probe_spec(id: ProbeId, os: Os) -> Option<ProbeSpec> {
    probe_spec_with(id, os, &on_path)
}

/// The spec for `id` on `os`. `has_bin` answers whether a program is on PATH.
/// `None` when the id isn't for this OS or no known package manager is there.
pub fn probe_spec_with(id: ProbeId, os: Os, has_bin: &dyn Fn(&str) -> bool) -> Option<ProbeSpec> {
    if !ProbeId::all(os).contains(&id) {
        return None;
    }
    let spec = match id {
        ProbeId::DiskUsage => ProbeSpec::new(id, "df", &["-P", "-k"]),
        ProbeId::Memory => ProbeSpec::new(id, "free", &["-b"]),
        ProbeId::FailedServices => {
            ProbeSpec::new(id, "systemctl", &["--failed", "--no-legend", "--no-pager", "--plain"])
        }
        ProbeId::BootErrors => {
            ProbeSpec::new(id, "journalctl", &["-p", "err", "-b", "--no-pager", "-q", "-n", "20", "-o", "short"])
        }
        ProbeId::NetworkLinks => ProbeSpec::new(id, "ip", &["-brief", "address"]),
        ProbeId::Dns => ProbeSpec::new(id, "getent", &["hosts", "example.com"]),
        ProbeId::PackageHealth => match PackageManager::detect(has_bin)? {
            // `apt-get check` takes the dpkg lock, so as a user it fails;
            // `dpkg --audit` is the read-only check for half-installed packages.
            PackageManager::Apt => ProbeSpec::new(id, "dpkg", &["--audit"]),
            PackageManager::Dnf => ProbeSpec::new(id, "dnf", &["-q", "-C", "check"]),
            PackageManager::Pacman => ProbeSpec::new(id, "pacman", &["-Qkq"]),
            PackageManager::Zypper => ProbeSpec::new(id, "zypper", &["--non-interactive", "--no-refresh", "verify", "--dry-run"]).admin(),
        },
        // Read the package lists already on disk; never refresh them.
        ProbeId::PendingUpdates => match PackageManager::detect(has_bin)? {
            PackageManager::Apt => ProbeSpec::new(id, "apt", &["list", "--upgradable"]),
            PackageManager::Dnf => ProbeSpec::new(id, "dnf", &["-q", "-C", "check-update"]),
            PackageManager::Pacman => ProbeSpec::new(id, "pacman", &["-Qu"]),
            PackageManager::Zypper => ProbeSpec::new(id, "zypper", &["--non-interactive", "--no-refresh", "list-updates"]),
        },
        ProbeId::Volumes => powershell(
            id,
            "foreach ($v in Get-Volume) { \"{0}`t{1}`t{2}`t{3}\" -f $v.DriveLetter, $v.FileSystemLabel, $v.Size, $v.SizeRemaining }",
        ),
        ProbeId::ServicesStoppedAuto => powershell(
            id,
            "foreach ($s in Get-Service) { if ($s.StartType -eq 'Automatic' -and $s.Status -ne 'Running') { \"{0}`t{1}\" -f $s.Name, $s.DisplayName } }",
        ),
        ProbeId::EventErrors24h => powershell(
            id,
            "foreach ($e in Get-EventLog -LogName System -EntryType Error -After (Get-Date).AddHours(-24) -Newest 20 -ErrorAction SilentlyContinue) { \"{0}`t{1}`t{2}\" -f $e.TimeGenerated.ToString('s'), $e.Source, ($e.Message -replace '\\s+', ' ') }",
        ),
        ProbeId::NetTest => powershell(
            id,
            "\"{0}`t{1}\" -f (Test-NetConnection -InformationLevel Quiet -WarningAction SilentlyContinue), [bool](Resolve-DnsName example.com -ErrorAction SilentlyContinue)",
        ),
        ProbeId::WingetUpgrades => ProbeSpec::new(id, "winget", &["upgrade", "--disable-interactivity"]),
        ProbeId::WuPending => powershell(
            id,
            "\"{0}`t{1}\" -f (Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired'), (New-Object -ComObject Microsoft.Update.Session).CreateUpdateSearcher().Search('IsInstalled=0 and IsHidden=0').Updates.Count",
        ),
    };
    Some(spec)
}

/// Is `name` an executable on PATH? No shell: walks PATH with `std::path`.
pub fn on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: &[&str] = if cfg!(windows) { &["exe", "cmd", "bat"] } else { &[""] };
    std::env::split_paths(&paths).any(|dir| {
        exts.iter().any(|ext| {
            let p = if ext.is_empty() { dir.join(name) } else { dir.join(name).with_extension(ext) };
            p.is_file()
        })
    })
}

/// Programs that run other programs or raise privileges. A probe is never one.
const WRAPPERS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "fish", "ksh", "csh", "tcsh", "busybox", "cmd", "env", "xargs", "nohup",
    "sudo", "doas", "pkexec", "su", "runas", "gsudo", "python", "python3", "perl", "ruby", "node",
];

/// Argv words that change the system, the package database, or a service.
const WRITE_WORDS: &[&str] = &[
    "rm", "rmdir", "mv", "cp", "dd", "mkfs", "shred", "truncate", "chmod", "chown", "kill", "pkill", "killall",
    "install", "reinstall", "remove", "purge", "erase", "autoremove", "dist-upgrade", "full-upgrade", "update",
    "refresh", "clean", "makecache", "distro-sync", "sync", "-S", "-Sy", "-Syu", "-Syy", "-R", "-Rs", "-Rns",
    "-U", "-D", "--sync", "--remove", "--upgrade", "--refresh", "-y", "--yes", "--assume-yes", "--fix",
    "--fix-broken", "-f", "--force", "--all", "-r", "--recurse", "--install", "--uninstall",
    "start", "stop", "restart", "reload", "enable", "disable", "mask", "unmask", "isolate",
    "reboot", "poweroff", "halt", "shutdown", "suspend", "hibernate", "set", "edit", "vacuum",
    "--vacuum-size", "--vacuum-time", "--rotate", "--flush", "--setup-keys", "flush",
];

/// Text that chains, redirects, or substitutes commands, anywhere in an arg.
const SHELL_MARKS: &[&str] = &[">", "<", "|", ";", "&&", "$(", "\n", "\r"];

/// PowerShell verbs and calls that write, run code, or download. Matched
/// case-insensitively anywhere in the script string.
const PS_WRITES: &[&str] = &[
    "set-", "remove-", "stop-", "restart-", "start-", "clear-", "install-", "uninstall-", "add-", "enable-",
    "disable-", "suspend-", "resume-", "rename-", "move-", "copy-", "update-", "repair-", "reset-", "register-",
    "unregister-", "invoke-", "iex", "new-item", "new-service", "new-itemproperty", "new-psdrive",
    "write-eventlog", "out-file", "tee-object", "export-", "import-", "mount-", "dismount-", "initialize-",
    "optimize-", "format-volume", "restore-", "checkpoint-", "createupdateinstaller", "createupdatedownloader",
    ".install(", ".download(", ".delete(", ".kill(", "-encodedcommand", "-file", "-executionpolicy",
];

/// `Some(reason)` when a spec could write or wrap a shell; `None` when read-only.
pub fn read_only_violation(spec: &ProbeSpec) -> Option<String> {
    let program = std::path::Path::new(&spec.program)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if WRAPPERS.contains(&program.as_str()) {
        return Some(format!("{program} is a shell or wrapper"));
    }
    if WRITE_WORDS.contains(&program.as_str()) {
        return Some(format!("{program} writes"));
    }
    if program == "powershell" || program == "pwsh" {
        return powershell_violation(&spec.argv);
    }
    for arg in &spec.argv {
        if let Some(mark) = SHELL_MARKS.iter().find(|m| arg.contains(**m)) {
            return Some(format!("{mark:?} chains or redirects"));
        }
        if WRITE_WORDS.contains(&arg.as_str()) {
            return Some(format!("{arg} writes"));
        }
    }
    if program == "winget" {
        let allowed = ["upgrade", "--disable-interactivity", "list", "--include-unknown"];
        if let Some(arg) = spec.argv.iter().find(|a| !allowed.contains(&a.as_str())) {
            return Some(format!("winget {arg} is not a list"));
        }
    }
    None
}

fn powershell_violation(argv: &[String]) -> Option<String> {
    let [no_profile, non_interactive, command, script] = argv else {
        return Some("powershell takes -NoProfile -NonInteractive -Command <script> only".into());
    };
    if no_profile != "-NoProfile" || non_interactive != "-NonInteractive" || command != "-Command" {
        return Some("powershell takes -NoProfile -NonInteractive -Command <script> only".into());
    }
    if let Some(mark) = SHELL_MARKS.iter().find(|m| script.contains(**m)) {
        return Some(format!("{mark:?} chains or redirects"));
    }
    let lower = script.to_ascii_lowercase();
    if let Some(word) = PS_WRITES.iter().find(|w| lower.contains(**w)) {
        return Some(format!("{word} writes or runs code"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_spec(os: Os) -> Vec<ProbeSpec> {
        let managers: [&dyn Fn(&str) -> bool; 4] = [
            &|b| b == "apt-get",
            &|b| b == "dnf",
            &|b| b == "pacman",
            &|b| b == "zypper",
        ];
        let mut out = Vec::new();
        for id in ProbeId::all(os) {
            for has in managers {
                out.push(probe_spec_with(*id, os, has).expect("spec"));
            }
        }
        out
    }

    #[test]
    fn every_probe_argv_on_both_oses_is_read_only() {
        let linux = every_spec(Os::Linux);
        let windows = every_spec(Os::Windows);
        assert_eq!(linux.len(), 32);
        assert_eq!(windows.len(), 24);
        for spec in linux.iter().chain(windows.iter()) {
            assert_eq!(read_only_violation(spec), None, "{}", spec.command_line());
            assert_eq!(spec.timeout, Duration::from_secs(10));
        }
    }

    #[test]
    fn the_classifier_catches_writes_and_shell_wrappers() {
        let spec = |program: &str, argv: &[&str]| ProbeSpec::new(ProbeId::PackageHealth, program, argv);
        let cases: &[(ProbeSpec, &str)] = &[
            (spec("apt-get", &["remove", "firefox"]), "remove writes"),
            (spec("apt-get", &["install", "vim"]), "install writes"),
            (spec("pacman", &["-Sy"]), "-Sy writes"),
            (spec("rm", &["-rf", "/tmp/x"]), "rm writes"),
            (spec("systemctl", &["restart", "nginx"]), "restart writes"),
            (spec("df", &["-h", ">", "/tmp/out"]), "\">\" chains or redirects"),
            (spec("df", &["-h|tee"]), "\"|\" chains or redirects"),
            (spec("df", &["-h;", "reboot"]), "\";\" chains or redirects"),
            (spec("df", &["&&", "reboot"]), "\"&&\" chains or redirects"),
            (spec("sh", &["-c", "df -h"]), "sh is a shell or wrapper"),
            (spec("/bin/bash", &["-c", "df"]), "bash is a shell or wrapper"),
            (spec("cmd.exe", &["/c", "dir"]), "cmd is a shell or wrapper"),
            (spec("sudo", &["df"]), "sudo is a shell or wrapper"),
            (spec("winget", &["upgrade", "--all"]), "--all writes"),
            (spec("winget", &["upgrade", "Mozilla.Firefox"]), "winget Mozilla.Firefox is not a list"),
            (
                powershell(ProbeId::Volumes, "Stop-Service wuauserv"),
                "stop- writes or runs code",
            ),
            (
                powershell(ProbeId::Volumes, "Set-Service -Name x -StartupType Disabled"),
                "set- writes or runs code",
            ),
            (powershell(ProbeId::Volumes, "Remove-Item C:\\x"), "remove- writes or runs code"),
            (powershell(ProbeId::Volumes, "Restart-Computer"), "restart- writes or runs code"),
            (powershell(ProbeId::Volumes, "Invoke-Expression $x"), "invoke- writes or runs code"),
            (powershell(ProbeId::Volumes, "Get-Volume | Format-List"), "\"|\" chains or redirects"),
            (powershell(ProbeId::Volumes, "Get-Volume; Restart-Computer"), "\";\" chains or redirects"),
            (powershell(ProbeId::Volumes, "Get-Volume > C:\\out.txt"), "\">\" chains or redirects"),
            (
                ProbeSpec::new(ProbeId::Volumes, "powershell", &["-Command", "Get-Volume"]),
                "powershell takes -NoProfile -NonInteractive -Command <script> only",
            ),
        ];
        for (spec, want) in cases {
            let got = read_only_violation(spec).unwrap_or_default();
            assert!(got.contains(want), "{}: got {got:?}, want {want:?}", spec.command_line());
        }
    }

    #[test]
    fn apt_get_remove_is_caught_by_the_classifier() {
        let bad = ProbeSpec::new(ProbeId::PackageHealth, "apt-get", &["remove", "-y", "libreoffice"]);
        assert_eq!(read_only_violation(&bad), Some("remove writes".into()));
    }

    #[test]
    fn package_probes_follow_the_package_manager_on_path() {
        let apt = |b: &str| b == "apt-get";
        let pacman = |b: &str| b == "pacman";
        let zypper = |b: &str| b == "zypper";
        let none = |_: &str| false;
        let line = |id, has: &dyn Fn(&str) -> bool| probe_spec_with(id, Os::Linux, has).map(|s| s.command_line());
        assert_eq!(line(ProbeId::PackageHealth, &apt), Some("dpkg --audit".into()));
        assert_eq!(line(ProbeId::PendingUpdates, &apt), Some("apt list --upgradable".into()));
        assert_eq!(line(ProbeId::PackageHealth, &pacman), Some("pacman -Qkq".into()));
        assert_eq!(line(ProbeId::PendingUpdates, &pacman), Some("pacman -Qu".into()));
        assert_eq!(line(ProbeId::PackageHealth, &none), None);
        let zyp = probe_spec_with(ProbeId::PackageHealth, Os::Linux, &zypper).unwrap();
        assert!(zyp.needs_admin);
    }

    #[test]
    fn ids_parse_only_for_their_own_os() {
        assert_eq!(ProbeId::parse("disk_usage", Os::Linux), Some(ProbeId::DiskUsage));
        assert_eq!(ProbeId::parse(" DNS ", Os::Linux), Some(ProbeId::Dns));
        assert_eq!(ProbeId::parse("disk_usage", Os::Windows), None);
        assert_eq!(ProbeId::parse("volumes", Os::Windows), Some(ProbeId::Volumes));
        assert_eq!(ProbeId::parse("df -h; rm -rf /", Os::Linux), None);
        assert_eq!(probe_spec_with(ProbeId::Volumes, Os::Linux, &|_| true), None);
    }

    #[test]
    fn windows_probes_run_powershell_without_a_profile_or_prompt() {
        let spec = probe_spec_with(ProbeId::Volumes, Os::Windows, &|_| false).unwrap();
        assert_eq!(spec.program, "powershell");
        assert_eq!(&spec.argv[..3], &["-NoProfile", "-NonInteractive", "-Command"]);
        let winget = probe_spec_with(ProbeId::WingetUpgrades, Os::Windows, &|_| false).unwrap();
        assert_eq!(winget.command_line(), "winget upgrade --disable-interactivity");
    }
}
