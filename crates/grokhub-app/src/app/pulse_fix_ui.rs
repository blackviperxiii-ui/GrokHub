//! Pulse cards that explain themselves: the verdict line on a row, and the
//! Fix it sheet a PC health card's main button opens. The sheet's rows come
//! from `grokhub_core::pc_health_advice`; hardware rows are steps the user
//! follows, never changes the app makes.

use super::*;
use grokhub_core::pc_health_advice::{self as advice, FixKind, FixOption, SessionStat};
use grokhub_core::pulse::{PulseVerdict, VerdictTone};

/// The open Fix it sheet: its card, the session behind it, and the one row
/// whose steps (or table, or warn lines) are showing.
#[derive(Debug, Clone, Default)]
pub(super) struct FixSheet {
    pub card_id: String,
    pub stat: Option<SessionStat>,
    pub history: Vec<SessionStat>,
    pub open: Option<FixKind>,
    pub gpu_warn: u8,
    pub cpu_warn: u8,
}

/// The color a verdict reads in.
pub(super) fn tone_color(tone: VerdictTone) -> egui::Color32 {
    match tone {
        VerdictTone::Fine => crate::theme::live(),
        VerdictTone::Watch => crate::theme::amber(),
        VerdictTone::Act => crate::theme::signal_red(),
    }
}

/// A dot and the verdict line, above a card's detail.
pub(super) fn paint_verdict(ui: &mut egui::Ui, v: &PulseVerdict) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 16.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 3.5, tone_color(v.tone));
        ui.add(
            egui::Label::new(
                RichText::new(&v.line)
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::fg()),
            )
            .wrap(),
        );
    });
}

/// The main button's word on a card that opens the Fix it sheet: Fix it when
/// the verdict found something, Options when it's normal. `None` for cards
/// that keep Run.
pub(super) fn fix_label(card: &grokhub_core::UpdateCard) -> Option<&'static str> {
    if !grokhub_core::is_pc_health_card(card) {
        return None;
    }
    Some(match card.pulse.verdict.as_ref().map(|v| v.tone) {
        Some(VerdictTone::Watch | VerdictTone::Act) => "Fix it",
        _ => "Options",
    })
}

/// What a row's button says for its kind and whether it is open.
fn row_button(kind: FixKind, open: bool, quiet_on: bool) -> &'static str {
    match kind {
        FixKind::Compare => {
            if open {
                "Hide"
            } else {
                "Compare"
            }
        }
        FixKind::FpsCap
        | FixKind::Upscale
        | FixKind::Airflow
        | FixKind::FanCurve
        | FixKind::Repaste => {
            if open {
                "Hide steps"
            } else {
                "Show steps"
            }
        }
        FixKind::QuietWhenNormal => {
            if quiet_on {
                "Turn off"
            } else {
                "Turn on"
            }
        }
        FixKind::WarnLines => {
            if open {
                "Hide"
            } else {
                "Change"
            }
        }
        FixKind::Pause => "Pause",
        FixKind::Ask => "Ask",
    }
}

fn fmt_c(v: Option<u8>) -> String {
    v.map(|c| format!("{c}°C")).unwrap_or_else(|| "–".into())
}

fn fmt_pct(v: Option<u8>) -> String {
    v.map(|c| format!("{c}%")).unwrap_or_else(|| "–".into())
}

impl Cabin {
    /// Open the sheet for a card, with its session read from the ledger.
    pub(super) fn open_pulse_fix(&mut self, id: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        let history = self.pc_health_history();
        let stat = advice::card_session(&card.source_id, &history).cloned();
        let (gpu_warn, cpu_warn) = advice::warn_lines(&self.cfg.pc_health);
        self.pulse_view.fix = Some(FixSheet {
            card_id: id.to_string(),
            stat,
            history,
            open: None,
            gpu_warn,
            cpu_warn,
        });
    }

    /// The sheet's rows right now. A card whose session fell out of the ledger
    /// still gets the settings rows and Ask.
    pub(super) fn pulse_fix_options(&self) -> Vec<FixOption> {
        let Some(sheet) = self.pulse_view.fix.as_ref() else {
            return Vec::new();
        };
        let stat = sheet.stat.clone().unwrap_or_default();
        let opts = advice::fix_options(&stat, &sheet.history, &self.cfg.pc_health);
        if sheet.stat.is_some() {
            opts
        } else {
            opts.into_iter()
                .filter(|o| {
                    matches!(
                        o.kind,
                        FixKind::QuietWhenNormal
                            | FixKind::WarnLines
                            | FixKind::Pause
                            | FixKind::Ask
                    )
                })
                .collect()
        }
    }

    /// A row's button. Steps, the table and the warn lines open in place;
    /// the settings rows save at once; Ask opens the chat and closes the sheet.
    pub(super) fn pulse_fix_do(&mut self, kind: FixKind) {
        let Some(sheet) = self.pulse_view.fix.as_mut() else {
            return;
        };
        match kind {
            FixKind::Compare
            | FixKind::FpsCap
            | FixKind::Upscale
            | FixKind::Airflow
            | FixKind::FanCurve
            | FixKind::Repaste
            | FixKind::WarnLines => {
                sheet.open = if sheet.open == Some(kind) {
                    None
                } else {
                    Some(kind)
                };
            }
            FixKind::QuietWhenNormal => {
                let on = !self.cfg.pc_health.quiet_when_normal;
                self.cfg.pc_health.quiet_when_normal = on;
                self.persist_cfg();
                self.status = if on {
                    "PC health cards now post only when something's off.".into()
                } else {
                    "PC health cards post after every session again.".into()
                };
            }
            FixKind::Pause => {
                self.cfg.pc_health.paused_until_ms = now_ms() + advice::PAUSE_MS;
                self.persist_cfg();
                self.status = "PC health cards are paused for a week.".into();
                self.pulse_view.fix = None;
            }
            FixKind::Ask => {
                let id = sheet.card_id.clone();
                self.pulse_view.fix = None;
                self.pulse_run(&id);
            }
        }
    }

    /// Save the sheet's warn lines. The built-in values save as no override.
    pub(super) fn pulse_fix_save_warn(&mut self, gpu: u8, cpu: u8) {
        let pc = &mut self.cfg.pc_health;
        let gpu_default = grokhub_core::pc_health::GPU_HOT_C as u8;
        let cpu_default = grokhub_core::pc_health::CPU_HOT_C as u8;
        pc.gpu_warn_c = (gpu != gpu_default).then_some(gpu);
        pc.cpu_warn_c = (cpu != cpu_default).then_some(cpu);
        self.persist_cfg();
        self.status = format!("I'll call it warm at GPU {gpu}°C and CPU {cpu}°C.");
        if let Some(sheet) = self.pulse_view.fix.as_mut() {
            sheet.open = None;
        }
    }

    pub(super) fn paint_pulse_fix_sheet(&mut self, ctx: &egui::Context) {
        let Some(sheet) = self.pulse_view.fix.clone() else {
            return;
        };
        let Some(card) = self.pulse_card(&sheet.card_id) else {
            self.pulse_view.fix = None;
            return;
        };
        let options = self.pulse_fix_options();
        let quiet_on = self.cfg.pc_health.quiet_when_normal;
        let mut picked: Option<FixKind> = None;
        let mut close = false;
        let mut warn = (sheet.gpu_warn, sheet.cpu_warn);
        let mut save_warn = false;
        let modal = egui::Modal::new(egui::Id::new("pulse-fix-sheet"))
            .backdrop_color(egui::Color32::from_black_alpha(153))
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_width(600.0_f32.min(ctx.content_rect().width() - 80.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&card.title)
                            .font(crate::theme::title_font(22.0))
                            .color(crate::theme::fg()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let x = ui
                            .add(
                                egui::Button::new(RichText::new("×").size(18.0))
                                    .frame(false)
                                    .min_size(egui::vec2(28.0, 28.0)),
                            )
                            .on_hover_text("Close (Esc)");
                        if x.clicked() {
                            close = true;
                        }
                    });
                });
                if let Some(v) = card.pulse.verdict.as_ref() {
                    ui.add_space(4.0);
                    paint_verdict(ui, v);
                }
                let body = card.body.as_deref().unwrap_or("").trim();
                if !body.is_empty() {
                    ui.label(
                        RichText::new(body)
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::muted()),
                    );
                }
                ui.add_space(10.0);
                egui::ScrollArea::vertical()
                    .id_salt("pulse-fix-rows")
                    .max_height((ctx.content_rect().height() - 220.0).max(200.0))
                    .show(ui, |ui| {
                        for o in &options {
                            let open = sheet.open == Some(o.kind);
                            egui::Frame::NONE
                                .fill(crate::theme::raised())
                                .corner_radius(10.0)
                                .inner_margin(egui::Margin::symmetric(12, 10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_top(|ui| {
                                        let text_w = (ui.available_width() - 120.0).max(120.0);
                                        ui.vertical(|ui| {
                                            ui.set_width(text_w);
                                            ui.label(
                                                RichText::new(&o.title)
                                                    .size(crate::theme::FONT_UI)
                                                    .strong()
                                                    .color(crate::theme::fg()),
                                            );
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&o.why)
                                                        .size(crate::theme::FONT_TIP)
                                                        .color(crate::theme::muted()),
                                                )
                                                .wrap(),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Min),
                                            |ui| {
                                                let style = if o.kind == FixKind::Ask {
                                                    super::pulse_ui::Quick::Primary
                                                } else {
                                                    super::pulse_ui::Quick::Outline
                                                };
                                                if super::pulse_ui::quick_button(
                                                    ui,
                                                    row_button(o.kind, open, quiet_on),
                                                    style,
                                                ) {
                                                    picked = Some(o.kind);
                                                }
                                            },
                                        );
                                    });
                                    if open {
                                        ui.add_space(6.0);
                                        match o.kind {
                                            FixKind::Compare => paint_compare(ui, &sheet),
                                            FixKind::WarnLines => {
                                                ui.horizontal(|ui| {
                                                    ui.label("GPU");
                                                    ui.add(
                                                        egui::DragValue::new(&mut warn.0)
                                                            .range(60..=100)
                                                            .suffix("°C"),
                                                    );
                                                    ui.add_space(12.0);
                                                    ui.label("CPU");
                                                    ui.add(
                                                        egui::DragValue::new(&mut warn.1)
                                                            .range(60..=105)
                                                            .suffix("°C"),
                                                    );
                                                    ui.add_space(12.0);
                                                    if super::pulse_ui::quick_button(
                                                        ui,
                                                        "Save",
                                                        super::pulse_ui::Quick::Primary,
                                                    ) {
                                                        save_warn = true;
                                                    }
                                                });
                                            }
                                            _ => {
                                                for (i, step) in o.steps.iter().enumerate() {
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(format!(
                                                                "{}. {step}",
                                                                i + 1
                                                            ))
                                                            .size(crate::theme::FONT_BODY)
                                                            .color(crate::theme::fg()),
                                                        )
                                                        .wrap(),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                });
                            ui.add_space(6.0);
                        }
                    });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(grokhub_core::brand::filled(
                        "{product} only reads sensors. It never changes fans, clocks or drivers, and never touches a game.",
                    ))
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::subtle()),
                );
            });
        if let Some(s) = self.pulse_view.fix.as_mut() {
            s.gpu_warn = warn.0;
            s.cpu_warn = warn.1;
        }
        if save_warn {
            self.pulse_fix_save_warn(warn.0, warn.1);
        }
        if let Some(kind) = picked {
            self.pulse_fix_do(kind);
        }
        if close || modal.should_close() {
            self.pulse_view.fix = None;
        }
    }
}

/// This session first, then the last few, newest first.
fn paint_compare(ui: &mut egui::Ui, sheet: &FixSheet) {
    let mut rows: Vec<&SessionStat> = Vec::new();
    if let Some(s) = sheet.stat.as_ref() {
        rows.push(s);
    }
    rows.extend(
        sheet
            .history
            .iter()
            .rev()
            .filter(|s| sheet.stat.as_ref().is_none_or(|t| t.at_ms != s.at_ms))
            .take(5),
    );
    egui::Grid::new("pulse-fix-compare")
        .num_columns(5)
        .spacing(egui::vec2(16.0, 4.0))
        .show(ui, |ui| {
            for head in ["Session", "Time", "GPU peak", "CPU peak", "GPU busy"] {
                ui.label(
                    RichText::new(head)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::subtle()),
                );
            }
            ui.end_row();
            for (i, s) in rows.iter().enumerate() {
                let name = if i == 0 && sheet.stat.is_some() {
                    "This one".to_string()
                } else if s.label.is_empty() {
                    s.game.clone()
                } else {
                    s.label.clone()
                };
                for cell in [
                    name,
                    format!("{} min", s.mins),
                    fmt_c(s.max_gpu_c),
                    fmt_c(s.max_cpu_c),
                    fmt_pct(s.avg_gpu_pct),
                ] {
                    ui.label(
                        RichText::new(cell)
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::fg()),
                    );
                }
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health_card(cabin: &mut Cabin, at: u64, tone: VerdictTone) -> String {
        let card_id = format!("pchealth:hades:{at}");
        let mut card = grokhub_core::update_feed::suggestion_card(
            &card_id,
            "PC health after hades",
            "30 min · CPU 69°C / GPU 81°C",
            at,
        );
        card.pulse.verdict = Some(PulseVerdict {
            tone,
            line: "Warm for a light load.".into(),
        });
        let id = card.id.clone();
        cabin.updates.push(card);
        id
    }

    #[test]
    fn the_main_button_says_fix_it_only_when_the_verdict_found_something() {
        let mut cabin = Cabin::quiet_for_test();
        let warm = health_card(&mut cabin, 5, VerdictTone::Watch);
        let fine = health_card(&mut cabin, 6, VerdictTone::Fine);
        assert_eq!(fix_label(&cabin.pulse_card(&warm).unwrap()), Some("Fix it"));
        assert_eq!(
            fix_label(&cabin.pulse_card(&fine).unwrap()),
            Some("Options")
        );
        let other = grokhub_core::update_feed::suggestion_card("x:1", "Idea", "Body", 1);
        assert_eq!(fix_label(&other), None);
    }

    #[test]
    fn the_settings_rows_save_and_ask_closes_the_sheet() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("pulse-fix-rows");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        let id = health_card(&mut cabin, 5, VerdictTone::Watch);
        cabin.open_pulse_fix(&id);
        // Its session isn't in the ledger, so only the settings rows and Ask show.
        let kinds: Vec<FixKind> = cabin.pulse_fix_options().iter().map(|o| o.kind).collect();
        assert_eq!(
            kinds,
            vec![
                FixKind::QuietWhenNormal,
                FixKind::WarnLines,
                FixKind::Pause,
                FixKind::Ask
            ]
        );
        cabin.pulse_fix_do(FixKind::QuietWhenNormal);
        assert!(cabin.cfg.pc_health.quiet_when_normal);
        cabin.pulse_fix_do(FixKind::WarnLines);
        assert_eq!(
            cabin.pulse_view.fix.as_ref().unwrap().open,
            Some(FixKind::WarnLines)
        );
        cabin.pulse_fix_save_warn(78, 90);
        assert_eq!(cabin.cfg.pc_health.gpu_warn_c, Some(78));
        assert_eq!(
            cabin.cfg.pc_health.cpu_warn_c, None,
            "the default saves as no override"
        );
        cabin.pulse_fix_do(FixKind::Pause);
        assert!(cabin.cfg.pc_health.paused_until_ms > now_ms());
        assert!(cabin.pulse_view.fix.is_none());
    }

    #[test]
    fn steps_open_one_row_at_a_time() {
        let mut cabin = Cabin::quiet_for_test();
        let id = health_card(&mut cabin, 5, VerdictTone::Watch);
        cabin.open_pulse_fix(&id);
        let sheet = cabin.pulse_view.fix.as_mut().unwrap();
        sheet.stat = Some(SessionStat {
            at_ms: 5,
            game: "hades".into(),
            max_gpu_c: Some(81),
            avg_gpu_pct: Some(15),
            ..Default::default()
        });
        cabin.pulse_fix_do(FixKind::FpsCap);
        cabin.pulse_fix_do(FixKind::Airflow);
        assert_eq!(
            cabin.pulse_view.fix.as_ref().unwrap().open,
            Some(FixKind::Airflow)
        );
        cabin.pulse_fix_do(FixKind::Airflow);
        assert_eq!(cabin.pulse_view.fix.as_ref().unwrap().open, None);
        let kinds: Vec<FixKind> = cabin.pulse_fix_options().iter().map(|o| o.kind).collect();
        assert!(kinds.contains(&FixKind::FpsCap) && kinds.contains(&FixKind::FanCurve));
    }

    #[test]
    fn the_sheet_paints_its_rows_and_the_hardware_note() {
        let mut cabin = Cabin::quiet_for_test();
        let id = health_card(&mut cabin, 5, VerdictTone::Watch);
        cabin.open_pulse_fix(&id);
        let _paint = crate::theme::hold_paint_test();
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 1000.0),
            )),
            ..Default::default()
        };
        let mut texts = Vec::new();
        for _ in 0..2 {
            texts.clear();
            let out = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_pulse(ui));
            for clipped in &out.shapes {
                collect_text(&clipped.shape, &mut texts);
            }
        }
        let all = texts.join("\n");
        for want in [
            "Warm for a light load.",
            "Only post when something's off",
            "Pause these cards for a week",
            "never changes fans, clocks or drivers",
        ] {
            assert!(all.contains(want), "missing {want:?} in {all}");
        }
    }

    fn collect_text(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| collect_text(s, out)),
            _ => {}
        }
    }
}
