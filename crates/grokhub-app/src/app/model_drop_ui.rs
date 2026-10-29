//! New 3D models in Downloads (`grokhub_core::model_drop`), opt-in under
//! Settings → Behavior. Every 20 seconds the cabin lists Downloads and keeps
//! only model names; a new one gets a preview rendered off the UI thread, a
//! Pulse card, and one-taps first in the composer. Printing goes through the
//! agent, so it asks first.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::*;
use grokhub_core::model_drop::{self as md, ModelFile, ModelOffer, Slicer};
use grokhub_core::QuickChip;

const SCAN_EVERY_MS: u64 = 20_000;
const PREVIEW_PX: usize = 192;

#[derive(Default)]
pub(super) struct ModelDropState {
    seen: BTreeSet<String>,
    first_done: bool,
    scanned_ms: u64,
    rx: Option<mpsc::Receiver<ModelOffer>>,
    offer: Option<ModelOffer>,
    slicers: Option<Vec<Slicer>>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn downloads() -> PathBuf {
    let home = home();
    let user_dirs = std::fs::read_to_string(home.join(".config").join("user-dirs.dirs")).ok();
    md::downloads_dir(&home, user_dirs.as_deref())
}

fn detect_slicers() -> Vec<Slicer> {
    if cfg!(windows) {
        let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        md::windows_slicers(&roots)
    } else {
        let flatpak = [
            home().join(".local/share/flatpak/app"),
            PathBuf::from("/var/lib/flatpak/app"),
        ];
        md::linux_slicers(&grokhub_core::runtime_download::find_on_path, &flatpak)
    }
}

/// The preview key for a model: an image path the Feed reads from its cache.
fn thumb_key(file: &ModelFile) -> String {
    format!("model-thumb:{}:{}.png", file.name, file.bytes)
}

/// Read the model (when small enough), size it and write its preview where
/// the Feed's image cache looks for `thumb_key`.
fn prepare(file: ModelFile, thumb_path: &dyn Fn(&str) -> PathBuf) -> ModelOffer {
    let mut offer = ModelOffer {
        dims: None,
        thumb: None,
        file,
    };
    if offer.file.bytes > md::PREVIEW_MAX_BYTES {
        return offer;
    }
    let Ok(bytes) = std::fs::read(&offer.file.path) else {
        return offer;
    };
    let (own_thumb, mesh) = md::mesh_for(&offer.file.name, &bytes);
    offer.dims = mesh.as_ref().and_then(md::Mesh::size);
    let png = own_thumb.or_else(|| {
        let rgba = md::render(mesh.as_ref()?, PREVIEW_PX)?;
        let img = image::RgbaImage::from_raw(PREVIEW_PX as u32, PREVIEW_PX as u32, rgba)?;
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
        Some(out)
    });
    if let Some(png) = png {
        let key = thumb_key(&offer.file);
        let path = thumb_path(&key);
        let wrote = path
            .parent()
            .map(|dir| std::fs::create_dir_all(dir).is_ok())
            .unwrap_or(false)
            && std::fs::write(&path, png).is_ok();
        if wrote {
            offer.thumb = Some(key);
        }
    }
    offer
}

impl Cabin {
    fn models_folder(&self) -> PathBuf {
        if self.cfg.models_folder.trim().is_empty() {
            home().join("Models")
        } else {
            PathBuf::from(&self.cfg.models_folder)
        }
    }

    fn first_slicer(&mut self) -> Option<Slicer> {
        self.focus
            .models
            .slicers
            .get_or_insert_with(detect_slicers)
            .first()
            .cloned()
    }

    /// Each frame: list Downloads every [`SCAN_EVERY_MS`], and take a prepared offer.
    pub(super) fn tick_model_drop(&mut self) {
        if !self.cfg.model_drop {
            return;
        }
        self.take_model_offer();
        let now = now_ms();
        let state = &mut self.focus.models;
        if state.rx.is_some() || now.saturating_sub(state.scanned_ms) < SCAN_EVERY_MS {
            return;
        }
        state.scanned_ms = now;
        let first = !std::mem::replace(&mut state.first_done, true);
        self.note_downloads(&downloads(), first);
    }

    /// Scan `dir`; every new model is prepared off the UI thread, one after
    /// another, and each gets its own offer.
    fn note_downloads(&mut self, dir: &Path, first: bool) {
        let state = &mut self.focus.models;
        let files = md::scan_new(dir, &mut state.seen, first);
        if files.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        state.rx = Some(rx);
        std::thread::spawn(move || {
            for file in files {
                if tx
                    .send(prepare(file, &super::pulse_ui::pulse_image_path))
                    .is_err()
                {
                    return;
                }
            }
        });
    }

    fn take_model_offer(&mut self) {
        let Some(rx) = self.focus.models.rx.take() else {
            return;
        };
        // Every offer ready now posts, then the feed is written once.
        let mut posted = false;
        loop {
            match rx.try_recv() {
                Ok(offer) => posted |= self.post_model_offer(offer),
                Err(mpsc::TryRecvError::Empty) => {
                    self.focus.models.rx = Some(rx);
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if posted {
            self.persist_updates();
        }
    }

    /// Posts the card and makes `offer` the composer's; true when the feed changed.
    fn post_model_offer(&mut self, offer: ModelOffer) -> bool {
        let slicer = self.first_slicer();
        let posted =
            grokhub_core::post_update(&mut self.updates, offer.card(slicer.as_ref(), now_ms()));
        self.focus.models.offer = Some(offer);
        self.chip_paint_key.clear();
        posted
    }

    /// The new model's one-taps, unless dismissed.
    pub(super) fn model_chips(&self) -> Vec<QuickChip> {
        let Some(offer) = self
            .focus
            .models
            .offer
            .as_ref()
            .filter(|_| self.cfg.model_drop)
        else {
            return Vec::new();
        };
        // Detected when the offer was posted.
        let slicer = self.focus.models.slicers.as_ref().and_then(|s| s.first());
        offer
            .chips(slicer, &self.models_folder(), cfg!(windows))
            .into_iter()
            .filter(|c| {
                !self
                    .chip_dismissed
                    .iter()
                    .any(|d| d == &c.id || d == &c.value)
            })
            .collect()
    }

    pub(super) fn ui_model_drop_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.model_drop;
        if crate::cards::settings_toggle(
            ui,
            "Offer next steps for new 3D models",
            "When an .stl, .3mf, .obj, .step or .stp file lands in Downloads, show a preview and offer to open it in your slicer, suggest a print profile, or move it to your models folder. Only files with those extensions are looked at. Printing always asks first.",
            &mut on,
        ) {
            self.cfg.model_drop = on;
            self.focus.models = ModelDropState::default();
            self.chip_paint_key.clear();
            self.persist_cfg();
            self.status = "Saved".into();
        }
        if !self.cfg.model_drop {
            return;
        }
        let folder = self.models_folder().to_string_lossy().to_string();
        let slicers: Vec<&str> = self
            .focus
            .models
            .slicers
            .get_or_insert_with(detect_slicers)
            .iter()
            .map(|s| s.name)
            .collect();
        let found = if slicers.is_empty() {
            "No slicer found.".to_string()
        } else {
            format!("Slicer: {}.", slicers.join(", "))
        };
        ui.indent("model-drop", |ui| {
            crate::cards::help_text(ui, &found);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Models folder: {folder}")).size(crate::theme::FONT_TIP),
                );
                if crate::cards::ghost_pill(ui, "Change") {
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title("Models folder")
                        .pick_folder()
                    {
                        self.cfg.models_folder = dir.to_string_lossy().to_string();
                        self.persist_cfg();
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grokhub-model-ui-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const STL: &str = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 40 0 0\nvertex 0 20 10\nendloop\nendfacet\nendsolid t\n";

    #[test]
    fn a_new_model_gets_a_preview_and_sizes() {
        let dir = tmp("prepare");
        std::fs::write(dir.join("bracket.stl"), STL).unwrap();
        let file = ModelFile {
            path: dir.join("bracket.stl"),
            name: "bracket.stl".into(),
            bytes: STL.len() as u64,
        };
        let cache = dir.join("cache");
        let offer = prepare(file, &|key| {
            cache.join(grokhub_core::pulse::image_cache_name(key))
        });
        assert_eq!(offer.dims, Some([40.0, 20.0, 10.0]));
        let key = offer.thumb.expect("thumb");
        let png = std::fs::read(cache.join(grokhub_core::pulse::image_cache_name(&key))).unwrap();
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!(
            (img.width(), img.height()),
            (PREVIEW_PX as u32, PREVIEW_PX as u32)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_new_models_offer_and_off_shows_nothing() {
        let root = crate::config::test_config_root("model-drop");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.model_drop = true;
        cabin.focus.models.slicers = Some(Vec::new());
        let dir = tmp("scan");
        std::fs::write(dir.join("old.stl"), STL).unwrap();
        cabin.note_downloads(&dir, true);
        assert!(cabin.focus.models.rx.is_none(), "already there at start");
        std::fs::write(dir.join("photo.jpg"), b"x").unwrap();
        cabin.note_downloads(&dir, false);
        assert!(cabin.focus.models.rx.is_none(), "not a model");
        cabin.post_model_offer(ModelOffer {
            file: ModelFile {
                path: dir.join("new.stl"),
                name: "new.stl".into(),
                bytes: 10,
            },
            dims: None,
            thumb: None,
        });
        let labels: Vec<String> = cabin.model_chips().into_iter().map(|c| c.label).collect();
        assert_eq!(
            labels,
            vec!["Suggest a print profile", "Print new", "Move to models"]
        );
        assert!(cabin
            .updates
            .iter()
            .any(|c| c.title == "New model: new.stl"));
        cabin.cfg.model_drop = false;
        assert!(cabin.model_chips().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_model_from_one_scan_gets_an_offer() {
        let root = crate::config::test_config_root("model-drop-many");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.model_drop = true;
        cabin.focus.models.slicers = Some(Vec::new());
        let dir = tmp("many");
        cabin.note_downloads(&dir, true);
        for name in ["arm.stl", "base.stl", "lid.stl"] {
            std::fs::write(dir.join(name), STL).unwrap();
        }
        cabin.note_downloads(&dir, false);
        let started = std::time::Instant::now();
        while cabin.focus.models.rx.is_some() && started.elapsed().as_secs() < 30 {
            cabin.take_model_offer();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut titles: Vec<&str> = cabin
            .updates
            .iter()
            .filter(|c| c.title.starts_with("New model: "))
            .map(|c| c.title.as_str())
            .collect();
        titles.sort();
        assert_eq!(
            titles,
            vec![
                "New model: arm.stl",
                "New model: base.stl",
                "New model: lid.stl"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
