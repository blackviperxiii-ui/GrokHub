//! Imagine generate, wall, and toolbox.

use super::*;


#[derive(Default)]
pub(super) struct ImagineBarOut {
    generate: bool,
    stop: bool,
    go_settings: bool,
}

enum ImaginePickKind {
    Sources,
    Mask,
    VideoStill,
}

pub(super) struct ImaginePickOut {
    kind: ImaginePickKind,
    uris: Result<Vec<String>, String>,
}

pub(super) enum ImagineCall {
    Generate {
        model: String,
        n: u32,
        resolution: String,
        aspect: String,
        quality: String,
    },
    Edit {
        model: String,
        n: u32,
        resolution: String,
        aspect: String,
        sources: Vec<String>,
        mask: Option<String>,
    },
    Video {
        op: grokhub_core::ImagineVideoOp,
        model: String,
        duration: u32,
        resolution: String,
        aspect: String,
        audio: bool,
        image_url: String,
        video_url: String,
    },
}

/// Extra Imagine controls and the Grok sign-in used only by this page.
pub(super) struct ImagineNative {
    pub tokens: Option<grokhub_core::ImagineTokens>,
    pub email: String,
    pub results: Vec<String>,
    pub note: String,
    pub image_op: u8,
    pub image_model: u8,
    pub count: u8,
    pub edit_sources: Vec<String>,
    pub edit_mask: Option<String>,
    pub video_op: u8,
    pub video_model: u8,
    pub video_res_1080: bool,
    pub video_image: Option<String>,
    pub video_url: String,
    pub more_open: bool,
    pub more_anchor: egui::Rect,
    pub offer_key: bool,
    pub offer_settings: bool,
    pub force_key: bool,
    pub cred_oauth: bool,
    pub auth_busy: bool,
    pub device_user_code: String,
    pub device_verify_uri: String,
    pub device_code: String,
    pub device_interval: u64,
    pub device_next: Option<Instant>,
    pub poll_inflight: bool,
    pub auth_rx: Option<mpsc::Receiver<crate::imagine_auth::ImagineAuthEvent>>,
    pub poll_rx: Option<mpsc::Receiver<Result<crate::imagine_auth::DevicePoll, String>>>,
    pub pick_rx: Option<mpsc::Receiver<ImaginePickOut>>,
    pub item_save_rx: Option<mpsc::Receiver<Result<String, String>>>,
}

impl Default for ImagineNative {
    fn default() -> Self {
        Self {
            tokens: None,
            email: String::new(),
            results: Vec::new(),
            note: String::new(),
            image_op: 0,
            image_model: 0,
            count: 1,
            edit_sources: Vec::new(),
            edit_mask: None,
            video_op: 0,
            video_model: 0,
            video_res_1080: false,
            video_image: None,
            video_url: String::new(),
            more_open: false,
            more_anchor: egui::Rect::NOTHING,
            offer_key: false,
            offer_settings: false,
            force_key: false,
            cred_oauth: false,
            auth_busy: false,
            device_user_code: String::new(),
            device_verify_uri: String::new(),
            device_code: String::new(),
            device_interval: 5,
            device_next: None,
            poll_inflight: false,
            auth_rx: None,
            poll_rx: None,
            pick_rx: None,
            item_save_rx: None,
        }
    }
}


pub(super) fn imagine_popup(
    ctx: &egui::Context,
    id: &'static str,
    anchor: egui::Rect,
    rows: &[(String, bool)],
) -> (Option<usize>, egui::Rect) {
    let mut picked = None;
    let mut menu_rect = egui::Rect::NOTHING;
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(anchor.left_bottom() + egui::vec2(0.0, 6.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(anchor.width().max(168.0));
                ui.spacing_mut().item_spacing.y = 2.0;
                for (i, (label, on)) in rows.iter().enumerate() {
                    if ui.selectable_label(*on, label).clicked() {
                        picked = Some(i);
                    }
                }
                menu_rect = ui.min_rect();
            });
        });
    (picked, menu_rect)
}


pub(super) fn paint_wall_cover(
    key: &str,
    model: &str,
    id: &str,
    dir: &std::path::Path,
    title: &str,
    prompt: &str,
    prompt_b: &str,
    tall: bool,
    created_ms: u64,
) -> Result<WallGif, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let aspect = if tall { Some("2:3") } else { Some("16:9") };
    let src_a = grok_imagine_opts(key, model, prompt, aspect, Some("1k"))?;
    let path_a = dir.join(format!("{id}_a.png"));
    std::fs::copy(&src_a, &path_a).map_err(|e| e.to_string())?;
    let path_b = dir.join(format!("{id}_b.png"));
    match grok_imagine_opts(key, model, prompt_b, aspect, Some("1k")) {
        Ok(src_b) => {
            if std::fs::copy(&src_b, &path_b).is_err() {
                crate::desktop::sibling_still(&path_a, &path_b)?;
            }
        }
        Err(_) => crate::desktop::sibling_still(&path_a, &path_b)?,
    }
    if !path_b.exists() {
        crate::desktop::sibling_still(&path_a, &path_b)?;
    }
    Ok(WallGif {
        id: id.into(),
        title: title.into(),
        prompt: prompt.into(),
        created_ms,
        path_a: path_a.display().to_string(),
        path_b: path_b.display().to_string(),
        tall,
    })
}

impl Cabin {

    pub(super) fn kick_imagine(&mut self) {
        let prompt = self.imagine_prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        if self.running {
            self.status = "Halt the live job before Imagine, or wait.".into();
            return;
        }
        let kind = self.imagine_kind;
        let cred = if self.imagine_native.force_key {
            self.imagine_native.force_key = false;
            let key = self.console_key();
            let key = key.trim();
            if key.is_empty() {
                self.imagine_error = grokhub_core::IMAGINE_NEED_SIGNIN.into();
                self.status = self.imagine_error.clone();
                return;
            }
            grokhub_core::ImagineCred {
                secret: key.to_string(),
                kind: grokhub_core::ImagineCredKind::ConsoleKey,
            }
        } else {
            match self.imagine_cred() {
                Ok(cred) => cred,
                Err(msg) => {
                    self.imagine_error = msg.into();
                    self.status = self.imagine_error.clone();
                    self.imagine_native.offer_key = false;
                    self.imagine_native.offer_settings = false;
                    return;
                }
            }
        };
        if let Some(msg) = self.imagine_call_block() {
            self.imagine_error = msg.into();
            self.status = self.imagine_error.clone();
            return;
        }
        self.imagine_native.cred_oauth = cred.kind == grokhub_core::ImagineCredKind::OAuth;
        self.imagine_native.offer_key = false;
        self.imagine_native.offer_settings = false;
        let cred_oauth = self.imagine_native.cred_oauth;
        let job_tokens = if cred_oauth {
            self.imagine_native.tokens.clone()
        } else {
            None
        };
        let call = self.imagine_call();
        self.engine_note("imagine", "generated", "They generate images.");
        let aspect = imagine_aspect_label(self.imagine_aspect).to_string();
        let video_res =
            imagine_video_resolution(imagine_video_res_label(self.imagine_video_res)).to_string();
        let prompt = compose_imagine_prompt(&ImagineSpec {
            prompt: &prompt,
            kind,
            quality: self.imagine_quality,
            style: imagine_style_label(self.imagine_style),
            aspect: &aspect,
            video_res: &video_res,
            video_dur: imagine_video_dur_label(self.imagine_video_dur),
            video_audio: self.imagine_video_audio,
        });
        self.running = true;
        self.imagine_pending = true;
        self.imagine_error.clear();
        self.imagine_job_prompt = prompt.clone();
        self.imagine_expand = false;
        self.roll_today();
        bump_usage(&mut self.usage, "imagine");
        self.persist_usage();
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        self.status = match kind {
            ImagineKind::Image => "Imagining…".into(),
            ImagineKind::Video => "Imagining video…".into(),
            ImagineKind::Agent => "Imagining agent still…".into(),
        };
        let key = cred.secret;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let (key, rotated) = if cred_oauth {
                if let Some(tokens) = job_tokens {
                    match crate::imagine_auth::access_for_job(&tokens) {
                        Ok(pair) => pair,
                        Err(e) => {
                            let _ = tx.send(JobOut::Err(e));
                            return;
                        }
                    }
                } else {
                    (key, None)
                }
            } else {
                (key, None)
            };
            let sent = match run_imagine_call(&key, &prompt, call) {
                Ok((urls, note)) if !urls.is_empty() => JobOut::ImagineBatch(urls, note, rotated),
                Ok(_) => JobOut::Err("empty Imagine reply".into()),
                Err(e) => JobOut::Err(e),
            };
            let _ = tx.send(sent);
        });
    }

    pub(super) fn pin_generation_to_wall(&mut self, path: &str, prompt: &str) {
        let path = path.trim();
        if path.is_empty() {
            return;
        }
        let aspect = imagine_aspect_label(self.imagine_aspect);
        let mut gif = wall_gif_from_generation(path, prompt, now_ms(), aspect);
        let dir = config::wall_dir();
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or(if grokhub_core::imagine_is_video_path(path) {
                "mp4"
            } else {
                "png"
            });
        let dest_a = dir.join(format!("{}_a.{ext}", gif.id));
        let dest_b = dir.join(format!("{}_b.{ext}", gif.id));
        let _ = std::fs::create_dir_all(&dir);
        if std::fs::copy(path, &dest_a).is_ok() {
            gif.path_a = dest_a.display().to_string();
            if grokhub_core::imagine_is_video_path(path) || std::fs::copy(path, &dest_b).is_err() {
                gif.path_b = gif.path_a.clone();
            } else {
                gif.path_b = dest_b.display().to_string();
            }
        }
        if self
            .wall
            .gifs
            .iter()
            .any(|g| g.path_a == gif.path_a || g.id == gif.id)
        {
            return;
        }
        self.wall.gifs.push(gif);
        let (kept, evicted) = wall_evict(std::mem::take(&mut self.wall.gifs), WALL_GIF_MAX);
        self.wall.gifs = kept;
        self.wall.last_ms = now_ms();
        let wall = self.wall.clone();
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_wall(&wall);
            }
            for old in evicted {
                let _ = std::fs::remove_file(&old.path_a);
                if old.path_b != old.path_a {
                    let _ = std::fs::remove_file(&old.path_b);
                }
            }
        });
    }

    pub(super) fn play_imagine_media(&self, path: &str) {
        let path = path.trim().to_string();
        if path.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            let _ = crate::desktop::play_media(&path);
        });
    }

    pub(super) fn start_imagine_save(&mut self) {
        let src = self.imagine_last.trim().to_string();
        if src.is_empty() || self.imagine_save_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.imagine_save_rx = Some(rx);
        self.status = "Saving…".into();
        std::thread::spawn(move || {
            let out = match crate::desktop::save_file_dialog(&src) {
                None => Err("Save canceled".into()),
                Some(dest) => {
                    if let Some(dir) = dest.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    std::fs::copy(&src, &dest)
                        .map(|_| dest.display().to_string())
                        .map_err(|e| e.to_string())
                }
            };
            let _ = tx.send(out);
        });
    }

    pub(super) fn poll_imagine_save(&mut self) {
        let Some(rx) = self.imagine_save_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(path)) => self.status = format!("Saved {path}"),
            Ok(Err(e)) => self.status = e,
            Err(mpsc::TryRecvError::Empty) => self.imagine_save_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn ui_imagine(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut generate = false;
        let mut stop = false;
        let mut new_project = false;
        let mut go_settings = false;
        let mut seed: Option<String> = None;
        let word = crate::cards::imagine_word(now_ms());
        let selected = self.imagine_prompt.clone();
        let last = self.imagine_last.clone();
        let imagine_err = self.imagine_error.clone();
        let working = self.running && self.page_nav() == Nav::Imagine;
        let dock = imagine_toolbox_dock(
            !self.imagine_prompt.trim().is_empty(),
            !last.is_empty() || !imagine_err.is_empty(),
            working,
        );
        let stage_on = imagine_stage_visible(working, !last.is_empty()) || !imagine_err.is_empty();
        let video = self.imagine_kind == ImagineKind::Video;
        let aspect = imagine_aspect_label(self.imagine_aspect).to_string();
        let composer_id = egui::Id::new("imagine-composer");
        let cap = if imagine_toolbox_shows_title(dock) {
            260.0
        } else {
            180.0
        };
        let measured = ctx
            .memory(|m| m.area_rect(composer_id).map(|r| r.height()))
            .unwrap_or(0.0);
        let box_h = if measured > 80.0 {
            measured.min(cap)
        } else {
            cap - 40.0
        };
        let mut stage_hit = crate::cards::ImagineStageHit::default();
        let panel = egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ui, |ui| {
                let content = ui.max_rect();
                let toolbox_top = imagine_toolbox_top(content.top(), content.height(), box_h, dock);
                let stage_w = (content.width() - 48.0).max(280.0);
                if dock == ImagineToolboxDock::Bottom {
                    let view_h = (toolbox_top - content.top() - IMAGINE_WALL_GAP).max(0.0);
                    let viewport =
                        egui::Rect::from_min_size(content.min, egui::vec2(content.width(), view_h));
                    let leftover = view_h;
                    let stage_h = if stage_on {
                        imagine_stage_h(leftover, &aspect, stage_w)
                    } else {
                        0.0
                    };
                    ui.scope_builder(egui::UiBuilder::new().max_rect(viewport), |ui| {
                        ui.set_clip_rect(viewport);
                        egui::ScrollArea::vertical()
                            .id_salt("imagine-scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(viewport.width());
                                if stage_on && stage_h > 8.0 {
                                    let x = viewport.center().x - stage_w * 0.5;
                                    let (st, _) = ui.allocate_exact_size(
                                        egui::vec2(viewport.width(), stage_h),
                                        egui::Sense::hover(),
                                    );
                                    let stage = egui::Rect::from_center_size(
                                        egui::pos2(x + stage_w * 0.5, st.center().y),
                                        egui::vec2(stage_w, stage_h),
                                    );
                                    ui.scope_builder(
                                        egui::UiBuilder::new().max_rect(stage),
                                        |ui| {
                                            ui.set_clip_rect(stage);
                                            stage_hit = crate::cards::imagine_stage(
                                                ui,
                                                &last,
                                                working,
                                                video,
                                                &imagine_err,
                                            );
                                        },
                                    );
                                    ui.add_space(IMAGINE_WALL_GAP);
                                }
                                crate::cards::imagine_masonry(
                                    ui,
                                    &selected,
                                    now_ms(),
                                    &self.wall.gifs,
                                    |p| {
                                        seed = Some(p);
                                    },
                                );
                                self.ui_imagine_results(ui);
                            });
                    });
                } else {
                    let (wall_top, wall_h) = imagine_wall_bounds(
                        content.top(),
                        content.height(),
                        toolbox_top,
                        box_h,
                        dock,
                        0.0,
                    );
                    if wall_h > 8.0 {
                        let wall = egui::Rect::from_min_size(
                            egui::pos2(content.left(), wall_top),
                            egui::vec2(content.width(), wall_h),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(wall), |ui| {
                            ui.set_clip_rect(wall);
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                    crate::cards::imagine_masonry(
                                        ui,
                                        &selected,
                                        now_ms(),
                                        &self.wall.gifs,
                                        |p| {
                                            seed = Some(p);
                                        },
                                    );
                                    self.ui_imagine_results(ui);
                                });
                        });
                    }
                }
            });
        let content = panel.response.rect;
        let bar_w = (content.width() - 48.0).clamp(280.0, crate::theme::IMAGINE_BAR_W);
        let y = imagine_toolbox_top(content.top(), content.height(), box_h, dock);
        let x = content.center().x - bar_w * 0.5;
        egui::Area::new(egui::Id::new("imagine-new"))
            .fixed_pos(egui::pos2(content.right() - 148.0, content.top() + 12.0))
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                if crate::cards::white_pill(ui, "+ New project") {
                    new_project = true;
                }
            });
        egui::Area::new(composer_id)
            .default_size(egui::vec2(bar_w, 8.0))
            .fixed_pos(egui::pos2(x, y))
            .constrain_to(content)
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                ui.set_width(bar_w);
                ui.vertical(|ui| {
                    ui.set_width(bar_w);
                    if imagine_toolbox_shows_title(dock) {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(format!("Imagine {word}"))
                                    .font(crate::theme::title_font(crate::theme::IMAGINE_TITLE))
                                    .color(crate::theme::fg()),
                            );
                        });
                        ui.add_space(crate::theme::IMAGINE_GAP);
                    }
                    self.ui_attach_chip(ui, PlusTarget::Imagine);
                    self.paint_voice_mode_row(ui);
                    let bar = self.ui_imagine_bar(ui);
                    generate = bar.generate;
                    if bar.go_settings {
                        go_settings = true;
                    }
                    if bar.stop {
                        generate = false;
                    }
                    stop = bar.stop;
                });
            });
        let mut save_now = stage_hit.save;
        if stage_hit.play && !last.is_empty() {
            self.play_imagine_media(&last);
        }
        if stage_hit.expand {
            if grokhub_core::imagine_is_video_path(&last) {
                self.play_imagine_media(&last);
            } else {
                self.imagine_expand = true;
            }
        }
        if stage_hit.open && !last.is_empty() {
            self.play_imagine_media(&last);
        }
        if self.imagine_expand && !last.is_empty() && !grokhub_core::imagine_is_video_path(&last) {
            let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            egui::Area::new(egui::Id::new("imagine-lightbox"))
                .fixed_pos(content.min)
                .order(egui::Order::Tooltip)
                .show(&ctx, |ui| {
                    let (full, back) = ui.allocate_exact_size(content.size(), egui::Sense::click());
                    ui.painter()
                        .rect_filled(full, 0.0, egui::Color32::from_black_alpha(220));
                    let inner = full.shrink(28.0);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                        crate::cards::imagine_result_hero(ui, &last);
                    });
                    let save_rect = egui::Rect::from_min_size(
                        egui::pos2(full.right() - 220.0, full.top() + 16.0),
                        egui::vec2(200.0, 40.0),
                    );
                    ui.scope_builder(egui::UiBuilder::new().max_rect(save_rect), |ui| {
                        ui.horizontal(|ui| {
                            if crate::cards::white_pill(ui, "Save") {
                                save_now = true;
                            }
                            if crate::cards::white_pill(ui, "Close") {
                                close = true;
                            }
                        });
                    });
                    if back.clicked() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            if !inner.contains(pos) {
                                close = true;
                            }
                        }
                    }
                });
            if close {
                self.imagine_expand = false;
            }
        }
        if save_now {
            self.start_imagine_save();
        }
        if new_project {
            self.imagine_prompt.clear();
            self.imagine_last.clear();
            self.imagine_error.clear();
            self.imagine_native.results.clear();
            self.imagine_native.note.clear();
            self.imagine_expand = false;
            self.imagine_want_focus = true;
        }
        if let Some(p) = seed {
            self.imagine_prompt = p;
            self.imagine_want_focus = true;
        }
        if go_settings {
            if self.nav != Nav::Settings {
                self.settings_back = self.nav;
            }
            self.settings_sec = SettingsSec::Account;
            self.nav = Nav::Settings;
        }
        if stop {
            self.run_slash(Slash::Stop);
        } else if generate {
            self.kick_imagine();
        }
    }

    pub(super) fn ui_imagine_bar(&mut self, ui: &mut egui::Ui) -> ImagineBarOut {
        let mut out = ImagineBarOut::default();
        let bar_w = ui.available_width().min(crate::theme::IMAGINE_BAR_W);
        let focused = ui.memory(|m| m.has_focus(egui::Id::new("imagine-prompt")));
        let stroke = if focused {
            crate::theme::border_strong()
        } else {
            crate::theme::border()
        };
        let model = dedicated_imagine_model(&self.cfg.imagine_model);
        let ready = !self.imagine_prompt.trim().is_empty();
        let signed_in = self.imagine_ui_ready();
        egui::Frame::NONE
            .fill(crate::theme::surface())
            .corner_radius(crate::theme::IMAGINE_BAR_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, stroke))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(bar_w);
                let prompt_w = (ui.available_width() - 8.0).max(80.0);
                let prompt_h = crate::cards::imagine_prompt_h();
                let (prompt_rect, _) = ui.allocate_exact_size(
                    egui::vec2(prompt_w, prompt_h),
                    egui::Sense::hover(),
                );
                let edit = ui.put(
                    prompt_rect,
                    egui::TextEdit::singleline(&mut self.imagine_prompt)
                        .id(egui::Id::new("imagine-prompt"))
                        .desired_width(prompt_w)
                        .clip_text(true)
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2)))
                        .hint_text(crate::theme::hint("Type to imagine")),
                );
                if self.imagine_want_focus {
                    edit.request_focus();
                    self.imagine_want_focus = false;
                }
                if edit.has_focus()
                    && ui.input(|i| {
                        i.key_pressed(egui::Key::Enter) && !i.modifiers.shift && !i.modifiers.command
                    })
                {
                    if self.imagine_prompt.ends_with('\n') {
                        self.imagine_prompt.pop();
                    }
                    if ready {
                        out.generate = true;
                    }
                }
                if edit.has_focus()
                    && ready
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command)
                {
                    out.generate = true;
                }
                ui.add_space(crate::cards::imagine_prompt_chip_gap());
                let send_w = crate::cards::imagine_send_cluster_w();
                let chips_w = (ui.available_width() - send_w).max(crate::theme::IMAGINE_HIT * 4.0);
                let chip_h = crate::cards::imagine_chip_stack_h();
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(chips_w, chip_h),
                        egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                        |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                            let (plus_r, plus) = ui.allocate_exact_size(
                                egui::vec2(crate::theme::IMAGINE_HIT, crate::theme::IMAGINE_HIT),
                                egui::Sense::click(),
                            );
                            ui.painter()
                                .circle_filled(plus_r.center(), 18.0, crate::theme::panel());
                            crate::icons::paint_plus_at(ui.painter(), plus_r, crate::theme::muted());
                            if plus
                                .on_hover_text("Upload a file or paste clipboard")
                                .clicked()
                            {
                                self.open_plus(PlusTarget::Imagine, plus_r.left_bottom());
                            }
                            crate::cards::imagine_seg_track(ui, |ui| {
                                for kind in [
                                    ImagineKind::Image,
                                    ImagineKind::Video,
                                    ImagineKind::Agent,
                                ] {
                                    let on = self.imagine_kind == kind;
                                    let label = crate::cards::imagine_kind_label(kind);
                                    let ink = if on {
                                        crate::theme::fg()
                                    } else {
                                        crate::theme::muted()
                                    };
                                    if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                        match kind {
                                            ImagineKind::Image => {
                                                crate::icons::paint_image_mode(ui, 16.0, ink);
                                            }
                                            ImagineKind::Video => {
                                                crate::icons::paint_video_mode(ui, 16.0, ink);
                                            }
                                            ImagineKind::Agent => {
                                                crate::icons::paint_agent_mode(ui, 16.0, ink);
                                            }
                                        }
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(label)
                                                .size(crate::theme::FONT_CHROME)
                                                .color(ink),
                                        );
                                    }) {
                                        self.imagine_kind = kind;
                                        self.status = match kind {
                                            ImagineKind::Image => "Image still".into(),
                                            ImagineKind::Video => {
                                                "Video calls grok-imagine-video-1.5 and saves an mp4."
                                                    .into()
                                            }
                                            ImagineKind::Agent => {
                                                "Agent paints a character sprite still.".into()
                                            }
                                        };
                                    }
                                }
                            });
                            match self.imagine_kind {
                                ImagineKind::Video => {
                                    crate::cards::imagine_seg_track(ui, |ui| {
                                        for (i, label) in ["480p", "720p"].into_iter().enumerate() {
                                            let on = self.imagine_video_res == i as u8;
                                            if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                                ui.label(
                                                    RichText::new(label)
                                                        .size(crate::theme::FONT_CHROME)
                                                        .color(if on {
                                                            crate::theme::fg()
                                                        } else {
                                                            crate::theme::muted()
                                                        }),
                                                );
                                            }) {
                                                self.imagine_video_res = i as u8;
                                            }
                                        }
                                    });
                                    crate::cards::imagine_seg_track(ui, |ui| {
                                        for (i, label) in ["6s", "10s", "15s"].into_iter().enumerate()
                                        {
                                            let on = self.imagine_video_dur == i as u8;
                                            if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                                ui.label(
                                                    RichText::new(label)
                                                        .size(crate::theme::FONT_CHROME)
                                                        .color(if on {
                                                            crate::theme::fg()
                                                        } else {
                                                            crate::theme::muted()
                                                        }),
                                                );
                                            }) {
                                                self.imagine_video_dur = i as u8;
                                            }
                                        }
                                    });
                                    let audio_on = self.imagine_video_audio;
                                    if crate::cards::imagine_seg_chip(ui, audio_on, |ui| {
                                        ui.label(
                                            RichText::new("Video audio")
                                                .size(crate::theme::FONT_CHROME)
                                                .color(if audio_on {
                                                    crate::theme::fg()
                                                } else {
                                                    crate::theme::muted()
                                                }),
                                        );
                                    }) {
                                        self.imagine_video_audio = !self.imagine_video_audio;
                                    }
                                    ui.end_row();
                                }
                                ImagineKind::Image | ImagineKind::Agent => {
                                    crate::cards::imagine_seg_track(ui, |ui| {
                                        for quality in [false, true] {
                                            let on = self.imagine_quality == quality;
                                            let label = crate::cards::imagine_quality_label(quality);
                                            if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                                ui.label(
                                                    RichText::new(label)
                                                        .size(crate::theme::FONT_CHROME)
                                                        .color(if on {
                                                            crate::theme::fg()
                                                        } else {
                                                            crate::theme::muted()
                                                        }),
                                                );
                                            }) {
                                                self.imagine_quality = quality;
                                            }
                                        }
                                    });
                                }
                            }
                            let style_label = imagine_style_label(self.imagine_style);
                            let style_inner = egui::Frame::NONE
                                .fill(crate::theme::panel())
                                .corner_radius(crate::theme::IMAGINE_HIT)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_height(crate::theme::IMAGINE_HIT - 12.0);
                                    ui.set_min_width(56.0);
                                    ui.horizontal_centered(|ui| {
                                        crate::icons::paint_style_auto(ui, 16.0, crate::theme::fg());
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(style_label)
                                                .size(crate::theme::FONT_CHROME)
                                                .color(crate::theme::fg()),
                                        );
                                        ui.add_space(4.0);
                                        crate::icons::paint_menu_caret(ui, crate::theme::muted());
                                    });
                                });
                            let style = ui
                                .interact(
                                    style_inner.response.rect,
                                    egui::Id::new("imagine-style-hit"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text("Style — suffix on the still");
                            if style.clicked() {
                                self.imagine_style_open = !self.imagine_style_open;
                                self.imagine_aspect_open = false;
                                self.imagine_native.more_open = false;
                                self.imagine_style_anchor = style.rect;
                                self.imagine_menu_ignore = true;
                            }
                            let aspect = imagine_aspect_label(self.imagine_aspect);
                            let aspect_name = imagine_aspect_name(self.imagine_aspect);
                            let aspect_inner = egui::Frame::NONE
                                .fill(crate::theme::panel())
                                .corner_radius(crate::theme::IMAGINE_HIT)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_height(crate::theme::IMAGINE_HIT - 12.0);
                                    ui.set_min_width(56.0);
                                    ui.horizontal_centered(|ui| {
                                        crate::icons::paint_aspect_rect(
                                            ui,
                                            self.imagine_aspect,
                                            16.0,
                                            crate::theme::fg(),
                                        );
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(aspect)
                                                .size(crate::theme::FONT_CHROME)
                                                .color(crate::theme::fg()),
                                        );
                                        ui.add_space(4.0);
                                        crate::icons::paint_menu_caret(ui, crate::theme::muted());
                                    });
                                });
                            let aspect_hit = ui
                                .interact(
                                    aspect_inner.response.rect,
                                    egui::Id::new("imagine-aspect-hit"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text(format!("{aspect} {aspect_name} · {model}"));
                            if aspect_hit.clicked() {
                                self.imagine_aspect_open = !self.imagine_aspect_open;
                                self.imagine_style_open = false;
                                self.imagine_native.more_open = false;
                                self.imagine_aspect_anchor = aspect_hit.rect;
                                self.imagine_menu_ignore = true;
                            }
                            // Frames do not wrap on their own; a full first row would grow the box.
                            if self.imagine_kind != ImagineKind::Video {
                                ui.end_row();
                            }
                            let more_inner = egui::Frame::NONE
                                .fill(crate::theme::panel())
                                .corner_radius(crate::theme::IMAGINE_HIT)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_height(crate::theme::IMAGINE_HIT - 12.0);
                                    ui.horizontal_centered(|ui| {
                                        ui.label(
                                            RichText::new("More")
                                                .size(crate::theme::FONT_CHROME)
                                                .color(crate::theme::fg()),
                                        );
                                        ui.add_space(4.0);
                                        crate::icons::paint_menu_caret(ui, crate::theme::muted());
                                    });
                                });
                            let more = ui
                                .interact(
                                    more_inner.response.rect,
                                    egui::Id::new("imagine-more-hit"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text("Account, mode, model, and sources");
                            if more.clicked() {
                                self.imagine_native.more_open = !self.imagine_native.more_open;
                                self.imagine_style_open = false;
                                self.imagine_aspect_open = false;
                                self.imagine_native.more_anchor = more.rect;
                                self.imagine_menu_ignore = true;
                            }
                            if !signed_in
                                && !self.imagine_native.auth_busy
                                && crate::cards::ghost_pill(ui, "Sign in")
                            {
                                // One Account sign-in covers Imagine — send them
                                // there instead of opening a second OAuth flow.
                                self.nav = Nav::Settings;
                                self.settings_sec = SettingsSec::Account;
                            } else if self.running && self.page_nav() == Nav::Imagine {
                                ui.label(
                                    RichText::new("Imagining…")
                                        .size(crate::theme::FONT_META)
                                        .color(crate::theme::muted()),
                                );
                            }
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(send_w, chip_h),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let go = composer_go(self.running, ready);
                            let send = crate::icons::paint_bar_icon(
                                ui,
                                match go {
                                    ComposerGo::Stop => crate::icons::BarIcon::Stop,
                                    ComposerGo::Send => crate::icons::BarIcon::Send,
                                    ComposerGo::Idle => crate::icons::BarIcon::ArrowUp,
                                },
                                crate::theme::IMAGINE_HIT,
                                match go {
                                    ComposerGo::Idle => crate::theme::muted(),
                                    ComposerGo::Send | ComposerGo::Stop => crate::theme::fg(),
                                },
                            )
                            .on_hover_text(match go {
                                ComposerGo::Stop => composer_go_tip(true),
                                ComposerGo::Send | ComposerGo::Idle => "Generate still · Enter",
                            });
                            let go_hit = send.clicked()
                                || (send.is_pointer_button_down_on()
                                    && ui.input(|i| i.pointer.primary_pressed()));
                            match go {
                                ComposerGo::Stop => {
                                    if go_hit {
                                        out.stop = true;
                                    }
                                }
                                ComposerGo::Send => {
                                    if go_hit {
                                        out.generate = true;
                                    }
                                }
                                ComposerGo::Idle => {}
                            }
                            self.paint_voice_mic(ui, crate::theme::IMAGINE_HIT);
                        },
                    );
                });
                if self.ui_imagine_notice(ui) {
                    out.go_settings = true;
                }
            });
        out
    }

    pub(super) fn poll_wall(&mut self) {
        // A job may refresh the Imagine tokens and then fail, or (the wall) never
        // hand them back; keep the rotated pair so the next refresh is not stale.
        let current = self
            .imagine_native
            .tokens
            .as_ref()
            .and_then(|t| t.refresh_token.as_deref());
        if let Some(tokens) = crate::imagine_auth::take_refreshed(current) {
            self.note_imagine_tokens(tokens);
        }
        let Some(rx) = self.wall_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(gif)) => {
                self.wall_busy = false;
                self.wall.last_ms = now_ms();
                self.wall.gifs.push(gif.clone());
                let (kept, evicted) = wall_evict(std::mem::take(&mut self.wall.gifs), WALL_GIF_MAX);
                self.wall.gifs = kept;
                self.status = format!("New cover on the wall — {}", gif.title);
                let wall = self.wall.clone();
                let io = self.persist_io.clone();
                std::thread::spawn(move || {
                    if let Ok(_g) = io.lock() {
                        let _ = crate::store::save_wall(&wall);
                    }
                    for old in evicted {
                        let _ = std::fs::remove_file(&old.path_a);
                        let _ = std::fs::remove_file(&old.path_b);
                    }
                });
            }
            Ok(Err(e)) => {
                self.wall_busy = false;
                self.wall.last_ms = now_ms()
                    .saturating_sub(WALL_GIF_EVERY_MS)
                    .saturating_add(15 * 60 * 1000);
                self.status = format!("Wall cover held — {e}");
                let wall = self.wall.clone();
                let io = self.persist_io.clone();
                std::thread::spawn(move || {
                    if let Ok(_g) = io.lock() {
                        let _ = crate::store::save_wall(&wall);
                    }
                });
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.wall_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.wall_busy = false;
            }
        }
    }

    pub(super) fn tick_wall(&mut self) {
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        let imagine_signed_in = self
            .imagine_native
            .tokens
            .as_ref()
            .is_some_and(|t| grokhub_core::imagine_oauth_preferred(t, now_ms()))
            || self.account_oauth_present();
        if !wall_can_paint(
            self.has_key() || imagine_signed_in,
            self.cfg.imagine_wall,
            self.wall_busy,
            self.running,
            quiet,
            self.wall.last_ms,
            now_ms(),
        ) {
            return;
        }
        self.kick_wall();
    }

    pub(super) fn kick_wall(&mut self) {
        if self.wall_busy {
            return;
        }
        let taken: Vec<String> = self.wall.gifs.iter().map(|g| g.title.clone()).collect();
        let taken_ref: Vec<&str> = taken.iter().map(|s| s.as_str()).collect();
        let seed = pick_fresh_seed(now_ms(), &taken_ref);
        let id = format!("{:x}", now_ms());
        let dir = config::wall_dir();
        let cred = match self.imagine_cred() {
            Ok(cred) => cred,
            Err(_) => return,
        };
        let cred_oauth = cred.kind == grokhub_core::ImagineCredKind::OAuth;
        let job_tokens = if cred_oauth {
            self.imagine_native.tokens.clone()
        } else {
            None
        };
        let key = cred.secret;
        let model = dedicated_imagine_model(&self.cfg.imagine_model);
        let title = seed.title.to_string();
        let prompt = seed.prompt.to_string();
        let prompt_b = seed.prompt_b.to_string();
        let tall = seed.tall;
        let created_ms = now_ms();
        let (tx, rx) = mpsc::channel();
        self.wall_rx = Some(rx);
        self.wall_busy = true;
        self.status = format!("Painting a wall cover — {title}");
        std::thread::spawn(move || {
            let key = if cred_oauth {
                if let Some(tokens) = job_tokens {
                    match crate::imagine_auth::access_for_job(&tokens) {
                        Ok((key, _)) => key,
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                } else {
                    key
                }
            } else {
                key
            };
            let _ = tx.send(paint_wall_cover(
                &key, &model, &id, &dir, &title, &prompt, &prompt_b, tall, created_ms,
            ));
        });
    }

    /// Imagine keychain first, then Settings → Account (`secrets.oauth`), then the
    /// console key. Same order as Lab mode's `native_cred`, so one Grok sign-in
    /// covers Imagine and nothing re-prompts when Account is already live.
    pub(super) fn imagine_cred(&mut self) -> Result<grokhub_core::ImagineCred, &'static str> {
        let now = now_ms();
        if let Some(tokens) = self.imagine_native.tokens.clone() {
            if grokhub_core::imagine_oauth_preferred(&tokens, now) {
                if grokhub_core::imagine_access_usable(&tokens, now) {
                    return Ok(grokhub_core::ImagineCred {
                        secret: tokens.access_token,
                        kind: grokhub_core::ImagineCredKind::OAuth,
                    });
                }
                if let Ok((access, updated)) = crate::imagine_auth::access_for_job(&tokens) {
                    if let Some(next) = updated {
                        self.note_imagine_tokens(next);
                    }
                    return Ok(grokhub_core::ImagineCred {
                        secret: access,
                        kind: grokhub_core::ImagineCredKind::OAuth,
                    });
                }
                // Imagine refresh failed — fall through to Account / key.
            }
        }
        if let Some(access) = self.native_account_access(now) {
            return Ok(grokhub_core::ImagineCred {
                secret: access,
                kind: grokhub_core::ImagineCredKind::OAuth,
            });
        }
        grokhub_core::choose_imagine_bearer(None, false, self.console_key())
    }

    pub(super) fn account_oauth_present(&self) -> bool {
        self.secrets
            .oauth
            .as_ref()
            .is_some_and(|t| !t.access_token.trim().is_empty())
    }

    /// Composer / wall treat Account the same as Imagine's own keychain sign-in.
    pub(super) fn imagine_ui_ready(&self) -> bool {
        self.imagine_native.tokens.is_some()
            || self.account_oauth_present()
            || !self.console_key().trim().is_empty()
    }

    fn imagine_call_block(&self) -> Option<&'static str> {
        if self.imagine_kind == ImagineKind::Image
            && self.imagine_native.image_op == 1
            && self.imagine_native.edit_sources.is_empty()
        {
            return Some("Add at least one source image to edit.");
        }
        if self.imagine_kind == ImagineKind::Video
            && self.imagine_native.video_op == 1
            && self
                .imagine_native
                .video_image
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
        {
            return Some("Add a source image for image-to-video.");
        }
        if self.imagine_kind == ImagineKind::Video
            && matches!(self.imagine_native.video_op, 2 | 3)
            && self.imagine_native.video_url.trim().is_empty()
        {
            return Some("Add a video URL to edit or extend.");
        }
        None
    }

    fn imagine_image_model_now(&self) -> String {
        match self.imagine_native.image_model {
            1 => "grok-imagine-image-quality".into(),
            2 => grokhub_core::FALLBACK_IMAGINE_MODEL.into(),
            _ => grokhub_core::DEFAULT_IMAGINE_MODEL.into(),
        }
    }

    fn imagine_resolution_now(&self) -> String {
        imagine_image_resolution(self.imagine_quality).into()
    }

    fn imagine_quality_now(&self) -> String {
        if self.imagine_native.image_model != 0 {
            return String::new();
        }
        grokhub_core::imagine_image_quality(self.imagine_quality).into()
    }

    /// The composer's aspect pill is the one aspect control, for stills and video alike.
    fn imagine_api_aspect_now(&self) -> String {
        imagine_aspect_label(self.imagine_aspect).into()
    }

    fn video_duration_now(&self) -> u32 {
        imagine_video_duration_secs(imagine_video_dur_label(self.imagine_video_dur))
    }

    fn video_resolution_now(&self) -> String {
        let allow_1080 = self.imagine_native.video_model == 0 && self.imagine_native.video_op <= 1;
        if self.imagine_native.video_res_1080 && allow_1080 {
            "1080p".into()
        } else {
            imagine_video_resolution(imagine_video_res_label(self.imagine_video_res)).into()
        }
    }

    pub(super) fn imagine_call(&self) -> ImagineCall {
        if self.imagine_kind == ImagineKind::Video {
            let op = match self.imagine_native.video_op {
                1 => grokhub_core::ImagineVideoOp::ImageToVideo,
                2 => grokhub_core::ImagineVideoOp::Edit,
                3 => grokhub_core::ImagineVideoOp::Extend,
                _ => grokhub_core::ImagineVideoOp::TextToVideo,
            };
            let model = if self.imagine_native.video_model == 1 {
                grokhub_core::FALLBACK_VIDEO_MODEL
            } else {
                grokhub_core::DEFAULT_VIDEO_MODEL
            };
            return ImagineCall::Video {
                op,
                model: model.into(),
                duration: self.video_duration_now(),
                resolution: self.video_resolution_now(),
                aspect: self.imagine_api_aspect_now(),
                audio: self.imagine_video_audio,
                image_url: self.imagine_native.video_image.clone().unwrap_or_default(),
                video_url: self.imagine_native.video_url.clone(),
            };
        }
        if self.imagine_kind == ImagineKind::Image && self.imagine_native.image_op == 1 {
            return ImagineCall::Edit {
                model: self.imagine_image_model_now(),
                n: u32::from(self.imagine_native.count.clamp(1, 10)),
                resolution: self.imagine_resolution_now(),
                aspect: self.imagine_api_aspect_now(),
                sources: self.imagine_native.edit_sources.clone(),
                mask: self.imagine_native.edit_mask.clone(),
            };
        }
        ImagineCall::Generate {
            model: self.imagine_image_model_now(),
            n: u32::from(self.imagine_native.count.clamp(1, 10)),
            resolution: self.imagine_resolution_now(),
            aspect: self.imagine_api_aspect_now(),
            quality: self.imagine_quality_now(),
        }
    }

    pub(super) fn finish_imagine_job(
        &mut self,
        urls: Vec<String>,
        note: Option<String>,
        tokens: Option<grokhub_core::ImagineTokens>,
    ) {
        self.running = false;
        self.imagine_pending = false;
        self.imagine_error.clear();
        self.imagine_native.offer_key = false;
        self.imagine_native.offer_settings = false;
        if let Some(tokens) = tokens {
            self.note_imagine_tokens(tokens);
        }
        let first = urls.first().cloned().unwrap_or_default();
        self.imagine_last = first.clone();
        self.imagine_native.results = urls.clone();
        let job_prompt = self.imagine_job_prompt.clone();
        if let Some(note) = note.filter(|s| !s.trim().is_empty()) {
            self.imagine_native.note = note.clone();
            self.status = format!("Imagine ready. {note}");
        } else {
            self.imagine_native.note.clear();
            self.status = "Imagine ready".into();
        }
        if !first.is_empty() {
            self.pin_generation_to_wall(&first, &job_prompt);
        }
        for url in &urls {
            self.push_bound_msg("assistant", format!("IMAGINE: {url}"));
        }
        let summary = if first.is_empty() {
            "IMAGINE:".into()
        } else {
            format!("IMAGINE: {first}")
        };
        self.finish_hub_dispatch(&summary, true);
        self.abandon_turn_card();
        self.chat_job_thread = None;
        self.persist();
        self.maybe_continue_ptt();
    }

    pub(super) fn poll_imagine_auth(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.imagine_native.auth_rx.take() {
            match rx.try_recv() {
                Ok(ev) => self.apply_imagine_auth(ev),
                Err(mpsc::TryRecvError::Empty) => self.imagine_native.auth_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => self.imagine_native.auth_busy = false,
            }
        }
        let due = self
            .imagine_native
            .device_next
            .is_some_and(|t| Instant::now() >= t);
        if !self.imagine_native.device_code.is_empty() && !self.imagine_native.poll_inflight && due
        {
            self.imagine_native.poll_inflight = true;
            let code = self.imagine_native.device_code.clone();
            let interval = self.imagine_native.device_interval.max(1);
            let (tx, rx) = mpsc::channel();
            self.imagine_native.poll_rx = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(crate::imagine_auth::poll_device_once(&code, interval));
            });
        }
        if let Some(rx) = self.imagine_native.poll_rx.take() {
            match rx.try_recv() {
                Ok(res) => self.apply_imagine_poll(res),
                Err(mpsc::TryRecvError::Empty) => self.imagine_native.poll_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => self.imagine_native.poll_inflight = false,
            }
        }
        if let Some(rx) = self.imagine_native.pick_rx.take() {
            match rx.try_recv() {
                Ok(out) => self.apply_imagine_pick(out),
                Err(mpsc::TryRecvError::Empty) => self.imagine_native.pick_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if let Some(rx) = self.imagine_native.item_save_rx.take() {
            match rx.try_recv() {
                Ok(Ok(path)) => self.status = format!("Saved {path}"),
                Ok(Err(e)) => self.status = e,
                Err(mpsc::TryRecvError::Empty) => self.imagine_native.item_save_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if self.imagine_native.auth_rx.is_some()
            || self.imagine_native.poll_rx.is_some()
            || self.imagine_native.pick_rx.is_some()
            || self.imagine_native.item_save_rx.is_some()
            || !self.imagine_native.device_code.is_empty()
        {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn apply_imagine_auth(&mut self, ev: crate::imagine_auth::ImagineAuthEvent) {
        self.imagine_native.auth_busy = false;
        match ev {
            crate::imagine_auth::ImagineAuthEvent::Loaded(Ok(Some(tokens))) => {
                self.note_imagine_tokens(tokens);
            }
            crate::imagine_auth::ImagineAuthEvent::Loaded(Ok(None)) => {}
            crate::imagine_auth::ImagineAuthEvent::Loaded(Err(e)) => self.status = e,
            crate::imagine_auth::ImagineAuthEvent::SignedIn(tokens) => {
                crate::imagine_auth::forget_refreshed();
                self.clear_imagine_device();
                self.note_imagine_tokens(tokens);
                self.status = self.imagine_signed_in_label();
            }
            crate::imagine_auth::ImagineAuthEvent::DeviceReady {
                user_code,
                verify_uri,
                device_code,
                interval,
            } => {
                self.imagine_native.device_user_code = user_code.clone();
                self.imagine_native.device_verify_uri = verify_uri;
                self.imagine_native.device_code = device_code;
                self.imagine_native.device_interval = interval.max(1);
                self.imagine_native.device_next =
                    Some(Instant::now() + Duration::from_secs(interval.max(1)));
                self.imagine_native.poll_inflight = false;
                self.status = format!("Enter code {user_code} in the browser.");
            }
            crate::imagine_auth::ImagineAuthEvent::Failed(e) => self.status = e,
            crate::imagine_auth::ImagineAuthEvent::SignedOut => {
                crate::imagine_auth::forget_refreshed();
                self.imagine_native.tokens = None;
                self.imagine_native.email.clear();
                self.clear_imagine_device();
                self.status = "Signed out of Imagine".into();
            }
        }
    }

    fn apply_imagine_poll(&mut self, res: Result<crate::imagine_auth::DevicePoll, String>) {
        self.imagine_native.poll_inflight = false;
        match res {
            Ok(crate::imagine_auth::DevicePoll::Ready(tokens)) => {
                crate::imagine_auth::forget_refreshed();
                self.clear_imagine_device();
                self.note_imagine_tokens(tokens);
                self.status = self.imagine_signed_in_label();
            }
            Ok(crate::imagine_auth::DevicePoll::Wait { secs }) => {
                let secs = secs.max(1);
                self.imagine_native.device_interval = secs;
                self.imagine_native.device_next = Some(Instant::now() + Duration::from_secs(secs));
            }
            Ok(crate::imagine_auth::DevicePoll::Stop(msg)) => {
                self.clear_imagine_device();
                self.status = msg;
            }
            Err(e) => {
                self.status = e;
                let secs = self.imagine_native.device_interval.max(5);
                self.imagine_native.device_next = Some(Instant::now() + Duration::from_secs(secs));
            }
        }
    }

    fn apply_imagine_pick(&mut self, out: ImaginePickOut) {
        let uris = match out.uris {
            Ok(uris) => uris,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        if uris.is_empty() {
            return;
        }
        match out.kind {
            ImaginePickKind::Sources => {
                for uri in uris {
                    if self.imagine_native.edit_sources.len() >= 3 {
                        break;
                    }
                    self.imagine_native.edit_sources.push(uri);
                }
                self.status = format!("{} source image(s)", self.imagine_native.edit_sources.len());
            }
            ImaginePickKind::Mask => {
                self.imagine_native.edit_mask = uris.into_iter().next();
                self.status = "Mask added".into();
            }
            ImaginePickKind::VideoStill => {
                self.imagine_native.video_image = uris.into_iter().next();
                self.status = "Video source image added".into();
            }
        }
    }

    fn clear_imagine_device(&mut self) {
        self.imagine_native.device_code.clear();
        self.imagine_native.device_user_code.clear();
        self.imagine_native.device_verify_uri.clear();
        self.imagine_native.device_next = None;
        self.imagine_native.poll_inflight = false;
    }

    fn note_imagine_tokens(&mut self, tokens: grokhub_core::ImagineTokens) {
        if let Some(email) = tokens.email.clone().filter(|s| !s.trim().is_empty()) {
            self.imagine_native.email = email;
        }
        self.imagine_native.tokens = Some(tokens);
    }

    fn imagine_signed_in_label(&self) -> String {
        if !self.imagine_native.email.trim().is_empty() {
            return format!("Signed in as {}", self.imagine_native.email);
        }
        if let Some(email) = self
            .secrets
            .oauth
            .as_ref()
            .and_then(|t| t.email.as_ref())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return format!("Signed in as {email}");
        }
        "Signed in with Grok".into()
    }

    fn start_imagine_auth(&mut self, pkce: bool) {
        if self.imagine_native.auth_busy || self.imagine_native.auth_rx.is_some() {
            return;
        }
        self.imagine_native.auth_busy = true;
        self.imagine_native.auth_rx = Some(if pkce {
            crate::imagine_auth::begin_pkce()
        } else {
            crate::imagine_auth::begin_device()
        });
        self.status = if pkce {
            "Opening the browser to sign in…".into()
        } else {
            "Starting a device code…".into()
        };
    }

    fn start_imagine_sign_out(&mut self) {
        if self.imagine_native.auth_busy || self.imagine_native.auth_rx.is_some() {
            return;
        }
        self.imagine_native.auth_busy = true;
        self.imagine_native.auth_rx = Some(crate::imagine_auth::begin_sign_out());
    }

    fn start_imagine_pick(&mut self, kind: ImaginePickKind) {
        if self.imagine_native.pick_rx.is_some() {
            return;
        }
        let max = match kind {
            ImaginePickKind::Sources => 3usize.saturating_sub(self.imagine_native.edit_sources.len()),
            ImaginePickKind::Mask | ImaginePickKind::VideoStill => 1,
        };
        if max == 0 {
            self.status = "Three source images is the maximum.".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.imagine_native.pick_rx = Some(rx);
        std::thread::spawn(move || {
            let uris = crate::imagine_auth::pick_image_data_uris(max);
            let _ = tx.send(ImaginePickOut { kind, uris });
        });
    }

    fn start_imagine_result_save(&mut self, src: &str) {
        if self.imagine_native.item_save_rx.is_some() {
            return;
        }
        let src = src.to_string();
        let (tx, rx) = mpsc::channel();
        self.imagine_native.item_save_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(crate::imagine_auth::save_copy_dialog(&src));
        });
    }

    fn open_imagine_folder(&mut self) {
        let dir = crate::config::imagine_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = crate::desktop::open_path(&dir.display().to_string()) {
            self.status = e;
        }
    }

    /// Sign-in progress and recovery offers, inside the composer under the chips.
    fn ui_imagine_notice(&mut self, ui: &mut egui::Ui) -> bool {
        let n = &self.imagine_native;
        if n.note.is_empty() && n.device_user_code.is_empty() && !n.offer_key && !n.offer_settings {
            return false;
        }
        let mut settings = false;
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            if !self.imagine_native.note.is_empty() {
                ui.label(
                    RichText::new(&self.imagine_native.note)
                        .size(crate::theme::FONT_META)
                        .color(crate::theme::muted()),
                );
            }
            if !self.imagine_native.device_user_code.is_empty() {
                ui.label(
                    RichText::new(format!("Code {}", self.imagine_native.device_user_code))
                        .size(crate::theme::FONT_CHROME)
                        .color(crate::theme::fg()),
                );
                let uri = self.imagine_native.device_verify_uri.clone();
                // Through the trusted opener, not egui's link: the URI came from the server.
                if !uri.is_empty() && crate::cards::ghost_pill(ui, "Verify") {
                    if let Err(e) = crate::oauth::open_browser(&uri) {
                        self.status = e;
                    }
                }
            }
            if self.imagine_native.offer_key && crate::cards::ghost_pill(ui, "Use API key") {
                self.imagine_native.force_key = true;
                self.imagine_native.offer_key = false;
                self.kick_imagine();
            }
            if self.imagine_native.offer_settings && crate::cards::ghost_pill(ui, "Settings") {
                self.imagine_native.offer_settings = false;
                settings = true;
            }
        });
        settings
    }

    /// Account, mode, model, count and sources behind the composer's More chip.
    pub(super) fn ui_imagine_more(&mut self, ctx: &egui::Context) -> egui::Rect {
        let anchor = self.imagine_native.more_anchor;
        egui::Area::new(egui::Id::new("imagine_more_menu"))
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(anchor.left_top() - egui::vec2(0.0, 6.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 8.0);
                        self.ui_imagine_more_account(ui);
                        match self.imagine_kind {
                            ImagineKind::Image => self.ui_imagine_more_image(ui),
                            ImagineKind::Video => self.ui_imagine_more_video(ui),
                            ImagineKind::Agent => {}
                        }
                    });
            })
            .response
            .rect
    }

    fn ui_imagine_more_account(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            imagine_more_title(ui, "Account");
            if self.imagine_native.tokens.is_some() {
                ui.label(
                    RichText::new(self.imagine_signed_in_label())
                        .size(crate::theme::FONT_CHROME)
                        .color(crate::theme::fg()),
                );
                if crate::cards::ghost_pill(ui, "Sign out") {
                    self.start_imagine_sign_out();
                }
            } else if self.account_oauth_present() {
                // Settings → Account already covers Imagine; do not offer a
                // second Sign in that would re-prompt.
                ui.label(
                    RichText::new(self.imagine_signed_in_label())
                        .size(crate::theme::FONT_CHROME)
                        .color(crate::theme::fg()),
                );
            } else {
                if crate::cards::ghost_pill(ui, "Sign in with Grok") {
                    self.nav = Nav::Settings;
                    self.settings_sec = SettingsSec::Account;
                }
                if crate::cards::ghost_pill(ui, "Use a code") {
                    self.start_imagine_auth(false);
                }
            }
        });
    }

    fn ui_imagine_more_image(&mut self, ui: &mut egui::Ui) {
        let n = &mut self.imagine_native;
        if let Some(i) = imagine_more_seg(ui, "Mode", &["Generate", "Edit"], n.image_op) {
            n.image_op = i;
        }
        if let Some(i) = imagine_more_seg(ui, "Model", &["2.0", "Quality", "Original"], n.image_model)
        {
            n.image_model = i;
        }
        let count = n.count.clamp(1, 4) - 1;
        if let Some(i) = imagine_more_seg(ui, "Images", &["1", "2", "3", "4"], count) {
            n.count = i + 1;
        }
        if n.image_op != 1 {
            return;
        }
        let sources = n.edit_sources.len();
        let masked = n.edit_mask.is_some();
        ui.horizontal(|ui| {
            imagine_more_title(ui, "Sources");
            ui.label(
                RichText::new(format!("{sources}/3"))
                    .size(crate::theme::FONT_CHROME)
                    .color(crate::theme::fg()),
            );
            if sources < 3 && crate::cards::ghost_pill(ui, "Add image") {
                self.start_imagine_pick(ImaginePickKind::Sources);
            }
            if sources > 0 && crate::cards::ghost_pill(ui, "Clear") {
                self.imagine_native.edit_sources.clear();
            }
        });
        ui.horizontal(|ui| {
            imagine_more_title(ui, "Mask");
            if crate::cards::ghost_pill(ui, if masked { "Replace mask" } else { "Add mask" }) {
                self.start_imagine_pick(ImaginePickKind::Mask);
            }
            if masked && crate::cards::ghost_pill(ui, "Clear") {
                self.imagine_native.edit_mask = None;
            }
        });
    }

    fn ui_imagine_more_video(&mut self, ui: &mut egui::Ui) {
        let composer_res = imagine_video_res_label(self.imagine_video_res);
        let n = &mut self.imagine_native;
        if let Some(i) =
            imagine_more_seg(ui, "Mode", &["Text", "Image", "Edit", "Extend"], n.video_op)
        {
            n.video_op = i;
        }
        if let Some(i) = imagine_more_seg(ui, "Model", &["1.5", "Original"], n.video_model) {
            n.video_model = i;
        }
        if n.video_model == 0 && n.video_op <= 1 {
            let on = u8::from(n.video_res_1080);
            if let Some(i) = imagine_more_seg(ui, "Resolution", &[composer_res, "1080p"], on) {
                n.video_res_1080 = i == 1;
            }
        }
        if n.video_op == 1 {
            let picked = n.video_image.is_some();
            ui.horizontal(|ui| {
                imagine_more_title(ui, "Source");
                if crate::cards::ghost_pill(ui, if picked { "Replace image" } else { "Choose image" })
                {
                    self.start_imagine_pick(ImaginePickKind::VideoStill);
                }
            });
        }
        if matches!(self.imagine_native.video_op, 2 | 3) {
            ui.horizontal(|ui| {
                imagine_more_title(ui, "Video");
                ui.add(
                    egui::TextEdit::singleline(&mut self.imagine_native.video_url)
                        .hint_text("Video URL")
                        .desired_width(280.0),
                );
            });
        }
    }

    fn ui_imagine_results(&mut self, ui: &mut egui::Ui) {
        if self.imagine_native.results.is_empty() {
            return;
        }
        let paths = self.imagine_native.results.clone();
        ui.add_space(8.0);
        ui.label(RichText::new("Results").color(crate::theme::fg()));
        let mut save_at = None;
        let mut open_folder = false;
        let mut play: Option<String> = None;
        egui::Grid::new("imagine-results")
            .num_columns(2)
            .spacing(egui::vec2(8.0, 8.0))
            .show(ui, |ui| {
                for (i, path) in paths.iter().enumerate() {
                    ui.vertical(|ui| {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(168.0, 112.0),
                            egui::Sense::click(),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                            crate::cards::imagine_result_hero(ui, path);
                        });
                        if resp.clicked() && grokhub_core::imagine_is_video_path(path) {
                            play = Some(path.clone());
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("Save").clicked() {
                                save_at = Some(i);
                            }
                            if ui.small_button("Open folder").clicked() {
                                open_folder = true;
                            }
                        });
                    });
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
                if paths.len() % 2 == 1 {
                    ui.end_row();
                }
            });
        if let Some(i) = save_at {
            if let Some(path) = paths.get(i) {
                self.start_imagine_result_save(path);
            }
        }
        if open_folder {
            self.open_imagine_folder();
        }
        if let Some(path) = play {
            self.play_imagine_media(&path);
        }
    }
}

fn imagine_more_title(ui: &mut egui::Ui, title: &str) {
    ui.add_sized(
        [76.0, crate::theme::IMAGINE_HIT],
        egui::Label::new(
            RichText::new(title)
                .size(crate::theme::FONT_CHROME)
                .color(crate::theme::muted()),
        ),
    );
}

/// One labelled segmented row in the More menu; returns the index clicked.
fn imagine_more_seg(ui: &mut egui::Ui, title: &str, labels: &[&str], selected: u8) -> Option<u8> {
    let mut picked = None;
    ui.horizontal(|ui| {
        imagine_more_title(ui, title);
        crate::cards::imagine_seg_track(ui, |ui| {
            for (i, label) in labels.iter().enumerate() {
                let on = selected == i as u8;
                if crate::cards::imagine_seg_chip(ui, on, |ui| {
                    ui.label(RichText::new(*label).size(crate::theme::FONT_CHROME).color(if on {
                        crate::theme::fg()
                    } else {
                        crate::theme::muted()
                    }));
                }) {
                    picked = Some(i as u8);
                }
            }
        });
    });
    picked
}

fn run_imagine_call(
    key: &str,
    prompt: &str,
    call: ImagineCall,
) -> Result<(Vec<String>, Option<String>), String> {
    match call {
        ImagineCall::Generate {
            model,
            n,
            resolution,
            aspect,
            quality,
        } => {
            let urls = crate::xai::grok_imagine_generations(
                key, prompt, &model, n, &resolution, &aspect, &quality,
            )?;
            Ok((urls, None))
        }
        ImagineCall::Edit {
            model,
            n,
            resolution,
            aspect,
            sources,
            mask,
        } => crate::xai::grok_imagine_edits(
            key,
            prompt,
            &model,
            &sources,
            mask.as_deref(),
            n,
            &resolution,
            &aspect,
        ),
        ImagineCall::Video {
            op,
            model,
            duration,
            resolution,
            aspect,
            audio,
            image_url,
            video_url,
        } => {
            let req = grokhub_core::VideoBodyReq {
                prompt,
                model: &model,
                op,
                duration,
                resolution: &resolution,
                aspect: &aspect,
                generate_audio: audio,
                image_url: &image_url,
                video_url: &video_url,
            };
            let path = crate::xai::grok_imagine_video_op(key, &req)?;
            Ok((vec![path], None))
        }
    }
}
