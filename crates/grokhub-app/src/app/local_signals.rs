//! The live governor's sampler (`local_governor`): one thread reads this
//! machine every 2 s (5 s on battery) while the on-device model is switched on
//! and installed, and hands level changes to `route::local_gov`, which only
//! touches GrokHub's own sidecar. Fast signals (fullscreen, idle, GPU busy)
//! are read every sample; slow ones (power, thermal, memory, game processes,
//! the app lists) every fifth. Anything a reader can't see stays `None`.
//!
//! - Windows: `SHQueryUserNotificationState`, a foreground window covering
//!   its monitor, PDH `GPU Engine` without the sidecar's pid,
//!   `GetLastInputInfo`, `GetSystemPowerStatus`.
//! - macOS: the frontmost app's games category (`lsappinfo`), `ioreg` idle,
//!   `pmset` battery and thermal, the kernel's memory pressure level.
//! - Linux X11: `_NET_WM_STATE_FULLSCREEN` on `_NET_ACTIVE_WINDOW` and the
//!   XScreenSaver idle time. Wayland: Hyprland or Sway fullscreen, else game
//!   processes (gamemode, gamescope, Steam, Proton, Wine), else another app
//!   holding the GPU over 60%; idle from Mutter's IdleMonitor or logind.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use grokhub_agent::route::{local, local_gov, local_sidecar};
use grokhub_core::local_governor::{self as g, Governor, Level, Memory, Rules, Signals};
use grokhub_core::local_profile::{PowerInfo, Thermal};

#[cfg(any(target_os = "linux", windows))]
use super::local_probe;
use super::setup_wizard::ram_mb;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::setup_wizard::run;

/// Slow signals are read every this many samples.
const SLOW_EVERY: u32 = 5;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Start the sampler once per process.
pub(super) fn start(config_dir: PathBuf) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("local-governor".into())
        .spawn(move || {
            // Launch sizing needs the hardware profile, and the setup wizard
            // only reads it on a first run.
            let home = grokhub_core::paths::user_home().unwrap_or_else(|| PathBuf::from("."));
            grokhub_agent::route::local_autoconfig::set_profile(
                super::local_probe::read_profile(&home),
                config_dir.join("local"),
            );
            local_gov::set_log_dir(config_dir.join("local"));
            sample_loop(&config_dir)
        });
}

fn sample_loop(config_dir: &Path) {
    let rules_path = config_dir.join("local").join("governor.json");
    let mut gov = Governor::new();
    let mut reader = Reader::default();
    loop {
        let on = local::switched_on() && local_sidecar::assets_present(config_dir);
        if !on {
            // Switched off: forget any pause so switching on starts normally.
            if gov.level != Level::Normal {
                gov = Governor::new();
                local_gov::apply(Level::Normal, "normal use");
            }
            // A speed test can load the model while the switch is off.
            local_sidecar::tick_idle_unload();
            std::thread::sleep(Duration::from_secs(10));
            continue;
        }
        if reader.tick.is_multiple_of(SLOW_EVERY) {
            reader.rules = std::fs::read_to_string(&rules_path)
                .map(|t| Rules::from_json(&t))
                .unwrap_or_default();
        }
        let s = reader.sample(local_sidecar::global_pid());
        let want = g::target(&s, &reader.rules);
        if let Some(level) = gov.observe(want.map(|w| w.level), now_ms()) {
            local_gov::apply(level, want.map_or("normal use", |w| w.reason));
        }
        local_sidecar::tick_governor();
        // The idle rule runs here too, so an unused model unloads on time
        // even when nothing asks for it again.
        local_sidecar::tick_idle_unload();
        std::thread::sleep(Duration::from_millis(g::interval_ms(s.power.as_ref())));
    }
}

/// One fresh reading for the wind-down reminder (`wind_down_ui`), slow
/// signals included. Blocks on the OS readers, so call it off the UI thread.
pub(super) fn read_machine() -> grokhub_core::wind_down::Machine {
    let s = Reader::default().sample(None);
    let game = s.game;
    // The sampler has no Windows game reader; wind-down fails closed without one.
    #[cfg(windows)]
    let game = game.or_else(windows_game);
    grokhub_core::wind_down::Machine {
        idle_secs: s.idle_secs,
        game,
        fullscreen: s.fullscreen,
        external_gpu_pct: s.external_gpu_pct,
    }
}

/// A Steam game or a known game exe, from a Toolhelp snapshot of process
/// names: no process is opened, so no anti-cheat sees it. `None` when the
/// snapshot fails.
#[cfg(windows)]
fn windows_game() -> Option<bool> {
    grokhub_desktop::game_guard::windows_game()
}

#[derive(Default)]
struct Reader {
    tick: u32,
    rules: Rules,
    /// Slow signals from the last slow sample.
    slow: Slow,
    total_mb: Option<u64>,
    os: Os,
}

#[derive(Default, Clone)]
struct Slow {
    power: Option<PowerInfo>,
    thermal: Thermal,
    avail_mb: Option<u64>,
    os_pressure: Option<bool>,
    game: Option<bool>,
}

impl Reader {
    fn sample(&mut self, own: Option<u32>) -> Signals {
        if self.tick.is_multiple_of(SLOW_EVERY) {
            if self.total_mb.is_none() {
                self.total_mb = ram_mb();
            }
            self.slow = self.os.slow(own);
        }
        self.tick = self.tick.wrapping_add(1);
        let fast = self.os.fast(own);
        let memory = match (self.slow.avail_mb, self.total_mb) {
            (Some(avail), Some(total)) => Some(Memory {
                avail_mb: avail + own.and_then(own_rss_mb).unwrap_or(0),
                total_mb: total,
            }),
            _ => None,
        };
        Signals {
            fullscreen: fast.fullscreen,
            game: self.slow.game.or(fast.game),
            external_gpu_pct: fast.gpu,
            idle_secs: fast.idle,
            power: self.slow.power,
            thermal: self.slow.thermal,
            memory,
            os_pressure: self.slow.os_pressure,
            foreground: fast.foreground,
        }
    }
}

#[derive(Default)]
struct Fast {
    fullscreen: Option<bool>,
    game: Option<bool>,
    gpu: Option<u8>,
    idle: Option<u64>,
    foreground: Option<String>,
}

/// GPU busy by others. Readers that can't split per process report nothing
/// while the sidecar is answering, since its own load would count.
#[cfg(target_os = "linux")]
fn gpu_without_own(total: Option<u8>, own_pct: Option<u8>) -> Option<u8> {
    match own_pct {
        Some(own) => total.map(|t| g::external_gpu(t, own)),
        None if local_sidecar::in_flight() > 0 => None,
        None => total,
    }
}

// ---------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(target_os = "linux")]
fn own_rss_mb(pid: u32) -> Option<u64> {
    g::parse_vmrss_mb(&read(format!("/proc/{pid}/status"))?)
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct Os {
    x11: Option<X11>,
    session: Option<zbus::blocking::Connection>,
    system: Option<zbus::blocking::Connection>,
}

#[cfg(target_os = "linux")]
fn wayland() -> bool {
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    grokhub_core::session_is_wayland(wayland.as_deref(), session.as_deref())
}

#[cfg(target_os = "linux")]
impl Os {
    fn slow(&mut self, _own: Option<u32>) -> Slow {
        let gamemode = run("gamemoded", &["-s"]).map(|s| g::gamemode_active(&s));
        let procs = processes();
        let game = match (gamemode, procs) {
            (None, None) => None,
            (gm, p) => Some(gm == Some(true) || p.is_some_and(|p| g::game_in_processes(&p))),
        };
        Slow {
            power: local_probe::power_now(),
            thermal: local_probe::thermal_now(),
            avail_mb: local_probe::free_ram_now(),
            os_pressure: None,
            game,
        }
    }

    fn fast(&mut self, own: Option<u32>) -> Fast {
        let (fullscreen, foreground) = if wayland() {
            compositor_hint()
        } else {
            self.x11_front()
        };
        Fast {
            fullscreen,
            game: None,
            gpu: linux_gpu(own),
            idle: self.idle(),
            foreground,
        }
    }

    fn x11_front(&mut self) -> (Option<bool>, Option<String>) {
        if std::env::var_os("DISPLAY").is_none() {
            return (None, None);
        }
        if self.x11.is_none() {
            self.x11 = X11::connect();
        }
        let got = self.x11.as_ref().and_then(X11::front);
        if got.is_none() {
            // A dropped connection reconnects on the next sample.
            self.x11 = None;
        }
        got.unwrap_or((None, None))
    }

    fn idle(&mut self) -> Option<u64> {
        if !wayland() {
            if let Some(i) = self.x11.as_ref().and_then(X11::idle_secs) {
                return Some(i);
            }
        }
        if self.session.is_none() {
            self.session = zbus::blocking::Connection::session().ok();
        }
        if let Some(ms) = self.session.as_ref().and_then(mutter_idle_ms) {
            return Some(ms / 1_000);
        }
        if self.system.is_none() {
            self.system = zbus::blocking::Connection::system().ok();
        }
        self.system.as_ref().and_then(logind_idle_secs)
    }
}

/// Hyprland, then Sway. Other Wayland compositors give no hint.
#[cfg(target_os = "linux")]
fn compositor_hint() -> (Option<bool>, Option<String>) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        if let Some(j) = run("hyprctl", &["activewindow", "-j"]) {
            return g::parse_hyprctl_active(&j);
        }
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        if let Some(j) = run("swaymsg", &["-t", "get_tree"]) {
            return g::parse_sway_tree(&j);
        }
    }
    (None, None)
}

/// Every process's name and command line.
#[cfg(target_os = "linux")]
pub(super) fn processes() -> Option<Vec<(String, String)>> {
    grokhub_desktop::game_guard::processes()
}

/// NVIDIA through `nvidia-smi` (own share from `pmon`), else AMD sysfs.
#[cfg(target_os = "linux")]
fn linux_gpu(own: Option<u32>) -> Option<u8> {
    let nv = run(
        "nvidia-smi",
        &[
            "--query-gpu=utilization.gpu",
            "--format=csv,noheader,nounits",
        ],
    )
    .and_then(|s| g::parse_nvidia_util(&s));
    if let Some(total) = nv {
        let own_pct = match own {
            // pmon is only worth running when the total could count as busy.
            Some(pid) if total > g::GPU_BUSY_PCT => {
                run("nvidia-smi", &["pmon", "-c", "1", "-s", "u"])
                    .map(|s| g::parse_nvidia_pmon(&s, pid))
            }
            Some(_) => Some(0),
            None => Some(0),
        };
        return gpu_without_own(Some(total), own_pct);
    }
    let amd = std::fs::read_dir("/sys/class/drm").ok().and_then(|it| {
        it.flatten()
            .filter_map(|e| read(e.path().join("device/gpu_busy_percent")))
            .filter_map(|s| s.trim().parse::<u8>().ok())
            .max()
    });
    gpu_without_own(amd, own.is_none().then_some(0))
}

#[cfg(target_os = "linux")]
fn mutter_idle_ms(conn: &zbus::blocking::Connection) -> Option<u64> {
    conn.call_method(
        Some("org.gnome.Mutter.IdleMonitor"),
        "/org/gnome/Mutter/IdleMonitor/Core",
        Some("org.gnome.Mutter.IdleMonitor"),
        "GetIdletime",
        &(),
    )
    .ok()?
    .body()
    .deserialize::<u64>()
    .ok()
}

/// logind's idle hint, trusted only when it says idle (many desktops never set it).
#[cfg(target_os = "linux")]
fn logind_idle_secs(conn: &zbus::blocking::Connection) -> Option<u64> {
    let get = |prop: &str| {
        conn.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.login1.Session", prop),
        )
        .ok()?
        .body()
        .deserialize::<zbus::zvariant::OwnedValue>()
        .ok()
    };
    let idle = bool::try_from(get("IdleHint")?).ok()?;
    if !idle {
        return None;
    }
    let since_us = u64::try_from(get("IdleSinceHint")?).ok()?;
    Some((now_ms() * 1_000).saturating_sub(since_us) / 1_000_000)
}

#[cfg(target_os = "linux")]
struct X11 {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
    active: u32,
    state: u32,
    fullscreen: u32,
    pid: u32,
}

#[cfg(target_os = "linux")]
impl X11 {
    fn connect() -> Option<Self> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt as _;
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let atom = |name: &[u8]| Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom);
        let active = atom(b"_NET_ACTIVE_WINDOW")?;
        let state = atom(b"_NET_WM_STATE")?;
        let fullscreen = atom(b"_NET_WM_STATE_FULLSCREEN")?;
        let pid = atom(b"_NET_WM_PID")?;
        Some(X11 {
            conn,
            root,
            active,
            state,
            fullscreen,
            pid,
        })
    }

    /// (fullscreen, process name) of the active window.
    fn front(&self) -> Option<(Option<bool>, Option<String>)> {
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
        let win = self
            .conn
            .get_property(false, self.root, self.active, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?
            .value32()?
            .next()
            .filter(|w| *w != 0);
        let Some(win) = win else {
            return Some((Some(false), None));
        };
        let full = self
            .conn
            .get_property(false, win, self.state, AtomEnum::ATOM, 0, 32)
            .ok()?
            .reply()
            .ok()?
            .value32()
            .map(|mut atoms| atoms.any(|a| a == self.fullscreen));
        let name = self
            .conn
            .get_property(false, win, self.pid, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()))
            .and_then(|pid| read(format!("/proc/{pid}/comm")))
            .map(|c| c.trim().to_ascii_lowercase());
        Some((full.or(Some(false)), name))
    }

    fn idle_secs(&self) -> Option<u64> {
        use x11rb::protocol::screensaver::ConnectionExt as _;
        let info = self
            .conn
            .screensaver_query_info(self.root)
            .ok()?
            .reply()
            .ok()?;
        Some(u64::from(info.ms_since_user_input) / 1_000)
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
fn own_rss_mb(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    // SAFETY: the handle is checked and closed here; the counters struct is sized for the call.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut m: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = K32GetProcessMemoryInfo(h, &mut m, cb) != 0;
        CloseHandle(h);
        ok.then_some(m.WorkingSetSize as u64 / (1_024 * 1_024))
    }
}

#[cfg(windows)]
#[derive(Default)]
struct Os {
    pdh: Option<Pdh>,
}

#[cfg(windows)]
impl Os {
    fn slow(&mut self, _own: Option<u32>) -> Slow {
        Slow {
            power: win_power(),
            thermal: Thermal::Unknown,
            avail_mb: local_probe::free_ram_now(),
            os_pressure: None,
            game: None,
        }
    }

    fn fast(&mut self, own: Option<u32>) -> Fast {
        if self.pdh.is_none() {
            self.pdh = Pdh::open();
        }
        let gpu = self.pdh.as_ref().and_then(|p| p.sample(own));
        let (covers, foreground) = win_foreground();
        let quns = win_quns();
        let fullscreen = match (quns, covers) {
            (None, None) => None,
            (q, c) => Some(q == Some(true) || c == Some(true)),
        };
        Fast {
            fullscreen,
            game: None,
            gpu,
            idle: win_idle_secs(),
            foreground,
        }
    }
}

#[cfg(windows)]
fn win_quns() -> Option<bool> {
    use windows_sys::Win32::UI::Shell::SHQueryUserNotificationState;
    let mut state = 0;
    // SAFETY: an out-pointer to a local i32.
    (unsafe { SHQueryUserNotificationState(&mut state) } >= 0).then(|| g::quns_fullscreen(state))
}

/// Whether the foreground window covers its monitor (the desktop itself
/// doesn't count), and its executable's file name. The name comes from a
/// Toolhelp snapshot by pid: the foreground app (often a game) is never
/// opened, so its anti-cheat has nothing to log.
#[cfg(windows)]
fn win_foreground() -> (Option<bool>, Option<String>) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetShellWindow, GetWindowRect, GetWindowThreadProcessId,
    };
    // SAFETY: every out-pointer is a local sized for its call.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return (Some(false), None);
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
        let desktop = hwnd == GetShellWindow() || class == "WorkerW" || class == "Progman";
        let mut rect: RECT = std::mem::zeroed();
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let covers = (GetWindowRect(hwnd, &mut rect) != 0
            && GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) != 0)
            .then(|| {
                let m = info.rcMonitor;
                !desktop
                    && g::covers_monitor(
                        (rect.left, rect.top, rect.right, rect.bottom),
                        (m.left, m.top, m.right, m.bottom),
                    )
            });
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let name = (pid != 0)
            .then(|| grokhub_desktop::game_guard::exe_name_of(pid))
            .flatten();
        (covers, name)
    }
}

#[cfg(windows)]
fn win_idle_secs() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: an out-pointer to a local struct with cbSize set.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    // SAFETY: no arguments.
    let now = unsafe { GetTickCount() };
    Some(u64::from(now.wrapping_sub(info.dwTime)) / 1_000)
}

#[cfg(windows)]
fn win_power() -> Option<PowerInfo> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    // SAFETY: an out-pointer to a zeroed local struct.
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return None;
    }
    Some(g::power_from_status(
        s.ACLineStatus,
        s.BatteryFlag,
        s.BatteryLifePercent,
        s.SystemStatusFlag == 1,
    ))
}

/// A PDH query on `\GPU Engine(*)\Utilization Percentage`.
#[cfg(windows)]
struct Pdh {
    query: isize,
    counter: isize,
}

#[cfg(windows)]
impl Pdh {
    fn open() -> Option<Self> {
        use windows_sys::Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW,
        };
        let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage\0"
            .encode_utf16()
            .collect();
        // SAFETY: out-pointers to locals; the query is closed on failure or in Drop.
        unsafe {
            let mut query = 0isize;
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut counter = 0isize;
            if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                PdhCloseQuery(query);
                return None;
            }
            // A rate counter needs a first collection to compare against.
            PdhCollectQueryData(query);
            Some(Pdh { query, counter })
        }
    }

    fn sample(&self, own: Option<u32>) -> Option<u8> {
        use windows_sys::Win32::System::Performance::{
            PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W,
            PDH_FMT_DOUBLE, PDH_MORE_DATA,
        };
        // SAFETY: the first call sizes the buffer, the second fills it; every
        // item's name points into that buffer, which outlives the loop.
        // No data or a failed read is `None`, never an idle GPU.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            let sized = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                std::ptr::null_mut(),
            );
            if sized != PDH_MORE_DATA || size == 0 {
                return None;
            }
            let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
                Vec::with_capacity((size as usize).div_ceil(item));
            if PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                buf.as_mut_ptr(),
            ) != 0
            {
                return None;
            }
            buf.set_len(count as usize);
            let rows: Vec<(String, f64)> = buf
                .iter()
                .map(|it| {
                    let mut n = 0;
                    while *it.szName.add(n) != 0 {
                        n += 1;
                    }
                    let name = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, n));
                    (name, it.FmtValue.Anonymous.doubleValue)
                })
                .collect();
            Some(g::gpu_from_pdh(&rows, own))
        }
    }
}

#[cfg(windows)]
impl Drop for Pdh {
    fn drop(&mut self) {
        // SAFETY: the query came from PdhOpenQueryW and is closed once.
        unsafe {
            windows_sys::Win32::System::Performance::PdhCloseQuery(self.query);
        }
    }
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
fn own_rss_mb(pid: u32) -> Option<u64> {
    let kb: u64 = run("ps", &["-o", "rss=", "-p", &pid.to_string()])?
        .trim()
        .parse()
        .ok()?;
    Some(kb / 1_024)
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct Os;

#[cfg(target_os = "macos")]
impl Os {
    fn slow(&mut self, _own: Option<u32>) -> Slow {
        let power = run("pmset", &["-g", "batt"])
            .map(|b| g::parse_pmset_batt(&b, &run("pmset", &["-g"]).unwrap_or_default()));
        Slow {
            power,
            thermal: run("pmset", &["-g", "therm"])
                .map(|t| g::parse_pmset_therm(&t))
                .unwrap_or_default(),
            avail_mb: None,
            os_pressure: run("sysctl", &["-n", "kern.memorystatus_vm_pressure_level"])
                .and_then(|s| g::parse_mac_pressure(&s)),
            game: None,
        }
    }

    fn fast(&mut self, _own: Option<u32>) -> Fast {
        let bundle = run("lsappinfo", &["front"])
            .map(|asn| asn.trim().to_string())
            .filter(|asn| !asn.is_empty())
            .and_then(|asn| run("lsappinfo", &["info", "-only", "bundlepath", &asn]))
            .and_then(|s| g::parse_lsappinfo_bundle(&s));
        let game = bundle.as_ref().map(|b| {
            run(
                "defaults",
                &[
                    "read",
                    &format!("{b}/Contents/Info"),
                    "LSApplicationCategoryType",
                ],
            )
            .is_some_and(|c| g::is_game_category(&c))
        });
        let foreground = bundle.as_ref().and_then(|b| {
            Path::new(b)
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
        });
        Fast {
            fullscreen: None,
            game,
            gpu: None,
            idle: run("ioreg", &["-c", "IOHIDSystem", "-d", "4"])
                .and_then(|s| g::parse_ioreg_idle_secs(&s)),
            foreground,
        }
    }
}

// ---------------------------------------------------------------- other

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn own_rss_mb(_pid: u32) -> Option<u64> {
    None
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
#[derive(Default)]
struct Os;

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
impl Os {
    fn slow(&mut self, _own: Option<u32>) -> Slow {
        Slow::default()
    }

    fn fast(&mut self, _own: Option<u32>) -> Fast {
        Fast::default()
    }
}

/// The foreground app's process name, read once for focus help. `None`
/// where no reader sees it (macOS, and Wayland compositors other than
/// Hyprland and Sway). Never a window title.
pub(super) fn foreground_process() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if wayland() {
            return compositor_hint().1;
        }
        X11::connect()?.front()?.1
    }
    #[cfg(windows)]
    {
        win_foreground().1
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_samples_without_panicking() {
        let mut r = Reader::default();
        for _ in 0..SLOW_EVERY + 1 {
            let s = r.sample(None);
            if let Some(m) = s.memory {
                assert!(m.total_mb > 0 && m.avail_mb <= m.total_mb, "{s:?}");
            }
        }
        assert_eq!(r.tick, SLOW_EVERY + 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn gpu_load_without_a_per_process_split_waits_out_our_own_requests() {
        assert_eq!(gpu_without_own(Some(90), Some(70)), Some(20));
        assert_eq!(gpu_without_own(Some(90), None), Some(90));
        let busy = local_sidecar::busy();
        assert_eq!(gpu_without_own(Some(90), None), None);
        assert_eq!(gpu_without_own(Some(90), Some(0)), Some(90));
        drop(busy);
    }
}
