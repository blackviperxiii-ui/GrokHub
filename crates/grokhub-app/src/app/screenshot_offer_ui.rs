//! Command Centre card 49: opt-in screenshot folder watch and Explain chip.
//! The folder scan and the image load both run off the UI thread.

use std::path::PathBuf;

use grokhub_core::attach::PENDING_ATTACH_MAX;
use grokhub_core::screenshot_offer::{pictures_dir, ImageMeta};
use grokhub_core::{
    dismiss_screenshot_offer, newest_in_watch_dirs, tick_screenshot_offer, user_home, watch_dirs,
    ScreenshotOfferLedger, CHIP_LABEL, DISMISS_LABEL, EXPLAIN_PROMPT,
};

use super::*;
use crate::config;

const LEDGER_FILE: &str = "screenshot-offer.json";
const TICK_EVERY_MS: u64 = 30_000;

fn ledger_path() -> std::path::PathBuf {
    config::config_dir().join(LEDGER_FILE)
}

pub(super) fn load_screenshot_offer_ledger() -> ScreenshotOfferLedger {
    std::fs::read_to_string(ledger_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(super) fn save_screenshot_offer_ledger(ledger: &ScreenshotOfferLedger) {
    if let Ok(text) = serde_json::to_string_pretty(ledger) {
        let path = ledger_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

/// What says the chip cannot take the screenshot: the attachment tray is full.
const TRAY_FULL: &str = "The attachment tray is full. Remove one, then tap Explain again.";

/// A loaded screenshot and its tray thumbnail, both made on the worker.
type ShotLoad = (PlusPick, Option<egui::ColorImage>);

/// The background folder scan and Explain load in flight.
#[derive(Default)]
pub(super) struct ShotState {
    scan: Option<mpsc::Receiver<Option<ImageMeta>>>,
    /// The offer's path and its loading image with its tray thumbnail.
    explain: Option<(String, mpsc::Receiver<ShotLoad>)>,
}

impl ShotState {
    /// A scan or an Explain load is still running.
    pub(super) fn busy(&self) -> bool {
        self.scan.is_some() || self.explain.is_some()
    }
}

impl Cabin {
    /// Each frame: take a finished folder scan or Explain load.
    pub(super) fn poll_screenshot_offer(&mut self) {
        if let Some(rx) = self.focus.shot.scan.take() {
            match rx.try_recv() {
                Ok(newest) => self.finish_screenshot_scan(newest, now_ms()),
                Err(mpsc::TryRecvError::Empty) => self.focus.shot.scan = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if let Some((path, rx)) = self.focus.shot.explain.take() {
            match rx.try_recv() {
                Ok((pick, thumb)) => self.finish_screenshot_explain(&path, pick, thumb),
                Err(mpsc::TryRecvError::Empty) => self.focus.shot.explain = Some((path, rx)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = "Couldn't read that screenshot.".into();
                }
            }
        }
    }

    fn finish_screenshot_scan(&mut self, newest: Option<ImageMeta>, now: u64) {
        let mut ledger = load_screenshot_offer_ledger();
        let _ = tick_screenshot_offer(&mut ledger, &self.cfg.screenshot_explain, newest, now);
        save_screenshot_offer_ledger(&ledger);
    }

    pub(super) fn tick_screenshot_offer(&mut self) {
        let now = now_ms();
        let mut ledger = load_screenshot_offer_ledger();
        if now.saturating_sub(ledger.last_tick_ms()) < TICK_EVERY_MS {
            return;
        }
        ledger.set_last_tick_ms(now);
        if !self.cfg.screenshot_explain.enabled {
            if ledger.pending.take().is_some() {
                save_screenshot_offer_ledger(&ledger);
            }
            return;
        }
        let Some(home) = user_home() else {
            save_screenshot_offer_ledger(&ledger);
            return;
        };
        let os = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        save_screenshot_offer_ledger(&ledger);
        if self.focus.shot.scan.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.focus.shot.scan = Some(rx);
        std::thread::spawn(move || {
            let pictures = pictures_dir(&home);
            let dirs = watch_dirs(&home, pictures.as_deref(), os);
            let _ = tx.send(newest_in_watch_dirs(&dirs, now));
        });
    }

    pub(super) fn ui_screenshot_offer_chip(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.screenshot_explain.enabled {
            return;
        }
        let now = now_ms();
        let ledger = load_screenshot_offer_ledger();
        let Some(offer) = ledger.offer_live(now) else {
            return;
        };
        let path = offer.path.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if ui
                .button(CHIP_LABEL)
                .on_hover_text("Read this screenshot and explain it in chat")
                .clicked()
            {
                self.accept_screenshot_explain(path);
            }
            if ui.button(DISMISS_LABEL).clicked() {
                let mut ledger = load_screenshot_offer_ledger();
                dismiss_screenshot_offer(&mut ledger);
                save_screenshot_offer_ledger(&ledger);
            }
        });
    }

    /// Explain: read and encode the image off the UI thread. The offer stays
    /// up until the image is attached.
    fn accept_screenshot_explain(&mut self, path: String) {
        if self.attach_tray.len() >= PENDING_ATTACH_MAX {
            self.status = TRAY_FULL.into();
            return;
        }
        if self.focus.shot.explain.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let file = PathBuf::from(&path);
        std::thread::spawn(move || {
            let pick = plus_from_path(PlusTarget::Chat, file.clone());
            let thumb = match &pick {
                PlusPick::Ready(r) if r.kind == AttachKind::Image => {
                    super::attach_tray::thumb_for_path(&file)
                }
                _ => None,
            };
            let _ = tx.send((pick, thumb));
        });
        self.focus.shot.explain = Some((path, rx));
        self.status = "Reading the screenshot…".into();
    }

    fn finish_screenshot_explain(
        &mut self,
        path: &str,
        pick: PlusPick,
        thumb: Option<egui::ColorImage>,
    ) {
        match pick {
            PlusPick::Ready(ready) => {
                let before = self.attach_tray.len();
                if ready.kind != AttachKind::Image || ready.image_url.is_some() {
                    self.stage_plus_ready_chat_thumb(ready, thumb);
                }
                if self.attach_tray.len() == before {
                    // Not staged: keep the offer so Explain can be tapped again.
                    if before >= PENDING_ATTACH_MAX {
                        self.status = TRAY_FULL.into();
                    }
                    return;
                }
                self.composer = EXPLAIN_PROMPT.into();
            }
            PlusPick::Err(e) => {
                self.status = e;
                return;
            }
            PlusPick::NativeMiss | PlusPick::ClipEmpty | PlusPick::ClipText(_) => {
                self.status = "That screenshot is gone.".into();
            }
        }
        let mut ledger = load_screenshot_offer_ledger();
        if ledger.pending.as_ref().is_some_and(|p| p.path == path) {
            dismiss_screenshot_offer(&mut ledger);
            save_screenshot_offer_ledger(&ledger);
        }
    }

    pub(super) fn ui_screenshot_explain_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("When you save a new screenshot, offer a small Explain chip. The image is not read until you tap.");
        let mut on = self.cfg.screenshot_explain.enabled;
        if ui.checkbox(&mut on, "Screenshot explain").changed() {
            self.cfg.screenshot_explain.enabled = on;
            if !on {
                let mut ledger = load_screenshot_offer_ledger();
                dismiss_screenshot_offer(&mut ledger);
                save_screenshot_offer_ledger(&ledger);
            }
            self.persist_cfg();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::attach_tray::AttachTrayRow;
    use super::*;
    use grokhub_core::screenshot_offer::PendingScreenshotOffer;
    use std::path::Path;

    fn pinned(label: &str) -> (crate::config::TestConfigDir, PathBuf) {
        let root = crate::config::test_config_root(label);
        std::fs::create_dir_all(&root).unwrap();
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    fn offer(path: &Path) {
        save_screenshot_offer_ledger(&ScreenshotOfferLedger {
            pending: Some(PendingScreenshotOffer {
                path: path.display().to_string(),
                offered_at_ms: now_ms(),
            }),
            ..Default::default()
        });
    }

    #[test]
    fn explain_loads_off_the_ui_thread_then_attaches_and_clears_the_offer() {
        let (_pin, root) = pinned("shot-explain");
        let shot = root.join("shot.png");
        image::RgbaImage::from_pixel(8, 4, image::Rgba([200, 80, 40, 255]))
            .save_with_format(&shot, image::ImageFormat::Png)
            .unwrap();
        offer(&shot);
        let mut cabin = Cabin::quiet_for_test();
        cabin.accept_screenshot_explain(shot.display().to_string());
        assert!(
            cabin.focus.shot.explain.is_some(),
            "the load runs on a worker"
        );
        assert!(
            cabin.attach_tray.is_empty(),
            "nothing attached on the click itself"
        );
        assert!(load_screenshot_offer_ledger().pending.is_some());
        for _ in 0..200 {
            cabin.poll_screenshot_offer();
            if cabin.focus.shot.explain.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(cabin.attach_tray.len(), 1);
        let thumb = cabin.attach_tray.rows[0]
            .thumb
            .as_ref()
            .expect("thumb from the worker");
        assert_eq!(
            thumb.size,
            [2, 1],
            "the worker made the quarter-size tray thumb"
        );
        assert_eq!(cabin.composer, EXPLAIN_PROMPT);
        assert!(load_screenshot_offer_ledger().pending.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_full_tray_says_so_and_keeps_the_offer() {
        let (_pin, root) = pinned("shot-full");
        let shot = root.join("shot.png");
        offer(&shot);
        let mut cabin = Cabin::quiet_for_test();
        for i in 0..PENDING_ATTACH_MAX {
            cabin
                .attach_tray
                .queue
                .stage(format!("/tmp/f{i}.txt"), 1)
                .unwrap();
            cabin.attach_tray.rows.push(AttachTrayRow::default());
        }
        cabin.accept_screenshot_explain(shot.display().to_string());
        assert_eq!(cabin.status, TRAY_FULL);
        assert!(cabin.focus.shot.explain.is_none(), "no load started");
        assert!(
            load_screenshot_offer_ledger().pending.is_some(),
            "the offer stays so Explain can be tapped after making room"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_folder_scan_runs_on_a_worker_and_its_result_lands_in_the_ledger() {
        let (_pin, root) = pinned("shot-scan");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.screenshot_explain.enabled = true;
        cabin.tick_screenshot_offer();
        assert!(
            cabin.focus.shot.scan.is_some(),
            "the scan runs off the UI thread"
        );
        let now = now_ms();
        let shot = ImageMeta {
            path: root.join("Screenshot.png"),
            modified_ms: now - 1_000,
        };
        cabin.finish_screenshot_scan(Some(shot.clone()), now);
        let ledger = load_screenshot_offer_ledger();
        assert_eq!(
            ledger.pending.map(|p| p.path),
            Some(shot.path.display().to_string())
        );
        assert_eq!(ledger.last_seen_modified_ms, now - 1_000);
        let _ = std::fs::remove_dir_all(root);
    }
}
