//! Router R3a in Settings: the "On-device model for background tasks" toggle
//! (`localModel`). The tick hands the flag to `route::local::set_enabled`, so
//! a flip takes effect without a restart. The route goes live only with a
//! runtime installed, and then only for background classes; chat never goes
//! local. Turning it on (or launching with it on) downloads the pinned
//! llama.cpp engine when no `llama-server` is found; **Get engine** does the
//! same by hand. Settings shows the runtime's state. Models come from the
//! model picker.

use std::sync::{mpsc, Arc};
use std::time::Duration;

use grokhub_agent::route::{local, local_sidecar};
use grokhub_core::model_download::{self as md, Control, Progress};
use grokhub_core::runtime_download::{
    self as rd, Fetch as RuntimeFetch, Manifest, Outcome as RuntimeOutcome,
};
use grokhub_core::ssrf::runtime_download_url_ok;

use super::*;

const MIB: u64 = 1_024 * 1_024;
const REDIRECTS_MAX: u32 = 8;

pub(super) enum RuntimeDlEvent {
    Progress(Progress),
    Finished(RuntimeOutcome),
}

/// A pinned engine download in flight.
pub(super) struct RuntimeDl {
    manifest: Manifest,
    ctl: Arc<Control>,
    rx: mpsc::Receiver<RuntimeDlEvent>,
    pub(super) done: u64,
    pub(super) total: Option<u64>,
    pub(super) verifying: bool,
}

/// The toggle's hint, by state. It leads with On or Off (SB-07).
pub(super) fn local_model_hint(
    on: bool,
    installed: bool,
    engine: bool,
    downloaded: Option<&str>,
) -> String {
    match (on, installed, engine, downloaded) {
        (false, _, _, _) => "Off. Background work uses your cloud model.".into(),
        (true, true, _, _) => {
            "On. Dream, digests and summaries run on this device. Chat still uses your cloud model."
                .into()
        }
        (true, false, false, Some(name)) => format!(
            "On. {name} is downloaded, but the local engine is not installed yet. Use Get engine, then finish Set up."
        ),
        (true, false, false, None) => {
            "On, but the local engine is not installed. Use Get engine, then Set up a model.".into()
        }
        (true, false, true, Some(name)) => format!(
            "On. The engine is ready; finish Set up to run {name} on this device."
        ),
        (true, false, true, None) => {
            "On. The engine is ready; Set up a local model to run background work on this device.".into()
        }
    }
}

/// The quiet status line after a flip, naming the live tier when the route is live.
pub(super) fn local_model_status(on: bool, installed: bool, tier: &str) -> String {
    match (on, installed) {
        (false, _) => "On-device model for background tasks: off".into(),
        (true, true) => format!("On-device model for background tasks: on ({tier})"),
        (true, false) => {
            "On-device model for background tasks: on, no local model installed".into()
        }
    }
}

struct HttpRuntimeFetch {
    url: String,
}

fn runtime_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(120))
        .resolver(grokhub_agent::public_only_resolve)
        .build()
}

fn redirect_target(from: &str, location: &str) -> String {
    if location.starts_with("https://") || location.starts_with("http://") {
        return location.to_string();
    }
    let (scheme, rest) = from.split_once("://").unwrap_or(("https", from));
    let host = rest.split('/').next().unwrap_or("");
    if location.starts_with('/') {
        format!("{scheme}://{host}{location}")
    } else {
        format!("{scheme}://{host}/{location}")
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

impl RuntimeFetch for HttpRuntimeFetch {
    fn open(&self, offset: u64) -> Result<md::Opened, String> {
        let mut url = self.url.clone();
        for hop in 0..=REDIRECTS_MAX {
            runtime_download_url_ok(&url)?;
            crate::xai::egress_ok(&url, &[])?;
            let mut req = runtime_agent().get(&url).set("user-agent", "GrokHub");
            if hop == 0 && offset > 0 {
                req = req.set("range", &format!("bytes={offset}-"));
            }
            let resp = match req.call() {
                Ok(resp) => resp,
                Err(ureq::Error::Status(status, resp)) if is_redirect(status) => {
                    let loc = resp.header("location").ok_or_else(|| {
                        format!("engine download redirect missing Location (HTTP {status})")
                    })?;
                    url = redirect_target(&url, loc);
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            };
            let status = resp.status();
            if is_redirect(status) {
                let loc = resp.header("location").ok_or_else(|| {
                    format!("engine download redirect missing Location (HTTP {status})")
                })?;
                url = redirect_target(&url, loc);
                continue;
            }
            let (start, total) = if resp.status() == 206 {
                (
                    offset,
                    resp.header("content-range")
                        .and_then(md::content_range_total),
                )
            } else {
                (
                    0,
                    resp.header("content-length")
                        .and_then(|v| v.trim().parse().ok()),
                )
            };
            return Ok((Box::new(resp.into_reader()), start, total));
        }
        Err("too many redirects while downloading the engine".into())
    }
}

pub(super) fn runtime_outcome_status(
    manifest: &Manifest,
    out: &RuntimeOutcome,
    err: Option<&str>,
) -> String {
    match out {
        RuntimeOutcome::Done { bytes, server } => format!(
            "Installed llama.cpp {} ({}) at {}",
            manifest.version,
            grokhub_core::local_setup::size_label(bytes / MIB),
            server.display()
        ),
        RuntimeOutcome::Paused { done } => format!(
            "Paused the engine download at {}. Resume picks up where it stopped.",
            md::progress_line(*done, Some(manifest.asset.size))
        ),
        RuntimeOutcome::Cancelled => "Cancelled the engine download.".into(),
        RuntimeOutcome::BadChecksum => {
            "The engine download didn't match its published checksum, so it was deleted. Try Get engine again."
                .into()
        }
        RuntimeOutcome::Failed(why) => {
            if let Some(prev) = err.filter(|s| !s.is_empty()) {
                format!("{why} {prev}")
            } else {
                format!("{why} Resume picks up where it stopped.")
            }
        }
    }
}

/// What Local AI being on does about the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EngineStep {
    Ready,
    Downloading,
    Download,
    /// No pinned build for this OS/arch; the user installs llama-server.
    NoBuild,
}

pub(super) fn engine_step(ready: bool, downloading: bool, has_build: bool) -> EngineStep {
    match (ready, downloading, has_build) {
        (true, _, _) => EngineStep::Ready,
        (false, true, _) => EngineStep::Downloading,
        (false, false, true) => EngineStep::Download,
        (false, false, false) => EngineStep::NoBuild,
    }
}

const NO_BUILD: &str = "Runtime missing: there's no llama.cpp build for this computer. Install llama-server and put it on PATH, or set GROKHUB_LLAMA_SERVER to its path.";

impl Cabin {
    /// Local AI is on: get llama-server when none is found, so offline
    /// answers don't fail silently.
    pub(super) fn ensure_local_engine(&mut self) {
        let manifest = rd::manifest_for_host();
        match engine_step(
            self.engine_ready(),
            self.setup.runtime_dl.is_some(),
            manifest.is_some(),
        ) {
            EngineStep::Download => {
                let Some(m) = manifest else { return };
                let version = m.version.clone();
                self.start_runtime_download(m);
                self.status = format!(
                    "Local AI is on, so GrokHub is downloading its engine (llama.cpp {version}) to {}.",
                    rd::bin_dir(&self.config_dir()).display()
                );
            }
            EngineStep::NoBuild => self.setup.runtime_dl_error = Some(NO_BUILD.into()),
            EngineStep::Ready | EngineStep::Downloading => {}
        }
    }

    fn config_dir(&self) -> std::path::PathBuf {
        crate::config::config_dir()
    }

    fn engine_ready(&self) -> bool {
        rd::engine_binary_available(&self.config_dir())
    }

    /// Save the toggle and hand it to the router now. A live route change
    /// refreshes the model list so the router picks the tier up (or drops it)
    /// with no prompt.
    pub(super) fn set_local_model(&mut self, on: bool) {
        if self.cfg.local_model == on {
            return;
        }
        self.cfg.local_model = on;
        if on {
            self.setup.engine_checked = false;
        }
        self.persist_cfg();
        local::set_enabled(on);
        let installed = local::installed();
        if installed {
            self.refresh_models_now();
        }
        self.status = local_model_status(on, installed, local::live_tier());
    }

    /// The Settings → Cabin defaults rows: engine download, toggle, Set up, Test speed.
    pub(super) fn ui_local_model_rows(&mut self, ui: &mut egui::Ui) {
        self.ui_runtime_engine_rows(ui);
        let installed = local::installed();
        let mut on = self.cfg.local_model;
        let downloaded = self.downloaded_model_name();
        let engine = self.engine_ready();
        if crate::cards::settings_toggle(
            ui,
            "On-device model for background tasks",
            &local_model_hint(on, installed, engine, downloaded),
            &mut on,
        ) {
            self.set_local_model(on);
        }
        if !installed
            && crate::cards::settings_action(
                ui,
                "Local model setup",
                &self.local_model_row_hint(),
                "Set up",
            )
        {
            self.open_local_setup();
        }
        let dl_model = self.setup.dl.as_ref().map(|d| d.model);
        if let Some(m) = self.ui_model_choice(ui) {
            self.ui_model_pick_rows(ui, dl_model.unwrap_or(m));
        }
        if installed
            && crate::cards::settings_action(
                ui,
                "Local speed",
                "Tokens/s, first-token latency, load time and a small on-device eval. Same as `grokhub bench local`.",
                "Test speed",
            )
        {
            self.run_local_speed_test();
        }
    }

    fn runtime_engine_row_hint(&self, manifest: &Manifest) -> String {
        let host = manifest.asset.source_host().unwrap_or("github.com");
        format!(
            "llama.cpp {} · {} · from {}",
            manifest.version,
            grokhub_core::local_setup::size_label(manifest.asset.size / MIB),
            host
        )
    }

    fn ui_runtime_engine_rows(&mut self, ui: &mut egui::Ui) {
        let runtime = local_sidecar::runtime(&self.config_dir());
        crate::cards::settings_note(
            ui,
            &format!("Local engine: {}", local_sidecar::runtime_label(&runtime)),
        );
        if runtime != local_sidecar::Runtime::Missing {
            return;
        }
        let manifest = match rd::manifest_for_host() {
            Some(m) => m,
            None => {
                crate::cards::settings_note(ui, NO_BUILD);
                return;
            }
        };
        if let Some(dl) = &self.setup.runtime_dl {
            let line = if dl.verifying {
                format!("Downloading engine {}: checking…", dl.manifest.version)
            } else {
                format!(
                    "Downloading engine {}: {}",
                    dl.manifest.version,
                    md::progress_line(dl.done, dl.total)
                )
            };
            crate::cards::settings_note(ui, &line);
            crate::cards::settings_progress(ui, md::percent(dl.done, dl.total), crate::theme::fg());
            let ctl = dl.ctl.clone();
            let mut pause = false;
            let mut cancel = false;
            ui.horizontal(|ui| {
                if crate::cards::ghost_pill(ui, "Pause") {
                    pause = true;
                }
                if crate::cards::ghost_pill(ui, "Cancel") {
                    cancel = true;
                }
            });
            if pause {
                ctl.pause();
            }
            if cancel {
                self.cancel_runtime_download();
            }
            return;
        }
        let partial = rd::partial_bytes(&rd::bin_dir(&self.config_dir()), &manifest.asset);
        let label = match partial {
            Some(b) => format!(
                "Resume engine {} ({})",
                manifest.version,
                md::progress_line(b, Some(manifest.asset.size))
            ),
            None => format!(
                "Get engine {} ({})",
                manifest.version,
                grokhub_core::local_setup::size_label(manifest.asset.size / MIB)
            ),
        };
        crate::cards::settings_note(ui, &self.runtime_engine_row_hint(&manifest));
        let mut get = false;
        let mut discard = false;
        ui.horizontal(|ui| {
            if crate::cards::white_pill(ui, &label) {
                get = true;
            }
            if partial.is_some() && crate::cards::ghost_pill(ui, "Discard") {
                discard = true;
            }
        });
        if get {
            self.start_runtime_download(manifest);
        } else if discard {
            self.discard_runtime_partial(&manifest);
        }
        if let Some(err) = self.setup.runtime_dl_error.as_deref() {
            crate::cards::settings_note(ui, err);
            if crate::cards::ghost_pill(ui, "Clear error") {
                self.setup.runtime_dl_error = None;
            }
        }
    }

    pub(super) fn start_runtime_download(&mut self, manifest: Manifest) {
        if self.setup.runtime_dl.is_some() {
            return;
        }
        let cfg = self.config_dir();
        let total_bytes = manifest.asset.size;
        let version = manifest.version.clone();
        let ctl = Arc::new(Control::default());
        let (tx, rx) = mpsc::channel();
        let fetch = HttpRuntimeFetch {
            url: manifest.asset.url.clone(),
        };
        let worker = ctl.clone();
        let man = manifest.clone();
        let cfg_worker = cfg.clone();
        std::thread::spawn(move || {
            let out = rd::download_engine(&cfg_worker, &man, &fetch, &worker, &mut |p| {
                let _ = tx.send(RuntimeDlEvent::Progress(p));
            });
            let _ = tx.send(RuntimeDlEvent::Finished(out));
        });
        let done = rd::partial_bytes(&rd::bin_dir(&cfg), &manifest.asset).unwrap_or(0);
        self.status = format!(
            "Downloading llama.cpp {} to {}",
            version,
            rd::bin_dir(&cfg).display()
        );
        self.setup.runtime_dl = Some(RuntimeDl {
            manifest,
            ctl,
            rx,
            done,
            total: Some(total_bytes),
            verifying: false,
        });
    }

    pub(super) fn cancel_runtime_download(&mut self) {
        if let Some(dl) = &self.setup.runtime_dl {
            dl.ctl.cancel();
        }
    }

    fn discard_runtime_partial(&mut self, manifest: &Manifest) {
        let _ = std::fs::remove_file(rd::part_path_for_asset(
            &rd::bin_dir(&self.config_dir()),
            &manifest.asset,
        ));
        self.status = "Discarded the partial engine download.".into();
    }

    pub(super) fn poll_runtime_download(&mut self) {
        let Some(dl) = &mut self.setup.runtime_dl else {
            return;
        };
        let mut finished = None;
        loop {
            match dl.rx.try_recv() {
                Ok(RuntimeDlEvent::Progress(Progress::Bytes { done, total })) => {
                    dl.done = done;
                    if total.is_some() {
                        dl.total = total;
                    }
                    dl.verifying = false;
                }
                Ok(RuntimeDlEvent::Progress(Progress::Verifying)) => dl.verifying = true,
                Ok(RuntimeDlEvent::Finished(out)) => {
                    finished = Some(out);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = Some(RuntimeOutcome::Failed(
                        "The engine download stopped.".into(),
                    ));
                    break;
                }
            }
        }
        let Some(out) = finished else {
            return;
        };
        let manifest = dl.manifest.clone();
        self.setup.runtime_dl = None;
        self.setup.runtime_dl_error = None;
        let status = runtime_outcome_status(&manifest, &out, None);
        match &out {
            RuntimeOutcome::Failed(_) | RuntimeOutcome::BadChecksum => {
                self.setup.runtime_dl_error = Some(status);
            }
            RuntimeOutcome::Done { .. } => {
                self.status = status;
                local_sidecar::stop_global();
            }
            _ => self.status = status,
        }
    }

    /// Set up: the setup wizard's on-device model step.
    pub(super) fn open_local_setup(&mut self) {
        self.open_setup(super::setup_wizard::SetupStep::LocalAi);
    }

    /// Run the on-device bench + eval; the status line gets the first report line.
    pub(super) fn run_local_speed_test(&mut self) {
        if !local::installed() {
            self.status = "Local speed: install a local model first.".into();
            return;
        }
        self.status = match grokhub_agent::route::local_bench::run_for_ui() {
            Ok(report) => report
                .lines()
                .next()
                .unwrap_or("Local speed: done")
                .to_string(),
            Err(e) => format!("Local speed: {e}"),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_and_status_name_each_state() {
        assert_eq!(
            local_model_hint(false, false, false, None),
            "Off. Background work uses your cloud model."
        );
        assert_eq!(
            local_model_hint(false, true, true, Some("Gemma 4 E4B")),
            "Off. Background work uses your cloud model."
        );
        assert_eq!(
            local_model_hint(true, true, true, None),
            "On. Dream, digests and summaries run on this device. Chat still uses your cloud model."
        );
        assert_eq!(
            local_model_hint(true, false, false, Some("Gemma 4 E4B")),
            "On. Gemma 4 E4B is downloaded, but the local engine is not installed yet. Use Get engine, then finish Set up."
        );
        assert_eq!(
            local_model_hint(true, false, true, Some("Gemma 4 E4B")),
            "On. The engine is ready; finish Set up to run Gemma 4 E4B on this device."
        );
        assert_eq!(
            local_model_hint(true, false, true, None),
            "On. The engine is ready; Set up a local model to run background work on this device."
        );
        assert_eq!(
            local_model_status(false, true, "local:small"),
            "On-device model for background tasks: off"
        );
        assert_eq!(
            local_model_status(true, true, "local:tiny"),
            "On-device model for background tasks: on (local:tiny)"
        );
        assert_eq!(
            local_model_status(true, false, "local:small"),
            "On-device model for background tasks: on, no local model installed"
        );
    }

    #[test]
    fn turning_local_ai_on_downloads_a_missing_engine_once() {
        assert_eq!(engine_step(true, false, true), EngineStep::Ready);
        assert_eq!(engine_step(false, true, true), EngineStep::Downloading);
        assert_eq!(engine_step(false, false, true), EngineStep::Download);
        assert_eq!(engine_step(false, false, false), EngineStep::NoBuild);
        let mut app = Cabin::quiet_for_test();
        app.setup.engine_checked = true;
        app.cfg.local_model = false;
        app.set_local_model(true);
        // The next tick checks the engine again.
        assert!(!app.setup.engine_checked);
        local::set_enabled(false);
    }

    #[test]
    fn the_toggle_saves_localmodel_and_routes_nothing_while_no_model_is_installed() {
        let root = std::env::temp_dir().join(format!("grokhub-local-ai-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        assert!(!app.cfg.local_model);
        app.set_local_model(true);
        assert!(app.cfg.local_model);
        assert_eq!(
            app.status,
            "On-device model for background tasks: on, no local model installed"
        );
        assert!(!local::installed() && !local::available());
        assert!(!app.harness.router.force_table);
        let mut saved = String::new();
        for _ in 0..200 {
            saved = std::fs::read_to_string(root.join("app.json")).unwrap_or_default();
            if saved.contains("\"localModel\": true") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(saved.contains("\"localModel\": true"), "{saved}");
        app.status.clear();
        app.set_local_model(true);
        assert_eq!(app.status, "");
        app.set_local_model(false);
        assert!(!app.cfg.local_model);
        assert_eq!(app.status, "On-device model for background tasks: off");
        app.open_local_setup();
        assert_eq!(
            app.setup.step,
            Some(super::super::setup_wizard::SetupStep::LocalAi)
        );
        local::set_enabled(false);
        let _ = std::fs::remove_dir_all(root);
    }
}
