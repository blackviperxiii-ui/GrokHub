//! Settings → Privacy & data (deck slide 12, layout D): whether anything left
//! this PC this week, the real switches that decide what does, and a
//! seven-day ledger of off-device calls. The counts come from the egress log,
//! which records every call but not its size, so the ledger counts calls.

use std::path::Path;
use std::sync::{LazyLock, Mutex};
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
/// Signed in, so chat and background work can reach Grok, but nothing has yet.
pub(super) const HERO_IDLE: &str = "Nothing left this PC this week.";
pub(super) const LEDGER_HEAD: &str = "Off-device ledger · 7 days";
pub(super) const PC_TILES: [&str; 4] = ["Local model", "Memory", "Files", "Game data"];
const WIPE_ARMED_ID: &str = "privacy-wipe-armed";
const OFFLINE_ROW_ID: &str = "privacy-offline-row";

pub(super) const OFFLINE_TITLE: &str = "Offline only";
pub(super) static OFFLINE_HINT: LazyLock<&'static str> = LazyLock::new(|| {
    grokhub_core::brand::filled(
        "{product} stays off the internet and uses your local model. Web search, cloud models and online connectors pause.",
    )
});
pub(super) const OFFLINE_NOT_READY: &str =
    "No local model is ready, and nothing can be downloaded while Offline only is on.";

/// Save Offline only to `{dir}/offline.toml` and apply it, then reload the MCP
/// slots so online connectors pause or come back.
pub(super) fn switch_offline(dir: &Path, on: bool) -> Result<(), String> {
    grokhub_core::offline::set_in(dir, on)?;
    grokhub_agent::mcp::invalidate();
    Ok(())
}

/// The Offline only row between frames: the not-ready confirm, and what the
/// last save said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct OfflineRow {
    /// Waiting on Turn on anyway / Cancel because no local model is ready.
    pub confirming: bool,
    /// A save that failed, shown under the switch.
    pub error: Option<String>,
}

impl OfflineRow {
    /// The switch was flipped to `on`. Turning it on with no local model ready
    /// asks first; `local_ready` is read only then.
    pub(super) fn flip(&mut self, dir: &Path, on: bool, local_ready: impl FnOnce() -> bool) {
        self.error = None;
        self.confirming = on && !local_ready();
        if !self.confirming {
            self.save(dir, on);
        }
    }

    pub(super) fn turn_on_anyway(&mut self, dir: &Path) {
        self.confirming = false;
        self.save(dir, true);
    }

    pub(super) fn cancel(&mut self) {
        self.confirming = false;
    }

    fn save(&mut self, dir: &Path, on: bool) {
        self.error = switch_offline(dir, on).err();
    }
}

/// The Offline only switch, its confirm, and any save error.
fn paint_offline_row(ui: &mut egui::Ui) {
    let id = egui::Id::new(OFFLINE_ROW_ID);
    let mut row: OfflineRow = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    // The cabin's config dir (the same folder `offline::set` uses outside tests).
    let dir = crate::config::config_dir();
    let mut on = grokhub_core::offline::enabled();
    if crate::cards::settings_toggle(ui, OFFLINE_TITLE, *OFFLINE_HINT, &mut on) {
        row.flip(&dir, on, grokhub_agent::route::local::ready_for_offline);
    }
    if row.confirming {
        crate::cards::settings_note(ui, OFFLINE_NOT_READY);
        let size = egui::vec2(180.0, 40.0);
        ui.horizontal(|ui| {
            if crate::ui_kit::recipe::button(ui, None, "TURN ON ANYWAY", true, size, true).clicked()
            {
                row.turn_on_anyway(&dir);
            }
            if crate::ui_kit::recipe::button(ui, None, "CANCEL", false, size, true).clicked() {
                row.cancel();
            }
        });
        ui.add_space(10.0);
    }
    if let Some(err) = &row.error {
        ui.label(RichText::new(err).size(13.0).color(crate::theme::offline()));
        ui.add_space(8.0);
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, row));
}

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
    pub(super) fn total(&self) -> Option<u32> {
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

/// What can reach the cloud right now, from the real routing: once signed in,
/// chat and background work go to Grok while any cloud model answers, and an
/// installed on-device model takes over only when none does or offline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cloud {
    /// GrokHub's own sign-in or an API key is there.
    pub signed_in: bool,
    /// The on-device model is on and installed.
    pub local_jobs: bool,
}

impl Cloud {
    /// Nothing has left this week and nothing can.
    fn sealed(self, week: &Week) -> bool {
        week.total() == Some(0) && !self.signed_in
    }
}

pub(super) fn mode_tag(week: &Week, cloud: Cloud) -> &'static str {
    if cloud.sealed(week) {
        "LOCAL MODE"
    } else {
        "LOCAL FIRST"
    }
}

pub(super) fn hero(week: &Week, cloud: Cloud) -> &'static str {
    match week.total() {
        Some(0) if cloud.signed_in => HERO_IDLE,
        Some(0) => HERO_LOCAL,
        _ => HERO_SENT,
    }
}

/// Which features send what, under the week's count.
pub(super) fn cloud_line(cloud: Cloud) -> &'static str {
    if !cloud.signed_in {
        "Grok isn't signed in, so nothing can leave this PC."
    } else if cloud.local_jobs {
        "Chat and background tasks send their prompts to Grok in the cloud. This PC's model takes over only when no cloud model answers or you're offline."
    } else {
        "Chat and background tasks send their prompts to Grok in the cloud."
    }
}

/// The diagram's cloud: OFF only when nothing can leave, READY when it can
/// but nothing has this week, ON once something did.
pub(super) fn cloud_state(week: &Week, cloud: Cloud) -> &'static str {
    match week.total() {
        Some(0) if cloud.signed_in => "READY",
        Some(0) => "OFF",
        Some(_) => "ON",
        None => "—",
    }
}

/// The On-device model switch's hint, by what the router really does.
pub(super) fn local_hint(on: bool, installed: bool) -> &'static str {
    match (on, installed) {
        (false, _) => "Background tasks use Grok in the cloud",
        (true, false) => {
            "On, but the on-device model isn't installed, so background tasks still use Grok"
        }
        (true, true) => "Used only when no cloud model answers or you're offline",
    }
}

/// How the on-device model is used, from `Router::choose` and the sidecar.
/// PRIVACY.md and the installer's privacy.txt list the same lines.
pub(super) const LOCAL_ROUTING: [&str; 5] = [
    "Online, the cloud takes every step while any cloud model answers. This PC's model is the last resort, never for hard or long work.",
    "Offline, this PC's model takes what it can. Work that needs the cloud waits for the network.",
    "Sensitive data you haven't allowed to the cloud stays on this PC.",
    "The model loads only when it's needed and unloads when it sits idle.",
    "Explicit 18+ chats go only to xAI or this PC's model.",
];

/// The lines under the On-device model switch: only while it can run.
pub(super) fn local_notes(on: bool, installed: bool) -> &'static [&'static str] {
    if on && installed {
        &LOCAL_ROUTING
    } else {
        &[]
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

/// Tests: the ledger as a fresh process has it, still loading. Settings now
/// paints every section, so an earlier test in the same `cargo test` process
/// may have read the ledger already.
#[cfg(test)]
fn ledger_still_loading_for_test() {
    if let Ok(mut g) = LEDGER.lock() {
        *g = LedgerCache {
            at: Some(Instant::now()),
            busy: false,
            week: None,
        };
    }
}

/// The last seven days of the egress log. The log is sealed and a read may
/// wait on the keyring, so it is read off the UI thread and kept for
/// [`LEDGER_TTL`].
pub(super) fn ledger_week(ctx: &egui::Context, now: u64) -> Week {
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
pub(super) fn paint_privacy_diagram(ui: &mut egui::Ui, week: &Week, cloud: Cloud) {
    let w = ui.available_width().min(560.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, DIAGRAM_H), egui::Sense::hover());
    let p = ui.painter_at(rect.expand(16.0));
    let off = cloud.sealed(week);
    let ready = week.total() == Some(0) && cloud.signed_in;
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
    if off || ready {
        let dash = if off { red } else { amber };
        let mut x = a.x;
        while x < b.x {
            let end = (x + 9.0).min(b.x);
            p.line_segment(
                [egui::pos2(x, a.y), egui::pos2(end, a.y)],
                egui::Stroke::new(2.0, dash),
            );
            x += 15.0;
        }
    }
    if off {
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
    } else if !ready {
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
    let state = cloud_state(week, cloud);
    let color = match state {
        "OFF" => red,
        "—" => crate::theme::muted(),
        _ => amber,
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
pub(super) fn paint_privacy_story(ui: &mut egui::Ui, week: &Week, cloud: Cloud) {
    // Ragged right with a gutter: a justified column stretched the hero's
    // letters at 1100x720 and ran it up to the switches.
    let w = (ui.available_width() - STORY_GUTTER).max(0.0);
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_max_width(w);
        paint_story_body(ui, week, cloud);
    });
}

fn paint_story_body(ui: &mut egui::Ui, week: &Week, cloud: Cloud) {
    // A row, so the tag hugs its text.
    ui.horizontal(|ui| mode_chip(ui, mode_tag(week, cloud)));
    ui.add_space(14.0);
    ui.label(
        RichText::new(hero(week, cloud))
            .font(crate::theme::title_font(32.0))
            .color(crate::theme::fg()),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(sent_line(week))
            .size(crate::theme::FONT_BODY + 1.0)
            .color(crate::theme::muted()),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(cloud_line(cloud))
            .size(crate::theme::FONT_BODY)
            .color(crate::theme::muted()),
    );
    ui.add_space(24.0);
    paint_privacy_diagram(ui, week, cloud);
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
        let cloud = Cloud {
            signed_in: self.agent_ready(),
            local_jobs: self.cfg.local_model && grokhub_agent::route::local::installed(),
        };
        if ui.available_width() >= TWO_COL_MIN_W {
            ui.columns(2, |cols| {
                paint_privacy_story(&mut cols[0], &week, cloud);
                self.privacy_switches(&mut cols[1], &week, now);
            });
        } else {
            paint_privacy_story(ui, &week, cloud);
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
        paint_offline_row(ui);
        let mut local = self.cfg.local_model;
        let hint = local_hint(local, local && grokhub_agent::route::local::installed());
        if crate::cards::settings_toggle(ui, "On-device model", hint, &mut local) {
            self.set_local_model(local);
        }
        for line in local_notes(local, grokhub_agent::route::local::installed()) {
            crate::cards::settings_note(ui, line);
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

    const SIGNED_OUT: Cloud = Cloud {
        signed_in: false,
        local_jobs: false,
    };
    const CLOUD_ONLY: Cloud = Cloud {
        signed_in: true,
        local_jobs: false,
    };

    #[test]
    fn the_story_follows_the_ledger() {
        let quiet = Week::Counts([0; 7]);
        assert_eq!(mode_tag(&quiet, SIGNED_OUT), "LOCAL MODE");
        assert_eq!(hero(&quiet, SIGNED_OUT), "Your data stays on this PC.");
        assert_eq!(sent_line(&quiet), "0 calls sent off-device this week");
        assert_eq!(ledger_total(&quiet), "0 CALLS");
        let one = Week::Counts([0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(mode_tag(&one, CLOUD_ONLY), "LOCAL FIRST");
        assert_eq!(
            hero(&one, CLOUD_ONLY),
            "Only what you allow leaves this PC."
        );
        assert_eq!(sent_line(&one), "1 call sent off-device this week");
        assert_eq!(ledger_total(&one), "1 CALL");
        let busy = Week::Counts([3, 0, 0, 0, 0, 2, 7]);
        assert_eq!(sent_line(&busy), "12 calls sent off-device this week");
        assert_eq!(
            sent_line(&Week::Locked),
            "The off-device log is sealed and your keyring is locked."
        );
        assert_eq!(hero(&Week::Locked, SIGNED_OUT), HERO_SENT);
        assert_eq!(ledger_total(&Week::Loading), "—");
    }

    #[test]
    fn a_quiet_week_is_not_cloud_off_while_the_cloud_is_in_use() {
        // QA: "Cloud OFF / Your data stays on this PC" next to "Background
        // tasks use the cloud model". Signed in, the cloud is ready, not off.
        let quiet = Week::Counts([0; 7]);
        assert_eq!(mode_tag(&quiet, CLOUD_ONLY), "LOCAL FIRST");
        assert_eq!(hero(&quiet, CLOUD_ONLY), "Nothing left this PC this week.");
        assert_eq!(cloud_state(&quiet, CLOUD_ONLY), "READY");
        assert_eq!(cloud_state(&quiet, SIGNED_OUT), "OFF");
        assert_eq!(
            cloud_state(&Week::Counts([0, 0, 0, 0, 0, 0, 2]), CLOUD_ONLY),
            "ON"
        );
        assert_eq!(cloud_state(&Week::Loading, CLOUD_ONLY), "—");
        assert_eq!(
            cloud_line(CLOUD_ONLY),
            "Chat and background tasks send their prompts to Grok in the cloud."
        );
        assert_eq!(
            cloud_line(Cloud {
                signed_in: true,
                local_jobs: true
            }),
            "Chat and background tasks send their prompts to Grok in the cloud. This PC's model takes over only when no cloud model answers or you're offline."
        );
        assert_eq!(
            cloud_line(SIGNED_OUT),
            "Grok isn't signed in, so nothing can leave this PC."
        );
        assert_eq!(
            local_hint(false, true),
            "Background tasks use Grok in the cloud"
        );
        assert_eq!(
            local_hint(true, false),
            "On, but the on-device model isn't installed, so background tasks still use Grok"
        );
        assert_eq!(
            local_hint(true, true),
            "Used only when no cloud model answers or you're offline"
        );
        assert_eq!(
            local_notes(true, true),
            [
                "Online, the cloud takes every step while any cloud model answers. This PC's model is the last resort, never for hard or long work.",
                "Offline, this PC's model takes what it can. Work that needs the cloud waits for the network.",
                "Sensitive data you haven't allowed to the cloud stays on this PC.",
                "The model loads only when it's needed and unloads when it sits idle.",
                "Explicit 18+ chats go only to xAI or this PC's model.",
            ]
        );
        assert!(local_notes(true, false).is_empty());
        assert!(local_notes(false, true).is_empty());
    }

    fn texts(week: &Week, cloud: Cloud) -> String {
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
                paint_privacy_story(ui, week, cloud);
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
        let quiet = texts(&Week::Counts([0; 7]), SIGNED_OUT);
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
        let sent = texts(&Week::Counts([0, 0, 0, 0, 0, 1, 2]), CLOUD_ONLY);
        assert!(sent.contains("\nON\n"), "cloud reads ON: {sent}");
        assert!(sent.contains("3 CALLS"), "{sent}");
        assert!(!sent.contains("\nOFF\n"), "{sent}");
        let ready = texts(&Week::Counts([0; 7]), CLOUD_ONLY);
        for want in [
            "LOCAL FIRST",
            "Nothing left this PC this week.",
            "\nREADY\n",
            "Chat and background tasks send their prompts to Grok in the cloud.",
        ] {
            assert!(ready.contains(want), "missing {want:?} in {ready}");
        }
        assert!(!ready.contains("\nOFF\n"), "{ready}");
        assert!(!ready.contains("Your data stays on this PC."), "{ready}");
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
        ledger_still_loading_for_test();
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

    fn offline_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-app-offline-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Leaves Offline only off for the rest of this test process.
    struct OffAfter(std::path::PathBuf);

    impl Drop for OffAfter {
        fn drop(&mut self) {
            let _ = grokhub_core::offline::set_in(&self.0, false);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn saved(dir: &Path) -> Option<String> {
        std::fs::read_to_string(dir.join("offline.toml")).ok()
    }

    #[test]
    fn the_offline_switch_writes_offline_toml_and_flips_enabled() {
        let dir = offline_dir("flip");
        let _off = OffAfter(dir.clone());
        let mut row = OfflineRow::default();
        row.flip(&dir, true, || true);
        assert_eq!(row, OfflineRow::default(), "a ready model needs no confirm");
        assert_eq!(saved(&dir).as_deref(), Some("offline_only = true\n"));
        assert!(grokhub_core::offline::enabled());
        row.flip(&dir, false, || panic!("turning it off never asks"));
        assert_eq!(row, OfflineRow::default());
        assert_eq!(saved(&dir).as_deref(), Some("offline_only = false\n"));
        assert!(!grokhub_core::offline::enabled());
    }

    #[test]
    fn with_no_local_model_it_asks_first_and_cancel_leaves_it_off() {
        let dir = offline_dir("confirm");
        let _off = OffAfter(dir.clone());
        grokhub_core::offline::set_in(&dir, false).unwrap();
        let mut row = OfflineRow::default();
        row.flip(&dir, true, || false);
        assert_eq!(
            row,
            OfflineRow {
                confirming: true,
                error: None
            }
        );
        assert_eq!(saved(&dir).as_deref(), Some("offline_only = false\n"));
        assert!(!grokhub_core::offline::enabled());
        row.cancel();
        assert_eq!(row, OfflineRow::default());
        assert_eq!(saved(&dir).as_deref(), Some("offline_only = false\n"));
        assert!(!grokhub_core::offline::enabled());
        row.flip(&dir, true, || false);
        row.turn_on_anyway(&dir);
        assert_eq!(row, OfflineRow::default());
        assert_eq!(saved(&dir).as_deref(), Some("offline_only = true\n"));
        assert!(grokhub_core::offline::enabled());
    }

    #[test]
    fn a_failed_save_shows_under_the_switch() {
        let dir = offline_dir("broken");
        let _off = OffAfter(dir.clone());
        grokhub_core::offline::set_in(&dir, false).unwrap();
        // A file where the config folder should be: the save can't land.
        let blocked = dir.join("not-a-dir");
        std::fs::write(&blocked, "x").unwrap();
        let mut row = OfflineRow::default();
        row.flip(&blocked, true, || true);
        let err = row.error.clone().expect("the error is kept for the row");
        assert!(err.starts_with("Couldn't save Offline only: "), "{err}");
        assert!(!row.confirming);
        assert!(!grokhub_core::offline::enabled());
    }

    fn paint_row(row: Option<OfflineRow>) -> String {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        if let Some(row) = row {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new(OFFLINE_ROW_ID), row));
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 600.0),
            )),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, paint_offline_row);
        });
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
        let mut s = String::new();
        for c in &out.shapes {
            walk(&c.shape, &mut s);
        }
        s
    }

    #[test]
    fn the_offline_row_paints_its_sentence_and_the_confirm_only_when_asked() {
        let _off = grokhub_core::offline::force_for_thread(false);
        let quiet = paint_row(None);
        assert!(quiet.contains("Offline only\n"), "{quiet}");
        assert!(
            quiet.contains("Loadout stays off the internet and uses your local model. Web search, cloud models and online connectors pause."),
            "{quiet}"
        );
        assert!(!quiet.contains("TURN ON ANYWAY"), "{quiet}");
        let asking = paint_row(Some(OfflineRow {
            confirming: true,
            error: None,
        }));
        for want in [
            "No local model is ready, and nothing can be downloaded while Offline only is on.",
            "TURN ON ANYWAY",
            "CANCEL",
        ] {
            assert!(asking.contains(want), "missing {want:?} in {asking}");
        }
        let failed = paint_row(Some(OfflineRow {
            confirming: false,
            error: Some("Couldn't save Offline only: disk full".into()),
        }));
        assert!(
            failed.contains("Couldn't save Offline only: disk full"),
            "{failed}"
        );
    }

    #[test]
    fn privacy_is_a_settings_section() {
        assert_eq!(settings_sec_title(SettingsSec::Privacy), "Privacy & data");
    }
}
