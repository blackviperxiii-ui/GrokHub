//! Rail, titlebar, and avatar.

use super::*;

pub(super) const RAIL_FOOTER_H: f32 = 40.0;
use crate::ui_kit::{card_frame, chip, empty_state, CardVariant};

/// Shown at the right of the chat flyout Search row: the key that opens the palette.
pub(super) const SEARCH_KEY_HINT: &str = "Ctrl+K";

fn rail_section_caption(ui: &mut egui::Ui, label: RichText) {
    ui.label(
        label
            .size(crate::theme::TYPE_CAPTION)
            .color(crate::theme::blend_color(
                crate::theme::fg(),
                crate::theme::bg(),
                0.35,
            )),
    );
}

pub(super) fn fit_rail_label(ui: &egui::Ui, label: &str, max_w: f32) -> String {
    let font = egui::FontId::proportional(crate::theme::FONT_CHROME);
    let fits = |s: &str| {
        ui.fonts_mut(|f| f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE))
            .size()
            .x
            <= max_w
    };
    if fits(label) {
        return label.to_string();
    }
    let mut t = label.to_string();
    while t.pop().is_some() {
        let candidate = format!("{}…", t.trim_end());
        if fits(&candidate) {
            return candidate;
        }
    }
    "…".into()
}

impl Cabin {
    pub(super) fn paint_loadout_window_border(ui: &mut egui::Ui) {
        if !crate::theme::is_loadout_paint() {
            return;
        }
        let rect = ui.max_rect();
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, crate::theme::signal_red()),
            egui::StrokeKind::Inside,
        );
    }

    pub(super) fn ui_titlebar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut run_pending_update = false;
        let update_chip = update_chip_label(self.update_pending_now());
        let loadout = crate::theme::is_loadout_paint();
        let bar_fill = if loadout {
            crate::theme::surface()
        } else {
            crate::theme::bg()
        };
        egui::Panel::top("titlebar")
            .exact_size(crate::theme::TITLEBAR_H)
            .frame(egui::Frame::NONE.fill(bar_fill))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    if loadout {
                        self.paint_loadout_titlebar_brand(ui);
                    } else {
                        ui.label(
                            RichText::new(grokhub_core::brand::product_name())
                                .font(crate::theme::title_font(crate::theme::FONT_CHROME))
                                .color(crate::theme::fg()),
                        );
                    }
                    if let Some(label) = update_chip {
                        ui.add_space(8.0);
                        if crate::cards::titlebar_update_chip(ui, label) {
                            run_pending_update = true;
                        }
                    }
                    let clock = Self::local_clock();
                    if let Some(quiet) =
                        quiet_until_chip(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end)
                    {
                        ui.add_space(8.0);
                        if crate::cards::titlebar_update_chip(ui, &quiet) {
                            self.nav = Nav::Settings;
                            self.settings_sec = SettingsSec::Behavior;
                        }
                    }
                    if loadout {
                        ui.add_space(8.0);
                        self.paint_loadout_titlebar_pills(ui);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let hide = crate::tray::should_hide_on_close(
                            self.cfg.close_to_tray,
                            self.tray.is_some(),
                        ) && !self.want_quit;
                        if titlebar_chrome_hit(
                            &titlebar_chrome_btn(ui, ChromeBtn::Close).on_hover_text(
                                crate::titlebar::titlebar_chrome_tip(ChromeBtn::Close, hide),
                            ),
                        ) {
                            if hide {
                                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                                self.hide_to_tray(&ctx);
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        let max_kind = if self.win_max {
                            ChromeBtn::Restore
                        } else {
                            ChromeBtn::Maximize
                        };
                        if titlebar_chrome_hit(
                            &titlebar_chrome_btn(ui, max_kind).on_hover_text(
                                crate::titlebar::titlebar_chrome_tip(max_kind, false),
                            ),
                        ) {
                            self.toggle_maximize(&ctx);
                        }
                        if titlebar_chrome_hit(
                            &titlebar_chrome_btn(ui, ChromeBtn::Minimize).on_hover_text(
                                crate::titlebar::titlebar_chrome_tip(ChromeBtn::Minimize, false),
                            ),
                        ) {
                            self.resume_fresh = true;
                            self.saw_minimized = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        // RTL: allocated after minimize, so it sits immediately to its left.
                        let _ = self.paint_session_actions_menu(ui);
                        if !loadout {
                            self.paint_personality_switch(ui);
                        }
                        let (_rect, drag) = ui.allocate_exact_size(
                            ui.available_size(),
                            egui::Sense::click_and_drag(),
                        );
                        if titlebar_should_start_drag(drag.drag_started()) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        if drag.double_clicked() {
                            self.toggle_maximize(&ctx);
                        }
                    });
                });
                let hair = ui.max_rect();
                ui.painter().hline(
                    hair.x_range(),
                    hair.bottom() - 0.5,
                    egui::Stroke::new(1.0_f32, crate::theme::border()),
                );
            });
        if run_pending_update {
            self.open_update_overlay();
            self.queue_cabin_update();
        }
    }

    fn paint_loadout_titlebar_brand(&self, ui: &mut egui::Ui) {
        let mark = crate::theme::loadout_mark(ui.ctx());
        let size = egui::vec2(24.0, 24.0);
        ui.add(egui::Image::from_texture((mark.id(), size)));
        ui.add_space(8.0);
        ui.label(
            RichText::new(grokhub_core::brand::product_name().to_ascii_uppercase())
                .font(crate::theme::loadout_font(15.0))
                .color(crate::theme::fg()),
        );
    }

    fn paint_loadout_titlebar_pills(&mut self, ui: &mut egui::Ui) {
        let name = self.titlebar_personality_label();
        let sent = crate::theme::titlebar_sent_label(self.titlebar_egress_bytes());
        for (label, red_ring) in [
            (format!("PERSONALITY · {}", name.to_ascii_uppercase()), true),
            (sent, false),
        ] {
            let pill = ui.add(
                egui::Label::new(
                    RichText::new(label)
                        .font(crate::theme::loadout_font(11.0))
                        .color(crate::theme::steel()),
                )
                .sense(egui::Sense::click()),
            );
            let rect = pill.rect;
            let stroke = if red_ring {
                crate::theme::signal_red()
            } else {
                crate::theme::border()
            };
            ui.painter().rect_stroke(
                rect.expand2(egui::vec2(6.0, 3.0)),
                20.0,
                egui::Stroke::new(1.0, stroke),
                egui::StrokeKind::Outside,
            );
            if red_ring && pill.clicked() {
                self.nav = Nav::Settings;
                self.settings_sec = SettingsSec::Personality;
            }
        }
    }

    fn titlebar_egress_bytes(&self) -> u64 {
        // Egress lines carry no payload sizes; the deck chip shows 0 B until byte totals exist.
        0
    }

    fn toggle_maximize(&mut self, ctx: &egui::Context) {
        let currently = maximized_now(ctx.input(|i| i.viewport().maximized), self.win_max);
        self.win_max = next_maximized(currently);
        self.cfg.window.maximized = self.win_max;
        self.geom_dirty = true;
        #[cfg(windows)]
        {
            let ppp = ctx.pixels_per_point().max(0.5);
            if self.win_max {
                if let Some((x, y, w, h)) = crate::win_native::work_area() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        w as f32 / ppp,
                        h as f32 / ppp,
                    )));
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                        x as f32 / ppp,
                        y as f32 / ppp,
                    )));
                    let _ = crate::win_native::show_cabin(x, y, w, h, true);
                }
            } else {
                let g = crate::window::clamp_geom(self.cfg.window);
                let x = (g.x.unwrap_or(100.0) * ppp).round() as i32;
                let y = (g.y.unwrap_or(100.0) * ppp).round() as i32;
                let w = (g.w * ppp).round() as i32;
                let h = (g.h * ppp).round() as i32;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(g.w, g.h)));
                if let Some([lx, ly]) = crate::window::launch_pos(&g) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(lx, ly)));
                }
                let _ = crate::win_native::show_cabin(x, y, w, h, false);
            }
        }
        #[cfg(not(windows))]
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.win_max));
        }
    }

    fn chat_rename_act(&mut self, ui: &mut egui::Ui, idx: usize) -> Option<TabAct> {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.rename_buf)
                .desired_width(ui.available_width())
                .hint_text(crate::theme::hint("Name this chat")),
        );
        if self.rename_focus {
            edit.request_focus();
            self.rename_focus = false;
        }
        if let Some(lock) = self.rename_lock.clone() {
            if self.rename_buf == lock {
                select_all_edit(ui, edit.id, &self.rename_buf);
            } else {
                self.rename_lock = None;
            }
        }
        if ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
            drop_key(ui, egui::Key::Escape);
            Some(TabAct::CancelRename)
        } else if ui.input(|inp| inp.key_pressed(egui::Key::Enter))
            || (edit.lost_focus() && !self.rename_focus)
        {
            drop_key(ui, egui::Key::Enter);
            Some(TabAct::CommitRename(idx))
        } else {
            None
        }
    }

    pub(super) fn icon_nav_row(
        ui: &mut egui::Ui,
        active: bool,
        icon: crate::icons::RailIcon,
        tip: &str,
    ) -> egui::Response {
        let tile = crate::theme::RAIL_TILE;
        let (slot, resp) = ui.allocate_exact_size(
            egui::vec2(crate::theme::SIDEBAR_W, tile),
            egui::Sense::click(),
        );
        if !ui.is_rect_visible(slot) {
            return resp;
        }
        let center = slot.center();
        let icon_rect = egui::Rect::from_center_size(
            center,
            egui::vec2(crate::theme::ICON_ACTION, crate::theme::ICON_ACTION),
        );
        let tile_rect = egui::Rect::from_center_size(center, egui::vec2(tile, tile));
        let color = if crate::theme::is_loadout_paint() {
            use crate::ui_kit::recipe::{look, paint_mark, Part, State, Mark};
            let st = if active {
                State::Selected
            } else if resp.hovered() {
                State::Hover
            } else {
                State::Default
            };
            let lk = look(Part::Nav, st);
            if st == State::Selected {
                ui.painter().rect_filled(tile_rect, lk.radius, lk.fill);
                let inner = slot.with_min_x(slot.left() + crate::ui_kit::recipe::MARK_W + 4.0);
                paint_mark(ui.painter(), inner, Mark::LeftBar);
            } else if st == State::Hover {
                ui.painter().rect_filled(tile_rect, lk.radius, lk.fill);
            } else if lk.stroke.width > 0.0 {
                ui.painter().rect_stroke(
                    tile_rect,
                    lk.radius,
                    lk.stroke,
                    egui::StrokeKind::Inside,
                );
            }
            lk.ink
        } else if active {
            ui.painter()
                .rect_filled(tile_rect, 8.0, crate::theme::selected());
            let tick = egui::Rect::from_min_max(
                egui::pos2(slot.left(), slot.top() + 6.0),
                egui::pos2(slot.left() + crate::theme::RAIL_TICK_W, slot.bottom() - 6.0),
            );
            ui.painter().rect_filled(tick, 2.0, crate::theme::amber());
            crate::theme::steel()
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(tile_rect, 8.0, crate::theme::hover());
            crate::theme::muted()
        } else {
            crate::theme::muted()
        };
        crate::icons::paint_rail_icon_at(ui.painter(), icon_rect, icon, color);
        crate::theme::paint_focus_ring(ui, tile_rect, 8.0, &resp);
        resp.on_hover_text(tip)
    }

    pub(super) fn nav_row(
        ui: &mut egui::Ui,
        active: bool,
        icon: crate::icons::RailIcon,
        label: &str,
        outline: bool,
    ) -> egui::Response {
        let color = if active {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        };
        let w = ui.available_width();
        let (slot, resp) =
            ui.allocate_exact_size(egui::vec2(w, crate::theme::NAV_ROW_H), egui::Sense::click());
        let (resp, rect, _) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
        crate::theme::glide_candidate(ui, "rail", rect, resp.hovered(), active);
        // Every chat in the list gets a row. One scrolled out of the rail keeps its slot
        // and response, but skips fitting and painting its title.
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        if outline {
            ui.painter().rect_stroke(
                rect,
                rect.height() * 0.5,
                egui::Stroke::new(1.0_f32, crate::theme::border_strong()),
                egui::StrokeKind::Middle,
            );
        }
        // Slide 07 nav: the selected item carries an amber bar on its left edge.
        // Only the row the glide highlights, so a second active row stays plain.
        if active
            && crate::theme::is_loadout_paint()
            && crate::theme::glide_selected_is(ui, "rail", rect)
        {
            use crate::ui_kit::recipe::{look, paint_mark, Part, State};
            // The mark sits just left of the rect it is given; shift it inside
            // the slot so the rail's clip doesn't cut it off.
            let inner = slot.with_min_x(slot.left() + crate::ui_kit::recipe::MARK_W + 4.0);
            paint_mark(ui.painter(), inner, look(Part::Nav, State::Selected).mark);
        }
        // The hover scale grows the highlight only. Icon and label stay on the resting
        // slot, or a 1.035 scale on a full-width row slides them ~4px left.
        let icon_c = egui::pos2(slot.left() + 20.0, slot.center().y);
        let icon_rect = egui::Rect::from_center_size(icon_c, egui::vec2(20.0, 20.0));
        crate::icons::paint_rail_icon_at(ui.painter(), icon_rect, icon, color);
        let text_left = slot.left() + 38.0;
        let text_right = slot.right() - 12.0;
        let painted = fit_rail_label(ui, label, (text_right - text_left).max(8.0));
        ui.painter().text(
            egui::pos2(text_left, slot.center().y),
            egui::Align2::LEFT_CENTER,
            &painted,
            egui::FontId::proportional(crate::theme::FONT_CHROME),
            color,
        );
        crate::theme::paint_focus_ring(ui, rect, rect.height() * 0.5, &resp);
        if painted != label {
            resp.on_hover_text(label)
        } else {
            resp
        }
    }

    pub(super) fn avatar_chrome(&self) -> AvatarMenu {
        avatar_menu(
            &self.cfg.display_name,
            &self.cfg.profile_picture,
            self.secrets.oauth.as_ref().and_then(|t| t.name.as_deref()),
            &self.greeting_user_md,
        )
    }

    pub(super) fn photo_for_path(&self, picture_path: &str) -> Option<TextureHandle> {
        if picture_path.trim().is_empty() {
            self.oauth_photo.clone()
        } else if self.profile_photo_key == picture_path {
            self.profile_photo.clone()
        } else {
            None
        }
    }

    pub(super) fn cabin_avatar(
        ui: &mut egui::Ui,
        name: &str,
        photo: Option<&TextureHandle>,
    ) -> egui::Response {
        let (_rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), RAIL_FOOTER_H),
            egui::Sense::click(),
        );
        let (resp, rect, wash) = crate::theme::feel_button(ui, resp, egui::Color32::TRANSPARENT);
        if wash.a() > 0 {
            ui.painter().rect_filled(rect, 10.0, wash);
        }
        let c = egui::pos2(rect.left() + 20.0, rect.center().y);
        if let Some(tex) = photo {
            let size = egui::vec2(28.0, 28.0);
            egui::Image::from_texture(tex)
                .fit_to_exact_size(size)
                .corner_radius(14.0)
                .paint_at(ui, egui::Rect::from_center_size(c, size));
        } else {
            ui.painter().circle_filled(c, 14.0, crate::theme::panel());
        }
        ui.painter().circle_stroke(
            c,
            14.0,
            egui::Stroke::new(1.0_f32, crate::theme::border_strong()),
        );
        let text_left = rect.left() + 42.0;
        let text_right = rect.right() - 12.0;
        let painted = fit_rail_label(ui, name, (text_right - text_left).max(8.0));
        ui.painter().text(
            egui::pos2(text_left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            painted,
            egui::FontId::proportional(crate::theme::FONT_META),
            crate::theme::fg(),
        );
        resp
    }

    pub(super) fn ui_sidebar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut chat_icon: Option<egui::Response> = None;
        egui::Panel::left("rail")
            .exact_size(crate::theme::SIDEBAR_W)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::surface())
                    .stroke(egui::Stroke::new(1.0, crate::theme::border())),
            )
            .show(ui, |ui| {
                egui::Panel::bottom("rail-footer")
                    .frame(egui::Frame::NONE)
                    .show_separator_line(false)
                    .exact_size(RAIL_FOOTER_H)
                    .show(ui, |ui| {
                        let settings = Self::icon_nav_row(
                            ui,
                            self.nav == Nav::Settings,
                            crate::icons::RailIcon::Settings,
                            "Settings",
                        );
                        if settings.clicked() {
                            self.set_nav_id("settings");
                        }
                    });
                let rest = ui.available_rect_before_wrap();
                ui.set_clip_rect(rest.intersect(ui.clip_rect()));
                ui.add_space(4.0);
                crate::theme::glide_paint(ui, "rail");
                let cur = self.nav_id();
                for (id, label) in crate::theme::LOADOUT_NAV {
                    let active = cur == *id;
                    let resp =
                        Self::icon_nav_row(ui, active, crate::icons::rail_icon_for(id), label);
                    if *id == "chat" {
                        chat_icon = Some(resp.clone());
                    }
                    if resp.secondary_clicked() && *id == "chat" {
                        self.rail_chat_drawer_open = !self.rail_chat_drawer_open;
                    } else if resp.clicked() {
                        match *id {
                            "home" => self.open_fresh_home(),
                            "profile" => {
                                self.settings_menu_open = !self.settings_menu_open;
                                self.settings_menu_ignore = true;
                            }
                            "chat" => {
                                if self.rail_home_active() {
                                    self.rail_chat_drawer_open = !self.rail_chat_drawer_open;
                                } else {
                                    self.set_nav_id(id);
                                }
                            }
                            _ => self.set_nav_id(id),
                        }
                    }
                }
                crate::theme::glide_aim(ui, "rail");
            });
        if self.rail_chat_drawer_open {
            self.ui_rail_chat_drawer(&ctx, chat_icon);
        }
    }

    fn rail_home_active(&self) -> bool {
        self.nav == Nav::Chat && self.messages.is_empty() && !self.running
    }

    fn ui_rail_chat_drawer(&mut self, ctx: &egui::Context, anchor: Option<egui::Response>) {
        let anchor = anchor.map(|r| r.rect).unwrap_or_else(|| {
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0))
        });
        let w = 260.0_f32;
        egui::Area::new(egui::Id::new("rail-chat-drawer"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(
                anchor.right(),
                anchor.top() - crate::theme::RAIL_TILE * 0.5,
            ))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(crate::theme::bg())
                    .stroke(egui::Stroke::new(1.0, crate::theme::signal_red()))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(w);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Chat")
                                    .font(crate::theme::title_font(crate::theme::FONT_SECTION))
                                    .color(crate::theme::fg()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if crate::theme::felt_icon_hit(
                                        ui,
                                        "×",
                                        22.0,
                                        crate::theme::muted(),
                                        16.0,
                                    )
                                    .clicked()
                                    {
                                        self.rail_chat_drawer_open = false;
                                    }
                                },
                            );
                        });
                        let search = Self::nav_row(
                            ui,
                            false,
                            crate::icons::RailIcon::Search,
                            "Search",
                            false,
                        );
                        {
                            let pad = egui::vec2(crate::theme::SPACING_8, crate::theme::SPACING_4);
                            let font = egui::FontId::proportional(crate::theme::TYPE_CAPTION);
                            let text_c = crate::theme::fg();
                            let galley = ui.fonts_mut(|f| {
                                f.layout_no_wrap(SEARCH_KEY_HINT.to_owned(), font, text_c)
                            });
                            let size = galley.size() + pad * 2.0;
                            let rect = egui::Rect::from_center_size(
                                egui::pos2(
                                    search.rect.right() - 12.0 - size.x * 0.5,
                                    search.rect.center().y,
                                ),
                                size,
                            );
                            ui.painter().rect_filled(
                                rect,
                                crate::theme::RADIUS_SM,
                                crate::theme::surface(),
                            );
                            ui.painter().galley(
                                egui::pos2(
                                    rect.min.x + pad.x,
                                    rect.center().y - galley.size().y * 0.5,
                                ),
                                galley,
                                text_c,
                            );
                        }
                        if search.clicked() {
                            self.open_palette();
                        }
                        ui.add_space(crate::theme::SPACING_8);
                        ui.horizontal(|ui| {
                            rail_section_caption(ui, RichText::new("Projects"));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let plus = crate::theme::felt_icon_hit(
                                        ui,
                                        "+",
                                        22.0,
                                        crate::theme::muted(),
                                        16.0,
                                    )
                                    .on_hover_text("New folder");
                                    if plus.clicked() {
                                        self.stage_new_folder();
                                    }
                                },
                            );
                        });
                        self.ui_rail_projects_and_history(ui, w);
                    });
            });
    }

    fn ui_rail_projects_and_history(&mut self, ui: &mut egui::Ui, rail_w: f32) {
        let _ = rail_w;
        let tree = visible_tree(&self.projects);
        let mut proj_act: Option<(String, ProjectMenuAct, egui::Pos2)> = None;
        let mut section_act: Option<TabAct> = None;
        let live_empty = self.messages.is_empty();
        let row_gap = ui.spacing().item_spacing.y;
        let pitch = crate::theme::NAV_ROW_H + row_gap;
        let raw_h = 200.0_f32;
        let proj_h = (((raw_h + row_gap) / pitch).floor().max(1.0) * pitch - row_gap).max(40.0);
        egui::ScrollArea::vertical()
            .id_salt("rail-projects")
            .auto_shrink([false, true])
            .max_height(proj_h)
            .show(ui, |ui| {
                for (depth, idx) in tree {
                    let kind = self.projects[idx].kind;
                    let open = self.projects[idx].open;
                    if self.proj_rename.as_deref() == Some(self.projects[idx].id.as_str()) {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 30.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                crate::icons::paint_tree_gutter(ui, depth);
                                let cancel_w = 78.0;
                                let gap = ui.spacing().item_spacing.x;
                                let edit_w = (ui.available_width() - cancel_w - gap).max(48.0);
                                {
                                    let visuals = ui.visuals_mut();
                                    visuals.weak_text_color = Some(crate::theme::muted());
                                    visuals.text_cursor.stroke =
                                        egui::Stroke::new(2.0_f32, crate::theme::fg());
                                }
                                let edit = ui.add(
                                    egui::TextEdit::singleline(&mut self.proj_rename_buf)
                                        .desired_width(edit_w)
                                        .min_size(egui::vec2(edit_w, 30.0))
                                        .frame(
                                            egui::Frame::NONE
                                                .fill(crate::theme::elevated())
                                                .corner_radius(15.0)
                                                .stroke(egui::Stroke::new(
                                                    2.0_f32,
                                                    crate::theme::fg(),
                                                ))
                                                .inner_margin(egui::Margin::symmetric(10, 6)),
                                        )
                                        .hint_text(crate::theme::hint("Name"))
                                        .font(egui::FontId::proportional(13.0))
                                        .vertical_align(egui::Align::Center),
                                );
                                if self.proj_rename_focus {
                                    edit.request_focus();
                                    if edit.has_focus() {
                                        self.proj_rename_focus = false;
                                    }
                                }
                                if let Some(lock) = self.proj_rename_lock.clone() {
                                    if self.proj_rename_buf == lock {
                                        select_all_edit(ui, edit.id, &self.proj_rename_buf);
                                    } else {
                                        self.proj_rename_lock = None;
                                    }
                                }
                                let cancel_clicked = ui
                                    .allocate_ui_with_layout(
                                        egui::vec2(cancel_w, 30.0),
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| crate::cards::ghost_pill(ui, "Cancel"),
                                    )
                                    .inner;
                                if cancel_clicked {
                                    self.cancel_proj_rename();
                                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    drop_key(ui, egui::Key::Escape);
                                    self.cancel_proj_rename();
                                } else if ui.input(|i| i.key_pressed(egui::Key::Enter))
                                    || (edit.lost_focus() && !self.proj_rename_focus)
                                {
                                    drop_key(ui, egui::Key::Enter);
                                    self.finish_proj_rename();
                                }
                            },
                        );
                        continue;
                    }
                    let show_chats = match kind {
                        ProjectKind::Folder => open,
                        ProjectKind::Project => true,
                    };
                    let nid = self.projects[idx].id.clone();
                    if !Self::reserve_offscreen_rail_row(ui) {
                        let icon = match kind {
                            ProjectKind::Folder => crate::icons::RailIcon::Folder,
                            ProjectKind::Project => crate::icons::RailIcon::Chat,
                        };
                        let active = project_row_active(
                            self.project_sel.as_deref() == Some(self.projects[idx].id.as_str()),
                            kind == ProjectKind::Project,
                            self.nav,
                        );
                        let painted = ui.horizontal(|ui| {
                            crate::icons::paint_tree_gutter(ui, depth);
                            if kind == ProjectKind::Folder {
                                crate::icons::paint_folder_caret(ui, open, crate::theme::subtle());
                            }
                            Self::nav_row(ui, active, icon, &self.projects[idx].name, false)
                        });
                        let row = painted.inner;
                        if row.double_clicked() {
                            self.begin_proj_rename(
                                self.projects[idx].id.clone(),
                                self.projects[idx].name.clone(),
                            );
                        } else if row.clicked()
                            || (kind == ProjectKind::Folder
                                && painted.response.clicked()
                                && !painted.response.double_clicked())
                        {
                            let id = self.projects[idx].id.clone();
                            self.activate_project_row(&id);
                        }
                        let row_pos = row.rect.left_bottom();
                        row.context_menu(|ui| {
                            for a in project_menu_acts(kind) {
                                if ui.button(project_menu_label(*a)).clicked() {
                                    proj_act = Some((nid.clone(), *a, row_pos));
                                    ui.close();
                                }
                            }
                        });
                    }
                    if show_chats {
                        if let Some(act) = self.paint_section_chats(
                            ui,
                            &threads::project_section_chat_indices(
                                &self.threads,
                                &nid,
                                Some(self.thread_idx),
                                live_empty,
                            ),
                            depth.saturating_add(1),
                        ) {
                            section_act = Some(act);
                        }
                    }
                }
            });
        if let Some(act) = section_act {
            self.apply_tab_act(act);
        }
        if let Some((id, act, pos)) = proj_act {
            self.proj_menu_pos = pos;
            self.apply_project_menu(id, act);
        }
        ui.add_space(crate::theme::SPACING_8);
        ui.horizontal(|ui| {
            rail_section_caption(ui, RichText::new("History"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if chip(ui, "See all", true, true).clicked() {
                    self.nav = Nav::History;
                    self.rail_chat_drawer_open = false;
                }
            });
        });
        card_frame(ui, CardVariant::Subtle, true, |ui| {
            crate::cards::search_bar(
                ui,
                &mut self.sidebar_q,
                "Filter chats…",
                (ui.available_width() - crate::theme::SPACING_8).max(80.0),
            );
            ui.add_space(crate::theme::SPACING_4);
            let hist_h = 180.0_f32;
            egui::ScrollArea::vertical()
                .id_salt("rail-history")
                .auto_shrink([false, true])
                .max_height(hist_h)
                .show(ui, |ui| {
                    let q = self.sidebar_q.to_ascii_lowercase();
                    let live_empty = self.messages.is_empty();
                    let listed = threads::chat_section_indices(
                        &self.threads,
                        Some(self.thread_idx),
                        live_empty,
                    );
                    let listed: Vec<usize> = listed
                        .into_iter()
                        .filter(|&i| {
                            q.is_empty()
                                || self.thread_rail_title(i).to_ascii_lowercase().contains(&q)
                        })
                        .collect();
                    if listed.is_empty() {
                        let filtered = !q.is_empty();
                        if filtered {
                            if empty_state(ui, "No matches", "Try another filter.", Some("Clear")) {
                                self.sidebar_q.clear();
                            }
                        } else {
                            let _ = empty_state(
                                ui,
                                "No chats yet",
                                "Start a new chat from the rail.",
                                None,
                            );
                        }
                    } else if let Some(act) = self.paint_section_chats(ui, &listed, 0) {
                        self.apply_tab_act(act);
                    }
                });
        });
    }

    pub(super) fn page_nav(&self) -> Nav {
        if self.nav != Nav::Settings {
            return self.nav;
        }
        if self.settings_back == Nav::Settings {
            Nav::Chat
        } else {
            self.settings_back
        }
    }

    pub(super) fn nav_id(&self) -> &'static str {
        if self.settings_menu_open {
            return "profile";
        }
        if self.page_nav() == Nav::Chat && self.rail_home_active() {
            return "home";
        }
        match self.page_nav() {
            Nav::Chat => "chat",
            Nav::History => "history",
            Nav::Journal => "journal",
            Nav::Imagine => "imagine",
            Nav::Workboard => "workboard",
            Nav::Pulse => "pulse",
            Nav::Settings => "settings",
            Nav::Skills => "skills",
            Nav::Night => "automations",
            Nav::Command => "command",
            Nav::Agents => "queue",
            Nav::Devices => "devices",
            Nav::Memory => "memory",
        }
    }

    /// Switch a GROK_NAV page without side effects (Chat does not open a new thread).
    pub(super) fn select_main_nav(&mut self, id: &str) {
        self.nav = match id {
            "imagine" => {
                self.imagine_want_focus = true;
                Nav::Imagine
            }
            "workboard" => Nav::Workboard,
            "pulse" | "ideas" => Nav::Pulse,
            "skills" => Nav::Skills,
            "automations" => Nav::Night,
            "devices" => Nav::Devices,
            "chat" | "home" => Nav::Chat,
            _ => return,
        };
    }

    pub(super) fn set_nav_id(&mut self, id: &str) {
        self.nav = match id {
            "history" => Nav::History,
            "journal" => Nav::Journal,
            "imagine" => {
                self.imagine_want_focus = true;
                Nav::Imagine
            }
            "workboard" => Nav::Workboard,
            "pulse" | "ideas" => Nav::Pulse,
            "settings" => {
                if self.nav != Nav::Settings {
                    self.settings_back = self.nav;
                }
                self.settings_sec = SettingsSec::Account;
                Nav::Settings
            }
            "skills" => Nav::Skills,
            "automations" => Nav::Night,
            "command" => Nav::Command,
            "queue" => Nav::Agents,
            "devices" => Nav::Devices,
            "memory" => Nav::Memory,
            "eyes" => {
                self.open_recent_chat();
                Nav::Chat
            }
            "connectors" => {
                self.open_connectors();
                Nav::Settings
            }
            "home" => {
                self.open_fresh_home();
                return;
            }
            "profile" => {
                self.settings_menu_open = !self.settings_menu_open;
                self.settings_menu_ignore = true;
                return;
            }
            "chat" => {
                self.new_thread(false);
                Nav::Chat
            }
            _ => Nav::Chat,
        };
    }

    /// Folder click toggles the tree. Project click opens that project's chat.
    pub(super) fn activate_project_row(&mut self, id: &str) {
        let kind = self.projects.iter().find(|n| n.id == id).map(|n| n.kind);
        match kind {
            Some(ProjectKind::Folder) => {
                if let Some(n) = self.projects.iter_mut().find(|n| n.id == id) {
                    n.open = !n.open;
                }
                self.touch_projects();
                self.flush_projects();
            }
            Some(ProjectKind::Project) => {
                self.bind_project_id(id);
                self.open_project_chat(id);
            }
            None => {}
        }
    }

    /// A rail row is `NAV_ROW_H` plus the item spacing a real `horizontal` leaves
    /// under itself. Off-screen rows keep that slot and the one parent auto-id
    /// `horizontal` would burn, and skip the icon, label, and click target.
    fn reserve_offscreen_rail_row(ui: &mut egui::Ui) -> bool {
        if !super::chat_ui::reserve_offscreen_chat_row(ui, crate::theme::NAV_ROW_H) {
            return false;
        }
        ui.add_space(ui.spacing().item_spacing.y);
        ui.skip_ahead_auto_ids(1);
        true
    }

    /// One sidebar chat list. The project section and the chat section both use this.
    /// A click switches, pins, renames, or deletes that chat. It does not take a project
    /// selection, so the other list stays as it was.
    fn paint_section_chats(
        &mut self,
        ui: &mut egui::Ui,
        listed: &[usize],
        depth: u8,
    ) -> Option<TabAct> {
        struct Row {
            title: String,
            key: threads::SessionSortKey,
            idx: usize,
        }
        let mut rows: Vec<Row> = Vec::new();
        for &i in listed {
            let title = self.thread_rail_title(i);
            if threads::is_background_history_title(&title) {
                continue;
            }
            let t = &self.threads[i];
            rows.push(Row {
                title,
                key: threads::SessionSortKey {
                    pinned: t.pinned,
                    pinned_ms: t.pinned_ms,
                    accessed_ms: t.accessed_ms,
                    list_rank: 0,
                },
                idx: i,
            });
        }
        let order = threads::session_list_order(&rows.iter().map(|r| r.key).collect::<Vec<_>>());
        let mut act: Option<TabAct> = None;
        for pos in order {
            let title = rows[pos].title.clone();
            let pinned = rows[pos].key.pinned;
            let i = rows[pos].idx;
            if self.rename_idx == Some(i) {
                ui.horizontal(|ui| {
                    crate::icons::paint_tree_gutter(ui, depth);
                    if let Some(next) = self.chat_rename_act(ui, i) {
                        act = Some(next);
                    }
                });
                continue;
            }
            if Self::reserve_offscreen_rail_row(ui) {
                continue;
            }
            let icon = if pinned {
                crate::icons::RailIcon::Pin
            } else {
                crate::icons::RailIcon::Chat
            };
            let on = i == self.thread_idx && self.nav == Nav::Chat;
            let resp = ui
                .horizontal(|ui| {
                    crate::icons::paint_tree_gutter(ui, depth);
                    Self::nav_row(ui, on, icon, &title, false)
                })
                .inner;
            if resp.clicked() {
                act = Some(TabAct::Switch(i));
            }
            if resp.double_clicked() {
                act = Some(TabAct::StartRename(i));
            }
            let archived = self.threads.get(i).is_some_and(threads::is_archived);
            resp.context_menu(|ui| {
                if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                    act = Some(TabAct::Pin(i));
                    ui.close();
                }
                if ui
                    .button(if archived { "Unarchive" } else { "Archive" })
                    .clicked()
                {
                    act = Some(TabAct::Archive(i));
                    ui.close();
                }
                if ui.button("Rename").clicked() {
                    act = Some(TabAct::StartRename(i));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    act = Some(TabAct::Delete(i));
                    ui.close();
                }
            });
        }
        act
    }

    fn apply_tab_act(&mut self, act: TabAct) {
        match act {
            TabAct::Switch(i) => {
                self.switch_thread(i);
                self.nav = Nav::Chat;
                self.composer_want_focus = true;
            }
            TabAct::Pin(i) => self.pin_thread(i),
            TabAct::Archive(i) => {
                if self.threads.get(i).is_some_and(threads::is_archived) {
                    self.unarchive_thread_at(i);
                } else {
                    self.archive_thread_at(i);
                }
            }
            TabAct::StartRename(i) => self.begin_chat_rename(i),
            TabAct::CommitRename(i) => {
                let name = self.rename_buf.clone();
                self.rename_thread(i, &name);
            }
            TabAct::CancelRename => {
                self.rename_idx = None;
                self.rename_focus = false;
                self.rename_lock = None;
            }
            TabAct::Delete(i) => self.delete_thread_at(i),
        }
    }
}
