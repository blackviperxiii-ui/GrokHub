//! Imagine generate, wall, and toolbox.

use super::*;


#[derive(Default)]
pub(super) struct ImagineBarOut {
    generate: bool,
    stop: bool,
    go_settings: bool,
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
        let console = self.console_key().trim().to_string();
        let key = if !console.is_empty() {
            console
        } else {
            self.bearer()
        };
        if key.trim().is_empty() {
            self.imagine_error =
                "Add an xAI console API key in Settings, or run grok login.".into();
            self.status = self.imagine_error.clone();
            return;
        }
        let aspect = imagine_aspect_label(self.imagine_aspect).to_string();
        let resolution = imagine_image_resolution(self.imagine_quality).to_string();
        let video_res =
            imagine_video_resolution(imagine_video_res_label(self.imagine_video_res)).to_string();
        let video_dur =
            imagine_video_duration_secs(imagine_video_dur_label(self.imagine_video_dur));
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
        let image_model = dedicated_imagine_model(&self.cfg.imagine_model);
        let video_model = dedicated_video_model("");
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = match kind {
                ImagineKind::Video => {
                    grok_imagine_video(&key, &video_model, &prompt, video_dur, &aspect, &video_res)
                }
                ImagineKind::Image | ImagineKind::Agent => grok_imagine_opts(
                    &key,
                    &image_model,
                    &prompt,
                    Some(&aspect),
                    Some(&resolution),
                ),
            };
            let _ = tx.send(match r {
                Ok(u) => JobOut::Imagine(u),
                Err(e) => JobOut::Err(e),
            });
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

    pub(super) fn ui_imagine(&mut self, ctx: &egui::Context) {
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
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
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
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(viewport), |ui| {
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
                                    ui.allocate_new_ui(
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
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(wall), |ui| {
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
            .show(ctx, |ui| {
                if crate::cards::white_pill(ui, "+ New project") {
                    new_project = true;
                }
            });
        egui::Area::new(composer_id)
            .default_size(egui::vec2(bar_w, 8.0))
            .fixed_pos(egui::pos2(x, y))
            .constrain_to(content)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
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
                    go_settings = bar.go_settings;
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
                .show(ctx, |ui| {
                    let (full, back) = ui.allocate_exact_size(content.size(), egui::Sense::click());
                    ui.painter()
                        .rect_filled(full, 0.0, egui::Color32::from_black_alpha(220));
                    let inner = full.shrink(28.0);
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
                        crate::cards::imagine_result_hero(ui, &last);
                    });
                    let save_rect = egui::Rect::from_min_size(
                        egui::pos2(full.right() - 220.0, full.top() + 16.0),
                        egui::vec2(200.0, 40.0),
                    );
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(save_rect), |ui| {
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
        let authed = self.llm_ready();
        egui::Frame::none()
            .fill(crate::theme::surface())
            .rounding(crate::theme::IMAGINE_BAR_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, stroke))
            .inner_margin(egui::Margin::same(12.0))
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
                        .frame(false)
                        .hint_text("Type to imagine"),
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
                            let style_inner = egui::Frame::none()
                                .fill(crate::theme::panel())
                                .rounding(crate::theme::IMAGINE_HIT)
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
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
                                self.imagine_style_anchor = style.rect;
                                self.imagine_menu_ignore = true;
                            }
                            let aspect = imagine_aspect_label(self.imagine_aspect);
                            let aspect_name = imagine_aspect_name(self.imagine_aspect);
                            let aspect_inner = egui::Frame::none()
                                .fill(crate::theme::panel())
                                .rounding(crate::theme::IMAGINE_HIT)
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
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
                                self.imagine_aspect_anchor = aspect_hit.rect;
                                self.imagine_menu_ignore = true;
                            }
                            if !authed && crate::cards::ghost_pill(ui, "Connect Grok") {
                                out.go_settings = true;
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
            });
        out
    }

    pub(super) fn poll_wall(&mut self) {
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
        if !wall_can_paint(
            self.has_key(),
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
        let key = self.bearer();
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
            let _ = tx.send(paint_wall_cover(
                &key, &model, &id, &dir, &title, &prompt, &prompt_b, tall, created_ms,
            ));
        });
    }
}
