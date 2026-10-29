//! Settings → Privacy & data (deck slide 12, layout D): whether anything left
//! this PC this week, the real switches that decide what does, and a
//! seven-day ledger of off-device calls. The counts come from the egress log,
//! which records every call but not its size, so the ledger counts calls.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::harness as hx;
use grokhub_core::wire::PermissionMode;

use super::loadout_home::{caption, card};
use super::*;

const DAY_MS: u64 = 86_400_000;
/// How long a read of the egress log stays fresh on screen.
const LEDGER_TTL: Duration = Duration::from_secs(30);
/// Below this the story and the switches stack. The Settings page at
/// 1100x720 leaves about 800 px, which still fits both columns.
const TWO_COL_MIN_W: f32 = 760.0;
const DIAGRAM_H: f32 = 240.0;
/// Space kept between the story column's text and the switches.
const STORY_GUTTER: f32 = 24.0;
const BARS_H: f32 = 56.0;

pub(super) const HERO_LOCAL: &str = "Your data stays on this PC.";
pub(super) const HERO_SENT: &str = "Only what you allow leaves this PC.";
pub(super) const LEDGER_HEAD: &str = "Off-device ledger · 7 days";
pub(super) const PC_TILES: [&str; 4] = ["Local model", "Memory", "Files", "Game data"];
const WIPE_ARMED_ID: &str = "privacy-wipe-armed";

/// What the egress log says about the last seven days.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Week {
    Loading,
    /// The log is sealed and the keyring did not answer.
    Locked,
    /// Calls per UTC day, oldest first, today last.
    Counts([u32; 7]),
}

impl Week {
    fn total(&self) -> Option<u32> {
        match self {
            Week::Counts(days) => Some(days.iter().sum()),
            _ => None,
        }
    }
}

/// Calls per UTC day for the seven days ending today, oldest first.
pub(super) fn week_counts(stamps: &[u64], now_ms: u64) -> [u32; 7] {
    let today = now_ms / DAY_MS;
    let mut out = [0; 7];
    for &ts in stamps {
        let day = ts / DAY_MS;
        if day <= today && today - day < 7 {
            out[6 - (today - day) as usize] += 1;
        }
    }
    out
}

/// Weekday initial under each bar, oldest first (1970-01-01 was a Thursday).
pub(super) fn week_initials(now_ms: u64) -> [&'static str; 7] {
    const INITIALS: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];
    let today = now_ms / DAY_MS;
    std::array::from_fn(|i| INITIALS[((today + 5 + i as u64) % 7) as usize])
}

pub(super) fn mode_tag(week: &Week) -> &'static str {
    if week.total() == Some(0) {
        "LOCAL MODE"
    } else {
        "LOCAL FIRST"
    }
}

pub(super) fn hero(week: &Week) -> &'static str {
    if week.total() == Some(0) {
        HERO_LOCAL
    } else {
        HERO_SENT
    }
}

pub(super) fn sent_line(week: &Week) -> String {
    match week.total() {
        None if *week == Week::Locked => {
            "The off-device log is sealed and your keyring is locked.".into()
        }
        None => "Reading the off-device log…".into(),
        Some(0) => "0 calls sent off-device this week".into(),
        Some(1) => "1 call sent off-device this week".into(),
        Some(n) => format!("{n} calls sent off-device this week"),
    }
}

/// The ledger's top-right figure.
pub(super) fn ledger_total(week: &Week) -> String {
    match week.total() {
        Some(1) => "1 CALL".into(),
        Some(n) => format!("{n} CALLS"),
        None => "—".into(),
    }
}

struct LedgerCache {
    at: Option<Instant>,
    busy: bool,
    week: Option<Result<Vec<u64>, ()>>,
}

static LEDGER: Mutex<LedgerCache> = Mutex::new(LedgerCache {
    at: None,
    busy: false,
    week: None,
});

/// The last seven days of the egress log. The log is sealed and a read may
/// wait on the keyring, so it is read off the UI thread and kept for
/// [`LEDGER_TTL`].
fn ledger_week(ctx: &egui::Context, now: u64) -> Week {
    let Ok(mut g) = LEDGER.lock() else {
        return Week::Loading;
    };
    let stale = g.at.is_none_or(|at| at.elapsed() >= LEDGER_TTL);
    if stale && !g.busy {
        g.busy = true;
        let dir = crate::config::config_dir();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let read = hx::read_egress_report(&dir);
            let week = match read.locked {
                Some(_) => Err(()),
                None => Ok(read.lines.iter().map(|l| l.ts_ms).collect()),
            };
            if let Ok(mut g) = LEDGER.lock() {
                g.week = Some(week);
                g.at = Some(Instant::now());
                g.busy = false;
            }
            ctx.request_repaint();
        });
    }
    match &g.week {
        None => Week::Loading,
        Some(Err(())) => Week::Locked,
        Some(Ok(stamps)) => Week::Counts(week_counts(stamps, now)),
    }
}

/// The amber outlined tag above the hero.
fn mode_chip(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.5, crate::theme::amber()))
        .inner_margin(egui::Margin::symmetric(16, 8))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .font(crate::theme::title_font(crate::theme::TYPE_CAPTION + 1.0))
                    .extra_letter_spacing(2.5)
                    .color(crate::theme::amber()),
            );
        });
}

/// A cloud outline: the filled shape in the stroke colour, then the same
/// shape inset in the page colour.
fn paint_cloud(p: &egui::Painter, c: egui::Pos2, s: f32, stroke: Color32) {
    let lumps = [
        (egui::vec2(-0.42, 0.12), 0.30),
        (egui::vec2(0.0, -0.12), 0.42),
        (egui::vec2(0.42, 0.10), 0.32),
    ];
    let base = egui::Rect::from_min_max(
        c + egui::vec2(-0.42, 0.10) * s,
        c + egui::vec2(0.42, 0.42) * s,
    );
    for (inset, color) in [(0.0, stroke), (2.0, crate::theme::bg())] {
        for (off, r) in lumps {
            p.circle_filled(c + off * s, r * s - inset, color);
        }
        p.rect_filled(base.shrink(inset), 0.0, color);
    }
}

/// THIS PC with its four tiles, the link to the cloud, and the cloud's state.
pub(super) fn paint_privacy_diagram(ui: &mut egui::Ui, week: &Week) {
    let w = ui.available_width().min(560.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, DIAGRAM_H), egui::Sense::hover());
    let p = ui.painter_at(rect.expand(16.0));
    let off = week.total() == Some(0);
    let red = crate::theme::signal_red();
    let amber = crate::theme::amber();
    let pc = egui::Rect::from_min_size(
        rect.min + egui::vec2(12.0, 20.0),
        egui::vec2(w * 0.58, DIAGRAM_H - 32.0),
    );
    p.rect_filled(pc, 12.0, crate::theme::surface());
    p.rect_stroke(
        pc,
        12.0,
        egui::Stroke::new(1.5, red),
        egui::StrokeKind::Inside,
    );
    p.text(
        pc.min + egui::vec2(20.0, 22.0),
        egui::Align2::LEFT_CENTER,
        "THIS PC",
        crate::theme::title_font(crate::theme::TYPE_CAPTION + 1.0),
        crate::theme::muted(),
    );
    let grid = egui::Rect::from_min_max(
        pc.min + egui::vec2(20.0, 44.0),
        pc.max - egui::vec2(20.0, 20.0),
    );
    let (gap, tw, th) = (
        14.0,
        (grid.width() - 14.0) / 2.0,
        (grid.height() - 14.0) / 2.0,
    );
    for (i, label) in PC_TILES.iter().enumerate() {
        let at = grid.min + egui::vec2((i % 2) as f32 * (tw + gap), (i / 2) as f32 * (th + gap));
        let tile = egui::Rect::from_min_size(at, egui::vec2(tw, th));
        p.rect_filled(tile, 8.0, crate::theme::raised());
        p.text(
            tile.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(crate::theme::FONT_BODY + 1.0),
            crate::theme::fg(),
        );
    }
    if off {
        let badge = egui::pos2(pc.right() - 4.0, pc.top());
        p.circle_filled(badge, 16.0, amber);
        let ink = egui::Stroke::new(3.0, crate::theme::bg());
        p.line_segment(
            [badge + egui::vec2(-7.0, 0.0), badge + egui::vec2(-2.0, 5.0)],
            ink,
        );
        p.line_segment(
            [badge + egui::vec2(-2.0, 5.0), badge + egui::vec2(7.0, -5.0)],
            ink,
        );
    }
    // The cloud sits at the right edge; the link fills the gap before it.
    let cloud_s = ((rect.right() - pc.right()) * 0.4).clamp(48.0, 84.0);
    let cloud_c = egui::pos2(rect.right() - cloud_s * 0.8, pc.center().y - 14.0);
    let (a, b) = (
        egui::pos2(pc.right() + 6.0, pc.center().y),
        egui::pos2(cloud_c.x - cloud_s * 0.8, pc.center().y),
    );
    if off {
        let mut x = a.x;
        while x < b.x {
            let end = (x + 9.0).min(b.x);
            p.line_segment(
                [egui::pos2(x, a.y), egui::pos2(end, a.y)],
                egui::Stroke::new(2.0, red),
            );
            x += 15.0;
        }
        let m = egui::pos2((a.x + b.x) / 2.0, a.y);
        let cross = egui::Stroke::new(3.0, red);
        p.line_segment(
            [m + egui::vec2(-9.0, -14.0), m + egui::vec2(9.0, 14.0)],
            cross,
        );
        p.line_segment(
            [m + egui::vec2(9.0, -14.0), m + egui::vec2(-9.0, 14.0)],
            cross,
        );
    } else {
        p.line_segment([a, b], egui::Stroke::new(2.0, amber));
    }
    paint_cloud(&p, cloud_c, cloud_s, crate::theme::muted());
    let under = cloud_c.y + cloud_s * 0.42 + 26.0;
    p.text(
        egui::pos2(cloud_c.x, under),
        egui::Align2::CENTER_CENTER,
        "CLOUD",
        crate::theme::title_font(crate::theme::TYPE_CAPTION + 2.0),
        crate::theme::muted(),
    );
    let (state, color) = match week.total() {
        Some(0) => ("OFF", red),
        Some(_) => ("ON", amber),
        None => ("—", crate::theme::muted()),
    };
    p.text(
        egui::pos2(cloud_c.x, under + 24.0),
        egui::Align2::CENTER_CENTER,
        state,
        crate::theme::title_font(crate::theme::TYPE_CAPTION + 2.0),
        color,
    );
}

/// The tag, the hero, the week's count and the diagram.
pub(super) fn paint_privacy_story(ui: &mut egui::Ui, week: &Week) {
    // Ragged right with a gutter: a justified column stretched the hero's
    // letters at 1100x720 and ran it up to the switches.
    let w = (ui.available_width() - STORY_GUTTER).max(0.0);
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_max_width(w);
        paint_story_body(ui, week);
    });
}

fn paint_story_body(ui: &mut egui::Ui, week: &Week) {
    // A row, so the tag hugs its text.
    ui.horizontal(|ui| mode_chip(ui, mode_tag(week)));
    ui.add_space(14.0);
    ui.label(
        RichText::new(hero(week))
            .font(crate::theme::title_font(32.0))
            .color(crate::theme::fg()),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(sent_line(week))
            .size(crate::theme::FONT_BODY + 1.0)
            .color(crate::theme::muted()),
    );
    ui.add_space(28.0);
    paint_privacy_diagram(ui, week);
}

/// Seven bars of calls per day, today on the right.
pub(super) fn paint_ledger(ui: &mut egui::Ui, week: &Week, now: u64) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(caption(LEDGER_HEAD));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(ledger_total(week))
                        .size(crate::theme::TYPE_CAPTION)
                        .extra_letter_spacing(1.5)
                        .color(crate::theme::amber()),
                );
            });
        });
        ui.add_space(10.0);
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, BARS_H + 22.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let days = match week {
            Week::Counts(d) => *d,
            _ => [0; 7],
        };
        let max = days.iter().copied().max().unwrap_or(0).max(1) as f32;
        let slot = w / 7.0;
        for (i, (n, initial)) in days.iter().zip(week_initials(now)).enumerate() {
            let x0 = rect.left() + slot * i as f32 + 4.0;
            let base = rect.top() + BARS_H;
            let h = if *n == 0 {
                2.0
            } else {
                (*n as f32 / max * BARS_H).max(4.0)
            };
            let bar = egui::Rect::from_min_max(
                egui::pos2(x0, base - h),
                egui::pos2(x0 + slot - 8.0, base),
            );
            let fill = if *n == 0 {
                crate::theme::border_strong()
            } else {
                crate::theme::amber()
            };
            p.rect_filled(bar, 1.0, fill);
            p.text(
                egui::pos2(bar.center().x, base + 12.0),
                egui::Align2::CENTER_CENTER,
                initial,
                egui::FontId::proportional(crate::theme::TYPE_CAPTION - 2.0),
                crate::theme::muted(),
            );
        }
    });
}

impl Cabin {
    pub(super) fn ui_settings_privacy(&mut self, ui: &mut egui::Ui) {
        let now = now_ms();
        let week = ledger_week(ui.ctx(), now);
        if ui.available_width() >= TWO_COL_MIN_W {
            ui.columns(2, |cols| {
                paint_privacy_story(&mut cols[0], &week);
                self.privacy_switches(&mut cols[1], &week, now);
            });
        } else {
            paint_privacy_story(ui, &week);
            ui.add_space(20.0);
            self.privacy_switches(ui, &week, now);
        }
    }

    /// "Agent asks before changes" is the saved Ask / Auto choice.
    pub(super) fn set_asks_first(&mut self, on: bool) {
        let mode = if on {
            PermissionMode::Ask
        } else {
            PermissionMode::Auto
        };
        if self.permission_mode != mode {
            self.drop_turn_for_pin();
            self.set_permission_mode(mode);
            self.status = "Saved".into();
        }
    }

    fn privacy_switches(&mut self, ui: &mut egui::Ui, week: &Week, now: u64) {
        let mut local = self.cfg.local_model;
        let local_hint = if local {
            "Background tasks run on this PC"
        } else {
            "Background tasks use the cloud model"
        };
        if crate::cards::settings_toggle(ui, "On-device model", local_hint, &mut local) {
            self.set_local_model(local);
        }
        let mut asks = self.permission_mode == PermissionMode::Ask;
        if crate::cards::settings_toggle(
            ui,
            "Agent asks before changes",
            "Every tool that changes something waits for your OK. Off is Auto.",
            &mut asks,
        ) {
            self.set_asks_first(asks);
        }
        let mut know = self.know_me_on();
        if crate::cards::settings_toggle(
            ui,
            "Know me",
            "Learns tags from names on this PC, never file contents. Kept here.",
            &mut know,
        ) {
            self.set_know_me_on(know);
        }
        let lock = self.private_lock_for_paint();
        let mut sealed = lock.is_none();
        let sealed_hint = match &lock {
            None => "Personal memory and the logs are sealed with your keyring",
            Some(why) => super::privacy_ui::lock_hover(why),
        };
        // The same row as the switches above, read-only: its hint wraps ragged
        // right instead of stretching across the column.
        ui.add_enabled_ui(false, |ui| {
            crate::cards::settings_toggle(ui, "Encrypted memory", sealed_hint, &mut sealed);
        });
        ui.add_space(14.0);
        let armed_id = egui::Id::new(WIPE_ARMED_ID);
        let armed = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(armed_id))
            .unwrap_or(false);
        let size = egui::vec2(180.0, 40.0);
        ui.horizontal(|ui| {
            if crate::ui_kit::recipe::button(ui, None, "OPEN DATA FOLDER", false, size, true)
                .clicked()
            {
                let dir = crate::config::config_dir();
                if let Err(err) = crate::desktop::open_path(&dir.display().to_string()) {
                    self.status = format!("Could not open {}: {err}", dir.display());
                }
            }
            let label = if armed {
                "CONFIRM WIPE"
            } else {
                "WIPE KNOW ME"
            };
            if crate::ui_kit::recipe::button(ui, None, label, armed, size, true).clicked() {
                if armed {
                    self.wipe_know_me();
                }
                ui.ctx().data_mut(|d| d.insert_temp(armed_id, !armed));
            }
        });
        if armed {
            crate::cards::settings_note(
                ui,
                "Click again to turn Know me off and delete every tag it learned.",
            );
        }
        ui.add_space(14.0);
        paint_ledger(ui, week, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-28 (a Wednesday) at 05:00 UTC.
    const NOW: u64 = 20_754 * DAY_MS + 5 * 3_600_000;

    #[test]
    fn the_week_buckets_calls_by_day_and_drops_older_ones() {
        let stamps = [
            NOW,
            NOW - 3_600_000,
            NOW - DAY_MS,
            NOW - 6 * DAY_MS,
            NOW - 7 * DAY_MS,
            NOW + DAY_MS,
        ];
        assert_eq!(week_counts(&stamps, NOW), [1, 0, 0, 0, 0, 1, 2]);
        assert_eq!(week_counts(&[], NOW), [0; 7]);
        assert_eq!(week_initials(NOW), ["T", "F", "S", "S", "M", "T", "W"]);
    }

    #[test]
    fn the_story_follows_the_ledger() {
        let quiet = Week::Counts([0; 7]);
        assert_eq!(mode_tag(&quiet), "LOCAL MODE");
        assert_eq!(hero(&quiet), "Your data stays on this PC.");
        assert_eq!(sent_line(&quiet), "0 calls sent off-device this week");
        assert_eq!(ledger_total(&quiet), "0 CALLS");
        let one = Week::Counts([0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(mode_tag(&one), "LOCAL FIRST");
        assert_eq!(hero(&one), "Only what you allow leaves this PC.");
        assert_eq!(sent_line(&one), "1 call sent off-device this week");
        assert_eq!(ledger_total(&one), "1 CALL");
        let busy = Week::Counts([3, 0, 0, 0, 0, 2, 7]);
        assert_eq!(sent_line(&busy), "12 calls sent off-device this week");
        assert_eq!(
            sent_line(&Week::Locked),
            "The off-device log is sealed and your keyring is locked."
        );
        assert_eq!(hero(&Week::Locked), HERO_SENT);
        assert_eq!(ledger_total(&Week::Loading), "—");
    }

    fn texts(week: &Week) -> String {
        fn walk(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(t) => {
                    out.push_str(t.galley.text());
                    out.push('\n');
                }
                egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                paint_privacy_story(ui, week);
                paint_ledger(ui, week, NOW);
            });
        });
        let mut s = String::new();
        for c in &out.shapes {
            walk(&c.shape, &mut s);
        }
        s
    }

    #[test]
    fn the_page_paints_the_deck_story() {
        let quiet = texts(&Week::Counts([0; 7]));
        for want in [
            "LOCAL MODE",
            "Your data stays on this PC.",
            "THIS PC",
            "Local model",
            "Memory",
            "Files",
            "Game data",
            "CLOUD",
            "OFF",
            "OFF-DEVICE LEDGER · 7 DAYS",
            "0 CALLS",
        ] {
            assert!(quiet.contains(want), "missing {want:?} in {quiet}");
        }
        let sent = texts(&Week::Counts([0, 0, 0, 0, 0, 1, 2]));
        assert!(sent.contains("\nON\n"), "cloud reads ON: {sent}");
        assert!(sent.contains("3 CALLS"), "{sent}");
        assert!(!sent.contains("\nOFF\n"), "{sent}");
    }

    #[test]
    fn the_asks_switch_saves_ask_or_auto() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.set_asks_first(false);
        assert_eq!(cabin.permission_mode, PermissionMode::Auto);
        assert_eq!(cabin.cfg.permission_mode, "auto");
        cabin.set_asks_first(true);
        assert_eq!(cabin.permission_mode, PermissionMode::Ask);
        assert_eq!(cabin.cfg.permission_mode, "ask");
    }

    #[test]
    fn loadout_settings_is_a_page_wide_enough_for_both_columns() {
        use super::super::settings::settings_page_rect;
        for (w, h) in [(1600.0, 900.0), (1100.0, 720.0)] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
            let page = settings_page_rect(screen, true, SettingsSec::Privacy);
            assert_eq!(page.min, egui::pos2(40.0, 40.0));
            assert_eq!(page.max, egui::pos2(w, h));
            // The nav column is 220 px and the body keeps a 40 px gutter.
            assert!(page.width() - 260.0 >= TWO_COL_MIN_W, "{w}x{h}");
        }
        // The sheet takes 80% of the window under the 40 px titlebar.
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let sheet = settings_page_rect(screen, false, SettingsSec::Privacy);
        assert_eq!(sheet.size(), egui::vec2(1280.0, 688.0));
        assert_eq!(sheet.center(), egui::pos2(800.0, 470.0));
    }

    #[test]
    fn at_1100x720_the_hero_stays_ragged_right_and_clear_of_the_switches() {
        let mut cabin = Cabin::quiet_for_test();
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 720.0),
            )),
            ..Default::default()
        };
        // The Settings body is a justified column, as on the page.
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    cabin.ui_settings_privacy(ui);
                });
            });
        });
        let mut texts: Vec<(String, egui::Rect, bool)> = Vec::new();
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect, bool)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    t.galley.job.justify,
                )),
                egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, out)),
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        let find = |want: &str| {
            texts
                .iter()
                .find(|t| t.0.starts_with(want))
                .unwrap_or_else(|| panic!("{want} not painted: {texts:?}"))
                .clone()
        };
        let (_, hero_rect, hero_justified) = find(HERO_SENT);
        let (_, switch_rect, _) = find("On-device model");
        assert!(!hero_justified, "the hero is ragged right");
        assert!(
            hero_rect.right() + STORY_GUTTER - 1.0 <= switch_rect.left(),
            "hero {hero_rect:?} runs into the switches at {switch_rect:?}"
        );
        // The hint under Encrypted memory says sealed or why not, by keyring.
        let at = texts
            .iter()
            .position(|t| t.0 == "Encrypted memory")
            .expect("Encrypted memory row");
        let (hint, _, hint_justified) = &texts[at + 1];
        assert!(!hint_justified, "{hint} is ragged right");
    }

    #[test]
    fn privacy_is_a_settings_section() {
        assert_eq!(settings_sec_title(SettingsSec::Privacy), "Privacy & data");
    }
}
