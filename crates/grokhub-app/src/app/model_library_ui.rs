//! Settings → AI & chat → Download models: every catalog model by tab
//! (Chat, Drafts, Image, Voice, Other) with its fit, what it is for, and
//! Download, Remove or In use, plus the best lineup for this PC (one model
//! per goal for hot-swap) and Download everything that fits. Downloads run one
//! at a time through the same downloader as the model picker; a queued chat
//! model doesn't replace the one in use, the router picks among them.

use std::path::PathBuf;

use grokhub_agent::route::local_sidecar;
use grokhub_core::local_picker::{BenchCache, Profile};
use grokhub_core::local_setup::{CpuSimd, LocalModel, ModelKind, ModelTab};
use grokhub_core::model_download as md;
use grokhub_core::model_library::{self as lib, Fit, Row};
use grokhub_core::model_lineup as lineup;

use super::model_download_ui::{drafters_dir, free_disk_now};
use super::setup_wizard::models_dir;
use super::*;

/// The folder a catalog model downloads into.
pub(super) fn dir_for(m: &LocalModel) -> PathBuf {
    if m.kind == ModelKind::Drafter {
        drafters_dir()
    } else {
        md::kind_dir(&models_dir(), m.kind)
    }
}

pub(super) fn on_disk(m: &LocalModel) -> bool {
    md::complete(&dir_for(m), m)
}

impl Cabin {
    fn library_rows(&self) -> Option<Vec<Row>> {
        let hw = self.setup.hw.as_ref()?;
        let p = Profile::from_hardware(hw, CpuSimd::detect());
        Some(lib::rows(&p, &BenchCache::new(), on_disk))
    }

    /// The Download models section.
    pub(super) fn ui_download_models(&mut self, ui: &mut egui::Ui) {
        let Some(rows) = self.library_rows() else {
            crate::cards::settings_note(ui, super::setup_wizard::CHECKING_HW);
            return;
        };
        let tab = self.setup.lib_tab.unwrap_or(ModelTab::Chat);
        ui.horizontal_wrapped(|ui| {
            for t in ModelTab::ALL {
                let n = rows.iter().filter(|r| r.tab() == t).count();
                let label = format!("{} ({n})", t.label());
                let hit = if t == tab {
                    crate::cards::white_pill(ui, &label)
                } else {
                    crate::cards::ghost_pill(ui, &label)
                };
                if hit {
                    self.setup.lib_tab = Some(t);
                }
            }
        });
        let shown: Vec<&Row> = rows.iter().filter(|r| r.tab() == tab).collect();
        if shown.is_empty() {
            crate::cards::settings_note(ui, lib::empty_tab_line(tab));
        }
        for r in shown {
            self.ui_library_row(ui, r, &rows);
        }
        if !self.setup.dl_queue.is_empty() {
            let left = self.setup.dl_queue.len();
            let hint =
                format!("{left} more after this one. Each starts when the one before it finishes.");
            if crate::cards::settings_action(ui, "Download queue", &hint, "Stop the queue") {
                self.setup.dl_queue.clear();
            }
            return;
        }
        let free = free_disk_now().or(self.setup.hw.as_ref().map(|hw| hw.free_disk_mb));
        // The best lineup for this PC, one model per goal, for hot-swap to
        // switch between. What it fetches shows before the click.
        let lineup = self.library_lineup(&rows, free);
        match lineup {
            Some(l) if !l.queue.is_empty() => {
                if crate::cards::settings_action(
                    ui,
                    "Best lineup for this PC",
                    &lineup::lineup_line(&l),
                    "Download lineup",
                ) {
                    self.download_all(l.queue);
                }
                return;
            }
            Some(l) => crate::cards::settings_note(ui, &lineup::lineup_line(&l)),
            None => {}
        }
        let all_q = lib::queue(&rows, None, free);
        if !all_q.is_empty()
            && crate::cards::settings_action(
                ui,
                "Download everything that fits",
                &lib::queue_line(&all_q),
                "Download everything",
            )
        {
            self.download_all(all_q);
        }
    }

    /// The recommended lineup from the hardware reading the setup already
    /// made on this PC; nothing is stored or sent.
    fn library_lineup(&self, rows: &[Row], free: Option<u64>) -> Option<lineup::Lineup> {
        let hw = self.setup.hw.as_ref()?;
        let p = Profile::from_hardware(hw, CpuSimd::detect());
        Some(lineup::recommend(rows, &p, free))
    }

    fn ui_library_row(&mut self, ui: &mut egui::Ui, r: &Row, rows: &[Row]) {
        let m = r.model;
        crate::cards::settings_note(ui, &lib::row_line(r));
        let mut about = lib::good_for(m);
        if r.fit != Fit::Yes && !r.why.is_empty() {
            about = format!("{about} {}", r.why);
        }
        crate::cards::settings_note(ui, &about);
        let downloading = self.setup.dl.as_ref().is_some_and(|d| d.model.id == m.id);
        let in_use = self.setup.installed.as_ref().is_some_and(|i| i.id == m.id);
        if downloading {
            self.ui_download_progress(ui);
        } else if in_use {
            crate::cards::settings_note(ui, "In use.");
        } else if r.on_disk {
            if crate::cards::ghost_pill(ui, &format!("Remove {}", m.name)) {
                self.remove_library_model(m);
            }
        } else if r.fit != Fit::No {
            let queued = self.setup.dl_queue.iter().any(|q| q.id == m.id);
            let label = match (queued, md::partial_bytes(&dir_for(m), m)) {
                (true, _) => None,
                (false, Some(_)) => Some(format!("Resume {}", m.name)),
                (false, None) => Some(format!("Download {}", m.name)),
            };
            match label {
                Some(l) if crate::cards::white_pill(ui, &l) => {
                    // A chat model brings its draft model along.
                    let drafter = m
                        .drafter
                        .and_then(|id| rows.iter().find(|o| o.model.id == id && !o.on_disk));
                    let mut q = vec![m];
                    q.extend(drafter.map(|d| d.model));
                    self.download_all(q);
                }
                Some(_) => {}
                None => crate::cards::settings_note(ui, "Queued."),
            }
        }
        // Why its last download stopped, under its own button.
        if let Some(e) = self.setup.dl_errors.get(m.id) {
            crate::cards::settings_note(ui, &e.message);
            crate::cards::settings_note(ui, &e.detail);
        }
    }

    /// Queue `q` behind anything already queued and start when idle.
    pub(super) fn download_all(&mut self, q: Vec<&'static LocalModel>) {
        for m in q {
            self.setup.dl_errors.remove(m.id);
            if !self.setup.dl_queue.iter().any(|x| x.id == m.id) {
                self.setup.dl_queue.push(m);
            }
        }
        if self.setup.dl.is_none() {
            self.advance_queue();
        }
    }

    /// Start the next queued model that isn't on disk yet. Its name, or
    /// `None` when the queue ran dry.
    pub(super) fn advance_queue(&mut self) -> Option<&'static str> {
        while !self.setup.dl_queue.is_empty() {
            let m = self.setup.dl_queue.remove(0);
            if on_disk(m) {
                continue;
            }
            // The first chat model becomes the one in use; later ones wait
            // for the router.
            let adopt = m.kind == ModelKind::Chat && self.setup.installed.is_none();
            if m.kind == ModelKind::Drafter {
                self.start_drafter_download(m);
            } else {
                self.start_model_download(m);
            }
            if let Some(dl) = self.setup.dl.as_mut() {
                dl.adopt = adopt;
            }
            return Some(m.name);
        }
        None
    }

    fn remove_library_model(&mut self, m: &'static LocalModel) {
        if m.kind == ModelKind::Drafter {
            // A running model may hold its draft model open.
            local_sidecar::stop_global();
        }
        let dir = dir_for(m);
        self.status = match md::remove(&dir, m) {
            Ok(()) => format!("Removed {} from {}.", m.name, dir.display()),
            Err(e) => e,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::super::model_download_ui::tests::STARTED;
    use super::*;
    use grokhub_core::local_setup as ls;

    #[test]
    fn the_queue_starts_one_at_a_time_and_skips_what_is_already_on_disk() {
        let g12 = ls::model_by_id("gemma-4-12b-it-abliterated-q4_k").unwrap();
        let d12 = ls::model_by_id("gemma-4-12b-it-abliterated-mtp").unwrap();
        let mut app = Cabin::quiet_for_test();
        app.download_all(vec![g12, d12, g12]);
        assert_eq!(STARTED.with(|s| s.take()), vec![g12.id]);
        assert_eq!(app.setup.dl_queue.len(), 1, "queued once, the rest waits");
        assert_eq!(app.advance_queue(), Some(d12.name));
        assert_eq!(STARTED.with(|s| s.take()), vec![d12.id]);
        assert_eq!(app.advance_queue(), None);
        assert_eq!(dir_for(d12), drafters_dir());
        assert_eq!(dir_for(g12), models_dir());
        // A voice model keeps out of the chat folder.
        let voice = ls::LocalModel {
            kind: ModelKind::SpeechToText,
            ..*g12
        };
        assert_eq!(dir_for(&voice), models_dir().join("voice"));
    }

    fn texts(app: &mut Cabin) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let out = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
            app.ui_download_models(ui)
        });
        let mut texts = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        texts
    }

    #[test]
    fn the_section_shows_tabs_rows_and_download_all() {
        let mut app = Cabin::quiet_for_test();
        app.setup.hw = Some(ls::Hardware {
            gpu: "NVIDIA GeForce RTX 4070".into(),
            vram_mb: 12_288,
            ram_mb: 32_768,
            free_disk_mb: 500_000,
        });
        let chat = ls::standard_models().count();
        let t = texts(&mut app);
        for want in [
            format!("Chat ({chat})"),
            "Image (3)".to_string(),
            "Download lineup".to_string(),
            "Download Gemma 4 12B".to_string(),
        ] {
            assert!(t.contains(&want), "missing {want:?} in {t:?}");
        }
        // The lineup says what it fetches and for which goal before the click.
        assert!(
            t.iter()
                .any(|x| x.contains("Balance: ") && x.contains("Video: ")),
            "{t:?}"
        );
        app.setup.lib_tab = Some(ModelTab::Image);
        let t = texts(&mut app);
        // The picture models and the guard that checks their pictures.
        for want in [
            "Download Z-Image Turbo",
            "Download Chroma1-HD",
            "Download Picture guard",
        ] {
            assert!(t.iter().any(|x| x.starts_with(want)), "{want:?} in {t:?}");
        }
    }
}
