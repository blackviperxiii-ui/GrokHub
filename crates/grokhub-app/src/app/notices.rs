//! Titlebar bell: a session-only list of what the cabin told you.
//!
//! Archive / unarchive, diagnostics saves and the status lines that raise a
//! toast land here instead of strips above the composer. Each row can carry
//! one action (Undo, Open folder). Nothing is written to disk.

use super::*;

/// Most notices the bell keeps; older ones drop off the end.
pub(super) const NOTICE_CAP: usize = 50;
/// Popover width, the same as the Activity list.
const BELL_LIST_W: f32 = 340.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NoticeKind {
    Info,
    Success,
    Error,
}

/// What a row's pill does. Undo actions name the chat by id and check its
/// state when run, so an Undo after the chat already moved does nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NoticeAction {
    /// Undo an archive: put the chat back in the list.
    Unarchive(String),
    /// Undo an unarchive: archive the chat again.
    Archive(String),
    /// Open the folder that holds this file.
    OpenFolder(std::path::PathBuf),
    /// Turn this recording folder's stills into a skill draft.
    SaveAsSkill(std::path::PathBuf),
    /// Undo the board sweep: put these archived cards back.
    RestoreCards(Vec<String>),
    /// Open this card on the Workboard.
    OpenCard(String),
    /// Open this Settings section (the sign-in note's pill).
    OpenSettings(SettingsSec),
}

impl NoticeAction {
    pub(super) fn label(&self) -> &'static str {
        match self {
            NoticeAction::Unarchive(_) | NoticeAction::Archive(_) => "Undo",
            NoticeAction::OpenFolder(_) => "Open folder",
            NoticeAction::SaveAsSkill(_) => "Save as skill",
            NoticeAction::RestoreCards(_) => "Undo",
            NoticeAction::OpenCard(_) => "Open card",
            NoticeAction::OpenSettings(_) => "Settings",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Notice {
    pub(super) at: u64,
    pub(super) kind: NoticeKind,
    pub(super) text: String,
    pub(super) action: Option<NoticeAction>,
}

/// Newest first, capped at [`NOTICE_CAP`].
#[derive(Debug, Default)]
pub(super) struct Notices {
    pub(super) items: Vec<Notice>,
    pub(super) unread: usize,
    /// Last status line already mirrored, so one line lands once.
    last_status: String,
    /// The standing sign-in note as last posted (`None` once signed in), so a
    /// signed-out session gets it once, not every heartbeat.
    pub(super) auth_note: Option<&'static str>,
}

impl Notices {
    pub(super) fn push(&mut self, notice: Notice) {
        self.items.insert(0, notice);
        self.items.truncate(NOTICE_CAP);
        self.unread = (self.unread + 1).min(self.items.len());
    }

    pub(super) fn mark_read(&mut self) {
        self.unread = 0;
    }

    pub(super) fn clear(&mut self) {
        self.items.clear();
        self.unread = 0;
    }

    /// Take a row's action so it runs once.
    pub(super) fn take_action(&mut self, idx: usize) -> Option<NoticeAction> {
        self.items.get_mut(idx).and_then(|n| n.action.take())
    }

    /// The chat moved on its own (a reply restored it): its Undo rows go quiet.
    pub(super) fn drop_actions_for(&mut self, thread_id: &str) {
        for n in &mut self.items {
            if matches!(
                &n.action,
                Some(NoticeAction::Unarchive(id) | NoticeAction::Archive(id)) if id == thread_id
            ) {
                n.action = None;
            }
        }
    }

    /// A toast-worthy status line that no notice already says. Returns the
    /// kind to log it as, once per new line.
    fn status_to_log(&mut self, status: &str) -> Option<NoticeKind> {
        if status == self.last_status {
            return None;
        }
        self.last_status = status.to_string();
        if self.items.first().is_some_and(|n| n.text == status) {
            return None;
        }
        match super::status_toasts::status_toast_kind(status)? {
            super::status_toasts::StatusToastKind::Success => Some(NoticeKind::Success),
            super::status_toasts::StatusToastKind::Error => Some(NoticeKind::Error),
        }
    }
}

/// "now", "5m", "3h", "2d" since the notice.
pub(super) fn notice_age(now: u64, at: u64) -> String {
    let s = now.saturating_sub(at) / 1000;
    match s {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

impl Cabin {
    /// The one way into the bell.
    pub(super) fn notify(&mut self, kind: NoticeKind, text: String, action: Option<NoticeAction>) {
        self.notices.push(Notice {
            at: now_ms(),
            kind,
            text,
            action,
        });
    }

    /// Each frame, next to the toast mirror: a new Saved / finished / error
    /// status line is kept in the bell too.
    pub(super) fn mirror_status_notice(&mut self) {
        let status = self.status.trim().to_string();
        if let Some(kind) = self.notices.status_to_log(&status) {
            self.notify(kind, status, None);
        }
    }

    pub(super) fn run_notice_action(&mut self, idx: usize) {
        let Some(action) = self.notices.take_action(idx) else {
            return;
        };
        match action {
            NoticeAction::Unarchive(id) => {
                if let Some(i) = self.threads.iter().position(|t| t.id == id) {
                    if threads::is_archived(&self.threads[i]) {
                        self.unarchive_thread_at(i);
                    }
                }
            }
            NoticeAction::Archive(id) => {
                if let Some(i) = self.threads.iter().position(|t| t.id == id) {
                    if !threads::is_archived(&self.threads[i]) {
                        self.archive_thread_at(i);
                    }
                }
            }
            NoticeAction::OpenFolder(path) => self.open_folder_of(&path),
            NoticeAction::RestoreCards(ids) => {
                let now = now_ms();
                let mut back = 0;
                for id in &ids {
                    if grokhub_core::restore_work_card(&mut self.board, id, now) {
                        back += 1;
                    }
                }
                if back > 0 {
                    self.flush_board();
                }
            }
            NoticeAction::OpenCard(id) => self.open_board_card(&id),
            NoticeAction::OpenSettings(sec) => {
                self.nav = Nav::Settings;
                self.settings_sec = sec;
            }
            NoticeAction::SaveAsSkill(dir) => {
                if super::screen_record_ui::watch_offer_dir().as_deref() != Some(dir.as_path()) {
                    return;
                }
                self.start_watch_learn_extract();
                // Still an offer (no key yet, stills gone): keep the pill for
                // another try.
                if super::screen_record_ui::watch_offer_dir().as_deref() == Some(dir.as_path()) {
                    if let Some(n) = self.notices.items.get_mut(idx) {
                        n.action = Some(NoticeAction::SaveAsSkill(dir));
                    }
                }
            }
        }
    }

    /// Titlebar bell, left of Activity: unread dot, list under it.
    pub(super) fn ui_notice_bell(&mut self, ui: &mut egui::Ui) {
        let color = if self.notices.unread > 0 {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        };
        let resp = crate::icons::glyph_button(
            ui,
            crate::icons::ph::regular::BELL,
            crate::theme::TITLEBAR_H - 8.0,
            color,
            crate::motion::icon::Kick::Lift,
        )
        .on_hover_text(bell_tip(self.notices.unread));
        if self.notices.unread > 0 {
            let c = egui::pos2(resp.rect.right() - 9.0, resp.rect.top() + 9.0);
            ui.painter()
                .circle_filled(c, 3.5, crate::theme::signal_red());
        }
        let id = ui.make_persistent_id("notice-bell-list");
        if resp.clicked() {
            egui::Popup::toggle_id(ui.ctx(), id);
        }
        if !egui::Popup::is_id_open(ui.ctx(), id) {
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            egui::Popup::close_id(ui.ctx(), id);
            return;
        }
        self.notices.mark_read();
        let now = now_ms();
        let mut run: Option<usize> = None;
        let mut clear = false;
        egui::Popup::new(id, ui.ctx().clone(), &resp, ui.layer_id())
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::BOTTOM_END)
            .align_alternatives(&[])
            .show(|ui| {
                ui.set_width(BELL_LIST_W - 24.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Notifications")
                            .strong()
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::fg()),
                    );
                    if !self.notices.items.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::cards::ghost_pill(ui, "Clear all") {
                                clear = true;
                            }
                        });
                    }
                });
                if self.notices.items.is_empty() {
                    ui.label(
                        RichText::new("Nothing yet")
                            .size(crate::theme::FONT_META)
                            .color(crate::theme::muted()),
                    );
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (i, n) in self.notices.items.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let dot = match n.kind {
                                    NoticeKind::Error => crate::theme::signal_red(),
                                    NoticeKind::Success => crate::theme::amber(),
                                    NoticeKind::Info => crate::theme::muted(),
                                };
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(6.0, 6.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(r.center(), 3.0, dot);
                                ui.label(
                                    RichText::new(notice_age(now, n.at))
                                        .size(crate::theme::FONT_META)
                                        .color(crate::theme::muted()),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&n.text)
                                            .size(crate::theme::FONT_META)
                                            .color(crate::theme::fg()),
                                    )
                                    .wrap(),
                                );
                                if let Some(a) = &n.action {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if crate::cards::ghost_pill(ui, a.label()) {
                                                run = Some(i);
                                            }
                                        },
                                    );
                                }
                            });
                        }
                    });
            });
        if clear {
            self.notices.clear();
        }
        if let Some(i) = run {
            self.run_notice_action(i);
        }
    }
}

fn bell_tip(unread: usize) -> String {
    match unread {
        0 => "Notifications".into(),
        1 => "1 new notification".into(),
        n => format!("{n} new notifications"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(text: &str) -> Notice {
        Notice {
            at: 0,
            kind: NoticeKind::Info,
            text: text.into(),
            action: None,
        }
    }

    #[test]
    fn the_bell_keeps_fifty_newest_first() {
        let mut n = Notices::default();
        for i in 0..60 {
            n.push(note(&format!("n{i}")));
        }
        assert_eq!(n.items.len(), NOTICE_CAP);
        assert_eq!(n.items[0].text, "n59");
        assert_eq!(n.items[NOTICE_CAP - 1].text, "n10");
        assert_eq!(n.unread, NOTICE_CAP);
        n.mark_read();
        assert_eq!(n.unread, 0);
        n.push(note("new"));
        assert_eq!(n.unread, 1);
        n.clear();
        assert!(n.items.is_empty() && n.unread == 0);
    }

    #[test]
    fn a_status_line_lands_once_and_only_when_it_toasts() {
        let mut n = Notices::default();
        assert_eq!(n.status_to_log("Pinned General"), None);
        assert_eq!(n.status_to_log("Saved"), Some(NoticeKind::Success));
        n.push(note("Saved"));
        assert_eq!(n.status_to_log("Saved"), None);
        assert_eq!(
            n.status_to_log("Couldn't open folder: gone"),
            Some(NoticeKind::Error)
        );
        // Already said by an explicit notice with the same words.
        n.push(note("Saved x.zip to your Desktop"));
        assert_eq!(n.status_to_log("Saved x.zip to your Desktop"), None);
    }

    #[test]
    fn notice_age_reads_short() {
        assert_eq!(notice_age(10_000, 9_000), "now");
        assert_eq!(notice_age(5 * 60_000, 0), "5m");
        assert_eq!(notice_age(3 * 3_600_000, 0), "3h");
        assert_eq!(notice_age(2 * 86_400_000, 0), "2d");
    }

    #[test]
    fn an_action_runs_once_and_a_reply_quiets_undo() {
        let mut n = Notices::default();
        n.push(Notice {
            action: Some(NoticeAction::Unarchive("t1".into())),
            ..note("Archived A")
        });
        n.push(Notice {
            action: Some(NoticeAction::Archive("t2".into())),
            ..note("Unarchived B")
        });
        n.drop_actions_for("t1");
        assert_eq!(n.items[1].action, None);
        assert_eq!(n.take_action(0), Some(NoticeAction::Archive("t2".into())));
        assert_eq!(n.take_action(0), None);
    }

    fn texts_of(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_sign_in_note_lands_in_the_bell_once_with_a_settings_pill() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("bell-auth-note");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.get_started_done = true;
        super::super::native_engine::set_local_ready_for_test(Some(false));
        // Signed out, no Local AI: one note, with a Settings pill, posted once.
        cabin.sync_auth_notice();
        cabin.sync_auth_notice();
        assert_eq!(cabin.notices.items.len(), 1);
        assert_eq!(
            cabin.notices.items[0].text,
            super::super::pages::NATIVE_SIGNIN_NOTE
        );
        assert_eq!(cabin.notices.items[0].kind, NoticeKind::Info);
        assert_eq!(
            cabin.notices.items[0].action,
            Some(NoticeAction::OpenSettings(SettingsSec::AiModels))
        );
        assert_eq!(
            cabin.notices.items[0].action.as_ref().unwrap().label(),
            "Settings"
        );
        cabin.run_notice_action(0);
        assert_eq!(
            (cabin.nav, cabin.settings_sec),
            (Nav::Settings, SettingsSec::AiModels)
        );
        // Local AI becomes ready: the same note changes to the local-only wording.
        super::super::native_engine::set_local_ready_for_test(Some(true));
        cabin.sync_auth_notice();
        cabin.sync_auth_notice();
        assert_eq!(cabin.notices.items.len(), 1);
        assert_eq!(
            cabin.notices.items[0].text,
            super::super::pages::LOCAL_ONLY_NOTE
        );
        // Signed in (a key): nothing more is posted, and the state clears.
        cabin.cfg.api_key = "sk-abcdefghijklmnopqrstuv".into();
        cabin.sync_auth_notice();
        assert_eq!(cabin.notices.items.len(), 1);
        assert_eq!(cabin.notices.auth_note, None);
        super::super::native_engine::set_local_ready_for_test(None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn nothing_about_signing_in_is_painted_above_the_composer() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("bell-no-composer-strip");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.get_started_done = true;
        cabin.signin_card = false;
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        for local in [Some(false), Some(true)] {
            super::super::native_engine::set_local_ready_for_test(local);
            let out = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
                cabin.ui_native_auth_banner(ui);
            });
            let texts = texts_of(&out.shapes);
            assert!(
                texts.is_empty(),
                "the composer must not carry the sign-in strip (local ready: {local:?}): {texts:?}"
            );
        }
        super::super::native_engine::set_local_ready_for_test(None);
        let _ = std::fs::remove_dir_all(root);
    }
}
