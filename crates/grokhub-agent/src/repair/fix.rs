//! Spike-9 fix plans (pillar P6 step 2): one plan per finding, in plain words,
//! with every step classified by rules up front.
//!
//! A plan's steps come in as drafts (command, files it touches, plain words);
//! [`FixPlan::new`] classifies each one through `harness::decide`
//! (`Step::Repair`), which is the existing hard classifier plus the repair
//! table here. Nothing that drafts a plan can pick a step's class. A plan with
//! an empty what, why, undo or risk is rejected.

use std::path::{Path, PathBuf};

use crate::harness::{self, classify, GateOutcome, HardClass, HardFloor, HardHit};

use super::interpret::{Finding, Severity};
use super::probes::{Os, PackageManager, ProbeId};

/// How a step is treated before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepClass {
    /// Reversible: runs through Grok Build under the pill.
    Soft,
    /// Parks a hard card. Always, Full and unattended can't skip it.
    Hard(HardClass),
    /// Never applied in the app. The card shows what to do by hand.
    HardFloor,
}

impl StepClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Hard(_) => "hard",
            Self::HardFloor => "hard_floor",
        }
    }
}

/// A step as drafted, before the rules classify it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftStep {
    pub command: String,
    /// Config files the step may change. Each is backed up first.
    pub touches: Vec<PathBuf>,
    /// What the step does, in plain words, for the card.
    pub plain: String,
    /// The OS will show its own password prompt (polkit or UAC). The user types it.
    pub elevated: bool,
}

impl DraftStep {
    pub fn new(command: impl Into<String>, plain: impl Into<String>) -> Self {
        Self { command: command.into(), touches: Vec::new(), plain: plain.into(), elevated: false }
    }

    pub fn touching(mut self, paths: &[&Path]) -> Self {
        self.touches = paths.iter().map(|p| p.to_path_buf()).collect();
        self
    }

    pub fn elevated(mut self) -> Self {
        self.elevated = true;
        self
    }
}

/// One classified step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixStep {
    pub class: StepClass,
    pub command: String,
    pub touches: Vec<PathBuf>,
    pub plain: String,
    pub elevated: bool,
}

impl FixStep {
    /// The card line for this step. A hard-floor step is guidance only.
    pub fn card_line(&self) -> String {
        match self.class {
            StepClass::Soft if self.elevated => format!("{} Your computer will ask for your password. Type it yourself; I'll wait.", self.plain),
            StepClass::Soft => self.plain.clone(),
            StepClass::Hard(_) => format!("{} I'll ask you before this step.", self.plain),
            StepClass::HardFloor => format!("{} {FLOOR_GUIDANCE}", self.plain),
        }
    }
}

/// What a hard-floor step's card says instead of an Apply button.
pub const FLOOR_GUIDANCE: &str = "I won't do this step: it can erase a disk or stop your computer from starting, and it can't be undone. If you really need it, get help from someone you trust or follow your system's manual.";

/// A fix for one finding: plain words for the card, then the steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixPlan {
    pub finding: Finding,
    /// What I'll do.
    pub what: String,
    /// Why it helps.
    pub why: String,
    /// How to undo it.
    pub undo: String,
    /// How risky it is.
    pub risk: String,
    pub steps: Vec<FixStep>,
}

/// Why a drafted plan was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// One of what, why, undo or risk is empty.
    Empty(&'static str),
    NoSteps,
}

impl FixPlan {
    /// Classify every step by rules and reject a plan missing any of its words.
    pub fn new(
        finding: Finding,
        what: impl Into<String>,
        why: impl Into<String>,
        undo: impl Into<String>,
        risk: impl Into<String>,
        steps: Vec<DraftStep>,
    ) -> Result<Self, PlanError> {
        let (what, why, undo, risk) = (what.into(), why.into(), undo.into(), risk.into());
        for (name, text) in [("what", &what), ("why", &why), ("undo", &undo), ("risk", &risk)] {
            if text.trim().is_empty() {
                return Err(PlanError::Empty(name));
            }
        }
        if steps.is_empty() {
            return Err(PlanError::NoSteps);
        }
        let steps = steps
            .into_iter()
            .map(|d| FixStep {
                class: classify_step(&d.command, &d.touches),
                command: d.command,
                touches: d.touches,
                plain: d.plain,
                elevated: d.elevated,
            })
            .collect();
        Ok(Self { finding, what, why, undo, risk, steps })
    }

    /// The card's five headings, in order.
    pub fn card_sections(&self) -> [(&'static str, &str); 5] {
        [
            ("What's wrong", self.finding.plain.as_str()),
            ("What I'll do", self.what.as_str()),
            ("Why", self.why.as_str()),
            ("How to undo", self.undo.as_str()),
            ("Risk", self.risk.as_str()),
        ]
    }

    pub fn has_hard(&self) -> bool {
        self.steps.iter().any(|s| matches!(s.class, StepClass::Hard(_)))
    }

    /// Steps the app may run (soft and hard). Hard-floor steps never count.
    pub fn runnable(&self) -> usize {
        self.steps.iter().filter(|s| s.class != StepClass::HardFloor).count()
    }

    /// Every config file the plan may change.
    pub fn touches(&self) -> Vec<PathBuf> {
        let mut all: Vec<PathBuf> = Vec::new();
        for p in self.steps.iter().flat_map(|s| s.touches.iter()) {
            if !all.contains(p) {
                all.push(p.clone());
            }
        }
        all
    }
}

/// Classify one step through the gate (`Step::Repair`).
pub fn classify_step(command: &str, touches: &[PathBuf]) -> StepClass {
    match harness::decide(harness::Step::Repair { command: &gate_text(command, touches) }) {
        GateOutcome::Refuse { .. } => StepClass::HardFloor,
        GateOutcome::Park { hard, .. } => StepClass::Hard(hard.unwrap_or(HardClass::IrreversibleOs)),
        GateOutcome::Allow => StepClass::Soft,
    }
}

/// What `Step::Repair` checks: the command, then every file it touches, so a
/// step that edits `/etc/fstab` is hard even when its command looks harmless.
pub fn gate_text(command: &str, touches: &[PathBuf]) -> String {
    let mut text = command.to_string();
    for p in touches {
        text.push(' ');
        text.push_str(&p.to_string_lossy().replace('\\', "/"));
    }
    text
}

/// `harness::decide` for `Step::Repair`: the shell floor and hard class first,
/// then the repair floor, then the repair table. Floor wins.
pub(crate) fn repair_hit(command: &str) -> HardHit {
    let shell = serde_json::json!({ "command": command }).to_string();
    let base = classify("run_terminal_command", &shell);
    if let HardHit::Floor(_) = base {
        return base;
    }
    let words = words(command);
    if let Some(reason) = repair_floor(&words) {
        return HardHit::Floor(HardFloor { reason: reason.into() });
    }
    if let HardHit::Class(_) = base {
        return base;
    }
    match repair_class(&words, command) {
        Some(class) => HardHit::Class(class),
        None => HardHit::None,
    }
}

/// Lowercase words with quotes, commas and shell separators taken out and
/// `.exe` dropped, so `reg.exe delete` and `'Remove-Item'` read the same.
fn words(command: &str) -> Vec<String> {
    command
        .to_ascii_lowercase()
        .replace(['\'', '"', ',', '(', ')', ';', '|', '&', '`', '{', '}'], " ")
        .split_whitespace()
        .map(|w| w.strip_suffix(".exe").unwrap_or(w).to_string())
        .collect()
}

/// `seq` appears in `words` in a row. A rule word ending in `*` matches by prefix.
fn has_seq(words: &[String], seq: &[&str]) -> bool {
    if seq.is_empty() || words.len() < seq.len() {
        return false;
    }
    words.windows(seq.len()).any(|win| {
        win.iter().zip(seq).all(|(w, r)| match r.strip_suffix('*') {
            Some(prefix) => w.starts_with(prefix),
            None => w == r,
        })
    })
}

/// Never in-app, on top of the shell floor (`mkfs`, `dd` to a disk, `rm -rf /`).
const REPAIR_FLOOR: &[(&[&str], &str)] = &[
    (&["format-volume"], "hard floor: format a drive"),
    (&["clear-disk"], "hard floor: wipe a disk"),
    (&["initialize-disk"], "hard floor: wipe a disk"),
    (&["wipefs"], "hard floor: wipe a disk"),
    (&["blkdiscard"], "hard floor: wipe a disk"),
    (&["shred", "/dev/*"], "hard floor: wipe a disk"),
    (&["diskpart"], "hard floor: disk partitioning"),
];

fn repair_floor(words: &[String]) -> Option<&'static str> {
    if let Some((_, why)) = REPAIR_FLOOR.iter().find(|(seq, _)| has_seq(words, seq)) {
        return Some(why);
    }
    // `format C:` (cmd's format, not PowerShell's Format-Table and friends).
    let drive = words.windows(2).any(|w| w[0] == "format" && w[1].len() == 2 && w[1].ends_with(':'));
    drive.then_some("hard floor: format a drive")
}

/// Package removal and cleaning that deletes files.
const REMOVE_PACKAGES: &[&[&str]] = &[
    &["apt", "remove"], &["apt", "purge"], &["apt", "autoremove"], &["apt-get", "remove"], &["apt-get", "purge"],
    &["apt-get", "autoremove"], &["dpkg", "-r"], &["dpkg", "--remove"], &["dpkg", "-p"], &["dpkg", "--purge"],
    &["dnf", "remove"], &["dnf", "erase"], &["dnf", "autoremove"], &["yum", "remove"], &["yum", "erase"],
    &["rpm", "-e"], &["zypper", "remove"], &["zypper", "rm"], &["pacman", "-r*"], &["pacman", "--remove"],
    &["snap", "remove"], &["flatpak", "uninstall"], &["flatpak", "remove"], &["winget", "uninstall"],
    &["uninstall-package"], &["choco", "uninstall"], &["remove-appxpackage"],
];
const CLEAN_FILES: &[&[&str]] = &[
    &["paccache", "-r*"], &["pacman", "-sc*"], &["apt", "clean"], &["apt-get", "clean"], &["dnf", "clean"],
    &["yum", "clean"], &["zypper", "clean"], &["journalctl", "--vacuum*"], &["cleanmgr"], &["clear-recyclebin"],
    &["remove-item"], &["del"], &["erase"], &["rd"],
];
/// Registry deletes.
const REGISTRY_DELETES: &[&[&str]] = &[&["reg", "delete"], &["remove-itemproperty"], &["remove-itemproperty*"]];
/// Driver uninstall.
const DRIVERS: &[&[&str]] = &[
    &["rmmod"], &["modprobe", "-r"], &["modprobe", "--remove"], &["dkms", "remove"], &["pnputil", "/delete-driver"],
    &["pnputil", "/remove-device"], &["remove-windowsdriver"], &["sc", "delete"],
];
/// Partitions, the bootloader and early boot.
const BOOT: &[&[&str]] = &[
    &["parted"], &["fdisk"], &["gdisk"], &["sgdisk"], &["sfdisk"], &["cfdisk"], &["grub-install"], &["grub-mkconfig"],
    &["grub2-mkconfig"], &["grub2-install"], &["update-grub"], &["bootctl"], &["efibootmgr"], &["mkinitcpio"],
    &["dracut"], &["update-initramfs"], &["kernelstub"], &["bcdedit"], &["bcdboot"], &["bootrec"], &["new-partition"],
    &["remove-partition"], &["resize-partition"], &["set-partition"], &["restart-computer"], &["stop-computer"],
];
/// Files that decide whether the computer starts.
const BOOT_FILES: &[&str] = &["/etc/fstab", "/etc/crypttab", "/etc/default/grub", "/boot/", "/efi/", "/etc/kernel/"];
/// Services the computer can't start or log in without.
const BOOT_CRITICAL: &[&str] = &[
    "systemd-logind", "systemd-journald", "systemd-udevd", "dbus", "dbus-broker", "polkit", "display-manager", "gdm",
    "gdm3", "sddm", "lightdm", "getty@*", "systemd-fsck*", "rpcss", "dcomlaunch", "lsm", "eventlog", "plugplay",
    "power", "winmgmt", "samss", "profsvc", "brokerinfrastructure",
];
/// A password piped into the OS: the agent never types one.
const CREDENTIAL_PIPES: &[&[&str]] = &[&["sudo", "-s"], &["sudo", "--stdin"], &["chpasswd"], &["--password*"]];

fn repair_class(words: &[String], raw: &str) -> Option<HardClass> {
    let any = |rules: &[&[&str]]| rules.iter().any(|seq| has_seq(words, seq));
    if any(CREDENTIAL_PIPES) {
        return Some(HardClass::Credentials);
    }
    if any(REMOVE_PACKAGES) || any(CLEAN_FILES) || any(REGISTRY_DELETES) {
        return Some(HardClass::Delete);
    }
    let path = raw.to_ascii_lowercase().replace('\\', "/");
    if any(DRIVERS) || any(BOOT) || BOOT_FILES.iter().any(|f| path.contains(f)) || disables_boot_critical(words) {
        return Some(HardClass::IrreversibleOs);
    }
    None
}

/// `systemctl disable|mask|stop <critical>`, `Set-Service <critical> -StartupType Disabled`,
/// `Stop-Service <critical>`, `sc config <critical> start= disabled`.
fn disables_boot_critical(words: &[String]) -> bool {
    let critical = |w: &str| {
        let unit = w.trim_end_matches(".service");
        BOOT_CRITICAL.iter().any(|c| match c.strip_suffix('*') {
            Some(prefix) => unit.starts_with(prefix),
            None => unit == *c,
        })
    };
    let names_critical = words.iter().any(|w| critical(w));
    if !names_critical {
        return false;
    }
    has_seq(words, &["systemctl", "disable"])
        || has_seq(words, &["systemctl", "mask"])
        || has_seq(words, &["systemctl", "stop"])
        || has_seq(words, &["stop-service"])
        || (has_seq(words, &["set-service"]) && words.iter().any(|w| w == "disabled"))
        || (has_seq(words, &["sc", "config"]) && words.iter().any(|w| w == "disabled"))
}

/// Run `command` with the OS's own admin prompt in front of it: polkit's
/// `pkexec` on Linux, a UAC `RunAs` on Windows. The user types the password.
pub fn elevate(os: Os, command: &str) -> String {
    match os {
        Os::Linux => format!("pkexec {command}"),
        Os::Windows => format!("Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile -Command {command}'"),
    }
}

/// A name from probe output that is safe to put on a command line.
fn safe_name(raw: &str) -> Option<String> {
    let ok = !raw.is_empty()
        && raw.len() <= 80
        && !raw.starts_with('-')
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | ':' | '-' | '+'));
    ok.then(|| raw.to_string())
}

/// First words of the finding's detail lines (unit, service or package names).
fn detail_names(finding: &Finding, cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in finding.detail.lines() {
        let Some(name) = line.split(['\t', ' ']).map(str::trim).find(|w| !w.is_empty()).and_then(safe_name) else {
            continue;
        };
        if !out.contains(&name) {
            out.push(name);
        }
        if out.len() == cap {
            break;
        }
    }
    out
}

/// Plans for the findings that need fixing (warning or worse), worst first.
/// Findings with no safe fix get none.
pub fn plans_for(findings: &[Finding], os: Os, has_bin: &dyn Fn(&str) -> bool) -> Vec<FixPlan> {
    let mut plans: Vec<FixPlan> = Vec::new();
    for f in findings.iter().filter(|f| f.severity >= Severity::Warning) {
        // One network fix covers both "not connected" and "can't look up names".
        let network = |p: ProbeId| matches!(p, ProbeId::NetworkLinks | ProbeId::Dns | ProbeId::NetTest);
        if network(f.probe) && plans.iter().any(|p| network(p.finding.probe)) {
            continue;
        }
        if let Some(plan) = plan_for(f, os, has_bin) {
            plans.push(plan);
        }
    }
    plans
}

const NO_FILES_UNDO: &str = "Nothing on disk changes, so there is nothing to put back. If something seems off afterwards, restarting your computer puts it back the way it was.";

fn plan_for(f: &Finding, os: Os, has_bin: &dyn Fn(&str) -> bool) -> Option<FixPlan> {
    let plan = |what: &str, why: &str, undo: &str, risk: &str, steps: Vec<DraftStep>| {
        FixPlan::new(f.clone(), what, why, undo, risk, steps).ok()
    };
    match f.probe {
        ProbeId::NetworkLinks | ProbeId::Dns | ProbeId::NetTest => {
            let dns_only = f.plain.contains("look up website names") && !f.plain.contains("can look up");
            let step = match os {
                Os::Windows if dns_only => DraftStep::new("ipconfig /flushdns", "Clear the list of website addresses Windows remembers."),
                Os::Windows => DraftStep::new("ipconfig /release; ipconfig /renew", "Ask the router for a fresh network address."),
                Os::Linux if dns_only && has_bin("resolvectl") => DraftStep::new(
                    "resolvectl flush-caches; systemctl restart systemd-resolved",
                    "Clear the list of website addresses and restart the part that looks them up.",
                ),
                Os::Linux if has_bin("nmcli") => {
                    DraftStep::new("nmcli networking off; nmcli networking on", "Turn networking off and on again.")
                }
                Os::Linux => DraftStep::new("systemctl restart systemd-networkd", "Restart the part that connects you to the network."),
            };
            let what = if dns_only {
                "I'll clear the list of website addresses your computer remembers, so it looks them up fresh."
            } else {
                "I'll turn your network connection off and on again."
            };
            plan(
                what,
                "A stuck connection or a stale address list stops websites loading. Starting fresh often brings it back.",
                NO_FILES_UNDO,
                "Low. You may be offline for a few seconds.",
                vec![step],
            )
        }
        ProbeId::FailedServices | ProbeId::ServicesStoppedAuto => {
            let names = detail_names(f, 3);
            if names.is_empty() {
                return None;
            }
            let steps = names
                .iter()
                .map(|n| match os {
                    Os::Linux => DraftStep::new(
                        format!("systemctl restart {n}"),
                        format!("Restart the background service {}.", n.trim_end_matches(".service")),
                    ),
                    Os::Windows => {
                        DraftStep::new(elevate(os, &format!("Start-Service -Name {n}")), format!("Start the background service {n}.")).elevated()
                    }
                })
                .collect();
            let shown: Vec<&str> = names.iter().map(|n| n.trim_end_matches(".service")).collect();
            plan(
                &format!("I'll restart the background service that stopped: {}.", shown.join(", ")),
                "A service that crashed often works again after a restart.",
                NO_FILES_UNDO,
                "Low. If it keeps crashing, it will stop again and I'll tell you.",
                steps,
            )
        }
        ProbeId::DiskUsage | ProbeId::Volumes => {
            let step = match os {
                Os::Windows => DraftStep::new(
                    "Remove-Item -Path $env:TEMP\\* -Recurse -Force -ErrorAction SilentlyContinue",
                    "Delete the temporary files apps left behind.",
                ),
                Os::Linux => {
                    let cmd = match PackageManager::detect(has_bin)? {
                        PackageManager::Pacman if has_bin("paccache") => "paccache -rk1",
                        PackageManager::Pacman => "pacman -Sc --noconfirm",
                        PackageManager::Apt => "apt-get clean",
                        PackageManager::Dnf => "dnf clean packages",
                        PackageManager::Zypper => "zypper clean",
                    };
                    DraftStep::new(elevate(os, cmd), "Delete old copies of app installers kept after updates.").elevated()
                }
            };
            plan(
                "I'll delete files your computer doesn't need to run anything, to free up space.",
                "Your disk is almost full. Freeing space lets updates install and apps save files again.",
                "Deleted files can't be put back. Nothing you use needs them; they come back on their own when needed.",
                "Medium. It deletes files, so I'll ask you first.",
                vec![step],
            )
        }
        ProbeId::PackageHealth => {
            let mgr = PackageManager::detect(has_bin)?;
            let (what, step) = match mgr {
                PackageManager::Apt => (
                    "I'll finish setting up the apps that were left half-installed.".to_string(),
                    DraftStep::new(elevate(os, "dpkg --configure -a"), "Finish setting up half-installed apps.").elevated(),
                ),
                PackageManager::Pacman => {
                    let pkgs = detail_names(f, 10);
                    if pkgs.is_empty() {
                        return None;
                    }
                    (
                        format!("I'll reinstall the apps with missing files: {}.", pkgs.join(", ")),
                        DraftStep::new(elevate(os, &format!("pacman -S --noconfirm {}", pkgs.join(" "))), "Reinstall the apps with missing files.")
                            .elevated(),
                    )
                }
                PackageManager::Dnf | PackageManager::Zypper => return None,
            };
            plan(
                &what,
                "Broken or half-installed apps can crash and stop updates from installing.",
                "If your computer takes system snapshots, Undo tells you how to go back to the one I take first. Otherwise the apps stay reinstalled, which is safe.",
                "Medium. It changes installed apps.",
                vec![step],
            )
        }
        ProbeId::WuPending if f.plain.contains("restart") => plan(
            "I'll restart your computer to finish installing Windows updates.",
            "Windows can't finish the updates until it restarts.",
            "A restart can't be undone, but nothing is lost if you save your work first.",
            "Medium. Save your work first. I'll ask before restarting.",
            vec![DraftStep::new("Restart-Computer", "Restart the computer.")],
        ),
        _ => None,
    }
}
