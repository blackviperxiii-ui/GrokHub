//! Fan fixes. A fan changes only after its settings are saved so Undo can put
//! them back, never while a game runs, and never through a third-party kernel
//! driver (Windows fan tools that load WinRing0 or PawnIO get flagged by
//! anti-cheats). Clock speeds are never touched: `classify_step` refuses them
//! as a hard floor.
//!
//! On Linux the fans are the hwmon `pwmN` channels that have a `pwmN_enable`
//! next to them. Writing them needs admin, so each change is one elevated
//! step the user types the password for. When CoolerControl runs the fans,
//! writing under it would fight it, so the card points to CoolerControl. On
//! Windows the PC maker's own app is the only safe way.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::fix::{elevate, DraftStep, FixPlan};
use super::interpret::{Finding, Severity};
use super::probes::{Os, ProbeId};
use super::DiagnoseReport;

/// Where Linux lists its hardware monitors.
pub const HWMON_ROOT: &str = "/sys/class/hwmon";
/// `pwmN` at full speed (sysfs-interface: 0 to 255).
pub const PWM_FULL: u8 = 255;
/// `pwmN_enable` for manual control.
pub const PWM_MANUAL: u8 = 1;

/// One fan output Linux lets us set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PwmChannel {
    /// The chip's `name` file, like `nct6798` or `amdgpu`.
    pub chip: String,
    pub pwm: PathBuf,
    pub enable: PathBuf,
}

/// A channel's settings when it was saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedFan {
    pub channel: PwmChannel,
    pub enable: u8,
    pub pwm: u8,
}

/// What runs the fans on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FanBackend {
    /// CoolerControl's daemon owns the fans.
    CoolerControl,
    Hwmon(Vec<PwmChannel>),
    /// Windows: only the PC maker's own app.
    Vendor,
    /// No fan output that can be set.
    Unsupported,
}

/// Why a fan fix won't run, in plain words for the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanBlock {
    GameRunning,
    CoolerControl,
    Vendor,
    NoControl,
}

impl FanBlock {
    pub fn plain(self) -> &'static str {
        match self {
            Self::GameRunning => "A game is running, so I won't touch the fans now. Close it and I can do it then.",
            Self::CoolerControl => "CoolerControl runs your fans, so change their profile in CoolerControl. Setting them from here would fight it.",
            Self::Vendor => "On Windows I only change fans through your PC maker's own app, like Alienware Command Center. Other fan tools load drivers that anti-cheats flag, so I won't use them.",
            Self::NoControl => "Your computer doesn't let me set its fan speeds, so there's nothing safe I can change.",
        }
    }
}

/// A path that is safe inside a single-quoted `sh -c` line.
fn plain_path(p: &Path) -> Option<String> {
    let s = p.to_str()?;
    let ok = s.starts_with('/')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'));
    ok.then(|| s.to_string())
}

/// Every `pwmN` under `root` (normally [`HWMON_ROOT`]) that has a
/// `pwmN_enable` beside it, sorted by path.
pub fn pwm_channels(root: &Path) -> Vec<PwmChannel> {
    let Ok(dirs) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = dirs.flatten().map(|e| e.path()).collect();
    dirs.sort();
    let mut out = Vec::new();
    for dir in dirs {
        let chip = fs::read_to_string(dir.join("name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        let mut pwms: Vec<String> = files
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| {
                n.strip_prefix("pwm")
                    .is_some_and(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))
            })
            .collect();
        pwms.sort();
        for name in pwms {
            let (pwm, enable) = (dir.join(&name), dir.join(format!("{name}_enable")));
            if enable.is_file() && plain_path(&pwm).is_some() {
                out.push(PwmChannel {
                    chip: chip.clone(),
                    pwm,
                    enable,
                });
            }
        }
    }
    out
}

pub fn fan_backend(
    os: Os,
    has_bin: &dyn Fn(&str) -> bool,
    channels: Vec<PwmChannel>,
) -> FanBackend {
    match os {
        Os::Windows => FanBackend::Vendor,
        Os::Linux if has_bin("coolercontrold") => FanBackend::CoolerControl,
        Os::Linux if channels.is_empty() => FanBackend::Unsupported,
        Os::Linux => FanBackend::Hwmon(channels),
    }
}

fn read_u8(p: &Path) -> Option<u8> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

/// Each channel's settings now. A channel that can't be read is left out,
/// so it is never changed.
pub fn save_fans(channels: &[PwmChannel]) -> Vec<SavedFan> {
    channels
        .iter()
        .filter_map(|c| {
            Some(SavedFan {
                channel: c.clone(),
                enable: read_u8(&c.enable)?,
                pwm: read_u8(&c.pwm)?,
            })
        })
        .collect()
}

/// One elevated `sh -c` line writing each `(value, file)` in order.
fn write_step(os: Os, writes: &[(u8, &Path)], plain: &str) -> Option<DraftStep> {
    let mut parts = Vec::new();
    for (value, file) in writes {
        parts.push(format!("echo {value} > {}", plain_path(file)?));
    }
    let files: Vec<&Path> = writes.iter().map(|(_, f)| *f).collect();
    Some(
        DraftStep::new(
            elevate(os, &format!("sh -c '{}'", parts.join(" && "))),
            plain,
        )
        .touching(&files)
        .elevated(),
    )
}

const FULL_WHAT: &str = "Run your fans at full speed, so the computer cools down.";
const FULL_WHY: &str =
    "Your computer is running hot, and more air through it brings the heat down.";
const FULL_UNDO: &str =
    "I saved each fan's setting first. Undo fix puts every fan back the way it was.";
const FULL_RISK: &str =
    "Low: the fans get louder. Nothing else changes, and clock speeds are never touched.";

/// Every saved fan at full speed. `saved` comes from [`save_fans`]; the
/// settings go into the fix's restore point because each step touches them.
pub fn full_speed_plan(
    finding: Finding,
    backend: &FanBackend,
    saved: &[SavedFan],
    game_running: bool,
) -> Result<FixPlan, FanBlock> {
    ready(backend, game_running)?;
    let mut writes: Vec<(u8, &Path)> = Vec::new();
    for s in saved {
        writes.push((PWM_MANUAL, &s.channel.enable));
        writes.push((PWM_FULL, &s.channel.pwm));
    }
    if writes.is_empty() {
        return Err(FanBlock::NoControl);
    }
    let plain = match saved.len() {
        1 => "Set your fan to full speed.".to_string(),
        n => format!("Set your {n} fans to full speed."),
    };
    let step = write_step(Os::Linux, &writes, &plain).ok_or(FanBlock::NoControl)?;
    FixPlan::new(
        finding,
        FULL_WHAT,
        FULL_WHY,
        FULL_UNDO,
        FULL_RISK,
        vec![step],
    )
    .map_err(|_| FanBlock::NoControl)
}

/// Every fan back to what [`save_fans`] read. A fan that was on manual gets
/// its speed back after its mode, so it never runs a moment unset.
pub fn restore_fans_plan(
    finding: Finding,
    saved: &[SavedFan],
    game_running: bool,
) -> Result<FixPlan, FanBlock> {
    if game_running {
        return Err(FanBlock::GameRunning);
    }
    let mut writes: Vec<(u8, &Path)> = Vec::new();
    for s in saved {
        writes.push((s.enable, &s.channel.enable));
        if s.enable == PWM_MANUAL {
            writes.push((s.pwm, &s.channel.pwm));
        }
    }
    if writes.is_empty() {
        return Err(FanBlock::NoControl);
    }
    let step = write_step(Os::Linux, &writes, "Put your fans back the way they were.")
        .ok_or(FanBlock::NoControl)?;
    FixPlan::new(
        finding,
        "Put your fans back the way they were before the fix.",
        "You asked to undo the fan change.",
        "This is the undo. Running the fan fix again sets full speed.",
        "Low: the fans go back to the settings I saved.",
        vec![step],
    )
    .map_err(|_| FanBlock::NoControl)
}

fn ready(backend: &FanBackend, game_running: bool) -> Result<(), FanBlock> {
    if game_running {
        return Err(FanBlock::GameRunning);
    }
    match backend {
        FanBackend::Hwmon(_) => Ok(()),
        FanBackend::CoolerControl => Err(FanBlock::CoolerControl),
        FanBackend::Vendor => Err(FanBlock::Vendor),
        FanBackend::Unsupported => Err(FanBlock::NoControl),
    }
}

/// Every saved fan now reads manual at full speed: the re-check after the fix.
pub fn fans_at_full(saved: &[SavedFan]) -> bool {
    !saved.is_empty()
        && saved.iter().all(|s| {
            read_u8(&s.channel.enable) == Some(PWM_MANUAL)
                && read_u8(&s.channel.pwm) == Some(PWM_FULL)
        })
}

/// Every saved fan reads the mode it had, and a manual one its speed too:
/// the re-check after Undo.
pub fn fans_restored(saved: &[SavedFan]) -> bool {
    !saved.is_empty()
        && saved.iter().all(|s| {
            read_u8(&s.channel.enable) == Some(s.enable)
                && (s.enable != PWM_MANUAL || read_u8(&s.channel.pwm) == Some(s.pwm))
        })
}

/// What a fan fix's re-check looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanGoal {
    /// Full speed, after the fix.
    Full,
    /// The saved settings, after Undo.
    Saved,
}

/// The fan re-check as a report the fix run reads. It reads the fan files
/// back, so it needs no probe and no admin.
pub fn fans_check(saved: &[SavedFan], goal: FanGoal) -> DiagnoseReport {
    let ok = match goal {
        FanGoal::Full => fans_at_full(saved),
        FanGoal::Saved => fans_restored(saved),
    };
    let plain = match (goal, ok) {
        (FanGoal::Full, true) => "Every fan reads full speed now.",
        (FanGoal::Full, false) => "The fans don't read full speed yet.",
        (FanGoal::Saved, true) => "Every fan is back on the setting I saved.",
        (FanGoal::Saved, false) => "Some fans aren't back on the setting I saved yet.",
    };
    DiagnoseReport {
        ask: None,
        findings: vec![Finding {
            severity: if ok { Severity::Ok } else { Severity::Warning },
            plain: plain.into(),
            detail: String::new(),
            probe: ProbeId::Fans,
        }],
        ran: vec![ProbeId::Fans],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair::{classify_step, StepClass};

    fn hwmon(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("grokhub-fans-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let chip = root.join("hwmon2");
        fs::create_dir_all(&chip).unwrap();
        fs::write(chip.join("name"), "nct6798\n").unwrap();
        fs::write(chip.join("pwm1"), "90\n").unwrap();
        fs::write(chip.join("pwm1_enable"), "5\n").unwrap();
        fs::write(chip.join("pwm2"), "120\n").unwrap();
        fs::write(chip.join("pwm2_enable"), "1\n").unwrap();
        // No enable file: not a fan we can set.
        fs::write(chip.join("pwm3"), "0\n").unwrap();
        fs::write(chip.join("fan1_input"), "1100\n").unwrap();
        root
    }

    fn finding() -> Finding {
        Finding {
            severity: Severity::Warning,
            plain: "Your CPU reached 96°C while gaming.".into(),
            detail: String::new(),
            probe: ProbeId::Fans,
        }
    }

    // The fan fix writes Linux sysfs paths, so its fake hwmon tree needs
    // Linux paths too. Windows gets only the vendor-app answer, tested below.
    #[cfg(target_os = "linux")]
    #[test]
    fn full_speed_saves_each_fan_and_undo_puts_it_back() {
        let root = hwmon("plan");
        let chip = root.join("hwmon2");
        let channels = pwm_channels(&root);
        assert_eq!(
            channels,
            vec![
                PwmChannel {
                    chip: "nct6798".into(),
                    pwm: chip.join("pwm1"),
                    enable: chip.join("pwm1_enable"),
                },
                PwmChannel {
                    chip: "nct6798".into(),
                    pwm: chip.join("pwm2"),
                    enable: chip.join("pwm2_enable"),
                },
            ]
        );
        let saved = save_fans(&channels);
        assert_eq!(
            saved.iter().map(|s| (s.enable, s.pwm)).collect::<Vec<_>>(),
            vec![(5, 90), (1, 120)]
        );
        let backend = fan_backend(Os::Linux, &|_| false, channels);
        let plan = full_speed_plan(finding(), &backend, &saved, false).unwrap();
        let c = chip.display();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(
            plan.steps[0].command,
            format!("pkexec sh -c 'echo 1 > {c}/pwm1_enable && echo 255 > {c}/pwm1 && echo 1 > {c}/pwm2_enable && echo 255 > {c}/pwm2'")
        );
        assert_eq!(plan.steps[0].plain, "Set your 2 fans to full speed.");
        assert!(plan.steps[0].elevated);
        assert_eq!(plan.steps[0].class, StepClass::Soft);
        // Every file it writes is in the restore point.
        assert_eq!(
            plan.touches(),
            vec![
                chip.join("pwm1_enable"),
                chip.join("pwm1"),
                chip.join("pwm2_enable"),
                chip.join("pwm2"),
            ]
        );
        assert_eq!(plan.undo, FULL_UNDO);

        // The fan that was automatic gets its mode back; the manual one its speed too.
        let undo = restore_fans_plan(finding(), &saved, false).unwrap();
        assert_eq!(
            undo.steps[0].command,
            format!("pkexec sh -c 'echo 5 > {c}/pwm1_enable && echo 1 > {c}/pwm2_enable && echo 120 > {c}/pwm2'")
        );

        // The re-check reads the files back.
        assert!(!fans_at_full(&saved));
        for f in ["pwm1_enable", "pwm2_enable"] {
            fs::write(chip.join(f), "1\n").unwrap();
        }
        for f in ["pwm1", "pwm2"] {
            fs::write(chip.join(f), "255\n").unwrap();
        }
        assert!(fans_at_full(&saved));
        let full = fans_check(&saved, FanGoal::Full);
        assert_eq!(full.ran, vec![ProbeId::Fans]);
        assert_eq!(full.findings[0].severity, Severity::Ok);
        assert_eq!(full.findings[0].plain, "Every fan reads full speed now.");
        let back = fans_check(&saved, FanGoal::Saved);
        assert_eq!(back.findings[0].severity, Severity::Warning);
        assert_eq!(
            back.findings[0].plain,
            "Some fans aren't back on the setting I saved yet."
        );
        // Undo ran: the automatic fan reads 5 again, the manual one 120.
        fs::write(chip.join("pwm1_enable"), "5\n").unwrap();
        fs::write(chip.join("pwm2"), "120\n").unwrap();
        assert!(fans_restored(&saved));
        assert_eq!(
            fans_check(&saved, FanGoal::Saved).findings[0].plain,
            "Every fan is back on the setting I saved."
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fans_stay_put_during_a_game_under_coolercontrol_and_on_windows() {
        let root = hwmon("blocks");
        let channels = pwm_channels(&root);
        let saved = save_fans(&channels);
        let hw = fan_backend(Os::Linux, &|_| false, channels.clone());
        assert_eq!(
            full_speed_plan(finding(), &hw, &saved, true).unwrap_err(),
            FanBlock::GameRunning
        );
        assert_eq!(
            restore_fans_plan(finding(), &saved, true).unwrap_err(),
            FanBlock::GameRunning
        );
        let cc = fan_backend(Os::Linux, &|b| b == "coolercontrold", channels.clone());
        assert_eq!(cc, FanBackend::CoolerControl);
        assert_eq!(
            full_speed_plan(finding(), &cc, &saved, false).unwrap_err().plain(),
            "CoolerControl runs your fans, so change their profile in CoolerControl. Setting them from here would fight it."
        );
        let win = fan_backend(Os::Windows, &|_| true, channels);
        assert_eq!(
            full_speed_plan(finding(), &win, &saved, false).unwrap_err(),
            FanBlock::Vendor
        );
        let none = fan_backend(Os::Linux, &|_| false, Vec::new());
        assert_eq!(
            full_speed_plan(finding(), &none, &[], false).unwrap_err(),
            FanBlock::NoControl
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clock_speeds_and_flagged_fan_drivers_never_run() {
        let floor = [
            "pkexec cpupower frequency-set -u 3.2GHz",
            "nvidia-smi -i 0 -lgc 1500,1800",
            "nvidia-smi --power-limit=200",
            "nvidia-settings -a [gpu:0]/GPUGraphicsClockOffset[3]=150",
            "echo 3200000 | pkexec tee /sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq",
            "echo manual > /sys/class/drm/card0/device/power_dpm_force_performance_level",
            "pkexec ryzenadj --stapm-limit=25000",
            "powercfg /setacvalueindex scheme_current sub_processor PROCTHROTTLEMAX 80",
            "sc.exe create WinRing0_1_2_0 type= kernel binPath= C:\\fan\\WinRing0x64.sys",
            "winget install namazso.PawnIO",
        ];
        for cmd in floor {
            assert_eq!(classify_step(cmd, &[]), StepClass::HardFloor, "{cmd}");
        }
        // Reading the clock to show it is fine; so is a fan file.
        assert_eq!(
            classify_step("nvidia-smi -q -d CLOCK", &[]),
            StepClass::Soft
        );
        let pwm = [PathBuf::from("/sys/class/hwmon/hwmon2/pwm1")];
        assert_eq!(
            classify_step(
                "pkexec sh -c 'echo 255 > /sys/class/hwmon/hwmon2/pwm1'",
                &pwm
            ),
            StepClass::Soft
        );
    }
}
