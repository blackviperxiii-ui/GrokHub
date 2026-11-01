//! The cabin side of the daily rhythm (`grokhub_core::rhythm`): a minute
//! counts while the user is typing in the cabin, low-priority proactive
//! nudges wait for the next usually-active hour, the Pulse page offers once to
//! post the brief at the hour the user usually sits down, and Settings →
//! Behavior shows the week with the learned hours. Quiet hours the user set
//! are checked first and always win.

use super::*;
use grokhub_core::proactive::Candidate;
use grokhub_core::rhythm::{self, hour_label, Rhythm};
use grokhub_core::LocalClock;

/// Typing within this long counts the current minute as active.
const ACTIVE_WINDOW: Duration = Duration::from_secs(120);
/// A proactive candidate at or over this value goes up even in an away hour.
pub(super) const NUDGE_URGENT_VALUE: f64 = 0.9;
const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

fn rhythm_path() -> std::path::PathBuf {
    crate::config::config_dir().join(rhythm::FILE_NAME)
}

/// "8 AM, 6 PM–10 PM" from the usually-active hours.
pub(super) fn active_ranges(r: &Rhythm) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut h = 0;
    while h < 24 {
        if !r.usually_active(h) {
            h += 1;
            continue;
        }
        let start = h;
        while h + 1 < 24 && r.usually_active(h + 1) {
            h += 1;
        }
        out.push(if start == h {
            hour_label(start)
        } else {
            format!("{}–{}", hour_label(start), hour_label(h))
        });
        h += 1;
    }
    out.join(", ")
}

impl Cabin {
    /// The user typed or clicked in the cabin.
    pub(super) fn touch(&mut self) {
        self.last_activity = Instant::now();
        self.reflected_idle = false;
    }

    /// Read from disk the first time anything needs it.
    pub(super) fn rhythm_now(&mut self) -> &mut Rhythm {
        self.sources
            .rhythm
            .get_or_insert_with(|| rhythm::load(&rhythm_path()))
    }

    fn save_rhythm(&mut self) {
        let state = self.rhythm_now().clone();
        if let Err(err) = rhythm::save(&rhythm_path(), &state) {
            self.status = err;
        }
    }

    /// From Housekeep: the current minute counts when the user typed lately.
    pub(super) fn note_rhythm(&mut self) {
        if self.last_activity.elapsed() > ACTIVE_WINDOW {
            return;
        }
        let clock = Self::local_clock();
        self.note_rhythm_at(&clock);
    }

    pub(super) fn note_rhythm_at(&mut self, clock: &LocalClock) {
        let before: u32 = self.rhythm_now().sittings.iter().sum();
        if !self.rhythm_now().note_active(clock) {
            return;
        }
        let started = self.rhythm_now().sittings.iter().sum::<u32>() > before;
        if started && self.rhythm_now().games_on {
            let offset =
                grokhub_core::pulse_writer::utc_offset_min(clock.hour, clock.minute, clock.now_ms);
            for (game, played_ms) in self.steam_last_played(clock.now_ms) {
                self.rhythm_now().note_played(&game, played_ms, offset);
            }
        }
        if started || self.rhythm_now().total_minutes().is_multiple_of(5) {
            self.save_rhythm();
        }
    }

    /// True when this is a learned away hour and nothing ranked is urgent, so
    /// the nudges wait in the quiet queue for the next usually-active hour.
    pub(super) fn rhythm_holds(&mut self, ranked: &[Candidate], hour: u32) -> bool {
        self.rhythm_now().usually_away(hour) && ranked.iter().all(|c| c.value < NUDGE_URGENT_VALUE)
    }

    /// The brief's hour gate for `tick_feed_pulse`.
    pub(super) fn brief_gate(&mut self, hour: u32, now: u64) -> Option<bool> {
        let last = self.cfg.feed_pulse.last_digest_ms;
        self.rhythm_now().brief_gate(hour, last, now)
    }

    pub(super) fn answer_brief_offer(&mut self, hour: Option<u32>) {
        match hour {
            Some(h) => {
                self.rhythm_now().accept_brief(h);
                self.status = format!(
                    "Your Pulse brief will come around {}. Settings → Behavior changes it.",
                    hour_label(h)
                );
            }
            None => {
                self.rhythm_now().decline_brief();
                self.status = "Okay, the brief keeps its usual timing.".into();
            }
        }
        self.save_rhythm();
    }

    /// One line under the Pulse header, asked once.
    pub(super) fn paint_brief_offer(&mut self, ui: &mut egui::Ui) {
        let Some(hour) = self.rhythm_now().brief_offer() else {
            return;
        };
        let mut answer = None;
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "You're usually on around {}. Want your Pulse brief then?",
                    hour_label(hour)
                ))
                .size(crate::theme::FONT_BODY)
                .color(crate::theme::fg()),
            );
            if crate::cards::ghost_pill(ui, "Yes") {
                answer = Some(Some(hour));
            }
            if crate::cards::ghost_pill(ui, "No thanks") {
                answer = Some(None);
            }
        });
        if let Some(a) = answer {
            self.answer_brief_offer(a);
        }
    }

    /// Settings → Behavior: the learned week, the hours it implies and the
    /// brief hour. Nothing here changes quiet hours without a tap.
    pub(super) fn ui_rhythm_rows(&mut self, ui: &mut egui::Ui) {
        let mut on = self.rhythm_now().on;
        if crate::cards::settings_toggle(
            ui,
            "Learn my daily rhythm",
            grokhub_core::brand::filled("Counts the minutes you're typing in {product}, by weekday and hour, on this computer. Low-priority nudges wait for an hour you're usually around."),
            &mut on,
        ) {
            self.rhythm_now().on = on;
            self.save_rhythm();
        }
        if !on {
            return;
        }
        let mut games = self.rhythm_now().games_on;
        if crate::cards::settings_toggle(
            ui,
            "Count when I last played",
            "Adds each game's last-played time from your Steam library. Off by default.",
            &mut games,
        ) {
            self.rhythm_now().games_on = games;
            self.save_rhythm();
        }
        let r = self.rhythm_now().clone();
        if r.total_minutes() > 0 {
            ui.add_space(6.0);
            paint_week(ui, &r);
        }
        let summary = if r.enough() {
            let mut line = format!("Usually on: {}.", active_ranges(&r));
            if let Some((start, end)) = r.suggested_quiet() {
                line.push_str(&format!(
                    " Usually away: {}–{}.",
                    hour_label(start),
                    hour_label(end)
                ));
            }
            line
        } else {
            "Still learning. A few days of use fills this in.".to_string()
        };
        crate::cards::settings_note(ui, &summary);
        if let Some((start, end)) = r.suggested_quiet() {
            let (s, e) = (format!("{start:02}:00"), format!("{end:02}:00"));
            if (s.as_str(), e.as_str())
                != (self.cfg.quiet_start.as_str(), self.cfg.quiet_end.as_str())
                && crate::cards::ghost_pill(ui, "Use as quiet hours")
            {
                self.cfg.quiet_start = s.clone();
                self.cfg.quiet_end = e.clone();
                self.quiet_start_buf = s;
                self.quiet_end_buf = e;
                self.persist_cfg();
                self.status = "Saved".into();
            }
        }
        let mut choices = vec!["Its usual timing".to_string()];
        choices.extend((0..24).map(hour_label));
        let selected = r
            .brief_hour
            .map(hour_label)
            .unwrap_or_else(|| choices[0].clone());
        if let Some(i) = crate::cards::settings_dropdown(
            ui,
            "Pulse brief time",
            "The brief posts once a day at this hour. Quiet hours still hold it.",
            &selected,
            &choices,
        ) {
            if i == 0 {
                self.rhythm_now().brief_hour = None;
            } else {
                self.rhythm_now().accept_brief(i as u32 - 1);
            }
            self.save_rhythm();
            self.status = "Saved".into();
        }
        if r.total_minutes() > 0 && crate::cards::ghost_pill(ui, "Forget my rhythm") {
            self.rhythm_now().reset();
            self.save_rhythm();
            self.status = "Rhythm forgotten.".into();
        }
        ui.add_space(8.0);
    }
}

/// Seven rows of 24 cells, brighter where the user is around more.
fn paint_week(ui: &mut egui::Ui, r: &Rhythm) {
    let busiest = r.busiest_slot().max(1) as f32;
    let label_w = 34.0;
    let cell = ((ui.available_width() - label_w) / 24.0).clamp(6.0, 16.0);
    let h = 10.0;
    for (day, name) in DAYS.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (lab, _) = ui.allocate_exact_size(egui::vec2(label_w, h), egui::Sense::hover());
            ui.painter().text(
                lab.left_center(),
                egui::Align2::LEFT_CENTER,
                *name,
                egui::FontId::proportional(10.0),
                crate::theme::muted(),
            );
            for hour in 0..24 {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(cell, h), egui::Sense::hover());
                let m = r.at(day as u8, hour);
                let t = m as f32 / busiest;
                let fill = if m == 0 {
                    crate::theme::surface()
                } else {
                    crate::theme::amber().gamma_multiply(0.25 + 0.75 * t)
                };
                ui.painter().rect_filled(rect.shrink(1.0), 2.0, fill);
                resp.on_hover_text(format!("{name} {}: {m} min", hour_label(hour)));
            }
        });
    }
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::proactive::{CandidateSource, Reversibility};

    const T0: u64 = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000);

    fn pinned(label: &str) -> crate::config::TestConfigDir {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        crate::config::TestConfigDir::set(root)
    }

    fn clock(at_ms: u64) -> LocalClock {
        let (weekday, hour) = rhythm::local_slot(at_ms, 0);
        LocalClock {
            now_ms: at_ms,
            weekday,
            hour,
            minute: ((at_ms / 60_000) % 60) as u32,
        }
    }

    /// A week of evenings, 18:00-23:00, written to the cabin's rhythm.
    fn evening_cabin() -> Cabin {
        let mut cabin = Cabin::quiet_for_test();
        cabin.sources.rhythm = Some(Rhythm::default());
        for d in 0..7 {
            for m in (18 * 60)..(23 * 60) {
                cabin.note_rhythm_at(&clock(T0 + d * 86_400_000 + m * 60_000));
            }
        }
        cabin
    }

    fn nudge(topic: &str, value: f64) -> Candidate {
        let mut c = Candidate::soft(CandidateSource::Workboard, topic, topic, value, 0.9, "test");
        c.reversible = Reversibility::Ledger;
        c
    }

    #[test]
    fn typing_minutes_fill_the_week_and_name_the_hours() {
        let _dir = pinned("rhythm-week");
        let mut cabin = evening_cabin();
        let r = cabin.rhythm_now().clone();
        assert_eq!(r.total_minutes(), 7 * 5 * 60);
        assert_eq!(r.sittings[18], 7);
        assert_eq!(active_ranges(&r), "6 PM–10 PM");
        assert_eq!(r.suggested_quiet(), Some((23, 18)));
        let saved = rhythm::load(&rhythm_path());
        assert_eq!(saved.sittings[18], 7, "sittings save as they start");
    }

    #[test]
    fn away_hours_hold_low_nudges_and_let_urgent_ones_through() {
        let _dir = pinned("rhythm-hold");
        let mut cabin = evening_cabin();
        let low = vec![nudge("tidy the board", 0.5)];
        let urgent = vec![
            nudge("tidy the board", 0.5),
            nudge("disk almost full", 0.95),
        ];
        assert!(cabin.rhythm_holds(&low, 3), "3 AM is an away hour");
        assert!(!cabin.rhythm_holds(&urgent, 3));
        assert!(!cabin.rhythm_holds(&low, 19), "7 PM is usually active");
        cabin.rhythm_now().on = false;
        assert!(!cabin.rhythm_holds(&low, 3), "off learns and holds nothing");
    }

    #[test]
    fn held_nudges_queue_and_go_up_in_the_active_hour() {
        let _dir = pinned("rhythm-queue");
        let mut cabin = evening_cabin();
        let ranked = vec![nudge("tidy the board", 0.5)];
        let held = cabin.rhythm_holds(&ranked, 3);
        assert!(held);
        let out = cabin.proactive.surface(ranked, T0, held, false);
        assert!(out.is_empty());
        assert_eq!(cabin.proactive.queue.len(), 1);
        let later = cabin.rhythm_holds(&[], 19);
        assert!(!later);
        let out = cabin
            .proactive
            .surface(Vec::new(), T0 + 16 * 3_600_000, later, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].topic, "tidy the board");
        assert!(cabin.proactive.queue.is_empty());
    }

    #[test]
    fn the_brief_offer_answers_once_and_sets_the_gate() {
        let _dir = pinned("rhythm-brief");
        let mut cabin = Cabin::quiet_for_test();
        cabin.sources.rhythm = Some(Rhythm::default());
        for d in 0..5 {
            cabin.note_rhythm_at(&clock(T0 + d * 86_400_000 + 19 * 3_600_000));
        }
        assert_eq!(cabin.rhythm_now().brief_offer(), Some(19));
        cabin.cfg.feed_pulse.last_digest_ms = 0;
        assert_eq!(cabin.brief_gate(19, T0), None);
        cabin.answer_brief_offer(Some(19));
        assert_eq!(
            cabin.status,
            "Your Pulse brief will come around 7 PM. Settings → Behavior changes it."
        );
        assert_eq!(cabin.rhythm_now().brief_offer(), None);
        assert_eq!(cabin.brief_gate(19, T0), Some(true));
        assert_eq!(cabin.brief_gate(8, T0), Some(false));
        assert_eq!(rhythm::load(&rhythm_path()).brief_hour, Some(19));
    }
}
