//! Settings page, cabin menu, and save.

use super::*;

/// Settings → Let Grok control the desktop. Names the hard floor next to the switch.
pub(super) static DESKTOP_CONTROL_HINT: std::sync::LazyLock<String> = std::sync::LazyLock::new(
    || {
        format!("{} can see the screen and use the mouse and keyboard. Ask still asks first. Deletes, sends, money and credentials always ask.", grokhub_core::brand::product_name())
    },
);

#[derive(Clone, Default)]
struct PermDraft {
    rule: String,
    action: String,
    grant: String,
}

/// The support URL to link from About, or `None` to hide the link: unset,
/// blank, or anything the browser opener would refuse.
pub(super) fn support_link(url: &str) -> Option<&str> {
    let url = url.trim();
    crate::desktop::url_safe_to_open(url).then_some(url)
}

pub(super) fn settings_group_home(group: SettingsGroup) -> SettingsSec {
    match group {
        SettingsGroup::General => SettingsSec::Account,
        // The About heading opens About (version and build), not Update.
        SettingsGroup::About => SettingsSec::About,
    }
}

pub(super) fn settings_sec_title(sec: SettingsSec) -> &'static str {
    match sec {
        SettingsSec::Account => "Account",
        SettingsSec::Privacy => "Privacy & data",
        SettingsSec::Appearance => "Appearance",
        SettingsSec::Behavior => "Behavior",
        SettingsSec::Update => "Update",
        SettingsSec::About => "About",
        SettingsSec::Defaults => "Defaults",
        SettingsSec::Labs => "Labs",
        SettingsSec::Permissions => "Permissions",
        SettingsSec::Connectors => "Connectors",
        SettingsSec::Personality => "Personality",
        SettingsSec::Playbooks => "Playbooks",
        SettingsSec::Pulse => "Pulse",
        SettingsSec::Background => "Background",
    }
}

/// `/fix` and the palette Health row open Settings → About.
pub(super) fn health_settings_sec() -> SettingsSec {
    SettingsSec::About
}

/// `GROKHUB_DECK_SETTINGS_SEC` pins the Settings section for deck screenshots
/// (`scripts/deck-shots.ps1` / `web-search-shots.ps1`), so Ctrl+, lands on
/// Personality / Behavior / Privacy / Cabin defaults without a fragile click.
pub(super) fn pinned_settings_sec() -> Option<SettingsSec> {
    static PIN: std::sync::OnceLock<Option<SettingsSec>> = std::sync::OnceLock::new();
    super::loadout_home::launch_pin(&PIN, || {
        match super::loadout_home::deck_switch("GROKHUB_DECK_SETTINGS_SEC")
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("personality") => Some(SettingsSec::Personality),
            Some("privacy") => Some(SettingsSec::Privacy),
            Some("behavior") => Some(SettingsSec::Behavior),
            // Cabin defaults holds Models & providers → Web search (optional
            // SearXNG URL / Brave key). Used by web-search-shots.ps1.
            Some("defaults") | Some("cabin") | Some("websearch") | Some("web_search") => {
                Some(SettingsSec::Defaults)
            }
            _ => None,
        }
    })
}

/// Settings modal size: Personality needs room for the slide 19 grid + detail panel.
/// The sheet's smallest and largest size before the window caps it.
const SHEET_MIN: egui::Vec2 = egui::vec2(920.0, 620.0);
const SHEET_MAX: egui::Vec2 = egui::vec2(1280.0, 880.0);
/// Share of the window the sheet takes between those bounds.
const SHEET_SHARE: f32 = 0.8;

pub(super) fn settings_modal_size(sec: SettingsSec, screen: egui::Vec2) -> egui::Vec2 {
    let margin = egui::vec2(48.0, 48.0);
    let max = (screen - margin).max(egui::vec2(320.0, 240.0));
    let want = if sec == SettingsSec::Personality && crate::theme::is_loadout_paint() {
        egui::vec2(1180.0, 720.0)
    } else if sec == SettingsSec::Defaults && super::loadout_home::web_search_settings_shot() {
        // Proof shot: taller pane so Web search (SearXNG URL) stays on screen.
        egui::vec2(920.0, 780.0)
    } else {
        // Grows with the window (maximized gets a bigger sheet), within bounds.
        (screen * SHEET_SHARE).clamp(SHEET_MIN, SHEET_MAX)
    };
    want.min(max)
}

/// Where Settings sits. Loadout fills the window right of the rail and under
/// the titlebar, as a page (deck slide 12 / #966); other themes keep the
/// centred sheet. Keeps the Personality picker inside the window at every size.
pub(super) fn settings_page_rect(screen: egui::Rect, page: bool, sec: SettingsSec) -> egui::Rect {
    if page {
        return egui::Rect::from_min_max(
            screen.min + egui::vec2(crate::theme::SIDEBAR_W, crate::theme::TITLEBAR_H),
            screen.max,
        );
    }
    // The sheet centres under the titlebar, so the window buttons stay live.
    let below = settings_sheet_cover(screen);
    egui::Rect::from_center_size(below.center(), settings_modal_size(sec, below.size()))
}

/// What the sheet dims: the window under the custom titlebar.
pub(super) fn settings_sheet_cover(screen: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        screen.min + egui::vec2(0.0, crate::theme::TITLEBAR_H),
        screen.max,
    )
}

/// True when Settings wasn't on screen last frame: a fresh open, so the page
/// lands on the asked-for section instead of an old scroll position.
fn settings_fresh_view(ctx: &egui::Context) -> bool {
    let id = egui::Id::new("settings-last-view");
    let pass = ctx.cumulative_pass_nr();
    let last = ctx.data_mut(|d| {
        let last = d.get_temp::<u64>(id);
        d.insert_temp(id, pass);
        last
    });
    !matches!(last, Some(at) if at + 1 >= pass)
}

/// The one Settings scroll, every section on it. Back at the top on a fresh
/// open and kept while open; `force` is where the scrollspy's jump puts it
/// this frame.
fn settings_body_scroll(max_h: f32, fresh: bool, force: Option<f32>) -> egui::ScrollArea {
    let body = egui::ScrollArea::vertical()
        .id_salt("settings-body")
        .auto_shrink([false, false])
        .max_height(max_h);
    match force {
        Some(y) => body.vertical_scroll_offset(y),
        None if fresh => body.vertical_scroll_offset(0.0),
        None => body,
    }
}

/// Room between one section's last row and the next section's title.
const SETTINGS_SECTION_GAP: f32 = 28.0;
/// A section becomes the marked one when its top crosses this share of the view.
const SPY_LINE: f32 = 0.30;
/// How far past the line a top must go before the mark moves, either way,
/// so a top resting on the line doesn't flicker between two sections.
const SPY_SLACK: f32 = 12.0;
/// A jump leaves this much room above the section title.
pub(super) const SPY_TOP_PAD: f32 = 8.0;

/// Which section the Settings nav marks, as an index into `tops` (each
/// section's top in scroll content, top to bottom). The last one whose top
/// crossed [`SPY_LINE`] of the view, with [`SPY_SLACK`] of give around
/// `current`. Scrolled to the end, the last one. Nothing laid out yet: the first.
pub(super) fn spy_pick(
    tops: &[f32],
    offset: f32,
    view_h: f32,
    at_end: bool,
    current: usize,
) -> usize {
    if tops.is_empty() {
        return 0;
    }
    let last = tops.len() - 1;
    if at_end {
        return last;
    }
    let current = current.min(last);
    let line = offset + view_h * SPY_LINE;
    let crossed = |line: f32| tops.iter().rposition(|t| *t <= line).unwrap_or(0);
    let ahead = crossed(line - SPY_SLACK);
    if ahead > current {
        return ahead;
    }
    let behind = crossed(line + SPY_SLACK);
    if behind < current {
        return behind;
    }
    current
}

/// A jump the scrollspy is landing: a nav click, a link from elsewhere, or a
/// fresh open on a section.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SpyJump {
    sec: SettingsSec,
    instant: bool,
    from: f32,
    started: Option<f64>,
}

/// The Settings scrollspy's memory between frames.
#[derive(Clone, Debug, Default)]
struct Spy {
    /// Each section's top in scroll content, last frame, top to bottom.
    tops: Vec<(SettingsSec, f32)>,
    offset: f32,
    max_offset: f32,
    /// The section last marked. `settings_sec` set to anything else means a
    /// jump was asked for.
    marked: Option<SettingsSec>,
    jump: Option<SpyJump>,
    /// A jumped-to section stays marked until the scroll leaves where it
    /// landed: the last sections can't reach the line, and a click on them
    /// must still mark them.
    hold: Option<(SettingsSec, Option<f32>)>,
}

impl Spy {
    fn id() -> egui::Id {
        egui::Id::new("settings-scrollspy")
    }

    fn load(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Spy>(Self::id()))
            .unwrap_or_default()
    }

    fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Self::id(), self));
    }

    /// Start of a frame: notice a fresh open or a section asked for from
    /// outside, and return where the jump puts the scroll this frame. A jump
    /// eases over [`crate::motion::SETTINGS_SCROLL_SECS`] and asks for frames
    /// only until it lands; reduced motion and a fresh open land at once.
    fn begin(
        &mut self,
        ctx: &egui::Context,
        opened: bool,
        asked: SettingsSec,
        reduced: bool,
    ) -> Option<f32> {
        if opened {
            *self = Spy::default();
        }
        if opened || self.marked != Some(asked) {
            self.marked = Some(asked);
            self.jump = Some(SpyJump {
                sec: asked,
                instant: opened || reduced,
                from: self.offset,
                started: None,
            });
        }
        let jump = self.jump.as_mut()?;
        self.hold = Some((jump.sec, None));
        let Some(top) = self
            .tops
            .iter()
            .find(|(s, _)| *s == jump.sec)
            .map(|(_, t)| *t)
        else {
            // Nothing laid out yet: one frame from the top, then the jump.
            return opened.then_some(0.0);
        };
        let target = (top - SPY_TOP_PAD).clamp(0.0, self.max_offset);
        let y = if jump.instant {
            target
        } else {
            let now = ctx.input(|i| i.time);
            let started = *jump.started.get_or_insert(now);
            let t = ((now - started) as f32 / crate::motion::SETTINGS_SCROLL_SECS).clamp(0.0, 1.0);
            if t < 1.0 {
                ctx.request_repaint();
                jump.from + (target - jump.from) * egui::emath::easing::cubic_out(t)
            } else {
                target
            }
        };
        if y == target {
            self.hold = Some((jump.sec, Some(target)));
            self.jump = None;
        }
        Some(y)
    }

    /// End of a frame: remember the layout and the scroll, and return the
    /// section the nav marks.
    fn end(
        &mut self,
        tops: Vec<(SettingsSec, f32)>,
        offset: f32,
        view_h: f32,
        content_h: f32,
    ) -> SettingsSec {
        self.max_offset = (content_h - view_h).max(0.0);
        self.offset = offset;
        if let Some((_, Some(at))) = self.hold {
            if (offset - at).abs() > 1.0 {
                self.hold = None;
            }
        }
        let marked = match self.hold {
            Some((sec, _)) => sec,
            None => {
                let current = self
                    .marked
                    .and_then(|m| tops.iter().position(|(s, _)| *s == m))
                    .unwrap_or(0);
                let at_end = self.max_offset > 0.0 && offset >= self.max_offset - 1.0;
                let ys: Vec<f32> = tops.iter().map(|(_, t)| *t).collect();
                tops.get(spy_pick(&ys, offset, view_h, at_end, current))
                    .map(|(s, _)| *s)
                    .or(self.marked)
                    .unwrap_or(SettingsSec::Account)
            }
        };
        self.tops = tops;
        self.marked = Some(marked);
        marked
    }
}

/// A section's title in the one Settings scroll. Returns its rect, for the spy.
fn settings_section_title(ui: &mut egui::Ui, sec: SettingsSec) -> egui::Rect {
    ui.label(
        RichText::new(settings_sec_title(sec))
            .font(crate::theme::title_font(18.0))
            .color(crate::theme::fg()),
    )
    .rect
}

/// Settings → Appearance → Reduce motion.
pub(super) const REDUCE_MOTION_HINT: &str =
    "Icons and cards stay still under the pointer, and Settings jumps to a section instead of scrolling.";

pub(super) fn settings_save_visible(sec: SettingsSec) -> bool {
    sec != SettingsSec::Appearance
}

/// Chat catalog ids plus Auto. Auto persists an empty model pin.
pub(super) fn cabin_default_models() -> Vec<(&'static str, &'static str)> {
    let mut out = vec![("", "Auto")];
    for row in grokhub_core::MODEL_CATALOG {
        if row.kind == "chat" {
            out.push((row.id, row.label));
        }
    }
    out
}

/// Empty stays empty. Any other value is a chat catalog id.
pub(super) fn cabin_default_model_id(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        String::new()
    } else {
        grokhub_core::sanitize_chat_model(t).to_string()
    }
}

pub(super) fn cabin_default_model_label(raw: &str) -> &'static str {
    let id = cabin_default_model_id(raw);
    cabin_default_models()
        .into_iter()
        .find(|(k, _)| *k == id)
        .map(|(_, label)| label)
        .unwrap_or("Auto")
}

const ROW_MODEL: &str = "Default model";
const ROW_PERMISSION: &str = "Permission";
fn row_desktop() -> &'static str {
    *grokhub_core::desktop_mcp::DESKTOP_CONTROL_ROW
}
const ROW_DESKTOP_TEST: &str = "Desktop control";
const ROW_SCREEN_RECORD: &str = "Screen recording";
const SCREEN_RECORD_HINT: &str = "Lets /record take a still every 2 seconds for up to 2 minutes, with a red indicator and Stop. Recordings stay in ~/GrokHub/recordings; a few stills go to the cloud model only for the diagnosis.";
const ROW_SESSION: &str = "Session mode";
const ROW_COLLAPSE: &str = "Always collapse";

/// Settings → Defaults, in order. There is no effort row: effort is automatic (Router R1).
#[cfg(test)]
pub(super) fn defaults_rows() -> [&'static str; 10] {
    [
        ROW_MODEL,
        super::budget_ui::FAST_ROW,
        super::budget_ui::CAP_ROW,
        super::budget_ui::CEILING_ROW,
        ROW_PERMISSION,
        row_desktop(),
        ROW_DESKTOP_TEST,
        ROW_SCREEN_RECORD,
        ROW_SESSION,
        ROW_COLLAPSE,
    ]
}

pub(super) fn cabin_default_permissions() -> &'static [(&'static str, &'static str)] {
    &[("ask", "Ask"), ("auto", "Auto")]
}

/// Settings dropdown is Ask or Auto. Always stays on the composer pill.
pub(super) fn cabin_default_permission_id(raw: &str) -> String {
    match grokhub_core::wire::PermissionMode::parse(raw) {
        Some(grokhub_core::wire::PermissionMode::Auto) => "auto".into(),
        _ => "ask".into(),
    }
}

pub(super) fn cabin_default_sessions() -> &'static [(&'static str, &'static str)] {
    crate::cards::composer_modes()
}

pub(super) fn cabin_default_session_id(raw: &str) -> &'static str {
    SessionMode::parse(raw)
        .unwrap_or(SessionMode::Chat)
        .as_str()
}

fn choice_label(choices: &[(&str, &str)], id: &str, fallback: &str) -> String {
    choices
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| fallback.to_string())
}

/// Settings toggle via the UI kit row (same return shape as `cards::settings_toggle`).
fn settings_toggle(ui: &mut egui::Ui, title: &str, hint: &str, on: &mut bool) -> bool {
    let before = *on;
    crate::ui_kit::setting_row(
        ui,
        title,
        hint,
        crate::ui_kit::SettingRowControl::Switch(on),
    );
    *on != before
}

/// Settings select via the UI kit row (same return shape as `cards::settings_dropdown`).
fn settings_dropdown(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    selected: &str,
    choices: &[String],
) -> Option<usize> {
    if choices.is_empty() {
        return None;
    }
    let mut idx = choices
        .iter()
        .position(|label| label == selected)
        .unwrap_or(0);
    let prev = idx;
    let labels: Vec<&str> = choices.iter().map(String::as_str).collect();
    crate::ui_kit::setting_row(
        ui,
        title,
        hint,
        crate::ui_kit::SettingRowControl::Select {
            selected: &mut idx,
            labels: &labels,
        },
    );
    (idx != prev).then_some(idx)
}

impl Cabin {
    pub(super) fn reset_home_learned(&mut self) {
        self.card_prefs = grokhub_core::CardPrefs::default();
        let _ = crate::card_prefs::save(&self.card_prefs);
        self.status = "Home learning reset".into();
    }

    pub(super) fn forget_home_learned(&mut self, bucket: grokhub_core::LearnedBucket, key: &str) {
        grokhub_core::forget_learned(&mut self.card_prefs, bucket, key);
        let _ = crate::card_prefs::save(&self.card_prefs);
    }

    fn ui_home_learned(&mut self, ui: &mut egui::Ui) {
        crate::ui_kit::section_header(
            ui,
            "What Home learned",
            Some("Groups and topics from cards you open or dismiss. Stays on this computer."),
        );
        let (liked, disliked) = grokhub_core::top_learned(&self.card_prefs, now_ms());
        let mut forget = None;
        if liked.is_empty() && disliked.is_empty() {
            crate::cards::settings_note(ui, "Nothing yet.");
        } else {
            for row in liked.iter().chain(disliked.iter()) {
                let sign = if row.sign > 0 { "+" } else { "-" };
                let hint = match row.bucket {
                    grokhub_core::LearnedBucket::Group => "Group",
                    grokhub_core::LearnedBucket::Topic => "Topic",
                };
                if crate::cards::settings_action(ui, &format!("{sign} {}", row.key), hint, "Forget")
                {
                    forget = Some((row.bucket, row.key.clone()));
                }
            }
        }
        if crate::cards::settings_action(
            ui,
            "Reset all",
            "Clears what Home learned. The signal log stays.",
            "Reset all",
        ) {
            self.reset_home_learned();
        }
        if let Some((bucket, key)) = forget {
            self.forget_home_learned(bucket, &key);
        }
    }

    pub(super) fn choose_theme(&mut self, choice: grokhub_core::ThemeChoice) {
        let current = grokhub_core::parse_theme(&self.cfg.theme);
        if let Some(next) = grokhub_core::pick_theme(current, choice) {
            self.cfg.theme = grokhub_core::theme_id(next).into();
            self.persist_cfg();
            self.status = "Saved".into();
        }
    }

    pub(super) fn set_close_to_tray(&mut self, on: bool) {
        if self.cfg.close_to_tray == on {
            return;
        }
        self.cfg.close_to_tray = on;
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn set_living_wall(&mut self, on: bool) {
        if self.cfg.imagine_wall == on {
            return;
        }
        self.cfg.imagine_wall = on;
        self.persist_cfg();
        self.status = "Saved".into();
    }

    #[cfg(feature = "fx")]
    pub(super) fn set_composer_glow(&mut self, on: bool) {
        crate::fx::dismiss_notice();
        if self.cfg.composer_glow == on {
            return;
        }
        self.cfg.composer_glow = on;
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn set_composer_autocomplete(&mut self, on: bool) {
        if self.cfg.composer_autocomplete == on {
            return;
        }
        self.cfg.composer_autocomplete = on;
        if !on {
            self.composer_ghost.clear();
        }
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn set_reduce_motion(&mut self, on: bool) {
        if self.cfg.reduce_motion == on {
            return;
        }
        self.cfg.reduce_motion = on;
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn set_dock_panels(&mut self, on: bool) {
        if self.cfg.dock_panels == on {
            return;
        }
        self.cfg.dock_panels = on;
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn ui_settings_menu(&mut self, ctx: &egui::Context) {
        if !self.settings_menu_open {
            return;
        }
        let mut pick: Option<&'static str> = None;
        let mut connect = false;
        let mut disconnect = false;
        let mut help = false;
        self.pull_account_oauth_from_disk();
        let authed = self.has_key();
        let chrome = self.avatar_chrome();
        let photo = self.photo_for_path(&chrome.picture_path);
        let shown = egui::Window::new("settings-menu")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -56.0])
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::panel())
                    .corner_radius(crate::theme::MENU_RADIUS)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(220.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                Self::cabin_avatar(ui, &chrome.name, photo.as_ref());
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                for (id, label) in crate::theme::CABIN_MENU {
                    if crate::cards::felt_menu_row(ui, label) {
                        pick = Some(*id);
                    }
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                if crate::cards::felt_menu_row(ui, "Help") {
                    help = true;
                }
                let auth_label = if authed { "Sign out" } else { "Connect Grok" };
                if crate::cards::felt_menu_row(ui, auth_label) {
                    if authed {
                        disconnect = true;
                    } else {
                        connect = true;
                    }
                }
            });
        let menu_rect = shown.map(|r| r.response.rect);
        if let Some(id) = pick {
            self.set_nav_id(id);
            self.settings_menu_open = false;
        }
        if help {
            self.shortcuts_open = true;
            self.settings_menu_open = false;
        }
        if connect {
            self.start_oauth();
            self.settings_menu_open = false;
        }
        if disconnect {
            self.sign_out_oauth();
            self.settings_menu_open = false;
        }
        let outside = ctx.input(|i| i.pointer.any_click())
            && ctx.pointer_interact_pos().is_some_and(|pos| {
                menu_rect
                    .map(|r| !r.expand(8.0).contains(pos))
                    .unwrap_or(true)
            });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.settings_menu_open = false;
        }
        if cabin_menu_should_dismiss(self.settings_menu_ignore, outside) {
            self.settings_menu_open = false;
        }
        self.settings_menu_ignore = false;
    }

    pub(super) fn ui_settings(&mut self, ctx: &egui::Context) {
        let mut save = false;
        let mut connect = false;
        let mut disconnect = false;
        let mut choose_picture = false;
        let mut clear_picture = false;
        let mut name_dirty = false;
        let mut update = false;
        let mut restart = false;
        let mut copy_diag = false;
        let pending_update = self.update_pending_now();
        let update_label = settings_update_label(pending_update);
        let update_hint = settings_update_hint(pending_update);
        let cabin_notify = self.cabin_update_available();
        let cabin_notice = self
            .cabin_latest
            .as_deref()
            .filter(|_| cabin_notify)
            .map(|tag| cabin_update_notice(&crate::update::build_version_label(), tag));
        self.pull_account_oauth_from_disk();
        let account_auth = account_connect_chrome(self.secrets.oauth.as_ref());
        let picture_set = !self.cfg.profile_picture.trim().is_empty();
        let picture_hint = if picture_set {
            "Saved in cabin config."
        } else {
            "A local image, kept in cabin config."
        };
        let pending = self
            .oauth_pending
            .as_ref()
            .map(|p| format!("Approve {} at {}", p.user_code, p.verification_uri));
        let doctor = self.doctor_text();
        let mut close = false;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }
        let mut next_sec: Option<SettingsSec> = None;
        // Every section on one scroll, in nav order; the nav marks the one in view.
        let general = [
            (SettingsSec::Account, "Account"),
            (SettingsSec::Privacy, "Privacy & data"),
            (SettingsSec::Appearance, "Appearance"),
            (SettingsSec::Personality, "Personality"),
            (SettingsSec::Playbooks, "Playbooks"),
            (SettingsSec::Pulse, "Pulse"),
            (SettingsSec::Background, "Background"),
            (SettingsSec::Behavior, "Behavior"),
            (SettingsSec::Permissions, "Permissions"),
            (SettingsSec::Connectors, "Connectors"),
            (SettingsSec::Defaults, "Defaults"),
            (SettingsSec::Labs, "Labs"),
        ];
        let about = [
            (SettingsSec::Update, "Update"),
            (SettingsSec::About, "About"),
        ];
        let opened = settings_fresh_view(ctx);
        let mut spy = Spy::load(ctx);
        let force = spy.begin(
            ctx,
            opened,
            self.settings_sec,
            crate::motion::reduced_motion_ctx(ctx),
        );
        let mut tops: Vec<(SettingsSec, f32)> = Vec::new();
        let mut spy_view: Option<(f32, f32, f32)> = None;
        let sec = self.settings_sec;
        let screen = ctx.content_rect();
        let page = crate::theme::is_loadout_paint();
        let modal = settings_page_rect(screen, page, sec);
        // A page leaves the rail and titlebar live; the sheet dims the rail
        // but leaves the titlebar (and its window buttons) live.
        let cover = if page {
            modal
        } else {
            settings_sheet_cover(screen)
        };
        egui::Area::new(egui::Id::new("settings-overlay"))
            .fixed_pos(cover.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(cover.size());
                ui.set_max_size(cover.size());
                if !page {
                    ui.painter()
                        .rect_filled(cover, 0.0, Color32::from_black_alpha(180));
                }
                ui.scope_builder(egui::UiBuilder::new().max_rect(modal), |ui| {
                    ui.set_clip_rect(modal);
                    egui::Frame::NONE
                        .fill(crate::theme::bg())
                        .corner_radius(if page { 0.0 } else { crate::theme::SHEET_RADIUS })
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::ZERO)
                        .show(ui, |ui| {
                            ui.set_min_size(modal.size());
                            ui.set_max_size(modal.size());
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(220.0, modal.height()),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::Frame::NONE
                                            .fill(crate::theme::surface())
                                            .inner_margin(egui::Margin::same(12))
                                            .show(ui, |ui| {
                                                ui.set_width(196.0);
                                                ui.set_min_height(modal.height() - 24.0);
                                                if crate::cards::section_label(ui, "General") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::General));
                                                }
                                                for (s, label) in general {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                                ui.add_space(10.0);
                                                if crate::cards::section_label(ui, "About") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::About));
                                                }
                                                for (s, label) in about {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                            });
                                    },
                                );
                                let body_w = (modal.width() - 220.0).max(280.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(body_w, modal.height()),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_max_width(body_w);
                                        ui.add_space(16.0);
                                        ui.horizontal(|ui| {
                                            ui.add_space(20.0);
                                            ui.label(
                                                RichText::new("Settings")
                                                    .font(crate::theme::title_font(22.0))
                                                    .color(crate::theme::fg()),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.add_space(16.0);
                                                    if crate::theme::felt_label_button(
                                                        ui,
                                                        "×",
                                                        Color32::TRANSPARENT,
                                                        crate::theme::muted(),
                                                        6.0,
                                                        egui::vec2(28.0, 28.0),
                                                        None,
                                                        false,
                                                    )
                                                    .clicked()
                                                    {
                                                        close = true;
                                                    }
                                                    if settings_save_visible(sec)
                                                        && crate::cards::ghost_pill(ui, "Save")
                                                    {
                                                        save = true;
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(12.0);
                                        // Personality owns its own grid scroll; keep the body
                                        // height capped so the panel never grows past the window.
                                        let body_h = (ui.available_height() - 8.0).max(120.0);
                                        let body = settings_body_scroll(body_h, opened, force);
                                        let shown = body.show(ui, |ui| {
                                                ui.set_width((body_w - 40.0).max(240.0));
                                                ui.set_max_width((body_w - 40.0).max(240.0));
                                                // The content's top, wherever the scroll has it.
                                                let origin = ui.max_rect().top();
                                                ui.add_space(8.0);
                                                for s in general.iter().chain(about.iter()).map(|(s, _)| *s) {
                                                ui.push_id(("settings-sec", s), |ui| {
                                                ui.indent("settings-body", |ui| {
                                                    ui.set_max_width(ui.available_width());
                                                    let title = settings_section_title(ui, s);
                                                    tops.push((s, title.top() - origin));
                                                    ui.add_space(8.0);
                                                    match s {
                                                        SettingsSec::Account => {
                                                            let name_before = self.cfg.display_name.clone();
                                                            crate::cards::settings_field(
                                                                ui,
                                                                "Name",
                                                                "Shown on the rail and the avatar menu. Leave this blank to use your Grok name.",
                                                                &mut self.cfg.display_name,
                                                                false,
                                                            );
                                                            if self.cfg.display_name != name_before {
                                                                name_dirty = true;
                                                            }
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                "Profile picture",
                                                                picture_hint,
                                                                "Choose",
                                                            ) {
                                                                choose_picture = true;
                                                            }
                                                            if picture_set
                                                                && crate::cards::settings_action(
                                                                    ui,
                                                                    "Remove picture",
                                                                    "The rail goes back to your Grok photo.",
                                                                    "Remove",
                                                                )
                                                            {
                                                                clear_picture = true;
                                                            }
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                account_auth.title,
                                                                &account_auth.hint,
                                                                account_auth.action,
                                                            ) {
                                                                if account_auth.connected {
                                                                    disconnect = true;
                                                                } else {
                                                                    connect = true;
                                                                }
                                                            }
                                                            if let Some(p) = &pending {
                                                                crate::cards::settings_note(ui, p);
                                                            }
                                                        }
                                                        SettingsSec::Appearance => {
                                                            crate::cards::settings_note(
                                                                ui,
                                                                appearance_hint(),
                                                            );
                                                            ui.horizontal_wrapped(|ui| {
                                                                let current = parse_theme(&self.cfg.theme);
                                                                let os_dark = crate::theme::desktop_prefers_dark();
                                                                for choice in appearance_choices() {
                                                                    let on = current == *choice;
                                                                    let preview = crate::theme::preview_swatch(
                                                                        *choice,
                                                                        os_dark,
                                                                    );
                                                                    if crate::cards::appearance_card(
                                                                        ui,
                                                                        theme_label(*choice),
                                                                        on,
                                                                        preview,
                                                                    ) {
                                                                        self.choose_theme(*choice);
                                                                    }
                                                                    ui.add_space(10.0);
                                                                }
                                                            });
                                                            if crate::theme::is_loadout_paint() {
                                                                ui.add_space(8.0);
                                                                crate::ui_kit::section_header(
                                                                    ui,
                                                                    "Loadout tokens",
                                                                    Some("Void, base, surface, raised, signal red, hover, selected, steel, amber, bright"),
                                                                );
                                                                crate::theme::paint_loadout_token_strip(ui);
                                                            }
                                                            self.ui_chat_layout_row(ui);
                                                            let mut calm = self.cfg.reduce_motion;
                                                            if settings_toggle(ui, "Reduce motion", REDUCE_MOTION_HINT, &mut calm) {
                                                                self.set_reduce_motion(calm);
                                                            }
                                                        }
                                                        SettingsSec::Behavior => {
                                                            // First, so small windows don't clip them.
                                                            crate::ui_kit::section_header(
                                                                ui,
                                                                "PC helpers",
                                                                None,
                                                            );
                                                            self.ui_wind_down_row(ui);
                                                            self.ui_downloads_tidy_row(ui);
                                                            crate::ui_kit::section_header(
                                                                ui,
                                                                "Tray & wall",
                                                                None,
                                                            );
                                                            let mut close_to_tray = self.cfg.close_to_tray;
                                                            if settings_toggle(ui, "Close to tray", "The cabin keeps working in the background.", &mut close_to_tray) {
                                                                self.set_close_to_tray(close_to_tray);
                                                            }
                                                            let mut living_wall = self.cfg.imagine_wall;
                                                            if settings_toggle(
                                                                ui,
                                                                "Living wall",
                                                                "Every few hours the cabin paints a new cover. Twenty live. Oldest leaves first.",
                                                                &mut living_wall,
                                                            ) {
                                                                self.set_living_wall(living_wall);
                                                            }
                                                            #[cfg(feature = "fx")]
                                                            {
                                                                let mut composer_glow = self.cfg.composer_glow;
                                                                if settings_toggle(
                                                                    ui,
                                                                    "Composer glow (GPU effects)",
                                                                    crate::fx::settings_caption(),
                                                                    &mut composer_glow,
                                                                ) {
                                                                    self.set_composer_glow(composer_glow);
                                                                }
                                                            }
                                                            let mut composer_autocomplete =
                                                                self.cfg.composer_autocomplete;
                                                            if settings_toggle(
                                                                ui,
                                                                "Sentence autocomplete",
                                                                "Muted ghost text finishes your line in the composer. Tab accepts; off by default.",
                                                                &mut composer_autocomplete,
                                                            ) {
                                                                self.set_composer_autocomplete(
                                                                    composer_autocomplete,
                                                                );
                                                            }
                                                            self.ui_focus_help_row(ui);
                                                            self.ui_screenshot_explain_settings(ui);
                                                            self.ui_pc_health_row(ui);
                                                            self.ui_code_resume_row(ui);
                                                            self.ui_backup_row(ui);
                                                            self.ui_net_log_row(ui);
                                                            self.ui_game_companion_row(ui);
                                                            self.ui_game_watch_row(ui);
                                                            self.ui_model_drop_row(ui);
                                                            let mut dock_panels = self.cfg.dock_panels;
                                                            if settings_toggle(
                                                                ui,
                                                                "Dockable panels",
                                                                "Main pages become rearrangeable tabs. Off keeps today's fixed layout. Layout saves in dock.json.",
                                                                &mut dock_panels,
                                                            ) {
                                                                self.set_dock_panels(dock_panels);
                                                            }
                                                            crate::ui_kit::section_header(
                                                                ui,
                                                                "Automations & limits",
                                                                None,
                                                            );
                                                            let quiet_menu = quiet_hours_menu(
                                                                &self.cfg.quiet_start,
                                                                &self.cfg.quiet_end,
                                                            );
                                                            let quiet_labels: Vec<String> =
                                                                quiet_menu.iter().map(|(l, _, _)| l.clone()).collect();
                                                            let quiet_selected = quiet_hours_choice_label(
                                                                &self.cfg.quiet_start,
                                                                &self.cfg.quiet_end,
                                                            );
                                                            if let Some(i) = settings_dropdown(
                                                                ui,
                                                                "Quiet hours",
                                                                "Inside quiet hours the cabin holds a destructive automation and stops anticipating. Off is start and end equal.",
                                                                &quiet_selected,
                                                                &quiet_labels,
                                                            ) {
                                                                if let Some((_, start, end)) = quiet_menu.get(i) {
                                                                    self.cfg.quiet_start = start.clone();
                                                                    self.cfg.quiet_end = end.clone();
                                                                    self.quiet_start_buf = start.clone();
                                                                    self.quiet_end_buf = end.clone();
                                                                    self.persist_cfg();
                                                                    self.status = "Saved".into();
                                                                }
                                                            }
                                                            self.ui_rhythm_rows(ui);
                                                            self.ui_ask_offers(ui);
                                                            self.ui_dream_time_row(ui);
                                                            self.ui_quiet_self_review_rows(ui);
                                                            self.ui_self_change_history(ui);
                                                            self.ui_memory_retention_row(ui);
                                                            self.ui_chat_archive_row(ui);
                                                            if crate::cards::settings_action(ui, "Setup", "Walk through first-run setup again.", "Open") {
                                                                self.open_setup(super::setup_wizard::SetupStep::Welcome);
                                                            }
                                                            let budgets = grokhub_core::TOKEN_BUDGETS;
                                                            let budget_labels: Vec<String> = budgets
                                                                .iter()
                                                                .map(|b| grokhub_core::token_budget_label(*b))
                                                                .collect();
                                                            let budget_hint = format!(
                                                                "Warns at 80% and when used up, once a day. {}.",
                                                                grokhub_core::budget_line(
                                                                    &self.usage,
                                                                    self.cfg.daily_token_budget,
                                                                )
                                                            );
                                                            if let Some(i) = settings_dropdown(
                                                                ui,
                                                                "Daily token budget",
                                                                &budget_hint,
                                                                &grokhub_core::token_budget_label(
                                                                    self.cfg.daily_token_budget,
                                                                ),
                                                                &budget_labels,
                                                            ) {
                                                                if let Some(b) = budgets.get(i) {
                                                                    self.cfg.daily_token_budget = *b;
                                                                    self.persist_cfg();
                                                                    self.status = "Saved".into();
                                                                }
                                                            }
                                                            if self.cfg.daily_token_budget > 0 {
                                                                let mut pause = self.cfg.budget_pauses_scheduled;
                                                                if settings_toggle(
                                                                    ui,
                                                                    "Pause scheduled work over budget",
                                                                    "Night jobs, loops, and anticipate wait until tomorrow. Chat still sends.",
                                                                    &mut pause,
                                                                ) {
                                                                    self.cfg.budget_pauses_scheduled = pause;
                                                                    self.persist_cfg();
                                                                    self.status = "Saved".into();
                                                                }
                                                            }
                                                            ui.add_space(8.0);
                                                            self.ui_home_learned(ui);
                                                        }
                                                        SettingsSec::Update => {
                                                            if let Some(notice) = cabin_notice.as_deref() {
                                                                crate::cards::settings_note(ui, notice);
                                                            }
                                                            if let Some(note) = self.update_cabin_note.as_deref() {
                                                                crate::cards::settings_note(ui, note);
                                                            }
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                update_label,
                                                                update_hint,
                                                                "Update",
                                                            ) {
                                                                update = true;
                                                            }
                                                            if let Some(pct) = self.update_pct {
                                                                let fill = if self.last_receipt_ok == Some(false) && !self.running {
                                                                    crate::theme::OFFLINE
                                                                } else {
                                                                    crate::theme::live()
                                                                };
                                                                crate::cards::settings_progress(ui, pct, fill);
                                                            }
                                                            if self.update_can_restart
                                                                && crate::cards::settings_action(
                                                                    ui,
                                                                    &format!("Restart {}", grokhub_core::brand::product_name()),
                                                                    "Reload hub, then start a new cabin and exit this one.",
                                                                    "Restart",
                                                                )
                                                            {
                                                                restart = true;
                                                            }
                                                            if !self.status.is_empty() {
                                                                crate::cards::settings_note(ui, &self.status);
                                                            }
                                                        }
                                                        SettingsSec::About => {
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "{} {}",
                                                                    grokhub_core::brand::product_name(),
                                                                    env!("CARGO_PKG_VERSION")
                                                                ))
                                                                .size(crate::theme::FONT_HEADING)
                                                                .color(crate::theme::fg()),
                                                            );
                                                            ui.add_space(6.0);
                                                            crate::cards::settings_note(ui, &crate::update::build_detail_line());
                                                            crate::cards::settings_note(ui, &format!("Runs on {}'s own engine.", grokhub_core::brand::product_name()));
                                                            crate::cards::settings_note(ui, &doctor);
                                                            if crate::cards::settings_action(ui, "Diagnostics", "Copy a redacted bundle. No secrets.", "Copy") {
                                                                copy_diag = true;
                                                            }
                                                            self.ui_support_link(ui);
                                                        }
                                                        SettingsSec::Defaults => {
                                                            crate::ui_kit::section_header(
                                                                ui,
                                                                "Models & providers",
                                                                None,
                                                            );
                                                            let models = self.router_model_choices();
                                                            let model_labels: Vec<String> =
                                                                models.iter().map(|(_, label)| label.clone()).collect();
                                                            let pin = cabin_default_model_id(&self.cfg.model);
                                                            let model_selected = models
                                                                .iter()
                                                                .find(|(id, _)| *id == pin)
                                                                .map(|(_, label)| label.clone())
                                                                .unwrap_or_else(|| cabin_default_model_label(&pin).to_string());
                                                            if let Some(i) = settings_dropdown(
                                                                ui,
                                                                ROW_MODEL,
                                                                "Auto picks the model for each step. A model you pick here is kept while it answers.",
                                                                &model_selected,
                                                                &model_labels,
                                                            ) {
                                                                if let Some((id, _)) = models.get(i) {
                                                                    let next = cabin_default_model_id(id);
                                                                    if next != self.cfg.model {
                                                                        self.cfg.model = next;
                                                                        self.persist_cfg();
                                                                        self.status = "Saved".into();
                                                                    }
                                                                }
                                                            }
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                "Refresh models",
                                                                "Check xAI's model list and your plan now.",
                                                                "Refresh",
                                                            ) {
                                                                self.refresh_models_now();
                                                            }
                                                            // Proof shot pins Web search above the
                                                            // long local-model block so the optional
                                                            // SearXNG / Brave fields stay on screen.
                                                            if super::loadout_home::web_search_settings_shot()
                                                            {
                                                                self.ui_web_search_rows(ui);
                                                                self.ui_local_model_rows(ui);
                                                                self.ui_provider_rows(ui);
                                                            } else {
                                                                self.ui_local_model_rows(ui);
                                                                self.ui_provider_rows(ui);
                                                                self.ui_web_search_rows(ui);
                                                            }
                                                            crate::cards::settings_note(ui, &self.how_auto_picks_lines());
                                                            self.ui_spend_rows(ui);
                                                            crate::ui_kit::section_header(
                                                                ui,
                                                                "Permissions & session",
                                                                None,
                                                            );
                                                            let perms = cabin_default_permissions();
                                                            let perm_labels: Vec<String> = perms
                                                                .iter()
                                                                .map(|(_, label)| (*label).to_string())
                                                                .collect();
                                                            let perm_id = cabin_default_permission_id(
                                                                &self.cfg.permission_mode,
                                                            );
                                                            let perm_selected =
                                                                choice_label(perms, &perm_id, "Ask");
                                                            if let Some(i) = settings_dropdown(
                                                                ui,
                                                                ROW_PERMISSION,
                                                                "Ask or Auto. Always is on the composer and sticks until you change it.",
                                                                &perm_selected,
                                                                &perm_labels,
                                                            ) {
                                                                if let Some((id, _)) = perms.get(i) {
                                                                    let next = cabin_default_permission_id(id);
                                                                    if let Some(mode) = PermissionMode::parse(&next)
                                                                    {
                                                                        if mode != PermissionMode::AlwaysApprove
                                                                            && (self.permission_mode != mode
                                                                                || self.cfg.permission_mode != next)
                                                                        {
                                                                            self.drop_turn_for_pin();
                                                                            self.set_permission_mode(mode);
                                                                            self.status = "Saved".into();
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            if settings_toggle(
                                                                ui,
                                                                row_desktop(),
                                                                DESKTOP_CONTROL_HINT.as_str(),
                                                                &mut self.cfg.desktop_control,
                                                            ) {
                                                                let on = self.cfg.desktop_control;
                                                                self.persist_cfg();
                                                                self.status = crate::desktop_mcp::desktop_switch_note(on).into();
                                                                crate::desktop_mcp::sync_native_cua(&self.cfg);
                                                                crate::desktop_mcp::set_desktop_enabled(on);
                                                            }
                                                            crate::cards::settings_note(
                                                                ui,
                                                                &crate::desktop_mcp::desktop_panel_lines(),
                                                            );
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                ROW_DESKTOP_TEST,
                                                                "Move to each monitor center and capture. Offset is reported when the pointer can be read back.",
                                                                "Test",
                                                            ) {
                                                                self.status = crate::desktop_mcp::request_desktop_test(
                                                                    self.cfg.desktop_control,
                                                                );
                                                            }
                                                            if settings_toggle(
                                                                ui,
                                                                ROW_SCREEN_RECORD,
                                                                SCREEN_RECORD_HINT,
                                                                &mut self.cfg.screen_record,
                                                            ) {
                                                                self.persist_cfg();
                                                                self.status = "Saved".into();
                                                            }
                                                            let sessions = cabin_default_sessions();
                                                            let session_labels: Vec<String> = sessions
                                                                .iter()
                                                                .map(|(_, label)| (*label).to_string())
                                                                .collect();
                                                            let session_id =
                                                                cabin_default_session_id(&self.cfg.session_mode);
                                                            let session_selected =
                                                                choice_label(sessions, session_id, "Chat");
                                                            if let Some(i) = settings_dropdown(
                                                                ui,
                                                                ROW_SESSION,
                                                                "Chat, Plan, or btw. Composer pills still change this launch.",
                                                                &session_selected,
                                                                &session_labels,
                                                            ) {
                                                                if let Some((id, _)) = sessions.get(i) {
                                                                    if let Some(mode) = SessionMode::parse(id) {
                                                                        if self.session_mode != mode
                                                                            || self.cfg.session_mode != mode.as_str()
                                                                        {
                                                                            self.drop_turn_for_pin();
                                                                            self.set_session_mode(mode);
                                                                            self.status = "Saved".into();
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            if settings_toggle(
                                                                ui,
                                                                ROW_COLLAPSE,
                                                                "Thoughts start collapsed in every session. Expand opens one at a time.",
                                                                &mut self.cfg.always_collapse_thoughts,
                                                            ) {
                                                                self.persist_cfg();
                                                                self.status = "Saved".into();
                                                            }
                                                        }
                                                        SettingsSec::Labs => {
                                                            crate::ui_kit::section_header(
                                                                ui,
                                                                "Release channel",
                                                                None,
                                                            );
                                                            // Beta channel: a Linux clone switches via install.sh --channel;
                                                            // without one (every Windows install) via the signed releases.
                                                            // Also: when beta caught up to main (same tree), auto-turn off (cooldown).
                                                            self.maybe_auto_off_beta_channel(false);
                                                            let status_line = crate::update::channel_labs_status();
                                                            {
                                                                let channel_on = crate::update::installed_channel()
                                                                    == grokhub_core::Channel::Beta;
                                                                let mut beta_on = channel_on;
                                                                let channel_busy = self.running
                                                                    && self.update_pct.is_some();
                                                                let hint = if channel_busy {
                                                                    "Switching channel…"
                                                                } else if cfg!(windows) {
                                                                    "On = the newest beta pre-release. Off = the latest stable release."
                                                                } else {
                                                                    "On = beta branch. Off = stable (main). Same as install.sh --channel."
                                                                };
                                                                ui.add_enabled_ui(!channel_busy, |ui| {
                                                                    if settings_toggle(
                                                                        ui,
                                                                        "Beta channel",
                                                                        hint,
                                                                        &mut beta_on,
                                                                    ) && beta_on != channel_on
                                                                    {
                                                                        let target = if beta_on {
                                                                            grokhub_core::Channel::Beta
                                                                        } else {
                                                                            grokhub_core::Channel::Stable
                                                                        };
                                                                        #[cfg(windows)]
                                                                        self.queue_release_channel_switch(target);
                                                                        #[cfg(not(windows))]
                                                                        self.queue_channel_switch(target);
                                                                    }
                                                                });
                                                                #[cfg(windows)]
                                                                crate::cards::settings_note(
                                                                    ui,
                                                                    crate::update::channel_windows_note(),
                                                                );
                                                                crate::cards::settings_note(ui, &status_line);
                                                                // Auto-off reads the clone's branches: Linux only.
                                                                #[cfg(not(windows))]
                                                                crate::cards::settings_note(
                                                                    ui,
                                                                    crate::update::channel_auto_off_note(),
                                                                );
                                                                if let Some(pct) = self.update_pct {
                                                                    let fill = if self.last_receipt_ok
                                                                        == Some(false)
                                                                        && !self.running
                                                                    {
                                                                        crate::theme::OFFLINE
                                                                    } else {
                                                                        crate::theme::live()
                                                                    };
                                                                    crate::cards::settings_progress(
                                                                        ui, pct, fill,
                                                                    );
                                                                }
                                                                if self.update_can_restart
                                                                    && crate::cards::settings_action(
                                                                        ui,
                                                                        &format!("Restart {}", grokhub_core::brand::product_name()),
                                                                        "Reload hub, then start a new cabin and exit this one.",
                                                                        "Restart",
                                                                    )
                                                                {
                                                                    restart = true;
                                                                }
                                                                if !self.status.is_empty()
                                                                    && (channel_busy
                                                                        || self.update_can_restart
                                                                        || self.last_receipt_ok
                                                                            == Some(false))
                                                                {
                                                                    crate::cards::settings_note(
                                                                        ui, &self.status,
                                                                    );
                                                                }
                                                            }
                                                            crate::cards::settings_note(
                                                                ui,
                                                                "This cabin's MCP servers are in Settings, Connectors.",
                                                            );
                                                            let cwd = self.grok_cwd();
                                                            crate::native_plugins::paint(ui, &cwd);
                                                            ui.add_space(12.0);
                                                            self.ui_labs_diagnostics(ui);
                                                            ui.add_space(12.0);
                                                            self.ui_labs_desktop_nyx(ui);
                                                            ui.add_space(12.0);
                                                            ui.label(
                                                                egui::RichText::new("Desktop-agent cursor (preview)")
                                                                    .size(crate::theme::FONT_UI)
                                                                    .color(crate::theme::fg()),
                                                            );
                                                            ui.label(
                                                                egui::RichText::new(
                                                                    "White/gray cursor-motion preview only (not the desktop-agent path).",
                                                                )
                                                                .size(crate::theme::FONT_TIP)
                                                                .color(crate::theme::muted()),
                                                            );
                                                            ui.add_space(6.0);
                                                            if crate::cards::ghost_pill(ui, "Preview cursor path") {
                                                                let now = ui.ctx().input(|i| i.time);
                                                                let origin = ui.ctx().content_rect().center()
                                                                    - egui::vec2(80.0, 40.0);
                                                                self.agent_cursor =
                                                                    Some(crate::motion::AgentCursorAnim::demo(now, origin));
                                                                self.status = "Cursor preview".into();
                                                            }
                                                        }
                                                        SettingsSec::Permissions => self.ui_permission_editor(ui),
                                                        SettingsSec::Connectors => self.ui_settings_connectors(ui),
                                                        SettingsSec::Personality => self.ui_settings_personality(ui),
                                                        SettingsSec::Playbooks => self.ui_settings_playbooks(ui),
                                                        SettingsSec::Pulse => self.ui_settings_pulse(ui),
                                                        SettingsSec::Background => self.ui_settings_background(ui),
                                                        SettingsSec::Privacy => self.ui_settings_privacy(ui),
                                                    }
                                                });
                                                });
                                                ui.add_space(SETTINGS_SECTION_GAP);
                                                }
                                            });
                                        spy_view = Some((
                                            shown.state.offset.y,
                                            shown.inner_rect.height(),
                                            shown.content_size.y,
                                        ));
                                    },
                                );
                            });
                        });
                });
            });
        if let Some((offset, view_h, content_h)) = spy_view {
            self.settings_sec = spy.end(tops, offset, view_h, content_h);
        }
        spy.store(ctx);
        // A nav click wins over the spy; next frame's spy scrolls to it.
        if let Some(s) = next_sec {
            self.settings_sec = s;
        }
        if close {
            self.nav = self.settings_back;
        }
        if connect {
            self.start_oauth();
        }
        if disconnect {
            self.sign_out_oauth();
        }
        if name_dirty {
            if self.cfg.display_name.chars().count() > PROFILE_NAME_MAX {
                self.cfg.display_name = clip_profile_name(&self.cfg.display_name);
            }
            self.persist_cfg();
        }
        if choose_picture {
            self.pick_profile_picture();
        }
        if clear_picture {
            self.clear_profile_picture();
        }
        if update {
            self.queue_cabin_update();
        }
        if restart {
            self.restart_after_update(ctx);
        }
        if copy_diag {
            self.copy_diagnostics(ctx);
        }
        if save {
            self.save_settings();
        }
    }

    /// Settings → Copy diagnostics and the palette row: the bundle goes to the
    /// clipboard, and the status line only says so.
    /// Settings → About: a quiet text link, only while `app.json` has a support URL.
    fn ui_support_link(&self, ui: &mut egui::Ui) {
        let Some(url) = support_link(&self.cfg.support_url) else {
            return;
        };
        ui.add_space(6.0);
        let label =
            RichText::new(grokhub_core::brand::support_label()).size(crate::theme::FONT_META);
        if ui.link(label).on_hover_text(url).clicked() {
            let _ = crate::desktop::open_url(url);
        }
    }

    pub(super) fn copy_diagnostics(&mut self, ctx: &egui::Context) {
        let version = crate::update::build_version_line();
        let bundle = diagnostics_bundle(
            version.strip_prefix("GrokHub ").unwrap_or(&version),
            self.has_key(),
            HUB_KIND,
            self.skill_list.len(),
            self.last_receipt_ok,
            self.board.len(),
            &self.status,
        );
        ctx.copy_text(bundle);
        self.status = "Diagnostics copied".into();
    }

    pub(super) fn ui_permission_editor(&mut self, ui: &mut egui::Ui) {
        let draft_id = egui::Id::new("grokhub-perm-draft");
        let mut draft = ui
            .ctx()
            .data(|data| data.get_temp::<PermDraft>(draft_id))
            .unwrap_or_default();
        if draft.action.is_empty() {
            draft.action = "allow".into();
        }
        self.ui_privacy_rows(ui);
        self.ui_scope_rows(ui);
        let locked = self
            .private_lock_for_paint()
            .map(|why| super::privacy_ui::lock_hover(&why));
        self.ui_premium_rows(ui, locked);
        self.ui_provider_grant_rows(ui, locked);
        let workspace = self.grok_cwd();
        let dir = grokhub_agent::perm::config_dir();
        crate::cards::section_heading(ui, super::privacy_ui::RULES_HEAD);
        crate::cards::settings_note(
            ui,
            "Deny wins over ask, and ask wins over allow. Dangerous commands still ask, including after Allow always.",
        );
        crate::cards::settings_field(
            ui,
            "Rule",
            "Bash(git *), Read(src/**), Edit(src/**), WebFetch(domain:example.com), MCPTool(server__*)",
            &mut draft.rule,
            false,
        );
        ui.horizontal(|ui| {
            for (id, label) in [("allow", "Allow"), ("ask", "Ask"), ("deny", "Deny")] {
                if draft.action == id {
                    ui.label(RichText::new(label).color(crate::theme::fg()));
                } else if crate::cards::ghost_pill(ui, label) {
                    draft.action = id.into();
                }
            }
        });
        ui.add_space(8.0);
        if crate::cards::settings_action(ui, "Add rule", "Saved for every project.", "Add") {
            let action = match draft.action.as_str() {
                "deny" => grokhub_agent::perm::Action::Deny,
                "ask" => grokhub_agent::perm::Action::Ask,
                _ => grokhub_agent::perm::Action::Allow,
            };
            match grokhub_agent::perm::parse_rule(draft.rule.trim(), action) {
                Ok(rule) => {
                    let mut rules = grokhub_agent::perm::load_rules(&dir);
                    rules.push(rule);
                    if grokhub_agent::perm::save_rules(&dir, &rules).is_ok() {
                        draft.rule.clear();
                        self.status = "Saved".into();
                    } else {
                        self.status = "Could not save rules".into();
                    }
                }
                Err(_) => self.status = "That rule could not be parsed".into(),
            }
        }
        let rules = grokhub_agent::perm::load_rules(&dir);
        if rules.is_empty() {
            crate::cards::settings_note(ui, "No saved rules.");
        }
        let mut drop_at = None;
        for (idx, rule) in rules.iter().enumerate() {
            let title = format!("{}  {}", rule.action, rule.source);
            if crate::cards::settings_action(ui, &title, "Saved rule", "Remove") {
                drop_at = Some(idx);
            }
        }
        if let Some(idx) = drop_at {
            let mut rules = grokhub_agent::perm::load_rules(&dir);
            if idx < rules.len() {
                rules.remove(idx);
                if grokhub_agent::perm::save_rules(&dir, &rules).is_ok() {
                    self.status = "Saved".into();
                } else {
                    self.status = "Could not save rules".into();
                }
            }
        }
        if let Some(imported) = grokhub_agent::perm::load_claude_project(&workspace) {
            if !imported.is_empty() {
                crate::cards::settings_note(ui, "From .claude/settings.json. Read only.");
                for rule in imported {
                    crate::cards::settings_note(ui, &format!("{}  {}", rule.action, rule.source));
                }
            }
        }
        crate::cards::settings_field(
            ui,
            "Remembered grant",
            "Allow always for this project. Dangerous commands are not stored.",
            &mut draft.grant,
            false,
        );
        if crate::cards::settings_action(
            ui,
            "Remember command",
            "Stored for this project.",
            "Remember",
        ) {
            let raw = draft.grant.trim().to_string();
            let args = serde_json::json!({ "command": raw }).to_string();
            match grokhub_agent::perm::remember_allow_always(
                &workspace,
                "run_terminal_command",
                &args,
            ) {
                Ok(true) => {
                    draft.grant.clear();
                    self.status = "Saved".into();
                }
                Ok(false) => {
                    self.status = "That command was not remembered".into();
                }
                Err(_) => self.status = "Could not save the grant".into(),
            }
        }
        let grants = grokhub_agent::perm::load_grants(&dir, &workspace);
        if grants.is_empty() {
            crate::cards::settings_note(ui, "No remembered grants for this project.");
        }
        let mut drop_grant = None;
        for grant in &grants {
            if crate::cards::settings_action(ui, grant, "Remembered for this project", "Remove") {
                drop_grant = Some(grant.clone());
            }
        }
        if let Some(grant) = drop_grant {
            if grokhub_agent::perm::remove_grant(&workspace, &grant).is_ok() {
                self.status = "Saved".into();
            } else {
                self.status = "Could not save the grant".into();
            }
        }
        ui.ctx().data_mut(|data| data.insert_temp(draft_id, draft));
    }

    /// Next cabin turn picks up a Settings pin the same way a composer pill does.
    pub(super) fn drop_turn_for_pin(&mut self) {
        self.confirm = None;
        if self.running {
            self.halt_in_flight();
        }
        self.acp = None;
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.grok_session = None;
        }
        self.persist_idle_key = self.persist_idle_now();
    }

    pub(super) fn save_settings(&mut self) {
        self.cfg.api_key.clear();
        self.cfg.quiet_start = normalize_hm(&self.quiet_start_buf, &self.cfg.quiet_start);
        self.cfg.quiet_end = normalize_hm(&self.quiet_end_buf, &self.cfg.quiet_end);
        self.cfg.daily_auto_cap = cap_from_text(&self.cap_auto_buf, self.cfg.daily_auto_cap);
        self.cfg.host_hour_cap = cap_from_text(&self.cap_host_buf, self.cfg.host_hour_cap);
        self.quiet_start_buf = self.cfg.quiet_start.clone();
        self.quiet_end_buf = self.cfg.quiet_end.clone();
        self.cap_auto_buf = self.cfg.daily_auto_cap.to_string();
        self.cap_host_buf = self.cfg.host_hour_cap.to_string();
        if let Ok(mut st) = self.hub.lock() {
            if !self.cfg.device_name.trim().is_empty() {
                st.device_name = self.cfg.device_name.clone();
            }
        }
        let p = expand_home(&self.cfg.project_dir);
        let tree_changed = self
            .threads
            .get(self.thread_idx)
            .and_then(|t| t.grok_cwd.as_deref())
            .map(|cwd| cwd != p)
            .unwrap_or(false);
        self.cfg.project_dir = p.clone();
        if !p.trim().is_empty() {
            let dir = p.clone();
            std::thread::spawn(move || {
                let _ = std::fs::create_dir_all(&dir);
            });
        }
        self.project_sel = upsert_bound(&mut self.projects, &p);
        self.touch_projects();
        self.status = "Saved".into();
        if tree_changed {
            if self.running {
                self.halt_in_flight();
            }
            self.acp = None;
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_cwd = None;
                t.grok_session = None;
            }
            self.persist();
        } else {
            self.flush_projects();
            self.persist_cfg();
            self.persist_hub();
            self.persist_secrets();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_follows_the_window_and_leaves_the_titlebar_live() {
        let sheet = |w: f32, h: f32| {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
            settings_page_rect(screen, false, SettingsSec::Labs)
        };
        // 1280x800: 80% of the 760 px under the titlebar, floored at 920x620.
        let r = sheet(1280.0, 800.0);
        assert_eq!(r.size(), egui::vec2(1024.0, 620.0));
        assert_eq!(r.center(), egui::pos2(640.0, 420.0));
        // Maximized 1920x1080 grows, up to 1280 wide.
        let r = sheet(1920.0, 1080.0);
        assert_eq!(r.size(), egui::vec2(1280.0, 832.0));
        assert_eq!(r.center(), egui::pos2(960.0, 560.0));
        // A small window caps it with a 24 px margin, still under the titlebar.
        let r = sheet(658.0, 458.0);
        assert_eq!(r.size(), egui::vec2(610.0, 370.0));
        assert!(r.min.y >= crate::theme::TITLEBAR_H, "{r:?}");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        assert_eq!(settings_sheet_cover(screen).min, egui::pos2(0.0, 40.0));
    }

    #[test]
    fn settings_opens_fresh_once_then_keeps_its_scroll() {
        let ctx = egui::Context::default();
        let mut seen = Vec::new();
        let mut pass = |open: bool| {
            let _ = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
                if open {
                    seen.push(settings_fresh_view(ui.ctx()));
                }
            });
        };
        pass(true);
        pass(true);
        pass(true);
        // Closed for a frame, then opened again.
        pass(false);
        pass(true);
        // Every section shares one scroll now, so only a fresh open is fresh.
        assert_eq!(seen, [true, false, false, true]);
    }

    #[test]
    fn the_body_scrolls_back_to_the_top_only_when_reopened() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
        // One frame of a tall body, optionally wheeled down or forced; its offset after.
        let frame = |open: bool, wheel: f32, force: Option<f32>| -> Option<f32> {
            let mut raw = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            if wheel != 0.0 {
                raw.events = vec![
                    egui::Event::PointerMoved(egui::pos2(300.0, 150.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -wheel),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ];
            } else {
                // Off the body, so a wheel still easing from the last frame moves nothing.
                raw.events = vec![egui::Event::PointerGone];
            }
            let mut offset = None;
            let _ = crate::theme::test_pass(&ctx, raw, |ui| {
                if !open {
                    return;
                }
                let fresh = settings_fresh_view(ui.ctx());
                let out = settings_body_scroll(300.0, fresh, force).show(ui, |ui| {
                    ui.allocate_space(egui::vec2(200.0, 2000.0));
                });
                offset = Some(out.state.offset.y);
            });
            offset
        };
        frame(true, 0.0, None);
        frame(true, 400.0, None);
        // Still open: the scroll is kept.
        let kept = frame(true, 0.0, None).unwrap();
        assert!(kept > 100.0, "kept {kept}");
        // A jump puts it exactly where the spy says.
        assert_eq!(frame(true, 0.0, Some(640.0)), Some(640.0));
        assert_eq!(frame(true, 0.0, None), Some(640.0));
        // Closed for a frame and reopened: the top.
        frame(false, 0.0, None);
        assert_eq!(frame(true, 0.0, None), Some(0.0));
    }

    #[test]
    fn the_spy_marks_the_section_whose_top_crossed_thirty_percent() {
        // Four sections, a 600 px view: the line sits 180 px under the scroll top.
        let tops = [0.0, 400.0, 900.0, 1500.0];
        // Load: the first section.
        assert_eq!(spy_pick(&tops, 0.0, 600.0, false, 0), 0);
        assert_eq!(spy_pick(&[], 0.0, 600.0, false, 3), 0);
        // 400 is past the line (410) but not by the slack: still the first.
        assert_eq!(spy_pick(&tops, 230.0, 600.0, false, 0), 0);
        // 12 px past it: the second.
        assert_eq!(spy_pick(&tops, 232.0, 600.0, false, 0), 1);
        // Back up a little: 400 sits under the line by less than the slack.
        assert_eq!(spy_pick(&tops, 220.0, 600.0, false, 1), 1);
        // Back past it by the slack: the first again.
        assert_eq!(spy_pick(&tops, 207.0, 600.0, false, 1), 0);
        // A fast scroll skips straight to the section in view.
        assert_eq!(spy_pick(&tops, 1400.0, 600.0, false, 0), 3);
        assert_eq!(spy_pick(&tops, 0.0, 600.0, false, 3), 0);
        // Scrolled to the end: the last, even with its top under the line.
        assert_eq!(spy_pick(&tops, 1000.0, 600.0, true, 1), 3);
        // A stale index from a longer list is clamped.
        assert_eq!(spy_pick(&tops[..2], 0.0, 600.0, false, 9), 0);
    }

    #[test]
    fn the_spy_does_not_flicker_at_a_boundary() {
        let tops = [0.0, 400.0];
        // Wobble one pixel either side of the exact crossing (offset 220).
        let mut marked = 0;
        let mut seen = Vec::new();
        for offset in [218.0, 221.0, 219.0, 222.0, 220.0, 219.0] {
            marked = spy_pick(&tops, offset, 600.0, false, marked);
            seen.push(marked);
        }
        assert_eq!(seen, [0, 0, 0, 0, 0, 0]);
    }

    /// Runs the spy over a fake page: four sections 500 px apart, 1700 px in
    /// all, in a 400 px view (so the scroll stops at 1300).
    fn spy_frame(
        ctx: &egui::Context,
        spy: &mut Spy,
        time: f64,
        opened: bool,
        asked: SettingsSec,
        reduced: bool,
        wheel_to: Option<f32>,
    ) -> (Option<f32>, SettingsSec, std::time::Duration) {
        let order = [
            SettingsSec::Account,
            SettingsSec::Privacy,
            SettingsSec::Appearance,
            SettingsSec::About,
        ];
        let raw = egui::RawInput {
            time: Some(time),
            ..Default::default()
        };
        let mut got = (None, SettingsSec::Account);
        let out = crate::theme::test_pass(ctx, raw, |ui| {
            let force = spy.begin(ui.ctx(), opened, asked, reduced);
            let offset = force.or(wheel_to).unwrap_or(spy.offset);
            let tops = order
                .iter()
                .enumerate()
                .map(|(i, s)| (*s, 8.0 + i as f32 * 500.0));
            got = (force, spy.end(tops.collect(), offset, 400.0, 1700.0));
        });
        let delay = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        (got.0, got.1, delay)
    }

    #[test]
    fn a_nav_click_scrolls_smoothly_and_marks_the_section_at_once() {
        let ctx = egui::Context::default();
        let mut spy = Spy::default();
        // A fresh open lays out from the top, then lands on the asked section at once.
        let (y, marked, _) =
            spy_frame(&ctx, &mut spy, 0.0, true, SettingsSec::Account, false, None);
        assert_eq!((y, marked), (Some(0.0), SettingsSec::Account));
        let (y, marked, idle) = spy_frame(
            &ctx,
            &mut spy,
            1.0,
            false,
            SettingsSec::Account,
            false,
            None,
        );
        assert_eq!((y, marked), (Some(0.0), SettingsSec::Account));
        let (_, _, idle2) = spy_frame(
            &ctx,
            &mut spy,
            2.0,
            false,
            SettingsSec::Account,
            false,
            None,
        );
        assert_eq!(
            idle2,
            std::time::Duration::MAX,
            "an idle Settings asks for no frame: {idle:?}"
        );
        // Click Appearance (top 1008, so the scroll goes to 1000): marked at once,
        // the scroll starts from where it was and asks for frames while it moves.
        let (y, marked, delay) = spy_frame(
            &ctx,
            &mut spy,
            3.0,
            false,
            SettingsSec::Appearance,
            false,
            None,
        );
        assert_eq!((y, marked), (Some(0.0), SettingsSec::Appearance));
        assert_eq!(delay, std::time::Duration::ZERO);
        let half = 3.0 + f64::from(crate::motion::SETTINGS_SCROLL_SECS) * 0.5;
        let (y, marked, _) = spy_frame(
            &ctx,
            &mut spy,
            half,
            false,
            SettingsSec::Appearance,
            false,
            None,
        );
        let y = y.unwrap();
        assert!(y > 500.0 && y < 1000.0, "half way, eased: {y}");
        assert_eq!(
            marked,
            SettingsSec::Appearance,
            "no flicker through Privacy"
        );
        let end = 3.0 + f64::from(crate::motion::SETTINGS_SCROLL_SECS) + 0.001;
        let (y, marked, _) = spy_frame(
            &ctx,
            &mut spy,
            end,
            false,
            SettingsSec::Appearance,
            false,
            None,
        );
        assert_eq!((y, marked), (Some(1000.0), SettingsSec::Appearance));
        let (y, marked, delay) = spy_frame(
            &ctx,
            &mut spy,
            end + 1.0,
            false,
            SettingsSec::Appearance,
            false,
            None,
        );
        assert_eq!((y, marked), (None, SettingsSec::Appearance));
        assert_eq!(delay, std::time::Duration::MAX, "landed: no more frames");
        // About's top can't reach the top of the view: the scroll goes as far as it can.
        spy_frame(&ctx, &mut spy, 10.0, false, SettingsSec::About, false, None);
        let (y, marked, _) =
            spy_frame(&ctx, &mut spy, 11.0, false, SettingsSec::About, false, None);
        assert_eq!((y, marked), (Some(1300.0), SettingsSec::About));
        // The user scrolls away: the spy takes over again.
        let (_, marked, _) = spy_frame(
            &ctx,
            &mut spy,
            12.0,
            false,
            SettingsSec::About,
            false,
            Some(520.0),
        );
        assert_eq!(marked, SettingsSec::Privacy);
    }

    #[test]
    fn reduced_motion_jumps_in_one_frame() {
        let ctx = egui::Context::default();
        let mut spy = Spy::default();
        spy_frame(&ctx, &mut spy, 0.0, true, SettingsSec::Account, true, None);
        spy_frame(&ctx, &mut spy, 1.0, false, SettingsSec::Account, true, None);
        let (y, marked, delay) = spy_frame(
            &ctx,
            &mut spy,
            2.0,
            false,
            SettingsSec::Appearance,
            true,
            None,
        );
        assert_eq!((y, marked), (Some(1000.0), SettingsSec::Appearance));
        assert_eq!(
            delay,
            std::time::Duration::MAX,
            "nothing eases, so no frame is asked for"
        );
        // Opening on a section lands there on the first laid-out frame either way.
        let mut spy = Spy::default();
        let (y, marked, _) = spy_frame(&ctx, &mut spy, 5.0, true, SettingsSec::About, false, None);
        assert_eq!((y, marked), (Some(0.0), SettingsSec::About));
        let (y, marked, _) = spy_frame(&ctx, &mut spy, 6.0, false, SettingsSec::About, false, None);
        assert_eq!((y, marked), (Some(1300.0), SettingsSec::About));
    }

    #[test]
    fn support_link_shows_only_for_a_web_url() {
        assert_eq!(support_link(""), None);
        assert_eq!(support_link("   "), None);
        assert_eq!(
            support_link("https://donate.example/my-app"),
            Some("https://donate.example/my-app")
        );
        assert_eq!(
            support_link("  https://donate.example/my-app \n"),
            Some("https://donate.example/my-app")
        );
        assert_eq!(support_link("donate.example/my-app"), None);
        assert_eq!(support_link("javascript:alert(1)"), None);
        assert_eq!(support_link("file:///etc/passwd"), None);
        assert_eq!(support_link("https://donate.example/a|calc"), None);
    }

    #[test]
    fn about_shows_the_support_link_quietly() {
        // Windows checkouts carry CRLF; the body split below looks for "\n    }\n".
        let src = include_str!("settings.rs").replace("\r\n", "\n");
        // Split markers are built with concat! so this test's own text never
        // matches the source scans other tests run on this file.
        let about = src
            .split(concat!("SettingsSec::", "About => {"))
            .nth(1)
            .and_then(|s| s.split(concat!("SettingsSec::", "Defaults => {")).next())
            .expect("About arm");
        assert!(about.contains("self.ui_support_link(ui);"), "{about}");
        let body = src
            .split("fn ui_support_link(")
            .nth(1)
            .and_then(|s| s.split("\n    }\n").next())
            .expect("ui_support_link");
        assert!(body.contains("brand::support_label()"), "{body}");
        assert!(
            body.contains("support_link(&self.cfg.support_url)"),
            "{body}"
        );
        assert!(body.contains("crate::desktop::open_url(url)"), "{body}");
        for loud in ["Window::new", "Modal", "toast", "notify", "settings_action"] {
            assert!(
                !body.contains(loud),
                "the support link stays a plain link: {loud}"
            );
        }
    }

    #[test]
    fn cabin_default_choices_match_the_headless_pins() {
        let models = cabin_default_models();
        assert_eq!(models[0], ("", "Auto"));
        let chat: Vec<_> = grokhub_core::MODEL_CATALOG
            .iter()
            .filter(|row| row.kind == "chat")
            .map(|row| row.id)
            .collect();
        assert!(chat.len() >= 7);
        for id in &chat {
            assert!(
                models.iter().any(|(k, _)| k == id),
                "missing chat model {id}"
            );
        }
        assert!(models.iter().all(|(id, _)| {
            id.is_empty()
                || grokhub_core::MODEL_CATALOG
                    .iter()
                    .any(|row| row.kind == "chat" && row.id == *id)
        }));
        assert_eq!(cabin_default_model_id(""), "");
        assert_eq!(cabin_default_model_label(""), "Auto");
        assert_eq!(
            cabin_default_model_id("grok-4.7"),
            grokhub_core::sanitize_chat_model("grok-4.7")
        );
        assert_eq!(
            cabin_default_model_id("nope"),
            grokhub_core::sanitize_chat_model("nope")
        );
        assert_eq!(
            defaults_rows(),
            [
                "Default model",
                "Use Grok 4.7 Fast when you're waiting",
                "Weekly spend cap",
                "Price limit",
                "Permission",
                "Let Loadout control the desktop",
                "Desktop control",
                "Screen recording",
                "Session mode",
                "Always collapse"
            ]
        );
        assert!(defaults_rows()
            .iter()
            .all(|r| !r.to_ascii_lowercase().contains("effort")));
        let perms = cabin_default_permissions();
        assert_eq!(perms, &[("ask", "Ask"), ("auto", "Auto")]);
        assert!(perms.iter().all(|(id, _)| *id != "always-approve"));
        assert_eq!(cabin_default_permission_id("always-approve"), "ask");
        assert_eq!(cabin_default_permission_id("always"), "ask");
        assert_eq!(cabin_default_permission_id("auto"), "auto");
        let sessions = cabin_default_sessions();
        assert_eq!(
            sessions,
            &[("chat", "Chat"), ("plan", "Plan"), ("ask", "btw")]
        );
        assert_eq!(cabin_default_session_id("ask"), "ask");
        assert_eq!(cabin_default_session_id("plan"), "plan");
        assert_eq!(cabin_default_session_id("nonsense"), "chat");
    }
}
