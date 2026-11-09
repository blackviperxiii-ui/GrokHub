//! Titlebar bell: a session-only list of what the cabin told you.
//!
//! Archive / unarchive, diagnostics saves, finished background tasks, game
//! and print alerts, the GPU fallback line and the Saved / finished / error
//! status lines land here, and only here: no strip above the composer and no
//! popup over the window. Each row says what happened and where (the page,
//! Settings section, chat or game it came from), and clicking it opens that
//! thing or place. A row can also carry one action (Undo, Show in folder).
//! Nothing is written to disk.

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
    /// Open the folder that holds this file, with the file selected.
    ShowInFolder(std::path::PathBuf),
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
            NoticeAction::ShowInFolder(_) => crate::desktop::reveal_label(),
            NoticeAction::SaveAsSkill(_) => "Save as skill",
            NoticeAction::RestoreCards(_) => "Undo",
            NoticeAction::OpenCard(_) => "Open card",
            NoticeAction::OpenSettings(_) => "Settings",
        }
    }
}

/// Where a notice came from: shown under its words, and where a click on
/// the row goes (a game only names itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NoticePlace {
    Page(Nav),
    Settings(SettingsSec),
    Chat { id: String, title: String },
    Game(String),
}

impl NoticePlace {
    /// "Settings › Appearance", "Chat: Trip plans", "Workboard".
    pub(super) fn name(&self) -> String {
        match self {
            NoticePlace::Page(nav) => super::settings::nav_name(*nav).to_string(),
            NoticePlace::Settings(sec) => {
                format!(
                    "Settings \u{203a} {}",
                    super::settings::settings_sec_title(*sec)
                )
            }
            NoticePlace::Chat { title, .. } => {
                let title = title.trim();
                if title.is_empty() {
                    "Chat".into()
                } else {
                    format!("Chat: {title}")
                }
            }
            NoticePlace::Game(name) => name.clone(),
        }
    }

    /// A page, Settings section or chat can be opened; a game can't.
    pub(super) fn opens(&self) -> bool {
        !matches!(self, NoticePlace::Game(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Notice {
    pub(super) at: u64,
    pub(super) kind: NoticeKind,
    pub(super) text: String,
    /// Where it came from, shown under the words; a click goes there.
    pub(super) from: Option<NoticePlace>,
    pub(super) action: Option<NoticeAction>,
    /// How many times the same words came in a row (folded into one row).
    pub(super) count: u32,
}

impl Notice {
    /// The row's words, with "(x3)" when the same line came three times.
    pub(super) fn shown_text(&self) -> String {
        if self.count > 1 {
            format!("{} (x{})", self.text, self.count)
        } else {
            self.text.clone()
        }
    }
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
        // The same words again fold into the newest row instead of stacking.
        if let Some(top) = self.items.first_mut() {
            if top.kind == notice.kind
                && top.text == notice.text
                && top.from == notice.from
                && top.action == notice.action
            {
                top.at = notice.at;
                top.count = top.count.saturating_add(notice.count.max(1));
                self.unread = self.unread.max(1);
                return;
            }
        }
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
        // The line stays up as you move on, so only a change is news.
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

/// The words a mirrored status line lands with. A bare "Saved" names what
/// was saved from where it happened; any other line already says what.
pub(super) fn described_status(status: &str, from: Option<&NoticePlace>) -> String {
    if status.trim().trim_end_matches('.') != "Saved" {
        return status.to_string();
    }
    match from {
        Some(NoticePlace::Settings(sec)) => format!(
            "Saved your {} settings",
            super::settings::settings_sec_title(*sec)
        ),
        Some(NoticePlace::Chat { title, .. }) if !title.trim().is_empty() => {
            format!("Saved in the chat {}", title.trim())
        }
        Some(NoticePlace::Page(nav)) => {
            format!("Saved your changes on {}", super::settings::nav_name(*nav))
        }
        _ => "Saved your changes".into(),
    }
}

/// An internet drop and its return are about the whole app, not a page.
fn is_net_line(status: &str) -> bool {
    status.starts_with("Offline since ") || status.starts_with("Back online.")
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
    /// The one way into the bell: what happened, where it came from (a click
    /// on the row goes there), and an optional action.
    pub(super) fn notify(
        &mut self,
        kind: NoticeKind,
        text: String,
        from: Option<NoticePlace>,
        action: Option<NoticeAction>,
    ) {
        self.notices.push(Notice {
            at: now_ms(),
            kind,
            text,
            from,
            action,
            count: 1,
        });
    }

    /// The page, Settings section or chat on screen now.
    pub(super) fn notice_here(&self) -> NoticePlace {
        match self.nav {
            Nav::Settings => NoticePlace::Settings(self.settings_sec),
            Nav::Chat => match self.threads.get(self.thread_idx) {
                Some(t) => NoticePlace::Chat {
                    id: t.id.clone(),
                    title: t.title.clone(),
                },
                None => NoticePlace::Page(Nav::Chat),
            },
            nav => NoticePlace::Page(nav),
        }
    }

    /// A chat by id, with its title as the row shows it.
    pub(super) fn notice_chat(&self, id: &str) -> NoticePlace {
        let title = self
            .threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        NoticePlace::Chat {
            id: id.to_string(),
            title,
        }
    }

    /// Each frame: a new Saved / finished / error status line is kept in the
    /// bell, with the place it was set on so the row says where and opens it.
    pub(super) fn mirror_status_notice(&mut self) {
        let status = self.status.trim().to_string();
        if let Some(kind) = self.notices.status_to_log(&status) {
            let from = (!is_net_line(&status)).then(|| self.notice_here());
            let text = described_status(&status, from.as_ref());
            self.notify(kind, text, from, None);
        }
    }

    /// Go where a notice came from.
    pub(super) fn open_notice_place(&mut self, place: &NoticePlace) {
        match place {
            NoticePlace::Page(nav) => self.nav = *nav,
            NoticePlace::Settings(sec) => {
                self.nav = Nav::Settings;
                self.settings_sec = *sec;
            }
            NoticePlace::Chat { id, .. } => {
                if let Some(idx) = self.threads.iter().position(|t| &t.id == id) {
                    self.switch_thread(idx);
                }
                self.nav = Nav::Chat;
            }
            NoticePlace::Game(_) => {}
        }
    }

    /// A click on a row's words opens the thing: the row's own Open-type
    /// action (card, folder, Settings) if it has one, else where it came from.
    pub(super) fn click_notice(&mut self, idx: usize) {
        let Some(n) = self.notices.items.get(idx) else {
            return;
        };
        let opens_thing = matches!(
            n.action,
            Some(
                NoticeAction::OpenCard(_)
                    | NoticeAction::ShowInFolder(_)
                    | NoticeAction::OpenSettings(_)
            )
        );
        if opens_thing {
            // These stay on the row: opening something twice is fine.
            let action = n.action.clone();
            self.run_notice_action(idx);
            if let Some(n) = self.notices.items.get_mut(idx) {
                n.action = action;
            }
            return;
        }
        if let Some(place) = n.from.clone().filter(NoticePlace::opens) {
            self.open_notice_place(&place);
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
            NoticeAction::ShowInFolder(path) => self.show_in_folder(&path),
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
            // The unread number, so "something new" also says how much.
            let galley = ui.painter().layout_no_wrap(
                unread_badge(self.notices.unread),
                egui::FontId::proportional(crate::theme::TYPE_BADGE),
                egui::Color32::WHITE,
            );
            let w = (galley.size().x + 6.0).max(13.0);
            let badge = egui::Rect::from_center_size(
                egui::pos2(resp.rect.right() - 8.0, resp.rect.top() + 9.0),
                egui::vec2(w, 13.0),
            );
            ui.painter()
                .rect_filled(badge, 6.5, crate::theme::signal_red());
            ui.painter().galley(
                badge.center() - galley.size() / 2.0,
                galley,
                egui::Color32::WHITE,
            );
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
        let mut open: Option<usize> = None;
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
                                let place = n.from.as_ref();
                                let opens = notice_opens(n);
                                // The pill on the right: the row's own action,
                                // else Open when it has somewhere to go.
                                let pill = n
                                    .action
                                    .as_ref()
                                    .map(NoticeAction::label)
                                    .or(opens.then_some("Open"));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(label) = pill {
                                            if crate::cards::ghost_pill(ui, label) {
                                                if n.action.is_some() {
                                                    run = Some(i);
                                                } else {
                                                    open = Some(i);
                                                }
                                            }
                                        }
                                        ui.with_layout(
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                ui.spacing_mut().item_spacing.y = 2.0;
                                                let words = ui.add(
                                                    egui::Label::new(
                                                        RichText::new(n.shown_text())
                                                            .size(crate::theme::FONT_META)
                                                            .color(crate::theme::fg()),
                                                    )
                                                    .wrap()
                                                    .sense(if opens {
                                                        egui::Sense::click()
                                                    } else {
                                                        egui::Sense::hover()
                                                    }),
                                                );
                                                if let Some(p) = place {
                                                    ui.label(
                                                        RichText::new(p.name())
                                                            .size(crate::theme::FONT_META)
                                                            .color(crate::theme::muted()),
                                                    );
                                                }
                                                if opens
                                                    && words
                                                        .on_hover_cursor(
                                                            egui::CursorIcon::PointingHand,
                                                        )
                                                        .clicked()
                                                {
                                                    open = Some(i);
                                                }
                                            },
                                        );
                                    },
                                );
                            });
                        }
                    });
            });
        if clear {
            self.notices.clear();
        }
        if let Some(i) = run {
            self.run_notice_action(i);
        } else if let Some(i) = open {
            self.click_notice(i);
        }
        if open.is_some() {
            // It took you somewhere: the list closes behind you.
            egui::Popup::close_id(ui.ctx(), id);
        }
    }
}

/// A click on the row's words opens something: an Open-type action, or the
/// page, Settings section or chat it came from.
pub(super) fn notice_opens(n: &Notice) -> bool {
    matches!(
        n.action,
        Some(
            NoticeAction::OpenCard(_)
                | NoticeAction::ShowInFolder(_)
                | NoticeAction::OpenSettings(_)
        )
    ) || n.from.as_ref().is_some_and(NoticePlace::opens)
}

/// The number on the bell: hidden at zero, "99+" past ninety-nine.
pub(super) fn unread_badge(unread: usize) -> String {
    match unread {
        0 => String::new(),
        1..=99 => unread.to_string(),
        _ => "99+".into(),
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
            from: None,
            action: None,
            count: 1,
        }
    }

    #[test]
    fn the_same_words_fold_into_one_row() {
        let mut n = Notices::default();
        n.push(note("Back online."));
        n.push(note("Back online."));
        n.push(note("Back online."));
        assert_eq!(n.items.len(), 1);
        assert_eq!(n.items[0].shown_text(), "Back online. (x3)");
        assert_eq!(n.unread, 1);
        n.mark_read();
        n.push(note("Back online."));
        assert_eq!(n.unread, 1, "a repeat after reading is new again");
        n.push(note("Offline since 02:01"));
        n.push(note("Back online."));
        assert_eq!(
            n.items.iter().map(Notice::shown_text).collect::<Vec<_>>(),
            ["Back online.", "Offline since 02:01", "Back online. (x4)"]
        );
        // A row with its own Undo never folds into one without.
        n.push(Notice {
            action: Some(NoticeAction::Unarchive("t1".into())),
            ..note("Back online.")
        });
        assert_eq!(n.items.len(), 4);
    }

    #[test]
    fn the_bell_number_hides_at_zero_and_caps() {
        assert_eq!(unread_badge(0), "");
        assert_eq!(unread_badge(1), "1");
        assert_eq!(unread_badge(99), "99");
        assert_eq!(unread_badge(120), "99+");
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
    fn a_bare_saved_says_what_was_saved_and_opens_where() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("bell-saved-says-what");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        // A settings toggle on Settings > Appearance.
        cabin.nav = Nav::Settings;
        cabin.settings_sec = SettingsSec::Appearance;
        cabin.status = "Saved".into();
        cabin.mirror_status_notice();
        let n = &cabin.notices.items[0];
        assert_eq!(n.text, "Saved your Appearance settings");
        assert_eq!(n.from, Some(NoticePlace::Settings(SettingsSec::Appearance)));
        assert_eq!(
            n.from.as_ref().unwrap().name(),
            "Settings \u{203a} Appearance"
        );
        assert!(notice_opens(n), "the row opens the section it came from");
        // A save on the Workboard names the page.
        cabin.nav = Nav::Workboard;
        cabin.status = "Pinned General".into();
        cabin.mirror_status_notice();
        cabin.status = "Saved".into();
        cabin.mirror_status_notice();
        assert_eq!(
            cabin.notices.items[0].text,
            "Saved your changes on Workboard"
        );
        // A line that already says what keeps its words and still says where.
        cabin.nav = Nav::History;
        cabin.status = "Rename failed: disk full".into();
        cabin.mirror_status_notice();
        assert_eq!(cabin.notices.items[0].text, "Rename failed: disk full");
        assert_eq!(
            cabin.notices.items[0].from,
            Some(NoticePlace::Page(Nav::History))
        );
        // The internet line is about the whole app, so it names no page.
        cabin.status = "Back online. The internet dropped at 02:01 for 5 min.".into();
        cabin.mirror_status_notice();
        assert_eq!(cabin.notices.items[0].from, None);
        assert!(!notice_opens(&cabin.notices.items[0]));
        // Clicking the Appearance row from elsewhere goes back there.
        cabin.nav = Nav::Chat;
        let idx = cabin
            .notices
            .items
            .iter()
            .position(|n| n.text == "Saved your Appearance settings")
            .unwrap();
        cabin.click_notice(idx);
        assert_eq!(
            (cabin.nav, cabin.settings_sec),
            (Nav::Settings, SettingsSec::Appearance)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_saved_in_chat_names_the_chat_and_opens_it() {
        let place = NoticePlace::Chat {
            id: "t1".into(),
            title: "Trip plans".into(),
        };
        assert_eq!(
            described_status("Saved", Some(&place)),
            "Saved in the chat Trip plans"
        );
        assert_eq!(place.name(), "Chat: Trip plans");
        assert!(place.opens());
        let game = NoticePlace::Game("World of Warcraft".into());
        assert_eq!(game.name(), "World of Warcraft");
        assert!(!game.opens(), "a game names itself but has no page to open");
        assert_eq!(described_status("Saved", None), "Saved your changes");
    }

    #[test]
    fn an_open_type_row_opens_its_thing_and_keeps_the_pill() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("bell-open-thing");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        cabin.notify(
            NoticeKind::Info,
            "Sign in to use online models".into(),
            Some(NoticePlace::Settings(SettingsSec::AiModels)),
            Some(NoticeAction::OpenSettings(SettingsSec::AiModels)),
        );
        cabin.click_notice(0);
        assert_eq!(
            (cabin.nav, cabin.settings_sec),
            (Nav::Settings, SettingsSec::AiModels)
        );
        assert_eq!(
            cabin.notices.items[0].action,
            Some(NoticeAction::OpenSettings(SettingsSec::AiModels)),
            "opening something can be done again"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// Every direct post into the bell says where it came from, so no row
    /// lands as bare words with nowhere to go.
    #[test]
    fn every_bell_post_names_where_it_came_from() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
                continue;
            }
            // Windows checkouts can carry CRLF line endings.
            let src = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            // Production code only: stop at the file's own test module.
            let src = src.split("#[cfg(test)]\nmod tests").next().unwrap();
            for (at, _) in src.match_indices(".notify(") {
                let args = call_args(&src[at + ".notify(".len()..]);
                assert_eq!(
                    args.len(),
                    4,
                    "{name}: notify takes kind, text, place, action"
                );
                assert_ne!(
                    args[2].trim(),
                    "None",
                    "{name}: a bell post must say where it came from: {args:?}"
                );
                checked += 1;
            }
        }
        assert!(checked >= 15, "found only {checked} bell posts");
    }

    /// The top-level arguments of a call, from just after its "(".
    fn call_args(rest: &str) -> Vec<String> {
        let (mut depth, mut cur, mut out) = (0i32, String::new(), Vec::new());
        let mut in_str = false;
        let mut prev = ' ';
        for ch in rest.chars() {
            if in_str {
                cur.push(ch);
                if ch == '"' && prev != '\\' {
                    in_str = false;
                }
                prev = ch;
                continue;
            }
            match ch {
                '"' => {
                    in_str = true;
                    cur.push(ch);
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    cur.push(ch);
                }
                ')' | ']' | '}' if depth == 0 => {
                    if !cur.trim().is_empty() {
                        out.push(cur.trim().to_string());
                    }
                    return out;
                }
                ')' | ']' | '}' => {
                    depth -= 1;
                    cur.push(ch);
                }
                ',' if depth == 0 => out.push(std::mem::take(&mut cur).trim().to_string()),
                _ => cur.push(ch),
            }
            prev = ch;
        }
        out
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
        // Local AI becomes ready: the same note changes to the local-only
        // wording, back on top and unread although the bell was read.
        cabin.notices.mark_read();
        super::super::native_engine::set_local_ready_for_test(Some(true));
        cabin.sync_auth_notice();
        cabin.sync_auth_notice();
        assert_eq!(cabin.notices.items.len(), 1);
        assert_eq!(cabin.notices.unread, 1);
        assert_eq!(
            cabin.notices.items[0].text,
            super::super::pages::LOCAL_ONLY_NOTE
        );
        // Signed in (a key): nothing more is posted, and the state clears.
        cabin.cfg.api_key = "sk-abcdefghijklmnopqrstuv".into();
        cabin.sync_auth_notice();
        assert_eq!(cabin.notices.items.len(), 1);
        assert_eq!(cabin.notices.auth_note, None);
        // Signed out again later: still one note, not a second copy.
        cabin.cfg.api_key.clear();
        cabin.sync_auth_notice();
        assert_eq!(cabin.notices.items.len(), 1);
        assert_eq!(
            cabin.notices.items[0].text,
            super::super::pages::LOCAL_ONLY_NOTE
        );
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
