//! The first-run setup wizard: welcome, app setup (close to tray, dream
//! time), and the local AI step, which reads this machine's GPU, RAM and free
//! disk and suggests the on-device model that fits. It opens once on a fresh
//! profile; finishing or skipping sets `setupDone`. Settings → Behavior →
//! Setup and Cabin defaults → Local model → Set up open it again. The local
//! AI step downloads the suggested model (`model_download_ui.rs`).

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use grokhub_agent::route::local::{self, Device};
use grokhub_core::local_setup::{self as ls, Hardware};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SetupStep {
    Welcome,
    App,
    LocalAi,
}

#[derive(Default)]
pub(super) struct SetupWizard {
    /// The open step; `None` while closed.
    pub(super) step: Option<SetupStep>,
    /// The first-run check ran this launch.
    checked: bool,
    pub(super) hw: Option<Hardware>,
    hw_rx: Option<mpsc::Receiver<Hardware>>,
    /// The model download in flight.
    pub(super) dl: Option<super::model_download_ui::ModelDl>,
    /// The model that finished downloading and passed its check.
    pub(super) installed: Option<grokhub_core::model_download::Installed>,
    /// The catalog id picked in the model dropdown this session.
    pub(super) choice: Option<String>,
    /// The llama-server engine download in flight.
    pub(super) runtime_dl: Option<super::local_ai_ui::RuntimeDl>,
    /// Last engine download failure (cleared by the user).
    pub(super) runtime_dl_error: Option<String>,
    /// Checked this launch (or since Local AI was turned on) that llama-server is here.
    pub(super) engine_checked: bool,
}

/// Open on its own only on a fresh profile that hasn't finished or skipped it.
pub(super) fn should_open_setup(setup_done: bool, has_history: bool) -> bool {
    !setup_done && !has_history
}

/// Where on-device models go: `~/GrokHub/models`.
pub(super) fn models_dir() -> PathBuf {
    grokhub_core::paths::user_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(grokhub_core::brand::INTERNAL_ID)
        .join("models")
}

/// The router's downshift ladder from what the wizard found.
pub(super) fn device_for(hw: &Hardware) -> Device {
    Device {
        battery_low: false,
        low_end: ls::low_end(hw),
    }
}

pub(super) const SETUP_DONE_STATUS: &str =
    "Setup saved. Settings → Behavior → Setup opens it again.";
pub(super) const SETUP_SKIPPED_STATUS: &str =
    "Setup skipped. Settings → Behavior → Setup opens it again.";

pub(super) fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    let out = c.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// GPU (nvidia-smi, else Linux DRM / Windows DXGI), RAM and free disk under `dir`.
/// Anything it can't read is 0.
pub(super) fn detect_hardware(dir: &Path) -> Hardware {
    let (gpu, vram_mb) = run(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ],
    )
    .and_then(|s| ls::parse_nvidia_smi(&s))
    .or_else(drm_or_dxgi_gpu)
    .unwrap_or_default();
    Hardware {
        gpu,
        vram_mb,
        ram_mb: ram_mb().unwrap_or(0),
        free_disk_mb: free_disk_mb(dir).unwrap_or(0),
    }
}

/// AMD / Intel discrete VRAM when nvidia-smi is missing.
fn drm_or_dxgi_gpu() -> Option<(String, u64)> {
    #[cfg(target_os = "linux")]
    {
        if let Some(v) = read_drm_vram() {
            return Some(v);
        }
    }
    #[cfg(windows)]
    {
        // CIM Win32_VideoController: Name + AdapterRAM (bytes). Same CSV shape as DXGI helper.
        if let Some(s) = run(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM | ConvertTo-Csv -NoTypeInformation",
            ],
        ) {
            if let Some(v) = ls::parse_dxgi_dedicated_mb(&s) {
                return Some(v);
            }
        }
    }
    None
}

/// Read `/sys/class/drm/*/device/mem_info_vram_total` (AMD and other DRM GPUs).
#[cfg(target_os = "linux")]
fn read_drm_vram() -> Option<(String, u64)> {
    let mut best: Option<(String, u64)> = None;
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        // card0, card1, … — skip card0-HDMI-A-1 style connectors.
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let vram_path = e.path().join("device/mem_info_vram_total");
        let Ok(body) = std::fs::read_to_string(&vram_path) else {
            continue;
        };
        let Some(mb) = ls::parse_drm_vram_total_bytes(&body) else {
            continue;
        };
        let gpu = drm_gpu_name(&e.path()).unwrap_or_else(|| "AMD GPU".into());
        best = Some(match best {
            Some((ref g, cur)) if cur >= mb => (g.clone(), cur),
            _ => (gpu, mb),
        });
    }
    best
}

#[cfg(target_os = "linux")]
fn drm_gpu_name(card_dir: &Path) -> Option<String> {
    let uevent = std::fs::read_to_string(card_dir.join("device/uevent")).ok()?;
    for line in uevent.lines() {
        if let Some(driver) = line.strip_prefix("DRIVER=") {
            let d = driver.trim();
            if d == "amdgpu" {
                return Some("AMD GPU".into());
            }
            if !d.is_empty() {
                return Some(d.into());
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub(super) fn ram_mb() -> Option<u64> {
    ls::parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(target_os = "macos")]
pub(super) fn ram_mb() -> Option<u64> {
    ls::parse_memsize_bytes(&run("sysctl", &["-n", "hw.memsize"])?)
}

#[cfg(windows)]
pub(super) fn ram_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: a zeroed MEMORYSTATUSEX with dwLength set is what the call expects.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then_some(m.ullTotalPhys / (1_024 * 1_024))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) fn ram_mb() -> Option<u64> {
    None
}

#[cfg(unix)]
pub(super) fn free_disk_mb(dir: &Path) -> Option<u64> {
    ls::parse_df_avail_mb(&run("df", &["-Pk", &dir.to_string_lossy()])?)
}

#[cfg(windows)]
pub(super) fn free_disk_mb(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: a NUL-terminated path and one out pointer; the other two may be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free / (1_024 * 1_024))
}

#[cfg(not(any(unix, windows)))]
pub(super) fn free_disk_mb(_dir: &Path) -> Option<u64> {
    None
}

impl Cabin {
    /// The Dream time picker (Settings → Behavior and the setup wizard).
    pub(super) fn ui_dream_time_row(&mut self, ui: &mut egui::Ui) {
        let hours: Vec<String> = (0..24).map(grokhub_core::hour_label).collect();
        let dream_line = grokhub_core::dream_time_line(self.dream_hour(), &Self::local_zone());
        if let Some(h) = crate::cards::settings_dropdown(
            ui,
            "Dream time",
            &dream_line,
            &grokhub_core::hour_label(self.dream_hour()),
            &hours,
        ) {
            self.cfg.dream_hour = h as u32;
            self.persist_cfg();
            self.status = format!(
                "Dream time: {}",
                grokhub_core::hour_label(self.cfg.dream_hour)
            );
        }
    }

    /// Once a launch: open on a fresh profile. Every frame: take the hardware read.
    pub(super) fn tick_setup_wizard(&mut self) {
        if !self.setup.checked {
            self.setup.checked = true;
            self.setup.installed = grokhub_core::model_download::installed(&models_dir());
            if should_open_setup(self.cfg.setup_done, self.has_real_history()) {
                self.open_setup(SetupStep::Welcome);
            }
        }
        self.poll_model_download();
        self.poll_runtime_download();
        if self.cfg.local_model && !self.setup.engine_checked {
            self.setup.engine_checked = true;
            self.ensure_local_engine();
        }
        let Some(rx) = self.setup.hw_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(hw) => self.take_setup_hardware(hw),
            Err(mpsc::TryRecvError::Empty) => self.setup.hw_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    /// Open at `step`, reading the hardware off the UI thread the first time.
    pub(super) fn open_setup(&mut self, step: SetupStep) {
        self.setup.checked = true;
        self.setup.step = Some(step);
        if self.setup.hw.is_none() && self.setup.hw_rx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.setup.hw_rx = Some(rx);
            std::thread::spawn(move || {
                let home = grokhub_core::paths::user_home().unwrap_or_else(|| PathBuf::from("."));
                let _ = tx.send(detect_hardware(&home));
                // The full profile sizes each local-model launch (GPU layers, context, threads).
                grokhub_agent::route::local_autoconfig::set_profile(
                    super::local_probe::read_profile(&home),
                    crate::config::config_dir().join("local"),
                );
            });
        }
    }

    /// The hardware read lands: keep it for the step, set the router's tier,
    /// and apply local-runtime perf defaults (context, GPU layers, idle unload).
    pub(super) fn take_setup_hardware(&mut self, hw: Hardware) {
        local::set_device(device_for(&hw));
        grokhub_agent::route::local_perf::apply_hardware(&hw);
        self.setup.hw = Some(hw);
    }

    /// Done or skipped: close, and never open on its own again.
    pub(super) fn finish_setup(&mut self, skipped: bool) {
        self.setup.step = None;
        if !self.cfg.setup_done {
            self.cfg.setup_done = true;
            self.persist_cfg();
        }
        self.status = if skipped {
            SETUP_SKIPPED_STATUS
        } else {
            SETUP_DONE_STATUS
        }
        .into();
    }

    pub(super) fn paint_setup_wizard(&mut self, ctx: &egui::Context) {
        let Some(step) = self.setup.step else {
            return;
        };
        let mut next: Option<Option<SetupStep>> = None;
        let mut finish: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("setup-wizard"))
            .backdrop_color(egui::Color32::from_black_alpha(153))
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_width(560.0_f32.min(ctx.content_rect().width() - 80.0));
                let welcome = format!("Welcome to {}", grokhub_core::brand::product_name());
                let (title, body) = match step {
                    SetupStep::Welcome => (
                        welcome.as_str(),
                        "A quick setup: two app settings, then an optional on-device model for background work. You can skip any step.",
                    ),
                    SetupStep::App => ("App setup", "Change either of these later in Settings → Behavior."),
                    SetupStep::LocalAi => (
                        "On-device model",
                        "An on-device model can run background work like dream and summaries on this machine. Chat stays on your cloud model.",
                    ),
                };
                ui.label(RichText::new(title).font(crate::theme::title_font(22.0)).color(crate::theme::fg()));
                ui.add_space(4.0);
                ui.label(RichText::new(body).size(crate::theme::FONT_BODY).color(crate::theme::muted()));
                ui.add_space(12.0);
                match step {
                    SetupStep::Welcome => {}
                    SetupStep::App => {
                        let mut close_to_tray = self.cfg.close_to_tray;
                        if crate::cards::settings_toggle(ui, "Close to tray", "The cabin keeps working in the background.", &mut close_to_tray) {
                            self.set_close_to_tray(close_to_tray);
                        }
                        self.ui_dream_time_row(ui);
                    }
                    SetupStep::LocalAi => self.ui_setup_local_ai(ui),
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if step != SetupStep::Welcome && crate::cards::ghost_pill(ui, "Back") {
                        next = Some(Some(if step == SetupStep::LocalAi { SetupStep::App } else { SetupStep::Welcome }));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let primary = if step == SetupStep::LocalAi { "Done" } else { "Next" };
                        if crate::cards::white_pill(ui, primary) {
                            match step {
                                SetupStep::Welcome => next = Some(Some(SetupStep::App)),
                                SetupStep::App => next = Some(Some(SetupStep::LocalAi)),
                                SetupStep::LocalAi => finish = Some(false),
                            }
                        }
                        let skip = if step == SetupStep::Welcome { "Skip setup" } else { "Skip for now" };
                        if crate::cards::ghost_pill(ui, skip) {
                            finish = Some(true);
                        }
                    });
                });
            });
        if let Some(skipped) = finish {
            self.finish_setup(skipped);
        } else if let Some(step) = next {
            self.setup.step = step;
        } else if modal.should_close() {
            self.finish_setup(true);
        }
    }

    fn ui_setup_local_ai(&mut self, ui: &mut egui::Ui) {
        let mut pick = None;
        match self.setup.hw.as_ref() {
            Some(hw) => {
                crate::cards::settings_note(
                    ui,
                    &format!("This machine: {}.", ls::hardware_line(hw)),
                );
                crate::cards::settings_note(ui, &ls::suggestion_line(hw));
                pick = self.ui_model_choice(ui);
            }
            None => crate::cards::settings_note(
                ui,
                "Checking this machine's GPU, memory and free disk…",
            ),
        }
        crate::cards::settings_note(ui, &format!("Models go to {}.", models_dir().display()));
        let dl_model = self.setup.dl.as_ref().map(|d| d.model);
        if let Some(m) = dl_model.or(pick) {
            self.ui_model_pick_rows(ui, m);
        }
        let installed = local::installed();
        let mut on = self.cfg.local_model;
        let downloaded = self.downloaded_model_name();
        let engine =
            grokhub_core::runtime_download::engine_binary_available(&crate::config::config_dir());
        let hint = super::local_ai_ui::local_model_hint(on, installed, engine, downloaded);
        if crate::cards::settings_toggle(ui, "On-device model for background tasks", &hint, &mut on)
        {
            self.set_local_model(on);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wizard_opens_once_on_a_fresh_profile_and_skip_persists() {
        assert!(should_open_setup(false, false));
        assert!(!should_open_setup(true, false));
        assert!(!should_open_setup(false, true));
        let root =
            std::env::temp_dir().join(format!("grokhub-setup-wizard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        app.tick_setup_wizard();
        assert_eq!(app.setup.step, Some(SetupStep::Welcome));
        app.finish_setup(true);
        assert_eq!(app.setup.step, None);
        assert!(app.cfg.setup_done);
        assert_eq!(
            app.status,
            "Setup skipped. Settings → Behavior → Setup opens it again."
        );
        let mut saved = String::new();
        for _ in 0..200 {
            saved = std::fs::read_to_string(root.join("app.json")).unwrap_or_default();
            if saved.contains("\"setupDone\": true") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(saved.contains("\"setupDone\": true"), "{saved}");
        // The next launch reads setupDone and stays closed.
        let mut next = Cabin::quiet_for_test();
        next.cfg.setup_done = true;
        next.tick_setup_wizard();
        assert_eq!(next.setup.step, None);
        // Settings opens it again at the step asked for; Done says saved.
        next.open_setup(SetupStep::LocalAi);
        assert_eq!(next.setup.step, Some(SetupStep::LocalAi));
        next.finish_setup(false);
        assert_eq!(
            (next.setup.step, next.status.as_str()),
            (
                None,
                "Setup saved. Settings → Behavior → Setup opens it again."
            )
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_profile_with_history_never_opens_the_wizard_on_its_own() {
        let mut app = Cabin::quiet_for_test();
        let mut t = crate::threads::ChatThread::new("Trip plan", false);
        t.messages = Arc::new(vec![("user".into(), "hi".into())]);
        app.threads = vec![t];
        app.tick_setup_wizard();
        assert_eq!(app.setup.step, None);
        assert!(!app.cfg.setup_done);
    }

    #[test]
    fn the_hardware_read_sets_the_router_tier_and_feeds_the_suggestion() {
        let mut app = Cabin::quiet_for_test();
        app.take_setup_hardware(Hardware {
            gpu: String::new(),
            vram_mb: 0,
            ram_mb: 4_096,
            free_disk_mb: 50_000,
        });
        assert_eq!(local::live_tier(), "local:tiny");
        // 4 GB of RAM leaves nothing after the OS's share: cloud only.
        assert_eq!(
            ls::suggest_for(app.setup.hw.as_ref().unwrap(), ls::CpuSimd::Avx2).map(|m| m.name),
            None
        );
        app.take_setup_hardware(Hardware {
            gpu: "NVIDIA GeForce RTX 4070".into(),
            vram_mb: 12_282,
            ram_mb: 32_768,
            free_disk_mb: 225_280,
        });
        assert_eq!(local::live_tier(), "local:small");
        assert_eq!(
            device_for(app.setup.hw.as_ref().unwrap()),
            Device {
                battery_low: false,
                low_end: false
            }
        );
        assert!(models_dir().ends_with(Path::new("GrokHub").join("models")));
    }

    #[test]
    fn detect_hardware_reads_this_machine_without_panicking() {
        let hw = detect_hardware(&std::env::temp_dir());
        // CI boxes always report memory and free disk; the GPU may be absent.
        if cfg!(any(target_os = "linux", windows)) {
            assert!(hw.ram_mb > 0 && hw.free_disk_mb > 0, "{hw:?}");
        }
    }
}
