//! Settings page, cabin menu, and save.

use super::*;

/// Settings → Let Grok control the desktop. Names the hard floor next to the switch.
pub(super) const DESKTOP_CONTROL_HINT: &str = "Grok can see the screen and use the mouse and keyboard through GrokHub. Ask still asks first. Deletes, sends, money and credentials always ask.";

#[derive(Clone, Default)]
struct PermDraft {
    rule: String,
    action: String,
    grant: String,
}

pub(super) fn settings_group_home(group: SettingsGroup) -> SettingsSec {
    match group {
        SettingsGroup::General => SettingsSec::Account,
        SettingsGroup::About => SettingsSec::Update,
    }
}

pub(super) fn settings_sec_title(sec: SettingsSec) -> &'static str {
    match sec {
        SettingsSec::Account => "Account",
        SettingsSec::Appearance => "Appearance",
        SettingsSec::Behavior => "Behavior",
        SettingsSec::Update => "Update",
        SettingsSec::About => "About",
        SettingsSec::Defaults => "Cabin defaults",
        SettingsSec::Labs => "Labs",
        SettingsSec::Permissions => "Permissions",
    }
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
const ROW_DESKTOP: &str = "Let Grok control the desktop";
const ROW_DESKTOP_TEST: &str = "Desktop control";
const ROW_SCREEN_RECORD: &str = "Screen recording";
const SCREEN_RECORD_HINT: &str = "Lets /record take a still every 2 seconds for up to 2 minutes, with a red indicator and Stop. Recordings stay in ~/GrokHub/recordings; a few stills go to Grok only for the diagnosis.";
const ROW_SESSION: &str = "Session mode";
const ROW_COLLAPSE: &str = "Always collapse";

/// Settings → Cabin defaults, in order. There is no effort row: effort is automatic (Router R1).
#[cfg(test)]
pub(super) const DEFAULTS_ROWS: &[&str] = &[
    ROW_MODEL,
    super::budget_ui::FAST_ROW,
    super::budget_ui::CAP_ROW,
    super::budget_ui::CEILING_ROW,
    ROW_PERMISSION, ROW_DESKTOP, ROW_DESKTOP_TEST, ROW_SCREEN_RECORD, ROW_SESSION, ROW_COLLAPSE];

pub(super) fn cabin_default_permissions() -> &'static [(&'static str, &'static str)] {
    &[("ask", "Ask"), ("auto", "Auto")]
}

/// Ask or Auto only. Always collapses to Ask.
pub(super) fn cabin_default_permission_id(raw: &str) -> String {
    crate::config::persistable_permission_mode(raw)
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
        ui.label(
            RichText::new("What Home learned")
                .size(15.0)
                .color(crate::theme::fg()),
        );
        ui.label(
            RichText::new("Groups and topics from cards you open or dismiss. Stays on this computer.")
                .size(12.0)
                .color(crate::theme::muted()),
        );
        ui.add_space(6.0);
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
                if crate::cards::settings_action(ui, &format!("{sign} {}", row.key), hint, "Forget") {
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
        let mut install_cli = false;
        let mut restart = false;
        let mut copy_diag = false;
        let cli_ready = grokhub_acp::find_grok().is_some() || grokhub_acp::grok_cli_known_good();
        let cli_installing = self.grok_install_rx.is_some();
        let show_cli_install =
            grokhub_core::should_show_manual_cli_install(cli_ready, cli_installing);
        let pending_update = self.update_pending_now();
        let update_label = settings_update_label(pending_update);
        let update_hint = settings_update_hint(pending_update);
        let cabin_notify = self.cabin_update_available();
        let cabin_notice = self
            .cabin_latest
            .as_deref()
            .filter(|_| cabin_notify)
            .map(|tag| cabin_update_notice(env!("CARGO_PKG_VERSION"), tag));
        let cli_notice = match (self.cli_installed.as_deref(), self.cli_alpha.as_deref()) {
            (Some(installed), Some(alpha))
                if should_update_cli_alpha(Some(installed), Some(alpha)) =>
            {
                Some(cli_update_notice(installed, alpha))
            }
            _ => None,
        };
        let cli_install_hint = if cli_installing {
            "Installing Grok Build CLI alpha (GROK_CHANNEL=alpha)…"
        } else if !self.grok_install_err.is_empty() {
            "Grok is missing or broken. Installs Grok Build CLI alpha from x.ai/cli."
        } else {
            "Installs Grok Build CLI alpha (GROK_CHANNEL=alpha / https://x.ai/cli/alpha) when grok is missing or broken."
        };
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
        let sec = self.settings_sec;
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("settings-overlay"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let modal = egui::Rect::from_center_size(
                    screen.center(),
                    egui::vec2(920.0, 620.0).min(screen.size() - egui::vec2(48.0, 48.0)),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(modal), |ui| {
                    egui::Frame::NONE
                        .fill(crate::theme::bg())
                        .corner_radius(crate::theme::SHEET_RADIUS)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::ZERO)
                        .show(ui, |ui| {
                            ui.set_min_size(modal.size());
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
                                                for (s, label) in [
                                                    (SettingsSec::Account, "Account"),
                                                    (SettingsSec::Appearance, "Appearance"),
                                                    (SettingsSec::Behavior, "Behavior"),
                                                    (SettingsSec::Permissions, "Permissions"),
                                                    (SettingsSec::Defaults, "Cabin defaults"),
                                                    (SettingsSec::Labs, "Labs"),
                                                ] {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                                ui.add_space(10.0);
                                                if crate::cards::section_label(ui, "About") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::About));
                                                }
                                                for (s, label) in [
                                                    (SettingsSec::Update, "Update"),
                                                    (SettingsSec::About, "About"),
                                                ] {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                            });
                                    },
                                );
                                ui.allocate_ui_with_layout(
                                    egui::vec2((modal.width() - 220.0).max(320.0), modal.height()),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.add_space(16.0);
                                        ui.horizontal(|ui| {
                                            ui.add_space(20.0);
                                            ui.label(
                                                RichText::new(settings_sec_title(sec))
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
                                                    if crate::cards::ghost_pill(ui, "Save") {
                                                        save = true;
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(12.0);
                                        egui::ScrollArea::vertical()
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width((modal.width() - 260.0).max(280.0));
                                                ui.add_space(8.0);
                                                ui.indent("settings-body", |ui| {
                                                    match sec {
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
                                                            ui.horizontal(|ui| {
                                                                let current = parse_theme(&self.cfg.theme);
                                                                let os_dark = crate::theme::desktop_prefers_dark();
                                                                for choice in appearance_choices() {
                                                                    let on = current == *choice;
                                                                    let preview = if resolve_dark(*choice, os_dark)
                                                                    {
                                                                        crate::theme::BG
                                                                    } else {
                                                                        crate::theme::LIGHT_BG
                                                                    };
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
                                                        }
                                                        SettingsSec::Behavior => {
                                                            let mut close_to_tray = self.cfg.close_to_tray;
                                                            if crate::cards::settings_toggle(ui, "Close to tray", "The cabin keeps working in the background.", &mut close_to_tray) {
                                                                self.set_close_to_tray(close_to_tray);
                                                            }
                                                            let mut living_wall = self.cfg.imagine_wall;
                                                            if crate::cards::settings_toggle(
                                                                ui,
                                                                "Living wall",
                                                                "Every few hours the cabin paints a new cover. Twenty live. Oldest leaves first.",
                                                                &mut living_wall,
                                                            ) {
                                                                self.set_living_wall(living_wall);
                                                            }
                                                            let mut home_deck = self.cfg.home_deck;
                                                            if crate::cards::settings_toggle(ui, "Cards on the empty chat", "Pulse holds your cards now. Turn this on to also stack them over a new chat.", &mut home_deck) {
                                                                self.cfg.home_deck = home_deck;
                                                                self.persist_cfg();
                                                            }
                                                            #[cfg(feature = "fx")]
                                                            {
                                                                let mut composer_glow = self.cfg.composer_glow;
                                                                if crate::cards::settings_toggle(
                                                                    ui,
                                                                    "Composer glow (GPU effects)",
                                                                    crate::fx::settings_caption(),
                                                                    &mut composer_glow,
                                                                ) {
                                                                    self.set_composer_glow(composer_glow);
                                                                }
                                                            }
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
                                                            if let Some(i) = crate::cards::settings_dropdown(
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
                                                            self.ui_dream_time_row(ui);
                                                            self.ui_quiet_self_review_rows(ui);
                                                            self.ui_memory_retention_row(ui);
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
                                                            if let Some(i) = crate::cards::settings_dropdown(
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
                                                                if crate::cards::settings_toggle(
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
                                                            if let Some(notice) = cli_notice.as_deref() {
                                                                crate::cards::settings_note(ui, notice);
                                                            }
                                                            if let Some(notice) = cabin_notice.as_deref() {
                                                                crate::cards::settings_note(ui, notice);
                                                            }
                                                            if let Some(note) = self.update_cabin_note.as_deref() {
                                                                crate::cards::settings_note(ui, note);
                                                            }
                                                            if show_cli_install
                                                                && crate::cards::settings_action(
                                                                    ui,
                                                                    "Install Grok Build CLI",
                                                                    cli_install_hint,
                                                                    if cli_installing { "Installing…" } else { "Install" },
                                                                )
                                                                && !cli_installing
                                                            {
                                                                install_cli = true;
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
                                                                    "Restart GrokHub",
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
                                                                    "GrokHub {}",
                                                                    env!("CARGO_PKG_VERSION")
                                                                ))
                                                                .size(crate::theme::FONT_HEADING)
                                                                .color(crate::theme::fg()),
                                                            );
                                                            ui.add_space(6.0);
                                                            crate::cards::settings_note(ui, "Native Grok Build cabin.");
                                                            crate::cards::settings_note(ui, &build_agent::grok_banner());
                                                            crate::cards::settings_note(ui, &doctor);
                                                            if crate::cards::settings_action(ui, "Diagnostics", "Copy a redacted bundle. No secrets.", "Copy") {
                                                                copy_diag = true;
                                                            }
                                                        }
                                                        SettingsSec::Defaults => {
                                                            let models = self.router_model_choices();
                                                            let model_labels: Vec<String> =
                                                                models.iter().map(|(_, label)| label.clone()).collect();
                                                            let pin = cabin_default_model_id(&self.cfg.model);
                                                            let model_selected = models
                                                                .iter()
                                                                .find(|(id, _)| *id == pin)
                                                                .map(|(_, label)| label.clone())
                                                                .unwrap_or_else(|| cabin_default_model_label(&pin).to_string());
                                                            if let Some(i) = crate::cards::settings_dropdown(
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
                                                            self.ui_local_model_rows(ui);
                                                            self.ui_provider_rows(ui);
                                                            crate::cards::settings_note(ui, &self.how_auto_picks_lines());
                                                            self.ui_spend_rows(ui);
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
                                                            if let Some(i) = crate::cards::settings_dropdown(
                                                                ui,
                                                                ROW_PERMISSION,
                                                                "Ask or Auto. Always stays on the composer for this launch.",
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
                                                            if crate::cards::settings_toggle(
                                                                ui,
                                                                ROW_DESKTOP,
                                                                DESKTOP_CONTROL_HINT,
                                                                &mut self.cfg.desktop_control,
                                                            ) {
                                                                let on = self.cfg.desktop_control;
                                                                self.persist_cfg();
                                                                self.status = if on {
                                                                    "Registering desktop tools...".into()
                                                                } else {
                                                                    "Removing desktop tools...".into()
                                                                };
                                                                crate::desktop_mcp::spawn_register(on);
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
                                                            if crate::cards::settings_toggle(
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
                                                            if let Some(i) = crate::cards::settings_dropdown(
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
                                                            if crate::cards::settings_toggle(
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
                                                            // Beta channel: Linux switches via install.sh --channel;
                                                            // Windows stays disabled until the installer supports it.
                                                            // Also: when beta caught up to main (same tree), auto-turn off (cooldown).
                                                            self.maybe_auto_off_beta_channel(false);
                                                            let status_line = crate::update::channel_labs_status();
                                                            #[cfg(windows)]
                                                            {
                                                                ui.add_enabled_ui(false, |ui| {
                                                                    let mut off = false;
                                                                    let _ = crate::cards::settings_toggle(
                                                                        ui,
                                                                        "Beta channel",
                                                                        "Fetch, build, and install from the beta branch.",
                                                                        &mut off,
                                                                    );
                                                                });
                                                                crate::cards::settings_note(
                                                                    ui,
                                                                    crate::update::channel_windows_note(),
                                                                );
                                                                crate::cards::settings_note(
                                                                    ui,
                                                                    crate::update::channel_auto_off_note(),
                                                                );
                                                                crate::cards::settings_note(ui, &status_line);
                                                            }
                                                            #[cfg(not(windows))]
                                                            {
                                                                let channel_on = crate::update::installed_channel()
                                                                    == grokhub_core::Channel::Beta;
                                                                let mut beta_on = channel_on;
                                                                let channel_busy = self.running
                                                                    && self.update_pct.is_some();
                                                                let hint = if channel_busy {
                                                                    "Switching channel…"
                                                                } else {
                                                                    "On = beta branch. Off = stable (main). Same as install.sh --channel."
                                                                };
                                                                ui.add_enabled_ui(!channel_busy, |ui| {
                                                                    if crate::cards::settings_toggle(
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
                                                                        self.queue_channel_switch(target);
                                                                    }
                                                                });
                                                                crate::cards::settings_note(ui, &status_line);
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
                                                                        "Restart GrokHub",
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
                                                            if crate::cards::settings_toggle(
                                                                ui,
                                                                "Native engine (no Grok CLI)",
                                                                "New chats talk to xAI directly. Tools stay read-only.",
                                                                &mut self.cfg.native_engine,
                                                            ) {
                                                                self.persist_cfg();
                                                                self.status = "Saved".into();
                                                            }
                                                            if self.cfg.native_engine {
                                                                crate::native_mcp::paint(ui);
                                                                let cwd = self.grok_cwd();
                                                                crate::native_plugins::paint(ui, &cwd);
                                                            }
                                                            ui.add_space(12.0);
                                                            ui.label(
                                                                egui::RichText::new("Desktop-agent cursor (preview)")
                                                                    .size(crate::theme::FONT_UI)
                                                                    .color(crate::theme::fg()),
                                                            );
                                                            ui.label(
                                                                egui::RichText::new(
                                                                    "White/gray Cua-like path stub. Full desktop agent comes later.",
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
                                                    }
                                                });
                                            });
                                    },
                                );
                            });
                        });
                });
            });
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
            self.queue_combined_update();
        }
        if install_cli {
            self.queue_grok_cli_install();
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
        let locked = self.private_lock_for_paint().map(|why| super::privacy_ui::lock_hover(&why));
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
    fn drop_turn_for_pin(&mut self) {
        self.confirm = None;
        if self.running {
            self.halt_in_flight();
        }
        self.acp = None;
        self.acp_spawn_rx = None;
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
            self.acp_spawn_rx = None;
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
            DEFAULTS_ROWS,
            &[
                "Default model",
                "Use Grok 4.7 Fast when you're waiting",
                "Weekly spend cap",
                "Price limit",
                "Permission",
                "Let Grok control the desktop", "Desktop control", "Screen recording", "Session mode", "Always collapse"]
        );
        assert!(DEFAULTS_ROWS.iter().all(|r| !r.to_ascii_lowercase().contains("effort")));
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
