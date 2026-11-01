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
    /// The ledger in memory: read from disk on first use and on the 30 s
    /// tick, written back on every change. Paint never reads the disk.
    ledger: Option<ScreenshotOfferLedger>,
}

impl ShotState {
    /// A scan or an Explain load is still running.
    pub(super) fn busy(&self) -> bool {
        self.scan.is_some() || self.explain.is_some()
    }
}

impl Cabin {
    /// The cached ledger, read from disk only the first time.
    fn shot_ledger(&mut self) -> &mut ScreenshotOfferLedger {
        self.focus
            .shot
            .ledger
            .get_or_insert_with(load_screenshot_offer_ledger)
    }

    /// Write the cached ledger back after a change.
    fn save_shot_ledger(&mut self) {
        save_screenshot_offer_ledger(self.shot_ledger());
    }

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
        let cfg = self.cfg.screenshot_explain.clone();
        let _ = tick_screenshot_offer(self.shot_ledger(), &cfg, newest, now);
        self.save_shot_ledger();
    }

    pub(super) fn tick_screenshot_offer(&mut self) {
        let now = now_ms();
        if now.saturating_sub(self.shot_ledger().last_tick_ms()) < TICK_EVERY_MS {
            return;
        }
        // The 30 s tick is the one time the ledger is read again.
        self.focus.shot.ledger = Some(load_screenshot_offer_ledger());
        self.shot_ledger().set_last_tick_ms(now);
        if !self.cfg.screenshot_explain.enabled {
            if self.shot_ledger().pending.take().is_some() {
                self.save_shot_ledger();
            }
            return;
        }
        let Some(home) = user_home() else {
            self.save_shot_ledger();
            return;
        };
        let os = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        self.save_shot_ledger();
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
        let Some(path) = self.shot_ledger().offer_live(now).map(|o| o.path.clone()) else {
            return;
        };
        // Same pills and spacing as the composer chips under it.
        ui.add_space(6.0);
        let (explain, dismiss) = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                (
                    crate::cards::ghost_pill(ui, CHIP_LABEL),
                    crate::cards::ghost_pill(ui, DISMISS_LABEL),
                )
            })
            .inner;
        if explain {
            self.accept_screenshot_explain(path);
        } else if dismiss {
            dismiss_screenshot_offer(self.shot_ledger());
            self.save_shot_ledger();
        }
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
        if self
            .shot_ledger()
            .pending
            .as_ref()
            .is_some_and(|p| p.path == path)
        {
            dismiss_screenshot_offer(self.shot_ledger());
            self.save_shot_ledger();
        }
    }

    pub(super) fn ui_screenshot_explain_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("When you save a new screenshot, offer a small Explain chip. The image is not read until you tap.");
        let mut on = self.cfg.screenshot_explain.enabled;
        if ui.checkbox(&mut on, "Screenshot explain").changed() {
            self.cfg.screenshot_explain.enabled = on;
            if !on {
                dismiss_screenshot_offer(self.shot_ledger());
                self.save_shot_ledger();
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

    /// The offer paints from the ledger in memory: once read, a paint does
    /// not go back to the disk, and Dismiss writes the change through.
    #[test]
    fn the_offer_paints_from_memory_and_dismiss_writes_through() {
        let (_pin, root) = pinned("shot-cache");
        let shot = root.join("shot.png");
        offer(&shot);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.screenshot_explain.enabled = true;
        assert!(cabin.shot_ledger().offer_live(now_ms()).is_some());
        // Gone from disk; paint keeps the copy it has.
        std::fs::remove_file(ledger_path()).unwrap();
        let ctx = egui::Context::default();
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            cabin.ui_screenshot_offer_chip(ui)
        });
        assert!(
            !ledger_path().exists(),
            "painting the offer neither reads nor writes the ledger file"
        );
        assert!(cabin.focus.shot.ledger.as_ref().unwrap().pending.is_some());
        dismiss_screenshot_offer(cabin.shot_ledger());
        cabin.save_shot_ledger();
        assert!(load_screenshot_offer_ledger().pending.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_offer_uses_the_app_pills() {
        let src = include_str!("screenshot_offer_ui.rs").replace("\r\n", "\n");
        let paint = src
            .split("fn ui_screenshot_offer_chip(")
            .nth(1)
            .and_then(|s| s.split("fn accept_screenshot_explain(").next())
            .expect("offer paint");
        assert!(paint.contains("ghost_pill(ui, CHIP_LABEL)"), "{paint}");
        assert!(paint.contains("ghost_pill(ui, DISMISS_LABEL)"), "{paint}");
        assert!(!paint.contains("ui.button("), "{paint}");
        assert!(!paint.contains("load_screenshot_offer_ledger"), "{paint}");
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
