//! Composer attachment tray: picker, drop, and paste share one ingest worker.

use super::plus::PlusReady;
use super::*;
use eframe::egui::{ColorImage, TextureHandle, TextureOptions};
use grokhub_core::model_registry::store::load_registry;
use grokhub_core::attach::{
    attach_kind, attach_name, attach_prompt_line, attach_send_line, AttachKind,
    PENDING_ATTACH_MAX, PendingAttach, PendingItemStatus,
    PendingValidateIssue, pending_item_user_message,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

/// Per-row UI state (decode/thumbnail runs off the UI thread).
#[derive(Clone, Default)]
pub(super) struct AttachTrayRow {
    pub preparing: bool,
    pub thumb: Option<ColorImage>,
    pub vision_url: Option<String>,
    pub worker_err: Option<String>,
}

#[derive(Default)]
pub(super) struct AttachTray {
    pub queue: PendingAttach,
    pub rows: Vec<AttachTrayRow>,
}

pub(super) enum AttachIngestWork {
    Paths(Vec<PathBuf>),
    ClipboardPaste,
}

pub(super) enum AttachIngestOut {
    Row {
        path: String,
        size_bytes: u64,
        thumb: Option<ColorImage>,
        vision_url: Option<String>,
        worker_err: Option<String>,
    },
    ClipboardText(String),
    ClipboardEmpty,
}

pub(super) struct AttachIngestHandle {
    work_tx: mpsc::Sender<AttachIngestWork>,
    out_rx: mpsc::Receiver<AttachIngestOut>,
}

impl AttachTray {
    pub(super) fn clear(&mut self) {
        self.queue.clear();
        self.rows.clear();
    }

    pub(super) fn len(&self) -> usize {
        self.queue.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub(super) fn remove_at(&mut self, idx: usize) {
        if idx >= self.queue.len() {
            return;
        }
        let kept: Vec<(String, u64, AttachTrayRow)> = self
            .queue
            .items()
            .iter()
            .zip(self.rows.iter())
            .enumerate()
            .filter_map(|(i, (e, r))| {
                if i == idx {
                    None
                } else {
                    Some((e.path.clone(), e.size_bytes, r.clone()))
                }
            })
            .collect();
        self.queue.clear();
        self.rows.clear();
        for (path, size, row) in kept {
            let _ = self.queue.stage(path, size);
            self.rows.push(row);
        }
    }

    pub(super) fn apply_modalities(&mut self, mods: &[&str]) {
        self.queue.apply_model_input_modalities(mods);
    }
}

fn chat_input_modalities(cabin: &Cabin) -> Vec<String> {
    let model = resolve_chat_model(&cabin.cfg.mode, &cabin.cfg.model);
    let dir = crate::config::config_dir();
    let reg = load_registry(&dir);
    if let Some(m) = reg.models.get(&model) {
        if let Some(mods) = &m.meta.input_modalities {
            if !mods.is_empty() {
                return mods.clone();
            }
        }
    }
    vec!["text".into(), "image".into()]
}

fn modality_refs(mods: &[String]) -> Vec<&str> {
    mods.iter().map(String::as_str).collect()
}

pub(super) fn spawn_attach_ingest() -> AttachIngestHandle {
    let (work_tx, work_rx) = mpsc::channel();
    let (out_tx, out_rx) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(job) = work_rx.recv() {
            match job {
                AttachIngestWork::Paths(paths) => {
                    for path in paths {
                        if out_tx.send(process_path(&path)).is_err() {
                            return;
                        }
                    }
                }
                AttachIngestWork::ClipboardPaste => {
                    if let Some(paths) = clipboard_file_paths() {
                        for path in paths {
                            if out_tx.send(process_path(&path)).is_err() {
                                return;
                            }
                        }
                    } else if let Some(p) = crate::desktop::clipboard_image() {
                        let _ = out_tx.send(process_path(&p));
                    } else if let Some(text) = crate::desktop::clipboard_once() {
                        let _ = out_tx.send(AttachIngestOut::ClipboardText(text));
                    } else {
                        let _ = out_tx.send(AttachIngestOut::ClipboardEmpty);
                    }
                }
            }
        }
    });
    AttachIngestHandle { work_tx, out_rx }
}

fn process_path(path: &Path) -> AttachIngestOut {
    let path_s = path.display().to_string();
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let kind = attach_kind(&path_s);
    let (thumb, vision_url, worker_err) = match kind {
        AttachKind::Image => decode_image_row(path),
        _ => (None, None, None),
    };
    AttachIngestOut::Row {
        path: path_s,
        size_bytes,
        thumb,
        vision_url,
        worker_err,
    }
}

fn decode_image_row(path: &Path) -> (Option<ColorImage>, Option<String>, Option<String>) {
    let thumb = thumb_for_path(path);
    match crate::desktop::load_image_data_url(path) {
        Ok(url) => (thumb, Some(url), None),
        Err(e) => (thumb, None, Some(e)),
    }
}

fn thumb_for_path(path: &Path) -> Option<ColorImage> {
    let img = image::open(path).ok()?;
    let w = ((img.width() as f32) * 0.25).max(1.0) as u32;
    let h = ((img.height() as f32) * 0.25).max(1.0) as u32;
    let small = img.resize(w, h, image::imageops::FilterType::Triangle);
    let rgba = small.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

#[cfg(windows)]
fn clipboard_file_paths() -> Option<Vec<PathBuf>> {
    use clipboard_win::{formats::FileListReader, get_clipboard, formats::FileList};
    get_clipboard(FileList).and_then(|fl| {
        let list = FileListReader::new(fl);
        let mut out = Vec::new();
        for p in list.iter() {
            out.push(PathBuf::from(p));
        }
        if out.is_empty() { None } else { Some(out) }
    })
}

#[cfg(not(windows))]
fn clipboard_file_paths() -> Option<Vec<PathBuf>> {
    None
}


pub(super) fn pick_chat_files() -> mpsc::Receiver<Option<Vec<PathBuf>>> {
    let (tx, rx) = mpsc::channel();
    let run = move || {
        let picked = rfd::FileDialog::new()
            .set_title("Add to chat")
            .pick_files();
        let _ = tx.send(picked);
    };
    if cfg!(target_os = "macos") {
        run();
    } else {
        thread::spawn(run);
    }
    rx
}

impl Cabin {
    pub(super) fn poll_attach_pick(&mut self) {
        let Some(rx) = self.attach_pick_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(paths)) => {
                self.enqueue_attach_paths(paths);
            }
            Ok(None) => {}
            Err(mpsc::TryRecvError::Empty) => {
                self.attach_pick_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn start_chat_file_pick(&mut self) {
        if self.attach_pick_rx.is_some() {
            self.status = "Choose a file…".into();
            return;
        }
        self.attach_pick_rx = Some(pick_chat_files());
        self.status = "Choose a file…".into();
    }

    pub(super) fn ensure_attach_ingest(&mut self) {
        if self.attach_ingest.is_none() {
            self.attach_ingest = Some(spawn_attach_ingest());
        }
    }

    pub(super) fn enqueue_attach_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        self.ensure_attach_ingest();
        if let Some(handle) = self.attach_ingest.as_ref() {
            let _ = handle.work_tx.send(AttachIngestWork::Paths(paths));
        }
    }

    pub(super) fn request_composer_clipboard_attach(&mut self) {
        self.ensure_attach_ingest();
        if let Some(handle) = self.attach_ingest.as_ref() {
            let _ = handle.work_tx.send(AttachIngestWork::ClipboardPaste);
        }
    }

    pub(super) fn poll_attach_ingest(&mut self) {
        let batch: Vec<AttachIngestOut> = match self.attach_ingest.as_ref() {
            Some(handle) => handle.out_rx.try_iter().collect(),
            None => return,
        };
        for msg in batch {
            self.apply_ingest_out(msg);
        }
    }

    pub(super) fn apply_ingest_out(&mut self, msg: AttachIngestOut) {
        match msg {
            AttachIngestOut::ClipboardText(text) => {
                self.composer = append_composer(&self.composer, &text);
                self.status = "Pasted clipboard".into();
            }
            AttachIngestOut::ClipboardEmpty => {
                self.status = plus_empty_status().into();
            }
            AttachIngestOut::Row {
                path,
                size_bytes,
                thumb,
                vision_url,
                worker_err,
            } => {
                if self.attach_tray.len() >= PENDING_ATTACH_MAX {
                    self.status = pending_item_user_message(
                        PendingItemStatus::Invalid(PendingValidateIssue::TooMany),
                    )
                    .unwrap_or("too many attachments")
                    .into();
                    return;
                }
                match self.attach_tray.queue.stage(path.clone(), size_bytes) {
                    Ok(()) => {
                        let mods = chat_input_modalities(self);
                        self.attach_tray.apply_modalities(&modality_refs(&mods));
                        let row = AttachTrayRow {
                            preparing: false,
                            thumb,
                            vision_url,
                            worker_err,
                        };
                        self.attach_tray.rows.push(row);
                        self.sync_legacy_attach_from_tray();
                        if self.attach_tray.len() == 1 {
                            let name = attach_name(&path);
                            let kind = attach_kind(&path);
                            self.status = chat_attach_status(kind, &name);
                        }
                    }
                    Err(issue) => {
                        let msg = pending_item_user_message(
                            PendingItemStatus::Invalid(issue),
                        )
                        .unwrap_or("could not attach file");
                        self.status = msg.into();
                    }
                }
            }
        }
    }

    pub(super) fn stage_plus_ready_chat(&mut self, ready: PlusReady) {
        if self.attach_tray.len() >= PENDING_ATTACH_MAX {
            self.status = "too many attachments".into();
            return;
        }
        let size = std::fs::metadata(&ready.raw)
            .map(|m| m.len())
            .unwrap_or(0);
        if self.attach_tray.queue.stage(ready.raw.clone(), size).is_err() {
            self.status = "too many attachments".into();
            return;
        }
        let thumb = if ready.kind == AttachKind::Image {
            thumb_for_path(Path::new(&ready.raw))
        } else {
            None
        };
        self.attach_tray.rows.push(AttachTrayRow {
            preparing: false,
            thumb,
            vision_url: ready.image_url,
            worker_err: None,
        });
        let mods = chat_input_modalities(self);
        self.attach_tray.apply_modalities(&modality_refs(&mods));
        self.sync_legacy_attach_from_tray();
        self.status = chat_attach_status(ready.kind, &ready.name);
        self.file_pick = None;
    }

    pub(super) fn sync_legacy_attach_from_tray(&mut self) {
        self.attach_url = None;
        self.attach_name = None;
        self.attach_path = None;
        self.attach_kind = None;
        for (entry, row) in self.attach_tray.queue.items().iter().zip(self.attach_tray.rows.iter()) {
            let status = entry.status;
            if matches!(
                status,
                PendingItemStatus::Valid(_) | PendingItemStatus::NeedsTranscode
            ) {
                if row.vision_url.is_some() && self.attach_url.is_none() {
                    self.attach_url = row.vision_url.clone();
                    self.attach_name = Some(attach_name(&entry.path));
                    self.attach_path = Some(entry.path.clone());
                    self.attach_kind = Some(AttachKind::Image);
                } else if self.attach_name.is_none() {
                    self.attach_name = Some(attach_name(&entry.path));
                    self.attach_path = Some(entry.path.clone());
                    self.attach_kind = Some(attach_kind(&entry.path));
                }
            }
        }
    }

    pub(super) fn clear_chat_attach_tray(&mut self) {
        self.attach_tray.clear();
        self.attach_url = None;
        self.attach_name = None;
        self.attach_path = None;
        self.attach_kind = None;
        self.status.clear();
    }

    pub(super) fn append_pending_attach_lines(&mut self, text: &mut String) {
        for (entry, row) in self
            .attach_tray
            .queue
            .items()
            .iter()
            .zip(self.attach_tray.rows.iter())
        {
            if row.worker_err.is_some() {
                continue;
            }
            let name = attach_name(&entry.path);
            let kind = attach_kind(&entry.path);
            match entry.status {
                PendingItemStatus::Valid(_) | PendingItemStatus::NeedsTranscode => {
                    let line = if kind == AttachKind::Image && row.vision_url.is_some() {
                        attach_prompt_line(AttachKind::Image, &name)
                    } else if kind != AttachKind::Image {
                        attach_send_line(kind, &name, &entry.path)
                    } else {
                        continue;
                    };
                    *text = append_composer(text, &line);
                }
                _ => {}
            }
        }
    }

    pub(super) fn first_pending_vision_url(&mut self) -> Option<String> {
        for (entry, row) in self
            .attach_tray
            .queue
            .items()
            .iter()
            .zip(self.attach_tray.rows.iter())
        {
            if matches!(
                entry.status,
                PendingItemStatus::Valid(_) | PendingItemStatus::NeedsTranscode
            ) {
                if let Some(url) = row.vision_url.clone() {
                    return Some(url);
                }
            }
        }
        None
    }

    pub(super) fn consume_pending_attach_on_send(&mut self) {
        self.attach_tray.clear();
        self.attach_url = None;
        self.attach_name = None;
        self.attach_path = None;
        self.attach_kind = None;
    }

    pub(super) fn try_composer_attach_paste(&mut self, ui: &egui::Ui, composer_id: egui::Id) -> bool {
        if !ui.memory(|m| m.has_focus(composer_id)) {
            return false;
        }
        let shortcut = egui::KeyboardShortcut::new(
            if cfg!(target_os = "macos") {
                egui::Modifiers::COMMAND
            } else {
                egui::Modifiers::CTRL
            },
            egui::Key::V,
        );
        let consumed = ui.input_mut(|i| i.consume_shortcut(&shortcut));
        if !consumed {
            return false;
        }
        ui.input_mut(|i| {
            i.events.retain(|ev| !matches!(ev, egui::Event::Paste(_)));
        });
        self.request_composer_clipboard_attach();
        true
    }

    pub(super) fn ui_attach_tray(&mut self, ui: &mut egui::Ui) {
        if self.attach_tray.is_empty() {
            return;
        }
        let mods = chat_input_modalities(self);
        self.attach_tray.apply_modalities(&modality_refs(&mods));
        let ctx = ui.ctx().clone();
        let thumbs: Vec<(usize, ColorImage)> = self
            .attach_tray
            .rows
            .iter()
            .enumerate()
            .filter_map(|(idx, row)| row.thumb.clone().map(|img| (idx, img)))
            .collect();
        for (idx, img) in thumbs {
            let _ = self.attach_thumb_texture(&ctx, idx, &img);
        }
        let mut paint = Vec::new();
        for (idx, (entry, row)) in self
            .attach_tray
            .queue
            .items()
            .iter()
            .zip(self.attach_tray.rows.iter())
            .enumerate()
        {
            let err = row
                .worker_err
                .clone()
                .or_else(|| {
                    pending_item_user_message(entry.status).map(str::to_string)
                });
            paint.push((
                idx,
                attach_name(&entry.path),
                attach_kind(&entry.path),
                row.preparing,
                err,
                self.attach_thumb_cache.get(idx).map(|t| t.id()),
            ));
        }
        ui.add_space(4.0);
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for (idx, name, kind, preparing, err, tex_id) in paint {
                let frame = egui::Frame::NONE
                    .fill(crate::theme::surface())
                    .corner_radius(8.0)
                    .stroke(egui::Stroke::new(1.0, crate::theme::border()))
                    .inner_margin(egui::Margin::same(6));
                frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(id) = tex_id {
                            ui.image((id, egui::vec2(40.0, 40.0)));
                        }
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(name)
                                    .size(12.0)
                                    .color(crate::theme::fg()),
                            );
                            let meta = if preparing {
                                "Preparing…"
                            } else if let Some(e) = err.as_deref() {
                                e
                            } else {
                                match kind {
                                    AttachKind::Image => "Image",
                                    AttachKind::Doc => "Document",
                                    AttachKind::Text => "File",
                                    _ => "Attachment",
                                }
                            };
                            let color = if err.is_some() {
                                crate::theme::offline()
                            } else {
                                crate::theme::muted()
                            };
                            ui.label(RichText::new(meta).size(11.0).color(color));
                        });
                        if ui.small_button("×").clicked() {
                            remove = Some(idx);
                        }
                    });
                });
            }
        });
        if let Some(idx) = remove {
            self.attach_tray.remove_at(idx);
            self.sync_legacy_attach_from_tray();
        }
    }

    fn attach_thumb_texture(
        &mut self,
        ctx: &egui::Context,
        idx: usize,
        img: &ColorImage,
    ) -> TextureHandle {
        if let Some(tex) = self.attach_thumb_cache.get(idx) {
            return tex.clone();
        }
        let tex = ctx.load_texture(
            format!("attach-thumb-{idx}"),
            img.clone(),
            TextureOptions::LINEAR,
        );
        if self.attach_thumb_cache.len() <= idx {
            self.attach_thumb_cache.resize(idx + 1, tex.clone());
        } else {
            self.attach_thumb_cache[idx] = tex.clone();
        }
        tex
    }
}

pub(super) fn drop_hint_line(imagine: bool, files: usize) -> String {
    let what = if imagine {
        "Drop to use as the Imagine reference"
    } else {
        "Drop to attach to your next message"
    };
    if files > 1 {
        format!("{what} — {files} files")
    } else {
        what.to_string()
    }
}

pub(super) fn drop_extra_note(extra: usize) -> Option<String> {
    if extra == 0 {
        return None;
    }
    Some(format!(
        "Attached {extra} more file(s) — up to {PENDING_ATTACH_MAX} per message"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::attach::{
        MODEL_CANT_READ_FILE, PendingFileRole, PendingItemStatus,
    };

    #[test]
    fn paste_routing_text_stays_text() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.apply_ingest_out(AttachIngestOut::ClipboardText("/tmp/notes.md".into()));
        assert_eq!(cabin.composer, "/tmp/notes.md");
        assert!(cabin.attach_tray.is_empty());
        assert!(!cabin.running);
    }

    #[test]
    fn paste_never_sends() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.composer = "hello".into();
        cabin.apply_ingest_out(AttachIngestOut::ClipboardText("more".into()));
        assert_eq!(cabin.composer, "hello\nmore");
        assert!(!cabin.running);
        assert!(cabin.chat_job_thread.is_none());
    }

    #[test]
    fn cap_ten_blocks_eleventh() {
        let mut cabin = Cabin::quiet_for_test();
        for i in 0..PENDING_ATTACH_MAX {
            assert!(
                cabin
                    .attach_tray
                    .queue
                    .stage(format!("/tmp/f{i}.txt"), 1)
                    .is_ok()
            );
            cabin.attach_tray.rows.push(AttachTrayRow::default());
        }
        cabin.apply_ingest_out(AttachIngestOut::Row {
            path: "/tmp/overflow.txt".into(),
            size_bytes: 1,
            thumb: None,
            vision_url: None,
            worker_err: None,
        });
        assert_eq!(cabin.attach_tray.len(), PENDING_ATTACH_MAX);
        assert_eq!(cabin.status, "too many attachments");
    }

    #[test]
    fn remove_drops_exactly_one() {
        let mut tray = AttachTray::default();
        for i in 0..3 {
            tray.queue.stage(format!("/tmp/a{i}.png"), 1).unwrap();
            tray.rows.push(AttachTrayRow::default());
        }
        tray.remove_at(1);
        assert_eq!(tray.len(), 2);
        assert_eq!(attach_name(&tray.queue.items()[0].path), "a0.png");
        assert_eq!(attach_name(&tray.queue.items()[1].path), "a2.png");
    }

    #[test]
    fn unsupported_model_shows_message() {
        let mut tray = AttachTray::default();
        tray.queue.stage("shot.png".into(), 100).unwrap();
        tray.rows.push(AttachTrayRow {
            vision_url: Some("data:image/png;base64,YQ==".into()),
            ..AttachTrayRow::default()
        });
        tray.apply_modalities(&["text"]);
        assert_eq!(
            tray.queue.items()[0].status,
            PendingItemStatus::ModelCantRead
        );
        assert_eq!(
            pending_item_user_message(tray.queue.items()[0].status),
            Some(MODEL_CANT_READ_FILE)
        );
    }

    #[test]
    fn ingest_worker_results_are_polled_not_applied_on_enqueue() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.ensure_attach_ingest();
        let path = std::env::temp_dir().join("grokhub-ingest-poll.txt");
        std::fs::write(&path, b"poll me").expect("temp file");
        cabin.enqueue_attach_paths(vec![path.clone()]);
        assert!(cabin.attach_tray.is_empty());
        for _ in 0..200 {
            cabin.poll_attach_ingest();
            if !cabin.attach_tray.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            cabin.attach_tray.len() == 1,
            "UI must pick up worker output via poll_attach_ingest"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn invalid_file_survives_siblings() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.apply_ingest_out(AttachIngestOut::Row {
            path: "/tmp/good.txt".into(),
            size_bytes: 10,
            thumb: None,
            vision_url: None,
            worker_err: None,
        });
        cabin.apply_ingest_out(AttachIngestOut::Row {
            path: "/tmp/bad.bin".into(),
            size_bytes: 10,
            thumb: None,
            vision_url: None,
            worker_err: None,
        });
        assert_eq!(cabin.attach_tray.len(), 2);
        assert_eq!(
            cabin.attach_tray.queue.items()[0].status,
            PendingItemStatus::Valid(PendingFileRole::Document)
        );
        assert!(matches!(
            cabin.attach_tray.queue.items()[1].status,
            PendingItemStatus::Invalid(_)
        ));
    }
}
