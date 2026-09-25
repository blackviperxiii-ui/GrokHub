//! Plus menu, file pick, and attach chips.

use super::*;


pub(super) enum PlusPick {
    NativeMiss,
    ClipEmpty,
    ClipText(String),
    Ready(PlusReady),
    Err(String),
}


pub(super) struct PlusReady {
    pub(super) kind: AttachKind,
    pub(super) name: String,
    pub(super) raw: String,
    pub(super) image_url: Option<String>,
    pub(super) text: Option<String>,
}


pub(super) fn plus_from_path(target: PlusTarget, path: PathBuf) -> PlusPick {
    let raw = path.display().to_string();
    let kind = attach_kind(&raw);
    let name = attach_name(&raw);
    match kind {
        AttachKind::Image if target == PlusTarget::Chat => match load_image_data_url(&path) {
            Ok(url) => PlusPick::Ready(PlusReady {
                kind,
                name,
                raw,
                image_url: Some(url),
                text: None,
            }),
            Err(e) => PlusPick::Err(e),
        },
        AttachKind::Text => match read_text_capped(&path) {
            Ok(t) => PlusPick::Ready(PlusReady {
                kind,
                name,
                raw,
                image_url: None,
                text: Some(t),
            }),
            Err(e) => PlusPick::Err(e),
        },
        _ => PlusPick::Ready(PlusReady {
            kind,
            name,
            raw,
            image_url: None,
            text: None,
        }),
    }
}

impl Cabin {

    pub(super) fn open_plus(&mut self, target: PlusTarget, anchor: egui::Pos2) {
        self.plus_menu = Some(target);
        self.plus_anchor = anchor;
        self.plus_ignore_close = true;
        self.file_pick = None;
    }

    pub(super) fn run_plus_act(&mut self, target: PlusTarget, act: PlusAct) {
        match act {
            PlusAct::Upload => {
                if self.pick_rx.is_some() {
                    self.status = "Choose a file…".into();
                    return;
                }
                let (tx, rx) = mpsc::channel();
                self.pick_rx = Some(rx);
                self.status = "Choose a file…".into();
                std::thread::spawn(move || {
                    let out = match pick_file() {
                        Some(p) => plus_from_path(target, p),
                        None => PlusPick::NativeMiss,
                    };
                    let _ = tx.send((target, out));
                });
            }
            PlusAct::Paste => {
                if self.pick_rx.is_some() {
                    self.status = "Reading clipboard…".into();
                    return;
                }
                let (tx, rx) = mpsc::channel();
                self.pick_rx = Some(rx);
                self.status = "Reading clipboard…".into();
                std::thread::spawn(move || {
                    let out = if let Some(p) = clipboard_image() {
                        plus_from_path(target, p)
                    } else if let Some(t) = crate::desktop::clipboard_once() {
                        PlusPick::ClipText(t)
                    } else {
                        PlusPick::ClipEmpty
                    };
                    let _ = tx.send((target, out));
                });
            }
        }
    }

    pub(super) fn poll_pick(&mut self) {
        let Some(rx) = self.pick_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((target, PlusPick::Ready(ready))) => {
                self.apply_plus_ready(target, ready);
            }
            Ok((target, PlusPick::NativeMiss)) => {
                self.file_pick = Some(target);
                if self.status == "Choose a file…" {
                    self.status.clear();
                }
            }
            Ok((target, PlusPick::ClipText(clip))) => {
                self.apply_clipboard(target, &clip);
            }
            Ok((_, PlusPick::ClipEmpty)) => {
                self.status = plus_empty_status().into();
            }
            Ok((_, PlusPick::Err(e))) => {
                self.status = e;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.pick_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                if self.status == "Choose a file…"
                    || self.status == "Reading clipboard…"
                    || self.status == "Reading file…"
                {
                    self.status.clear();
                }
            }
        }
    }

    pub(super) fn apply_clipboard(&mut self, target: PlusTarget, clip: &str) {
        match target {
            PlusTarget::Chat => {
                self.composer = append_composer(&self.composer, clip);
                self.status = "Pasted clipboard".into();
            }
            PlusTarget::Imagine => {
                self.imagine_prompt = append_composer(&self.imagine_prompt, clip);
                self.status = "Pasted clipboard".into();
            }
        }
    }

    pub(super) fn apply_plus_ready(&mut self, target: PlusTarget, ready: PlusReady) {
        match target {
            PlusTarget::Chat => match ready.kind {
                AttachKind::Image => {
                    if let Some(url) = ready.image_url {
                        self.attach_url = Some(url);
                        self.attach_name = Some(ready.name.clone());
                        self.status = chat_attach_status(ready.kind, &ready.name);
                    }
                }
                AttachKind::Text => {
                    if let Some(t) = ready.text {
                        self.composer = append_composer(&self.composer, &t);
                        self.status = chat_attach_status(ready.kind, &ready.name);
                    }
                }
                AttachKind::Other => {
                    self.composer = append_composer(&self.composer, &ready.raw);
                    self.status = chat_attach_status(ready.kind, &ready.name);
                }
            },
            PlusTarget::Imagine => match ready.kind {
                AttachKind::Image => {
                    self.imagine_ref = Some(ready.name.clone());
                    let hint = attach_prompt_line(ready.kind, &ready.name);
                    self.imagine_prompt = append_composer(&self.imagine_prompt, &hint);
                    self.status = imagine_ref_status(&ready.name);
                }
                AttachKind::Text => {
                    if let Some(t) = ready.text {
                        self.imagine_prompt = append_composer(&self.imagine_prompt, &t);
                        self.status = chat_attach_status(ready.kind, &ready.name);
                    }
                }
                AttachKind::Other => {
                    self.imagine_prompt = append_composer(&self.imagine_prompt, &ready.raw);
                    self.status = chat_attach_status(ready.kind, &ready.name);
                }
            },
        }
        self.file_pick = None;
    }

    pub(super) fn start_plus_path(&mut self, target: PlusTarget, path: PathBuf) {
        if self.pick_rx.is_some() {
            self.status = "Reading file…".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.pick_rx = Some(rx);
        self.status = "Reading file…".into();
        std::thread::spawn(move || {
            let _ = tx.send((target, plus_from_path(target, path)));
        });
    }

    pub(super) fn clear_chat_attach(&mut self) {
        self.attach_url = None;
        self.attach_name = None;
        self.status.clear();
    }

    pub(super) fn drop_leaving_thread_chrome(&mut self) {
        // Switching chats is not Stop. A live reply stays on its chat.
        // A hidden night/inbox/workboard job keeps running. Cabin-wide work
        // with no thread still halts so the next tab is not Thinking.
        let halt = self.running && self.leave_should_halt();
        if halt {
            self.halt_in_flight();
        }
        let keep_run = self.running && !halt;
        // grok -p, including the hidden background thread, is not the ACP
        // session. Drop a stale handle without SIGTERM of that process.
        let keep_acp = keep_run && self.grok_p_rx.is_none() && !self.job_on_background_thread();
        self.attach_url = None;
        self.attach_name = None;
        self.followup_step = 0;
        self.active_skill_follow = None;
        self.hands_attach = false;
        self.eyes_attach = false;
        self.last_receipt_ok = None;
        if !keep_acp {
            self.acp = None;
            self.acp_spawn_rx = None;
            self.perm_ask = None;
            self.perm_always_confirm = None;
            self.confirm = None;
            self.elicit_ask = None;
            self.elicit_draft.clear();
        }
        self.tool_cards.clear();
        self.live_blocks.clear();
        if keep_run && !self.chrome_here() && is_thinking_status(&self.status) {
            self.status.clear();
        }
    }

    pub(super) fn pick_entries(dir: &Path) -> Vec<(String, bool)> {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if dirs.len() + files.len() >= 400 {
                    break;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name.is_empty() {
                    continue;
                }
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    dirs.push(name);
                } else {
                    files.push(name);
                }
            }
        }
        dirs.sort();
        files.sort();
        let mut out = Vec::new();
        for d in dirs {
            out.push((d, true));
        }
        for f in files {
            out.push((f, false));
        }
        out
    }

    pub(super) fn cached_pick_entries(&mut self) -> &[(String, bool)] {
        let dir = self.pick_dir.clone();
        let stale = self
            .pick_cache
            .as_ref()
            .map(|(cached, _)| cached != &dir)
            .unwrap_or(true);
        if stale && self.pick_list_rx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.pick_list_rx = Some(rx);
            std::thread::spawn(move || {
                let entries = Self::pick_entries(Path::new(&dir));
                let _ = tx.send((dir, entries));
            });
        }
        if self
            .pick_cache
            .as_ref()
            .map(|(cached, _)| cached == &self.pick_dir)
            .unwrap_or(false)
        {
            return self
                .pick_cache
                .as_ref()
                .map(|(_, entries)| entries.as_slice())
                .unwrap_or(&[]);
        }
        &[]
    }

    pub(super) fn poll_pick_list(&mut self) {
        let Some(rx) = self.pick_list_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((dir, entries)) => {
                if dir == self.pick_dir {
                    self.pick_cache = Some((dir, entries));
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.pick_list_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn ui_plus_overlays(&mut self, ctx: &egui::Context) {
        if let Some(target) = self.plus_menu {
            let mut picked = None;
            let mut menu_rect = egui::Rect::NOTHING;
            egui::Area::new(egui::Id::new("plus-menu"))
                .fixed_pos(self.plus_anchor + egui::vec2(0.0, 6.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(168.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (label, act) in plus_menu_rows() {
                            if ui.selectable_label(false, *label).clicked() {
                                picked = Some(*act);
                            }
                        }
                        menu_rect = ui.min_rect();
                    });
                });
            if let Some(act) = picked {
                self.plus_menu = None;
                self.run_plus_act(target, act);
            } else if self.plus_ignore_close {
                self.plus_ignore_close = false;
            } else if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.pointer_interact_pos() {
                    if !menu_rect.expand(8.0).contains(pos) {
                        self.plus_menu = None;
                    }
                }
            }
        }
        if let Some(target) = self.file_pick {
            let mut picked: Option<PathBuf> = None;
            let mut up = false;
            let mut cancel = false;
            let mut paste = false;
            let dir = PathBuf::from(&self.pick_dir);
            let entries = self.cached_pick_entries().to_vec();
            egui::Window::new("Upload")
                .collapsible(false)
                .resizable(true)
                .default_width(420.0)
                .default_height(360.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(dir.display().to_string())
                            .size(12.0)
                            .color(crate::theme::muted()),
                    );
                    ui.horizontal(|ui| {
                        if crate::cards::ghost_pill(ui, "Up") {
                            up = true;
                        }
                        if crate::cards::ghost_pill(ui, "Home") {
                            if let Ok(home) = std::env::var("HOME") {
                                self.pick_dir = home;
                            }
                        }
                        if crate::cards::ghost_pill(ui, "Paste clipboard") {
                            paste = true;
                        }
                        if crate::cards::ghost_pill(ui, "Cancel") {
                            cancel = true;
                        }
                    });
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (name, is_dir) in &entries {
                                let icon = if *is_dir {
                                    crate::icons::RailIcon::Folder
                                } else {
                                    crate::icons::RailIcon::File
                                };
                                let row = ui
                                    .horizontal(|ui| {
                                        crate::icons::paint_rail_icon(
                                            ui,
                                            icon,
                                            16.0,
                                            crate::theme::muted(),
                                        );
                                        ui.selectable_label(false, name)
                                    })
                                    .inner;
                                if row.clicked() {
                                    let next = dir.join(name);
                                    if *is_dir {
                                        self.pick_dir = next.display().to_string();
                                    } else {
                                        picked = Some(next);
                                    }
                                }
                            }
                        });
                });
            if up {
                if let Some(parent) = dir.parent() {
                    self.pick_dir = parent.display().to_string();
                }
            }
            if let Some(p) = picked {
                self.start_plus_path(target, p);
            } else if paste {
                self.file_pick = None;
                self.run_plus_act(target, PlusAct::Paste);
            } else if cancel {
                self.file_pick = None;
            }
        }
    }

    pub(super) fn ui_imagine_overlays(&mut self, ctx: &egui::Context) {
        if self.page_nav() != Nav::Imagine {
            self.imagine_style_open = false;
            self.imagine_aspect_open = false;
            return;
        }
        let mut menu_rect = egui::Rect::NOTHING;
        let mut trigger = egui::Rect::NOTHING;
        if self.imagine_style_open {
            let rows: Vec<(String, bool)> = IMAGINE_STYLES
                .iter()
                .enumerate()
                .map(|(i, label)| ((*label).to_string(), self.imagine_style == i as u8))
                .collect();
            let (picked, rect) =
                imagine_popup(ctx, "imagine_style_menu", self.imagine_style_anchor, &rows);
            menu_rect = rect;
            trigger = self.imagine_style_anchor;
            if let Some(i) = picked {
                self.imagine_style = i as u8;
                self.imagine_style_open = false;
            }
        } else if self.imagine_aspect_open {
            let rows: Vec<(String, bool)> = IMAGINE_ASPECTS
                .iter()
                .enumerate()
                .map(|(i, (ratio, name))| {
                    (format!("{ratio}  {name}"), self.imagine_aspect == i as u8)
                })
                .collect();
            let (picked, rect) = imagine_popup(
                ctx,
                "imagine_aspect_menu",
                self.imagine_aspect_anchor,
                &rows,
            );
            menu_rect = rect;
            trigger = self.imagine_aspect_anchor;
            if let Some(i) = picked {
                self.imagine_aspect = i as u8;
                self.imagine_aspect_open = false;
            }
        }
        let outside = ctx.input(|i| i.pointer.any_click())
            && ctx.pointer_interact_pos().is_some_and(|pos| {
                !menu_rect.expand(8.0).contains(pos) && !trigger.expand(4.0).contains(pos)
            });
        if cabin_menu_should_dismiss(self.imagine_menu_ignore, outside) {
            self.imagine_style_open = false;
            self.imagine_aspect_open = false;
        }
        self.imagine_menu_ignore = false;
    }

    pub(super) fn ui_attach_chip(&mut self, ui: &mut egui::Ui, target: PlusTarget) {
        match target {
            PlusTarget::Chat => {
                if let Some(name) = self.attach_name.clone() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Attached {name}"))
                                .size(12.0)
                                .color(crate::theme::fg()),
                        );
                        if ui.small_button("×").clicked() {
                            self.clear_chat_attach();
                        }
                    });
                }
            }
            PlusTarget::Imagine => {
                if let Some(name) = self.imagine_ref.clone() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Reference {name}"))
                                .size(12.0)
                                .color(crate::theme::fg()),
                        );
                        if ui.small_button("×").clicked() {
                            self.imagine_ref = None;
                        }
                    });
                }
            }
        }
    }
}
