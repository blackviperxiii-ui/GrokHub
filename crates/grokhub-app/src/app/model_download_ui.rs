//! The setup wizard's model download: Hugging Face over HTTP behind
//! `model_download::Fetch`, run off the UI thread, with progress, Pause,
//! Cancel and Resume. A `.part` left by a pause, a dropped connection or a
//! restart resumes from where it stopped. When the checksum matches, the
//! on-device toggle turns on. This build has no runtime to run the file, so
//! the route stays off and the rows say so.

use std::io::Read;
use std::path::Path;
use std::sync::{mpsc, Arc};

use grokhub_core::local_setup::{self as ls, LocalModel};
use grokhub_core::model_download::{self as md, Control, Fetch, Installed, Outcome, Progress};

use super::setup_wizard::models_dir;
use super::*;

const MIB: u64 = 1_024 * 1_024;

pub(super) enum DlEvent {
    Progress(Progress),
    Finished(Outcome),
}

/// A download in flight.
pub(super) struct ModelDl {
    pub(super) model: &'static LocalModel,
    dir: std::path::PathBuf,
    ctl: Arc<Control>,
    rx: mpsc::Receiver<DlEvent>,
    pub(super) done: u64,
    pub(super) total: Option<u64>,
    pub(super) verifying: bool,
}

/// Hugging Face over HTTPS. A download carries none of your data.
struct HfFetch;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout_read(std::time::Duration::from_secs(60))
        .build()
}

impl Fetch for HfFetch {
    fn open(&self, url: &str, offset: u64) -> Result<md::Opened, String> {
        crate::xai::egress_ok(url, &[])?;
        let mut req = agent().get(url).set("user-agent", "GrokHub");
        if offset > 0 {
            req = req.set("range", &format!("bytes={offset}-"));
        }
        let resp = req.call().map_err(|e| e.to_string())?;
        let (start, total) = if resp.status() == 206 {
            (offset, resp.header("content-range").and_then(md::content_range_total))
        } else {
            (0, resp.header("content-length").and_then(|v| v.trim().parse().ok()))
        };
        Ok((Box::new(resp.into_reader()), start, total))
    }

    fn sha256(&self, repo: &str, file: &str) -> Result<String, String> {
        let url = md::tree_url(repo);
        crate::xai::egress_ok(&url, &[])?;
        let resp = agent().get(&url).set("user-agent", "GrokHub").call().map_err(|e| e.to_string())?;
        let mut body = String::new();
        resp.into_reader().take(4 * MIB).read_to_string(&mut body).map_err(|e| e.to_string())?;
        md::sha256_from_tree(&body, file).ok_or_else(|| format!("Hugging Face lists no checksum for {file}"))
    }
}

/// The status line for how a download ended.
pub(super) fn outcome_status(m: &LocalModel, out: &Outcome, dir: &Path) -> String {
    match out {
        Outcome::Done(rec) => format!("Downloaded {} ({}) to {}", m.name, ls::size_label(rec.bytes / MIB), dir.display()),
        Outcome::Paused { done } => {
            format!("Paused {} at {}. Resume picks up where it stopped.", m.name, md::progress_line(*done, Some(m.download_mb * MIB)))
        }
        Outcome::Cancelled => format!("Cancelled the {} download.", m.name),
        Outcome::BadChecksum => format!("{} didn't match its published checksum, so it was deleted. Try the download again.", m.name),
        Outcome::Failed(why) => format!("{why} Resume picks up where it stopped."),
    }
}

impl Cabin {
    /// The downloaded model's name, when one passed its check.
    pub(super) fn downloaded_model_name(&self) -> Option<&'static str> {
        self.setup.installed.as_ref().and_then(|r| ls::model_by_id(&r.id)).map(|m| m.name)
    }

    /// The Settings "Local model" row: the download in flight, what's on disk, or nothing.
    pub(super) fn local_model_row_hint(&self) -> String {
        if let Some(dl) = &self.setup.dl {
            return format!("Downloading {}: {}.", dl.model.name, md::progress_line(dl.done, dl.total));
        }
        match (self.setup.installed.as_ref(), self.downloaded_model_name()) {
            (Some(rec), Some(name)) => format!("{name} ({}) is in {}.", ls::size_label(rec.bytes / MIB), models_dir().display()),
            _ => "No local model installed.".into(),
        }
    }

    pub(super) fn start_model_download(&mut self, m: &'static LocalModel) {
        self.start_model_download_with(m, models_dir(), Box::new(HfFetch));
    }

    pub(super) fn start_model_download_with(&mut self, m: &'static LocalModel, dir: std::path::PathBuf, fetch: Box<dyn Fetch + Send>) {
        if self.setup.dl.is_some() {
            return;
        }
        let ctl = Arc::new(Control::default());
        let (tx, rx) = mpsc::channel();
        let worker = ctl.clone();
        let into = dir.clone();
        std::thread::spawn(move || {
            let out = md::download(&into, m, fetch.as_ref(), &worker, &mut |p| {
                let _ = tx.send(DlEvent::Progress(p));
            });
            let _ = tx.send(DlEvent::Finished(out));
        });
        let done = md::partial_bytes(&dir, m).unwrap_or(0);
        self.status = format!("Downloading {} to {}", m.name, dir.display());
        self.setup.dl = Some(ModelDl { model: m, dir, ctl, rx, done, total: Some(m.download_mb * MIB), verifying: false });
    }

    pub(super) fn pause_model_download(&self) {
        if let Some(dl) = &self.setup.dl {
            dl.ctl.pause();
        }
    }

    /// Cancel a running download, or discard a paused one's `.part`.
    pub(super) fn cancel_model_download(&mut self, m: &'static LocalModel) {
        if let Some(dl) = &self.setup.dl {
            dl.ctl.cancel();
            return;
        }
        let _ = std::fs::remove_file(md::part_path(&models_dir(), m));
        self.status = format!("Discarded the partial {} download.", m.name);
    }

    /// Take the download's progress; on its end, record it and say how it went.
    pub(super) fn poll_model_download(&mut self) {
        let Some(dl) = self.setup.dl.as_mut() else {
            return;
        };
        let mut finished = None;
        loop {
            match dl.rx.try_recv() {
                Ok(DlEvent::Progress(Progress::Bytes { done, total })) => {
                    dl.done = done;
                    if total.is_some() {
                        dl.total = total;
                    }
                }
                Ok(DlEvent::Progress(Progress::Verifying)) => dl.verifying = true,
                Ok(DlEvent::Finished(out)) => {
                    finished = Some(out);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = Some(Outcome::Failed(format!("The download of {} stopped.", dl.model.name)));
                    break;
                }
            }
        }
        let Some(out) = finished else {
            return;
        };
        let m = dl.model;
        let status = outcome_status(m, &out, &dl.dir);
        self.setup.dl = None;
        if let Outcome::Done(rec) = out {
            self.setup.installed = Some(rec);
            self.set_local_model(true);
        }
        self.status = status;
    }

    /// The wizard's download row for `m`: Download or Resume, progress with Pause and Cancel, or what's installed.
    pub(super) fn ui_model_download(&mut self, ui: &mut egui::Ui, m: &'static LocalModel) {
        if let Some(rec) = self.setup.installed.clone() {
            self.ui_installed_note(ui, &rec);
            return;
        }
        if let Some(dl) = &self.setup.dl {
            let line = if dl.verifying {
                format!("Checking {}…", dl.model.name)
            } else {
                format!("Downloading {}: {}", dl.model.name, md::progress_line(dl.done, dl.total))
            };
            crate::cards::settings_note(ui, &line);
            crate::cards::settings_progress(ui, md::percent(dl.done, dl.total), crate::theme::fg());
            let model = dl.model;
            ui.horizontal(|ui| {
                if crate::cards::ghost_pill(ui, "Pause") {
                    self.pause_model_download();
                }
                if crate::cards::ghost_pill(ui, "Cancel") {
                    self.cancel_model_download(model);
                }
            });
            return;
        }
        let part = md::partial_bytes(&models_dir(), m);
        let label = match part {
            Some(b) => format!("Resume {} ({})", m.name, md::progress_line(b, Some(m.download_mb * MIB))),
            None => format!("Download {} ({})", m.name, ls::size_label(m.download_mb)),
        };
        ui.horizontal(|ui| {
            if crate::cards::white_pill(ui, &label) {
                self.start_model_download(m);
            }
            if part.is_some() && crate::cards::ghost_pill(ui, "Discard") {
                self.cancel_model_download(m);
            }
        });
    }

    fn ui_installed_note(&self, ui: &mut egui::Ui, rec: &Installed) {
        let name = ls::model_by_id(&rec.id).map(|m| m.name).unwrap_or(rec.file.as_str());
        crate::cards::settings_note(
            ui,
            &format!(
                "Downloaded {name} ({}) to {}. This build can't run it yet, so background work stays on your cloud model for now.",
                ls::size_label(rec.bytes / MIB),
                models_dir().display()
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct FakeHf(Vec<u8>);

    impl Fetch for FakeHf {
        fn open(&self, url: &str, offset: u64) -> Result<md::Opened, String> {
            assert_eq!(url, "https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf");
            Ok((Box::new(Cursor::new(self.0[offset as usize..].to_vec())), offset, Some(self.0.len() as u64)))
        }
        fn sha256(&self, _repo: &str, _file: &str) -> Result<String, String> {
            Ok(md::sha256_hex(&self.0))
        }
    }

    #[test]
    fn a_finished_download_records_the_model_turns_the_toggle_on_and_names_it() {
        let root = std::env::temp_dir().join(format!("grokhub-dl-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        let _pin = crate::config::TestConfigDir::set(root.join("cfg"));
        let dir = root.join("models");
        let m = ls::model_by_id("qwen2.5-1.5b-instruct-q4_k_m").expect("1.5B");
        // The real fetch carries no data, so the egress guard lets both Hugging Face calls through.
        assert_eq!(crate::xai::egress_ok(&md::download_url(m), &[]), Ok(()));
        assert_eq!(crate::xai::egress_ok(&md::tree_url(m.repo), &[]), Ok(()));
        let mut app = Cabin::quiet_for_test();
        app.start_model_download_with(m, dir.clone(), Box::new(FakeHf(b"tiny fake gguf".to_vec())));
        assert_eq!(app.status, format!("Downloading Qwen 2.5 1.5B to {}", dir.display()));
        for _ in 0..500 {
            app.poll_model_download();
            if app.setup.dl.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.setup.dl.is_none(), "the download never finished");
        assert_eq!(app.status, format!("Downloaded Qwen 2.5 1.5B (0 MB) to {}", dir.display()));
        let rec = app.setup.installed.clone().expect("installed");
        assert_eq!((rec.id.as_str(), rec.bytes), ("qwen2.5-1.5b-instruct-q4_k_m", 14));
        assert_eq!(md::installed(&dir), Some(rec));
        assert!(app.cfg.local_model, "a finished download turns the toggle on");
        // No runtime in this build: the route stays off and the rows say why.
        assert!(!grokhub_agent::route::local::enabled());
        assert_eq!(app.downloaded_model_name(), Some("Qwen 2.5 1.5B"));
        assert_eq!(app.local_model_row_hint(), format!("Qwen 2.5 1.5B (0 MB) is in {}.", models_dir().display()));
        grokhub_agent::route::local::set_enabled(false);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn each_ending_reads_as_a_plain_status_line() {
        let m = ls::model_by_id("qwen2.5-7b-instruct-q4_k_m").expect("7B");
        let dir = Path::new("/home/v/GrokHub/models");
        assert_eq!(
            outcome_status(m, &Outcome::Paused { done: 2_100 * MIB }, dir),
            "Paused Qwen 2.5 7B at 2.1 GB of 4.6 GB. Resume picks up where it stopped."
        );
        assert_eq!(outcome_status(m, &Outcome::Cancelled, dir), "Cancelled the Qwen 2.5 7B download.");
        assert_eq!(
            outcome_status(m, &Outcome::BadChecksum, dir),
            "Qwen 2.5 7B didn't match its published checksum, so it was deleted. Try the download again."
        );
        assert_eq!(
            outcome_status(m, &Outcome::Failed("The download of Qwen 2.5 7B stopped: timed out".into()), dir),
            "The download of Qwen 2.5 7B stopped: timed out Resume picks up where it stopped."
        );
        let rec = Installed { id: m.id.into(), file: m.file.into(), sha256: "a".repeat(64), bytes: 4_680 * MIB };
        assert_eq!(outcome_status(m, &Outcome::Done(rec), dir), format!("Downloaded Qwen 2.5 7B (4.6 GB) to {}", dir.display()));
    }
}
