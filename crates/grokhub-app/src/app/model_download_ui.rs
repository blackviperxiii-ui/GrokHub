//! The setup wizard's model download: Hugging Face over HTTP behind
//! `model_download::Fetch`, run off the UI thread, with progress, Pause,
//! Cancel and Resume. A `.part` left by a pause, a dropped connection or a
//! restart resumes from where it stopped. When the checksum matches, the
//! on-device toggle turns on. This build has no runtime to run the file, so
//! the route stays off and the rows say so. A model with a draft model
//! (`local_draft`) offers it as a second download into `models/drafters/`;
//! it is never fetched on its own and never replaces the model GrokHub runs.

#[cfg(not(test))]
use std::io::Read;
use std::path::Path;
use std::sync::{mpsc, Arc};

use grokhub_agent::route::local_sidecar;
use grokhub_core::local_draft as ld;
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
    /// A draft model: it doesn't become the installed model.
    drafter: bool,
}

/// Where draft models download.
pub(super) fn drafters_dir() -> std::path::PathBuf {
    models_dir().join(ld::DRAFTERS_DIR)
}

/// What the draft model offer says.
pub(super) fn drafter_offer_line(d: &LocalModel) -> String {
    format!(
        "Optional: {} can draft replies for this model, which can make them faster on some machines. {} only downloads it if you ask, and keeps it only if the speed test finds it helps.",
        d.name,
        grokhub_core::brand::product_name()
    )
}

/// The status line for a finished draft model download.
pub(super) fn drafter_done_status(m: &LocalModel) -> String {
    format!(
        "Downloaded {} as a draft model. Test speed in Settings checks whether it makes replies faster; it only stays on if it does. The next chat turn uses it once that test keeps it (or Manual draft is On).",
        m.name
    )
}

/// Hugging Face over HTTPS. A download carries none of your data. Tests
/// record starts instead, so it is built only outside them.
#[cfg(not(test))]
struct HfFetch;

#[cfg(not(test))]
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout_read(std::time::Duration::from_secs(60))
        .build()
}

#[cfg(not(test))]
impl Fetch for HfFetch {
    fn open(&self, url: &str, offset: u64) -> Result<md::Opened, String> {
        crate::xai::egress_ok(url, &[])?;
        let mut req = agent().get(url).set("user-agent", "GrokHub");
        if offset > 0 {
            req = req.set("range", &format!("bytes={offset}-"));
        }
        let resp = req.call().map_err(|e| e.to_string())?;
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
        Ok((Box::new(resp.into_reader()), start, total))
    }

    fn sha256(&self, repo: &str, file: &str) -> Result<String, String> {
        let url = md::tree_url(repo);
        crate::xai::egress_ok(&url, &[])?;
        let resp = agent()
            .get(&url)
            .set("user-agent", "GrokHub")
            .call()
            .map_err(|e| e.to_string())?;
        let mut body = String::new();
        resp.into_reader()
            .take(4 * MIB)
            .read_to_string(&mut body)
            .map_err(|e| e.to_string())?;
        md::sha256_from_tree(&body, file)
            .ok_or_else(|| format!("Hugging Face lists no checksum for {file}"))
    }
}

/// The status line for how a download ended.
pub(super) fn outcome_status(m: &LocalModel, out: &Outcome, dir: &Path) -> String {
    match out {
        Outcome::Done(rec) => format!(
            "Downloaded {} ({}) to {}",
            m.name,
            ls::size_label(rec.bytes / MIB),
            dir.display()
        ),
        Outcome::Paused { done } => {
            format!(
                "Paused {} at {}. Resume picks up where it stopped.",
                m.name,
                md::progress_line(*done, Some(m.download_mb * MIB))
            )
        }
        Outcome::Cancelled => format!("Cancelled the {} download.", m.name),
        Outcome::BadChecksum => format!(
            "{} didn't match its published checksum, so it was deleted. Try the download again.",
            m.name
        ),
        Outcome::Failed(why) => format!("{why} Resume picks up where it stopped."),
    }
}

impl Cabin {
    /// The downloaded model's name, when one passed its check. An install the
    /// catalog no longer lists (Qwen 2.5) still loads, so it goes by its file.
    pub(super) fn downloaded_model_name(&self) -> Option<&str> {
        self.setup.installed.as_ref().map(|r| {
            ls::model_by_id(&r.id)
                .map(|m| m.name)
                .unwrap_or(r.file.as_str())
        })
    }

    /// The Settings "Local model" row: the download in flight, what's on disk, or nothing.
    pub(super) fn local_model_row_hint(&self) -> String {
        if let Some(dl) = &self.setup.dl {
            return format!(
                "Downloading {}: {}.",
                dl.model.name,
                md::progress_line(dl.done, dl.total)
            );
        }
        match (self.setup.installed.as_ref(), self.downloaded_model_name()) {
            (Some(rec), Some(name)) => format!(
                "{name} ({}) is in {}.",
                ls::size_label(rec.bytes / MIB),
                models_dir().display()
            ),
            _ => "No local model installed.".into(),
        }
    }

    pub(super) fn start_model_download(&mut self, m: &'static LocalModel) {
        #[cfg(test)]
        {
            // Tests never reach Hugging Face or the real models folder.
            tests::STARTED.with(|s| s.borrow_mut().push(m.id));
        }
        #[cfg(not(test))]
        self.start_model_download_with(m, models_dir(), Box::new(HfFetch));
    }

    /// `start_model_download` for a draft model, into its own folder.
    fn start_drafter_download(&mut self, d: &'static LocalModel) {
        #[cfg(test)]
        {
            tests::STARTED.with(|s| s.borrow_mut().push(d.id));
        }
        #[cfg(not(test))]
        self.start_model_download_with(d, drafters_dir(), Box::new(HfFetch));
    }

    /// The first click on a fresh download: ask, with the drive's free space
    /// read now rather than at launch.
    fn ask_download(&mut self, m: &'static LocalModel) {
        self.setup.confirm_dl = Some(m.id);
        self.setup.confirm_free_mb = free_disk_now();
    }

    /// The confirm for `m` when its Download was clicked once: the size and
    /// free disk, then Download or Not now. `false` when `m` isn't asking.
    fn ui_download_confirm(
        &mut self,
        ui: &mut egui::Ui,
        m: &'static LocalModel,
        drafter: bool,
    ) -> bool {
        if self.setup.confirm_dl != Some(m.id) {
            return false;
        }
        let free = self
            .setup
            .confirm_free_mb
            .or(self.setup.hw.as_ref().map(|hw| hw.free_disk_mb));
        let (line, fits) = download_confirm(m, free);
        crate::cards::settings_note(ui, &line);
        ui.horizontal(|ui| {
            if fits
                && crate::cards::white_pill(
                    ui,
                    &format!("Download {}", ls::size_label(m.download_mb)),
                )
            {
                self.setup.confirm_dl = None;
                self.setup.confirm_free_mb = None;
                if drafter {
                    self.start_drafter_download(m);
                } else {
                    self.start_model_download(m);
                }
            }
            if crate::cards::ghost_pill(ui, "Not now") {
                self.setup.confirm_dl = None;
                self.setup.confirm_free_mb = None;
            }
        });
        true
    }

    pub(super) fn start_model_download_with(
        &mut self,
        m: &'static LocalModel,
        dir: std::path::PathBuf,
        fetch: Box<dyn Fetch + Send>,
    ) {
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
        let drafter = dir.ends_with(ld::DRAFTERS_DIR);
        self.setup.dl = Some(ModelDl {
            model: m,
            dir,
            ctl,
            rx,
            done,
            total: Some(m.download_mb * MIB),
            verifying: false,
            drafter,
        });
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
                    finished = Some(Outcome::Failed(format!(
                        "The download of {} stopped.",
                        dl.model.name
                    )));
                    break;
                }
            }
        }
        let Some(out) = finished else {
            return;
        };
        let m = dl.model;
        let drafter = dl.drafter;
        let mut status = outcome_status(m, &out, &dl.dir);
        self.setup.dl = None;
        match out {
            Outcome::Done(_) if drafter => {
                status = drafter_done_status(m);
                // Next request reloads with the new drafter on disk (NeedsTest
                // until Test speed keeps it, or Manual draft On forces it).
                local_sidecar::stop_global();
            }
            Outcome::Done(rec) => self.adopt_local_model(rec),
            _ => {}
        }
        self.status = status;
    }

    /// The wizard's download row for `m`: Download or Resume, progress with Pause and Cancel, or what's installed.
    pub(super) fn ui_model_download(&mut self, ui: &mut egui::Ui, m: &'static LocalModel) {
        if let Some(rec) = self.setup.installed.clone() {
            self.ui_installed_note(ui, &rec);
            self.ui_drafter_offer(ui, &rec);
            return;
        }
        if self.ui_download_progress(ui) {
            return;
        }
        self.ui_download_button(ui, m);
    }

    /// Download or Resume `m`, with Discard for a paused `.part`.
    pub(super) fn ui_download_button(&mut self, ui: &mut egui::Ui, m: &'static LocalModel) {
        let part = md::partial_bytes(&models_dir(), m);
        let label = match part {
            Some(b) => format!(
                "Resume {} ({})",
                m.name,
                md::progress_line(b, Some(m.download_mb * MIB))
            ),
            None => format!("Download {} ({})", m.name, ls::size_label(m.download_mb)),
        };
        // A fresh download asks once, with its size and the free disk; a resume
        // was already agreed to.
        if part.is_none() && self.ui_download_confirm(ui, m, false) {
            return;
        }
        ui.horizontal(|ui| {
            if crate::cards::white_pill(ui, &label) {
                if part.is_some() {
                    self.start_model_download(m);
                } else {
                    self.ask_download(m);
                }
            }
            if part.is_some() && crate::cards::ghost_pill(ui, "Discard") {
                self.cancel_model_download(m);
            }
        });
    }

    /// Progress with Pause and Cancel while a download runs; `false` when none does.
    pub(super) fn ui_download_progress(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(dl) = &self.setup.dl else {
            return false;
        };
        let line = if dl.verifying {
            format!("Checking {}…", dl.model.name)
        } else {
            format!(
                "Downloading {}: {}",
                dl.model.name,
                md::progress_line(dl.done, dl.total)
            )
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
        true
    }

    /// The draft model for `target_file`, offered as its own download.
    pub(super) fn ui_drafter_offer_for(&mut self, ui: &mut egui::Ui, target_file: &str) {
        if self.ui_download_progress(ui) {
            return;
        }
        let Some(d) = ld::offer(target_file, &drafters_dir()) else {
            return;
        };
        crate::cards::settings_note(ui, &drafter_offer_line(d));
        let part = md::partial_bytes(&drafters_dir(), d);
        // The draft model is a download too: same size and free-disk check.
        if part.is_none() && self.ui_download_confirm(ui, d, true) {
            return;
        }
        let label = match part {
            Some(b) => format!(
                "Resume {} ({})",
                d.name,
                md::progress_line(b, Some(d.download_mb * MIB))
            ),
            None => format!(
                "Install and run draft model ({})",
                ls::size_label(d.download_mb)
            ),
        };
        if crate::cards::ghost_pill(ui, &label) {
            if part.is_some() {
                self.start_drafter_download(d);
            } else {
                self.ask_download(d);
            }
        }
    }

    /// The installed model's draft model, offered as its own download.
    fn ui_drafter_offer(&mut self, ui: &mut egui::Ui, rec: &Installed) {
        self.ui_drafter_offer_for(ui, &rec.file);
    }

    fn ui_installed_note(&self, ui: &mut egui::Ui, rec: &Installed) {
        let name = ls::model_by_id(&rec.id)
            .map(|m| m.name)
            .unwrap_or(rec.file.as_str());
        let tail = if grokhub_core::runtime_download::engine_binary_available(
            &crate::config::config_dir(),
        ) {
            "Turn on the on-device toggle when you are ready."
        } else {
            "Get engine in Settings first, then turn on the on-device toggle."
        };
        crate::cards::settings_note(
            ui,
            &format!(
                "Downloaded {name} ({}) to {}. {tail}",
                ls::size_label(rec.bytes / MIB),
                models_dir().display()
            ),
        );
    }
}

/// The confirm line before a download, and whether it fits on the drive.
/// `free_mb` is `None` until the hardware read lands (or when it failed).
/// Free disk on the drive that holds the models folder, read now. Tests use
/// `tests::FREE_NOW` instead of the CI runner's disk.
fn free_disk_now() -> Option<u64> {
    #[cfg(test)]
    {
        tests::FREE_NOW.with(|f| f.get())
    }
    #[cfg(not(test))]
    {
        // The models folder may not exist yet; its nearest existing parent is on the same drive.
        let dir = models_dir();
        let at = dir.ancestors().find(|p| p.is_dir())?;
        super::setup_wizard::free_disk_mb(at)
    }
}

pub(super) fn download_confirm(m: &LocalModel, free_mb: Option<u64>) -> (String, bool) {
    let size = ls::size_label(m.download_mb);
    match free_mb.filter(|f| *f > 0) {
        Some(free) if free < ls::disk_needed_mb(m) => (
            format!(
                "{} needs {} free, and this drive has {}. Free up space or pick a smaller model.",
                m.name,
                ls::size_label(ls::disk_needed_mb(m)),
                ls::size_label(free)
            ),
            false,
        ),
        Some(free) => (
            format!(
                "Download {} ({size})? This drive has {} free.",
                m.name,
                ls::size_label(free)
            ),
            true,
        ),
        None => (format!("Download {} ({size})?", m.name), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    thread_local! {
        /// Models `start_model_download` was asked for in this test thread.
        pub(super) static STARTED: std::cell::RefCell<Vec<&'static str>> =
            const { std::cell::RefCell::new(Vec::new()) };
        /// What `free_disk_now` reads in this test thread.
        pub(super) static FREE_NOW: std::cell::Cell<Option<u64>> =
            const { std::cell::Cell::new(None) };
    }

    #[test]
    fn the_confirm_names_the_size_and_the_free_disk() {
        let e4b = ls::local_models()
            .iter()
            .find(|m| m.name == "Gemma 4 E4B")
            .unwrap();
        let size = ls::size_label(e4b.download_mb);
        assert_eq!(
            download_confirm(e4b, Some(200_000)),
            (
                format!("Download Gemma 4 E4B ({size})? This drive has 195.3 GB free."),
                true
            )
        );
        // Not enough room: no Download button, and it says how much is needed.
        let (line, fits) = download_confirm(e4b, Some(1_024));
        assert!(!fits);
        assert_eq!(
            line,
            format!(
                "Gemma 4 E4B needs {} free, and this drive has 1 GB. Free up space or pick a smaller model.",
                ls::size_label(ls::disk_needed_mb(e4b))
            )
        );
        // Free disk unknown: still asks, with the size.
        assert_eq!(
            download_confirm(e4b, None),
            (format!("Download Gemma 4 E4B ({size})?"), true)
        );
    }

    #[test]
    fn one_click_asks_first_and_the_second_click_downloads() {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => {
                    out.push((t.galley.job.text.clone(), shape.visual_bounding_rect()))
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let e2b = ls::local_models()
            .iter()
            .find(|m| m.name == "Gemma 4 E2B")
            .unwrap();
        let m: &'static LocalModel = Box::leak(Box::new(LocalModel {
            id: "confirm-test-model",
            name: "Confirm Model",
            file: "confirm-test.gguf",
            ..*e2b
        }));
        let mut app = Cabin::quiet_for_test();
        app.setup.hw = Some(ls::Hardware {
            gpu: String::new(),
            vram_mb: 0,
            ram_mb: 16_384,
            free_disk_mb: 200_000,
        });
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let frame = |app: &mut Cabin, click: Option<egui::Pos2>| {
            let mut raw = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            if let Some(at) = click {
                raw.events = vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ];
            }
            let out = crate::theme::test_pass(&ctx, raw, |ui| app.ui_download_button(ui, m));
            let mut texts = Vec::new();
            for c in &out.shapes {
                walk(&c.shape, &mut texts);
            }
            texts
        };
        let at = |texts: &[(String, egui::Rect)], want: &str| {
            texts
                .iter()
                .find(|(t, _)| t == want)
                .map(|(_, r)| r.center())
                .unwrap_or_else(|| panic!("{want} missing: {texts:?}"))
        };
        let size = ls::size_label(m.download_mb);
        let texts = frame(&mut app, None);
        let first = at(&texts, &format!("Download Confirm Model ({size})"));
        frame(&mut app, Some(first));
        // The first click only asks.
        assert_eq!(app.setup.confirm_dl, Some("confirm-test-model"));
        assert!(app.setup.dl.is_none(), "nothing downloads on one click");
        let texts = frame(&mut app, None);
        at(
            &texts,
            &format!("Download Confirm Model ({size})? This drive has 195.3 GB free."),
        );
        // Not now goes back to the plain button.
        let not_now = at(&texts, "Not now");
        frame(&mut app, Some(not_now));
        assert_eq!(app.setup.confirm_dl, None);
        assert!(app.setup.dl.is_none());
        assert!(
            STARTED.with(|s| s.borrow().is_empty()),
            "Not now downloads nothing"
        );

        // Ask again, and this time the confirm's own button downloads.
        let texts = frame(&mut app, None);
        frame(
            &mut app,
            Some(at(&texts, &format!("Download Confirm Model ({size})"))),
        );
        let texts = frame(&mut app, None);
        frame(&mut app, Some(at(&texts, &format!("Download {size}"))));
        assert_eq!(app.setup.confirm_dl, None);
        assert_eq!(STARTED.with(|s| s.take()), vec!["confirm-test-model"]);
    }

    /// One headless frame of `paint`, clicking at `click` first; the painted
    /// text with where it landed.
    fn paint_frame(
        ctx: &egui::Context,
        click: Option<egui::Pos2>,
        paint: impl FnMut(&mut egui::Ui),
    ) -> Vec<(String, egui::Rect)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => {
                    out.push((t.galley.job.text.clone(), shape.visual_bounding_rect()))
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 400.0),
            )),
            ..Default::default()
        };
        if let Some(at) = click {
            raw.events = vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
        }
        let out = crate::theme::test_pass(ctx, raw, paint);
        let mut texts = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        texts
    }

    fn centre_of(texts: &[(String, egui::Rect)], want: &str) -> egui::Pos2 {
        texts
            .iter()
            .find(|(t, _)| t == want)
            .map(|(_, r)| r.center())
            .unwrap_or_else(|| panic!("{want} missing: {texts:?}"))
    }

    #[test]
    fn the_confirm_reads_the_free_disk_when_it_opens() {
        let e2b = ls::model_by_id("gemma-4-e2b-it-abliterated-q4_k").expect("E2B");
        let mut app = Cabin::quiet_for_test();
        // Launch saw plenty of room; the drive has since filled up.
        app.setup.hw = Some(ls::Hardware {
            gpu: String::new(),
            vram_mb: 0,
            ram_mb: 16_384,
            free_disk_mb: 200_000,
        });
        FREE_NOW.with(|f| f.set(Some(1_000)));
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let size = ls::size_label(e2b.download_mb);
        let texts = paint_frame(&ctx, None, |ui| app.ui_download_button(ui, e2b));
        let first = centre_of(&texts, &format!("Download {} ({size})", e2b.name));
        paint_frame(&ctx, Some(first), |ui| app.ui_download_button(ui, e2b));
        FREE_NOW.with(|f| f.set(None));
        assert_eq!(app.setup.confirm_free_mb, Some(1_000));
        let texts = paint_frame(&ctx, None, |ui| app.ui_download_button(ui, e2b));
        let want = format!(
            "{} needs {} free, and this drive has 1000 MB. Free up space or pick a smaller model.",
            e2b.name,
            ls::size_label(ls::disk_needed_mb(e2b))
        );
        assert!(texts.iter().any(|(t, _)| *t == want), "{texts:?}");
        assert!(
            !texts.iter().any(|(t, _)| *t == format!("Download {size}")),
            "no Download when it won't fit: {texts:?}"
        );
        // Not now forgets the reading, so the next ask reads again.
        paint_frame(&ctx, Some(centre_of(&texts, "Not now")), |ui| {
            app.ui_download_button(ui, e2b)
        });
        assert_eq!(app.setup.confirm_free_mb, None);
        assert!(STARTED.with(|s| s.take()).is_empty());
    }

    #[test]
    fn a_draft_model_asks_before_it_downloads_too() {
        let g12 = ls::model_by_id("gemma-4-12b-it-abliterated-q4_k").expect("12B");
        // The test home has no draft model yet, so the 12B is offered one.
        let d = ld::offer(g12.file, &drafters_dir()).expect("a draft model for the 12B");
        assert_eq!(md::partial_bytes(&drafters_dir(), d), None);
        let mut app = Cabin::quiet_for_test();
        FREE_NOW.with(|f| f.set(Some(500_000)));
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let size = ls::size_label(d.download_mb);
        let offer = |app: &mut Cabin, click| {
            paint_frame(&ctx, click, |ui| app.ui_drafter_offer_for(ui, g12.file))
        };
        let texts = offer(&mut app, None);
        let install = centre_of(&texts, &format!("Install and run draft model ({size})"));
        offer(&mut app, Some(install));
        FREE_NOW.with(|f| f.set(None));
        assert_eq!(app.setup.confirm_dl, Some(d.id));
        assert!(
            STARTED.with(|s| s.borrow().is_empty()),
            "one click only asks"
        );
        let texts = offer(&mut app, None);
        let ask = format!(
            "Download {} ({size})? This drive has 488.3 GB free.",
            d.name
        );
        assert!(texts.iter().any(|(t, _)| *t == ask), "{texts:?}");
        offer(
            &mut app,
            Some(centre_of(&texts, &format!("Download {size}"))),
        );
        assert_eq!(app.setup.confirm_dl, None);
        assert_eq!(STARTED.with(|s| s.take()), vec![d.id]);
    }

    struct FakeHf(Vec<u8>);

    impl Fetch for FakeHf {
        fn open(&self, url: &str, offset: u64) -> Result<md::Opened, String> {
            assert_eq!(
                url,
                "https://huggingface.co/acme/test-GGUF/resolve/main/test.gguf"
            );
            Ok((
                Box::new(Cursor::new(self.0[offset as usize..].to_vec())),
                offset,
                Some(self.0.len() as u64),
            ))
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
        let e2b = ls::model_by_id("gemma-4-e2b-it-abliterated-q4_k").expect("E2B");
        // The real fetch carries no data, so the egress guard lets both Hugging Face calls through.
        assert_eq!(crate::xai::egress_ok(&md::download_url(e2b), &[]), Ok(()));
        assert_eq!(crate::xai::egress_ok(&md::tree_url(e2b.repo), &[]), Ok(()));
        // A real GGUF can't be faked, so this model is pinned to the fake body.
        let body = b"tiny fake gguf".to_vec();
        let m: &'static LocalModel = Box::leak(Box::new(LocalModel {
            id: "test-model",
            name: "Test Model",
            repo: "acme/test-GGUF",
            file: "test.gguf",
            sha256: md::sha256_hex(&body).leak(),
            ..*e2b
        }));
        let mut app = Cabin::quiet_for_test();
        app.start_model_download_with(m, dir.clone(), Box::new(FakeHf(body)));
        assert_eq!(
            app.status,
            format!("Downloading Test Model to {}", dir.display())
        );
        for _ in 0..500 {
            app.poll_model_download();
            if app.setup.dl.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.setup.dl.is_none(), "the download never finished");
        assert_eq!(
            app.status,
            format!("Downloaded Test Model (0 MB) to {}", dir.display())
        );
        let rec = app.setup.installed.clone().expect("installed");
        assert_eq!((rec.id.as_str(), rec.bytes), ("test-model", 14));
        assert_eq!(md::installed(&dir), Some(rec));
        assert!(
            app.cfg.local_model,
            "a finished download turns the toggle on"
        );
        // No runtime in this build: the route stays off and the rows say why.
        assert!(!grokhub_agent::route::local::available());
        // Not a catalog id, like an older Qwen 2.5 install: it goes by its file.
        assert_eq!(app.downloaded_model_name(), Some("test.gguf"));
        assert_eq!(
            app.local_model_row_hint(),
            format!("test.gguf (0 MB) is in {}.", models_dir().display())
        );
        grokhub_agent::route::local::set_enabled(false);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_draft_model_downloads_beside_the_model_and_changes_nothing_else() {
        let root = std::env::temp_dir().join(format!("grokhub-dl-draft-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.join("cfg"));
        let g12 = ls::model_by_id("gemma-4-12b-it-abliterated-q4_k").expect("12B");
        let e2b = ls::model_by_id("gemma-4-e2b-it-abliterated-q4_k").expect("E2B");
        let drafters = root.join("models").join(ld::DRAFTERS_DIR);
        // Offered for the 12B until it's on disk; the line says it is optional.
        assert_eq!(ld::offer(g12.file, &drafters).map(|d| d.id), Some(e2b.id));
        assert_eq!(
            drafter_offer_line(e2b),
            "Optional: Gemma 4 E2B can draft replies for this model, which can make them faster on some machines. Loadout only downloads it if you ask, and keeps it only if the speed test finds it helps."
        );
        let body = b"tiny fake drafter".to_vec();
        // Pinned to the fake body, under the fake's repo and file.
        let d: &'static LocalModel = Box::leak(Box::new(LocalModel {
            repo: "acme/test-GGUF",
            file: "test.gguf",
            sha256: md::sha256_hex(&body).leak(),
            ..*e2b
        }));
        let mut app = Cabin::quiet_for_test();
        let installed = Installed {
            id: g12.id.into(),
            file: g12.file.into(),
            sha256: g12.sha256.into(),
            bytes: 7_039 * MIB,
        };
        app.setup.installed = Some(installed.clone());
        app.cfg.local_model = false;
        app.start_model_download_with(d, drafters.clone(), Box::new(FakeHf(body)));
        for _ in 0..500 {
            app.poll_model_download();
            if app.setup.dl.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.setup.dl.is_none(), "the download never finished");
        assert_eq!(
            app.status,
            "Downloaded Gemma 4 E2B as a draft model. Test speed in Settings checks whether it makes replies faster; it only stays on if it does. The next chat turn uses it once that test keeps it (or Manual draft is On)."
        );
        assert_eq!(
            app.setup.installed,
            Some(installed),
            "the model GrokHub runs is unchanged"
        );
        assert!(
            !app.cfg.local_model,
            "a draft model doesn't touch the toggle"
        );
        assert!(drafters.join("test.gguf").is_file());
        assert_eq!(md::installed(&root.join("models")), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn each_ending_reads_as_a_plain_status_line() {
        let m = ls::model_by_id("gemma-4-e4b-it-abliterated-q4_k").expect("E4B");
        let dir = Path::new("/home/v/GrokHub/models");
        assert_eq!(
            outcome_status(m, &Outcome::Paused { done: 2_100 * MIB }, dir),
            "Paused Gemma 4 E4B at 2.1 GB of 4.9 GB. Resume picks up where it stopped."
        );
        assert_eq!(
            outcome_status(m, &Outcome::Cancelled, dir),
            "Cancelled the Gemma 4 E4B download."
        );
        assert_eq!(
            outcome_status(m, &Outcome::BadChecksum, dir),
            "Gemma 4 E4B didn't match its published checksum, so it was deleted. Try the download again."
        );
        assert_eq!(
            outcome_status(
                m,
                &Outcome::Failed("The download of Gemma 4 E4B stopped: timed out".into()),
                dir
            ),
            "The download of Gemma 4 E4B stopped: timed out Resume picks up where it stopped."
        );
        let rec = Installed {
            id: m.id.into(),
            file: m.file.into(),
            sha256: "a".repeat(64),
            bytes: 5_057 * MIB,
        };
        assert_eq!(
            outcome_status(m, &Outcome::Done(rec), dir),
            format!("Downloaded Gemma 4 E4B (4.9 GB) to {}", dir.display())
        );
    }
}
