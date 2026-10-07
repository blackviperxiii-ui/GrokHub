//! On battery? The indexers pause when they are. Unknown reads as "not on
//! battery" (a desktop with no power-supply info keeps indexing).

use std::path::Path;

/// Where the scheduler asks about power. Tests use a fake.
pub trait PowerSource: Send + Sync {
    fn on_battery(&self) -> bool;
}

/// The real machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsPower;

impl PowerSource for OsPower {
    fn on_battery(&self) -> bool {
        on_battery()
    }
}

/// True when the machine runs from its battery right now.
/// Linux: `/sys/class/power_supply`; Windows: `GetSystemPowerStatus`.
pub fn on_battery() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut st: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        // SAFETY: `st` is a valid out pointer for the call.
        if unsafe { GetSystemPowerStatus(&mut st) } == 0 {
            return false;
        }
        // 0 = offline (on battery), 1 = online, 255 = unknown.
        st.ACLineStatus == 0
    }
    #[cfg(not(windows))]
    {
        on_battery_linux(Path::new("/sys/class/power_supply"))
    }
}

fn read_trim(path: &Path) -> String {
    std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Linux rule over a `power_supply` folder: any mains or USB supply online
/// means not on battery; else a battery that is discharging means on battery.
pub fn on_battery_linux(root: &Path) -> bool {
    let Ok(read) = std::fs::read_dir(root) else {
        return false;
    };
    let mut discharging = false;
    for entry in read.flatten() {
        let dir = entry.path();
        match read_trim(&dir.join("type")).as_str() {
            "Mains" | "USB" if read_trim(&dir.join("online")) == "1" => return false,
            "Battery" if read_trim(&dir.join("status")) == "Discharging" => discharging = true,
            _ => {}
        }
    }
    discharging
}

/// Drop the calling thread to low priority (the indexers' worker).
pub fn lower_thread_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: on Linux, PRIO_PROCESS with who = 0 sets the calling thread's nice value.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
    #[cfg(windows)]
    // SAFETY: GetCurrentThread is a pseudo handle valid for this call.
    unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_LOWEST};
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(root: &Path, name: &str, kind: &str, file: &str, value: &str) {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("type"), format!("{kind}\n")).unwrap();
        std::fs::write(d.join(file), format!("{value}\n")).unwrap();
    }

    #[test]
    fn linux_power_supply_rules() {
        let root = crate::harness::test_dir("power");
        assert!(!on_battery_linux(&root.join("missing")), "unknown means not on battery");
        assert!(!on_battery_linux(&root), "no supplies: a desktop");
        supply(&root, "BAT0", "Battery", "status", "Discharging");
        assert!(on_battery_linux(&root));
        supply(&root, "AC", "Mains", "online", "0");
        assert!(on_battery_linux(&root), "mains unplugged");
        supply(&root, "AC", "Mains", "online", "1");
        assert!(!on_battery_linux(&root), "mains plugged in wins");
        let _ = std::fs::remove_dir_all(&root);
    }
}
