//! Settings → Labs → Diagnostics mode: one-click Desktop debug bundle.

use super::*;
use grokhub_core::{
    content_level_label, diagnostics_markdown, diagnostics_preview_lines,
    diagnostics_redaction_summary, diagnostics_saved_status, diagnostics_zip_path,
    redact_for_diagnostics, redact_turn, turn_says, why_chat_vs_task, write_diagnostics_zip,
    DiagnosticsClassifier, DiagnosticsExport, DiagnosticsPlaybook, DiagnosticsPolicy,
    DiagnosticsToolCall, DiagnosticsTurn,
};
use std::time::SystemTime;

/// Labs Diagnostics state on the cabin.
#[derive(Default)]
pub(super) struct DiagnosticsState {
    /// Last zip written (Open folder).
    pub(super) last_zip: Option<std::path::PathBuf>,
    /// Built and redacted, shown for Save or Cancel; nothing is written yet.
    pub(super) pending: Option<PendingDiagnostics>,
}

/// One bundle as the preview shows it. Save writes exactly this export.
pub(super) struct PendingDiagnostics {
    pub(super) export: DiagnosticsExport,
    pub(super) markdown: String,
    pub(super) summary: String,
}

impl Cabin {
    /// Settings / `/doctor` status line — lives with Labs Diagnostics doctor rows.
    pub(super) fn doctor_text(&self) -> String {
        self.doctor_checks()
            .into_iter()
            .map(|l| format!("{} {}", if l.ok { "ok" } else { "ERR" }, l.text))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub(super) fn doctor_checks(&self) -> Vec<grokhub_core::DoctorLine> {
        let mut lines = grokhub_core::doctor_lines(self.llm_ready(), true, HUB_KIND);
        self.mark_stale_auth(&mut lines);
        lines.extend(grokhub_core::doctor_extras(
            self.last_receipt_ok,
            self.skill_list.len(),
        ));
        lines
    }

    pub(super) fn set_diagnostics_mode(&mut self, on: bool) {
        if self.cfg.diagnostics_mode == on {
            return;
        }
        self.cfg.diagnostics_mode = on;
        if !on {
            self.diagnostics.last_zip = None;
        }
        self.persist_cfg();
        self.status = "Saved".into();
    }

    /// The "Include recent app logs" box: kept in `app.json` once ticked.
    pub(super) fn set_diagnostics_include_logs(&mut self, on: bool) {
        self.cfg.diagnostics_include_logs = on;
        self.persist_cfg();
    }

    /// Settings → Labs: Diagnostics mode toggle, preview, and Open folder.
    pub(super) fn ui_labs_diagnostics(&mut self, ui: &mut egui::Ui) {
        crate::ui_kit::section_header(ui, "Diagnostics", None);
        let mut on = self.cfg.diagnostics_mode;
        if crate::cards::settings_toggle(
            ui,
            "Diagnostics mode",
            "Off by default. When on, chat and message actions can Save a redacted debug bundle to your Desktop (JSON + Markdown in one zip). Nothing leaves this machine unless you copy or share the file.",
            &mut on,
        ) {
            self.set_diagnostics_mode(on);
        }
        if self.cfg.diagnostics_mode {
            crate::cards::settings_note(ui, "Preview — each Save includes:");
            for line in diagnostics_preview_lines(self.cfg.diagnostics_include_logs) {
                crate::cards::settings_note(ui, &format!("• {line}"));
            }
            let mut logs = self.cfg.diagnostics_include_logs;
            if ui.checkbox(&mut logs, "Include recent app logs").changed() {
                self.set_diagnostics_include_logs(logs);
            }
            if crate::cards::settings_action(
                ui,
                "Save diagnostics for this chat",
                "Shows the redacted bundle first; Save there writes loadout-diagnostics-DATE-TIME.zip to your Desktop, or to a local folder when the Desktop syncs to the cloud.",
                "Save",
            ) {
                self.save_chat_diagnostics(None);
            }
            if self.diagnostics.last_zip.is_some() && crate::cards::ghost_pill(ui, "Open folder") {
                self.open_diagnostics_folder();
            }
        }
    }

    /// Open the Desktop folder that holds the last diagnostics zip.
    pub(super) fn open_diagnostics_folder(&mut self) {
        let Some(path) = self.diagnostics.last_zip.clone() else {
            self.status = "No diagnostics file yet".into();
            return;
        };
        self.open_folder_of(&path);
    }

    /// Open the folder that holds `path` (the bell's Open folder).
    pub(super) fn open_folder_of(&mut self, path: &std::path::Path) {
        let dir = path.parent().unwrap_or(path);
        if let Err(e) = crate::desktop::open_path(&dir.display().to_string()) {
            self.status = format!("Couldn't open folder: {e}");
        }
    }

    /// Whole chat (`None`) or one turn (`Some(turn)`): builds the redacted
    /// bundle and opens the preview. Nothing is written until Save there.
    pub(super) fn save_chat_diagnostics(&mut self, only_turn: Option<u32>) {
        if !self.cfg.diagnostics_mode {
            self.status = "Turn on Diagnostics mode in Settings → Labs first".into();
            return;
        }
        let Some(thread) = self.threads.get(self.thread_idx).cloned() else {
            self.status = "Nothing to export".into();
            return;
        };
        if thread.messages.is_empty() && self.messages.is_empty() {
            self.status = "Nothing to export".into();
            return;
        }
        let version = crate::update::build_version_line();
        let version = grokhub_core::strip_product_prefix(&version).to_string();
        let messages: Vec<(String, String)> = if self.thread_idx < self.threads.len()
            && self.threads[self.thread_idx].id == thread.id
            && !self.messages.is_empty()
        {
            self.messages.as_ref().clone()
        } else {
            thread.messages.as_ref().clone()
        };
        // The exported chat's own personality, not the running job's.
        let active = self.thread_personality(self.thread_idx);
        let personality = active.id.clone();
        let content_level = content_level_label(self.cfg.explicit_personalities, active.explicit);
        let model = if self.cfg.model.trim().is_empty() {
            "default".into()
        } else {
            self.cfg.model.clone()
        };
        let provider: String = if self.cfg.provider_model.trim().is_empty() {
            "xai".into()
        } else {
            "provider".into()
        };
        let config_dir = config::config_dir();
        let home = grokhub_core::user_home().and_then(|p| p.into_os_string().into_string().ok());
        let home_ref = home.as_deref();

        let mut export = DiagnosticsExport::with_defaults(&version, &thread.id, &thread.title);
        export.service_status = self.diagnostics_service_status();
        export.preview = diagnostics_preview_lines(self.cfg.diagnostics_include_logs);
        if self.cfg.diagnostics_include_logs {
            export.recent_logs = self.diagnostics_recent_logs(home_ref);
        }

        // Each store is read once; turns look their records up by raw turn.
        let all_spans =
            grokhub_agent::harness::read_spans(&config_dir, &thread.id).unwrap_or_default();
        let briefs: Vec<_> = grokhub_agent::brain::read_briefs(&config_dir)
            .into_iter()
            .filter(|l| l.chat_id == thread.id)
            .collect();
        let runs: std::collections::HashMap<String, grokhub_core::run_metrics::RunMetrics> =
            grokhub_core::run_metrics::read_runs(&config_dir)
                .into_iter()
                .map(|m| (m.run.clone(), m))
                .collect();

        for group in build_turns(&messages) {
            let turn_no = group.turn;
            if only_turn.is_some_and(|t| t != turn_no) {
                continue;
            }
            let raw = group.raw_first..=group.raw_last;
            let (user_text, assistant_raw) = (group.user, group.assistant);
            let brief = briefs.iter().rev().find(|l| raw.contains(&l.turn));
            let spans: Vec<_> = all_spans.iter().filter(|s| raw.contains(&s.turn)).collect();
            let metrics = raw
                .clone()
                .find_map(|t| runs.get(&format!("{}:{t}", thread.id)));

            let mut turn = DiagnosticsTurn {
                turn: turn_no,
                user_text: user_text.clone(),
                final_reply: turn_says(&assistant_raw).unwrap_or_else(|| assistant_raw.clone()),
                raw_saved_turn: assistant_raw.clone(),
                classifier: DiagnosticsClassifier::default(),
                playbook: DiagnosticsPlaybook::default(),
                personality: personality.clone(),
                content_level: content_level.clone(),
                brain_policy: None,
                length_cap: String::new(),
                model: metrics
                    .as_ref()
                    .map(|m| m.model.clone())
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| model.clone()),
                provider: provider.clone(),
                tool_calls: Vec::new(),
                approvals_asked: Vec::new(),
                approvals_granted: Vec::new(),
                errors: Vec::new(),
                wall_ms: metrics.as_ref().map(|m| m.wall_ms).unwrap_or(0),
                started_ms: 0,
                finished_ms: metrics.as_ref().map(|m| m.finished_at).unwrap_or(0),
            };

            if let Some(line) = &brief {
                turn.classifier = DiagnosticsClassifier {
                    intent: line.brief.intent.as_str().into(),
                    care: line.brief.care.as_str().into(),
                    domain: line.brief.domain.clone(),
                    why_chat_vs_task: why_chat_vs_task(line.brief.intent.as_str(), &line.brief.by),
                    by: line.brief.by.clone(),
                };
                if let Some(p) = &line.policy {
                    turn.brain_policy = Some(DiagnosticsPolicy {
                        row: p.row.clone(),
                        goal: p.goal.clone(),
                        reason: p.reason.clone(),
                        talk: p.talk.clone(),
                    });
                    turn.length_cap = p.style().label();
                    turn.playbook.why = format!("policy {} → playbook pick", p.row);
                }
                if !line.playbook.is_empty() || !line.playbook_id.is_empty() {
                    turn.playbook.name = line.playbook.clone();
                    turn.playbook.id = line.playbook_id.clone();
                    if turn.playbook.why.is_empty() {
                        turn.playbook.why = format!(
                            "matched {} × {}",
                            line.brief.intent.as_str(),
                            line.brief.care.as_str()
                        );
                    }
                }
            } else if let Some(m) = &metrics {
                if !m.brain.is_empty() {
                    turn.classifier = DiagnosticsClassifier {
                        intent: m.brain.intent.clone(),
                        care: m.brain.care.clone(),
                        domain: m.brain.domain.clone(),
                        why_chat_vs_task: why_chat_vs_task(&m.brain.intent, "metrics"),
                        by: "metrics".into(),
                    };
                    if !m.brain.policy.is_empty() {
                        turn.brain_policy = Some(DiagnosticsPolicy {
                            row: m.brain.policy.clone(),
                            goal: String::new(),
                            reason: String::new(),
                            talk: String::new(),
                        });
                    }
                    if !m.brain.playbook.is_empty() {
                        turn.playbook.id = m.brain.playbook.clone();
                        turn.playbook.name = m.brain.playbook.clone();
                    }
                }
            }

            let mut started = u64::MAX;
            let mut finished = 0u64;
            for span in spans {
                started = started.min(span.ts_ms);
                finished = finished.max(span.ts_ms);
                let summary = redact_for_diagnostics(&clip_chars(&span.result, 160), home_ref);
                turn.tool_calls.push(DiagnosticsToolCall {
                    tool: span.tool.clone(),
                    decision: span.decision.clone(),
                    approval_class: span.approval_class.clone(),
                    result_summary: summary,
                    hard_approved: span.hard_approved,
                });
                if span.decision == "park" || span.approval_class == "ask" {
                    turn.approvals_asked
                        .push(format!("{} ({})", span.tool, span.approval_class));
                }
                if span.hard_approved || span.decision == "approve" {
                    turn.approvals_granted
                        .push(format!("{} ({})", span.tool, span.decision));
                }
                if span.result == "error"
                    || span.result.starts_with("error")
                    || span.claim.to_ascii_lowercase().contains("error")
                {
                    turn.errors.push(redact_for_diagnostics(
                        &clip_chars(&span.claim, 200),
                        home_ref,
                    ));
                }
            }
            if started != u64::MAX {
                turn.started_ms = started;
                if turn.finished_ms == 0 {
                    turn.finished_ms = finished;
                }
                if turn.wall_ms == 0 && finished >= started {
                    turn.wall_ms = finished.saturating_sub(started);
                }
            }
            if assistant_raw.trim_start().starts_with("Error:") {
                turn.errors.push(redact_for_diagnostics(
                    &clip_chars(&assistant_raw, 240),
                    home_ref,
                ));
            }

            export.turns.push(redact_turn(turn, home_ref));
        }

        if export.turns.is_empty() {
            self.status = "No turns to export".into();
            return;
        }

        export.chat_title = redact_for_diagnostics(&export.chat_title, home_ref);
        export.recent_logs = redact_for_diagnostics(&export.recent_logs, home_ref);
        export.service_status = export
            .service_status
            .into_iter()
            .map(|s| redact_for_diagnostics(&s, home_ref))
            .collect();

        let markdown = diagnostics_markdown(&export);
        let summary = diagnostics_redaction_summary(&markdown);
        self.diagnostics.pending = Some(PendingDiagnostics {
            export,
            markdown,
            summary,
        });
    }

    /// Save in the preview: write exactly the bundle that was shown.
    pub(super) fn confirm_diagnostics_save(&mut self) {
        let Some(pending) = self.diagnostics.pending.take() else {
            return;
        };
        let path = match diagnostics_zip_path(SystemTime::now()) {
            Ok(p) => p,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        match write_diagnostics_zip(&path, &pending.export) {
            Ok(path) => {
                let on_desktop = path.parent() == grokhub_core::desktop_dir().as_deref();
                self.status = diagnostics_saved_status(&path, on_desktop);
                self.notify(
                    super::notices::NoticeKind::Success,
                    self.status.clone(),
                    Some(super::notices::NoticeAction::OpenFolder(path.clone())),
                );
                self.diagnostics.last_zip = Some(path);
            }
            Err(e) => self.status = format!("Couldn't save diagnostics: {e}"),
        }
    }

    /// Cancel in the preview: nothing is written.
    pub(super) fn cancel_diagnostics_save(&mut self) {
        if self.diagnostics.pending.take().is_some() {
            self.status = "Diagnostics not saved".into();
        }
    }

    /// The redacted bundle in a dialog with Save and Cancel. Escape or a
    /// click outside counts as Cancel.
    pub(super) fn ui_diagnostics_preview(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.diagnostics.pending.as_ref() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let width = (ctx.content_rect().width() - 64.0).clamp(280.0, 760.0);
        let modal = egui::Modal::new(egui::Id::new("diagnostics-preview"))
            .backdrop_color(egui::Color32::from_black_alpha(153))
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.label(
                    RichText::new("Diagnostics preview")
                        .font(crate::theme::title_font(22.0))
                        .color(crate::theme::fg()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&pending.summary)
                        .size(crate::theme::FONT_META)
                        .color(crate::theme::muted()),
                );
                ui.add_space(12.0);
                egui::ScrollArea::vertical()
                    .id_salt("diagnostics-preview-body")
                    .max_height((ctx.content_rect().height() * 0.6).max(160.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&pending.markdown)
                                    .monospace()
                                    .size(crate::theme::FONT_META),
                            )
                            .wrap(),
                        );
                    });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Save writes exactly this. Cancel writes nothing.")
                            .size(crate::theme::FONT_META)
                            .color(crate::theme::muted()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::cards::white_pill(ui, "Save") {
                            save = true;
                        }
                        if crate::cards::ghost_pill(ui, "Cancel") {
                            cancel = true;
                        }
                    });
                });
            });
        if save {
            self.confirm_diagnostics_save();
        } else if cancel || modal.should_close() {
            self.cancel_diagnostics_save();
        }
    }

    /// A sign-in or key on file that can't carry a turn reads as stale, not
    /// "present", so the status agrees with what the send does.
    fn mark_stale_auth(&self, lines: &mut [grokhub_core::DoctorLine]) {
        if self.has_key() && !self.cloud_cred_usable() {
            if let Some(first) = lines.first_mut() {
                *first = grokhub_core::xai_auth_stale_line(self.chat_ready());
            }
        }
    }

    pub(super) fn diagnostics_service_status(&self) -> Vec<String> {
        let mut doctor = grokhub_core::doctor_lines(
            self.has_key(),
            config::config_dir().join("memory").is_dir(),
            HUB_KIND,
        );
        self.mark_stale_auth(&mut doctor);
        let mut lines: Vec<String> = doctor
            .into_iter()
            .map(|l| {
                if l.ok {
                    format!("ok — {}", l.text)
                } else {
                    format!("fail — {}", l.text)
                }
            })
            .collect();
        for l in grokhub_core::doctor_extras(self.last_receipt_ok, self.skill_list.len()) {
            lines.push(if l.ok {
                format!("ok — {}", l.text)
            } else {
                format!("fail — {}", l.text)
            });
        }
        let cabin = grokhub_core::doctor_cabin_line(self.running);
        lines.push(if cabin.ok {
            format!("ok — {}", cabin.text)
        } else {
            format!("idle — {}", cabin.text)
        });
        lines.push(format!(
            "channel — {}",
            crate::update::installed_channel().as_str()
        ));
        lines
    }

    fn diagnostics_recent_logs(&self, home: Option<&str>) -> String {
        let dir = config::config_dir();
        let candidates = [
            dir.join("logs").join("cabin.log"),
            dir.join("cabin.log"),
            dir.join("hub.log"),
        ];
        for path in candidates {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let tail = tail_chars(&text, 4_000);
                return redact_for_diagnostics(&tail, home);
            }
        }
        String::new()
    }
}

/// One ask as the chat shows it.
#[derive(Debug, PartialEq, Eq)]
struct TurnGroup {
    /// 1-based among visible user rows (what a message's Save passes).
    turn: u32,
    /// The raw `user` message numbers this ask's run used. Traces, Briefs and
    /// run metrics count every raw `user` row, hidden workload markers too.
    raw_first: u32,
    raw_last: u32,
    user: String,
    /// Every assistant body until the next ask.
    assistant: String,
}

/// Visible asks with their replies. A hidden workload row (`HOST_RESULT`,
/// `SKILL_SAVED`, …) belongs to the ask before it.
fn build_turns(messages: &[(String, String)]) -> Vec<TurnGroup> {
    let mut out: Vec<TurnGroup> = Vec::new();
    let mut raw = 0u32;
    for (role, body) in messages {
        match role.as_str() {
            "user" => {
                raw += 1;
                if !grokhub_core::is_workload_user(body) {
                    out.push(TurnGroup {
                        turn: out.len() as u32 + 1,
                        raw_first: raw,
                        raw_last: raw,
                        user: body.clone(),
                        assistant: String::new(),
                    });
                } else if let Some(last) = out.last_mut() {
                    last.raw_last = raw;
                }
            }
            "assistant" => {
                if let Some(last) = out.last_mut() {
                    if !last.assistant.is_empty() {
                        last.assistant.push_str("\n\n");
                    }
                    last.assistant.push_str(body);
                }
            }
            _ => {}
        }
    }
    out
}

fn clip_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn tail_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        s.chars().skip(n - max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_number_visible_asks_and_keep_raw_trace_numbers() {
        let msgs: Vec<(String, String)> = [
            ("user", "ask1"),
            ("assistant", "reply1"),
            ("user", "SKILL_SAVED"),
            ("user", "ask2"),
            ("assistant", "TOOL_CALL"),
            ("user", "HOST_RESULT (facts only):\nok"),
            ("assistant", "reply2"),
        ]
        .iter()
        .map(|(r, b)| (r.to_string(), b.to_string()))
        .collect();
        assert_eq!(
            build_turns(&msgs),
            vec![
                TurnGroup {
                    turn: 1,
                    raw_first: 1,
                    raw_last: 2,
                    user: "ask1".into(),
                    assistant: "reply1".into(),
                },
                TurnGroup {
                    turn: 2,
                    raw_first: 3,
                    raw_last: 4,
                    user: "ask2".into(),
                    assistant: "TOOL_CALL\n\nreply2".into(),
                },
            ]
        );
    }

    #[test]
    fn diagnostics_mode_off_by_default_and_setter_persists() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("diag-mode-set");
        let _ = std::fs::remove_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.cfg.diagnostics_mode);
        cabin.set_diagnostics_mode(true);
        assert!(cabin.cfg.diagnostics_mode);
        assert_eq!(cabin.status, "Saved");
        cabin.persist_svc.flush();
        crate::config::save(&cabin.cfg).unwrap();
        assert!(crate::config::load().diagnostics_mode);
        cabin.set_diagnostics_mode(false);
        assert!(!cabin.cfg.diagnostics_mode);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_preview_writes_nothing_until_save_and_save_writes_what_it_showed() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("diag-preview");
        let _ = std::fs::remove_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());

        let home = std::env::temp_dir().join(format!("gh-diag-preview-{}", std::process::id()));
        let desk = home.join("Desktop");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&desk).unwrap();
        let old_home = std::env::var_os("HOME");
        let old_up = std::env::var_os("USERPROFILE");
        unsafe { std::env::set_var("HOME", &home) };
        unsafe { std::env::set_var("USERPROFILE", &home) };

        let mut cabin = Cabin::quiet_for_test();
        let mut thread = crate::threads::ChatThread::new("Hangout", false);
        thread.messages = std::sync::Arc::new(vec![
            ("user".into(), "my key is sk-abcdefghijklmnopqrstuv".into()),
            ("assistant".into(), "TURN_SAY:\nHid it.\n".into()),
        ]);
        cabin.messages = thread.messages.clone();
        cabin.threads = vec![thread];
        cabin.thread_idx = 0;
        cabin.set_diagnostics_mode(true);
        let zips = || std::fs::read_dir(&desk).unwrap().count();

        cabin.save_chat_diagnostics(None);
        let pending = cabin.diagnostics.pending.as_ref().expect("preview is open");
        assert_eq!(
            pending.summary,
            "Redacted: 1 key, 0 emails, 0 phones, 0 home paths"
        );
        assert!(
            pending.markdown.contains("my key is [redacted]"),
            "{}",
            pending.markdown
        );
        assert_eq!(zips(), 0, "nothing is written while the preview is open");
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let mut clock = 0.0;
        // One frame of the dialog with these input events; returns its texts.
        let mut frame = |cabin: &mut Cabin, events: Vec<egui::Event>| {
            clock += 0.1;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 720.0),
                )),
                time: Some(clock),
                events,
                ..Default::default()
            };
            let out =
                crate::theme::test_pass(&ctx, raw, |ui| cabin.ui_diagnostics_preview(ui.ctx()));
            let mut texts = Vec::new();
            out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
            texts
        };
        let click_at = |at: egui::Pos2| {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            vec![egui::Event::PointerMoved(at), button(true), button(false)]
        };
        let center = |texts: &[(String, egui::Rect)], name: &str| {
            texts
                .iter()
                .find(|(t, _)| t == name)
                .map(|(_, r)| r.center())
                .unwrap_or_else(|| panic!("{name} missing: {texts:?}"))
        };

        frame(&mut cabin, vec![]);
        let texts = frame(&mut cabin, vec![]);
        for want in [
            "Diagnostics preview",
            "Redacted: 1 key, 0 emails, 0 phones, 0 home paths",
            "Save",
            "Cancel",
        ] {
            center(&texts, want);
        }
        assert!(
            texts
                .iter()
                .any(|(t, _)| t.contains("my key is [redacted]")),
            "the redacted bundle is shown: {texts:?}"
        );
        assert!(
            cabin.diagnostics.pending.is_some(),
            "painting does not close it"
        );

        // Cancel, clicked: nothing written.
        frame(&mut cabin, click_at(center(&texts, "Cancel")));
        assert!(cabin.diagnostics.pending.is_none());
        assert_eq!(cabin.status, "Diagnostics not saved");
        assert_eq!(zips(), 0, "Cancel writes no file");
        assert!(cabin.diagnostics.last_zip.is_none());

        // Escape counts as Cancel.
        cabin.save_chat_diagnostics(None);
        frame(&mut cabin, vec![]);
        cabin.status.clear();
        frame(
            &mut cabin,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(cabin.diagnostics.pending.is_none(), "Escape closes it");
        assert_eq!(cabin.status, "Diagnostics not saved");
        assert_eq!(zips(), 0, "Escape writes no file");

        // A click outside counts as Cancel too.
        cabin.save_chat_diagnostics(None);
        frame(&mut cabin, vec![]);
        frame(&mut cabin, click_at(egui::pos2(4.0, 4.0)));
        assert!(
            cabin.diagnostics.pending.is_none(),
            "a click outside closes it"
        );
        assert_eq!(zips(), 0, "a click outside writes no file");

        // Save, clicked: writes exactly what was shown.
        cabin.save_chat_diagnostics(None);
        let shown = cabin.diagnostics.pending.as_ref().unwrap().markdown.clone();
        frame(&mut cabin, vec![]);
        let texts = frame(&mut cabin, vec![]);
        frame(&mut cabin, click_at(center(&texts, "Save")));
        assert!(cabin.diagnostics.pending.is_none());
        let path = cabin
            .diagnostics
            .last_zip
            .clone()
            .expect("Save wrote a zip");
        assert_eq!(zips(), 1);
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut md = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("diagnostics.md").unwrap(), &mut md)
            .unwrap();
        assert_eq!(md, shown, "the zip holds exactly what the preview showed");

        match old_home {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        match old_up {
            Some(v) => unsafe { std::env::set_var("USERPROFILE", v) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn save_writes_zip_when_mode_on() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("diag-save-zip");
        let _ = std::fs::remove_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());

        let home = std::env::temp_dir().join(format!("gh-diag-home-{}", std::process::id()));
        let desk = home.join("Desktop");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&desk).unwrap();
        let old_home = std::env::var_os("HOME");
        let old_up = std::env::var_os("USERPROFILE");
        unsafe { std::env::set_var("HOME", &home) };
        unsafe { std::env::set_var("USERPROFILE", &home) };

        let mut cabin = Cabin::quiet_for_test();
        let mut thread = crate::threads::ChatThread::new("Hangout", false);
        thread.personality = "pirate".into();
        thread.messages = std::sync::Arc::new(vec![
            ("user".into(), "hi there".into()),
            ("assistant".into(), "TURN_SAY:\nHey — what's up?\n".into()),
            ("user".into(), "SKILL_SAVED".into()),
            ("user".into(), "fix blender".into()),
            ("assistant".into(), "TURN_SAY:\nOn it.\n".into()),
        ]);
        // Another chat is running with its own personality.
        let mut running = crate::threads::ChatThread::new("Busy", false);
        running.personality = "butler".into();
        cabin.chat_job_thread = Some(running.id.clone());
        cabin.messages = thread.messages.clone();
        cabin.threads = vec![thread, running];
        cabin.thread_idx = 0;
        cabin.set_diagnostics_mode(true);
        cabin.save_chat_diagnostics(Some(2));
        cabin.confirm_diagnostics_save();
        let path = cabin.diagnostics.last_zip.clone().expect("zip path");
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(cabin.status, format!("Saved {name} to your Desktop"));
        assert_eq!(
            cabin.notices.items[0].action,
            Some(super::super::notices::NoticeAction::OpenFolder(
                path.clone()
            ))
        );
        assert_eq!(cabin.notices.items[0].text, cabin.status);
        assert!(path.starts_with(&desk), "{path:?}");
        assert!(
            name.starts_with("loadout-diagnostics-") && name.ends_with(".zip"),
            "{name}"
        );
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut json = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("diagnostics.json").unwrap(), &mut json)
            .unwrap();
        let export: DiagnosticsExport = serde_json::from_str(&json).unwrap();
        assert_eq!(export.turns.len(), 1);
        assert_eq!(export.turns[0].turn, 2);
        assert_eq!(export.turns[0].user_text, "fix blender");
        assert_eq!(export.turns[0].final_reply, "On it.");
        assert_eq!(export.turns[0].personality, "pirate");

        // A second Save never replaces the first bundle.
        let at = SystemTime::now();
        let same = grokhub_core::diagnostics_zip_path(at).unwrap();
        std::fs::write(&same, b"earlier").unwrap();
        cabin.save_chat_diagnostics(Some(2));
        cabin.confirm_diagnostics_save();
        let second = cabin.diagnostics.last_zip.clone().unwrap();
        assert_ne!(second, same);
        assert_eq!(std::fs::read(&same).unwrap(), b"earlier");
        assert!(path.is_file() && second.is_file());

        match old_home {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        match old_up {
            Some(v) => unsafe { std::env::set_var("USERPROFILE", v) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn app_logs_go_in_only_when_the_box_is_ticked() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("diag-logs-opt-in");
        let _ = std::fs::remove_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let logs = config::config_dir().join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("cabin.log"),
            "boot ok\nprovider key sk-abcdefghijklmnopqrstuv\n",
        )
        .unwrap();

        let home = std::env::temp_dir().join(format!("gh-diag-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Desktop")).unwrap();
        let old_home = std::env::var_os("HOME");
        let old_up = std::env::var_os("USERPROFILE");
        unsafe { std::env::set_var("HOME", &home) };
        unsafe { std::env::set_var("USERPROFILE", &home) };

        let mut cabin = Cabin::quiet_for_test();
        let mut thread = crate::threads::ChatThread::new("Hangout", false);
        thread.messages = std::sync::Arc::new(vec![
            ("user".into(), "hi there".into()),
            ("assistant".into(), "TURN_SAY:\nHey.\n".into()),
        ]);
        cabin.messages = thread.messages.clone();
        cabin.threads = vec![thread];
        cabin.thread_idx = 0;
        cabin.set_diagnostics_mode(true);
        assert!(
            !cabin.cfg.diagnostics_include_logs,
            "logs are off by default"
        );

        let read = |path: &std::path::Path| {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
            let mut json = String::new();
            std::io::Read::read_to_string(&mut zip.by_name("diagnostics.json").unwrap(), &mut json)
                .unwrap();
            let mut md = String::new();
            std::io::Read::read_to_string(&mut zip.by_name("diagnostics.md").unwrap(), &mut md)
                .unwrap();
            (json, md)
        };

        cabin.save_chat_diagnostics(None);
        cabin.confirm_diagnostics_save();
        let off = cabin
            .diagnostics
            .last_zip
            .clone()
            .expect("zip with logs off");
        let (json, md) = read(&off);
        assert!(!json.contains("recent_logs"), "{json}");
        assert!(!json.contains("boot ok"), "{json}");
        assert!(!md.contains("## Recent app logs"), "{md}");
        assert!(
            json.contains("No app logs (tick Include recent app logs to add them)"),
            "{json}"
        );

        cabin.set_diagnostics_include_logs(true);
        cabin.persist_svc.flush();
        assert!(
            crate::config::load().diagnostics_include_logs,
            "the box is saved"
        );
        cabin.save_chat_diagnostics(None);
        cabin.confirm_diagnostics_save();
        let on = cabin
            .diagnostics
            .last_zip
            .clone()
            .expect("zip with logs on");
        assert_ne!(on, off);
        let (json, md) = read(&on);
        let export: DiagnosticsExport = serde_json::from_str(&json).unwrap();
        assert_eq!(
            export.recent_logs, "boot ok\nprovider key [redacted]\n",
            "logs are attached and redacted"
        );
        assert!(md.contains("## Recent app logs"), "{md}");
        assert!(
            export
                .preview
                .contains(&"Recent app logs (last 4,000 characters, redacted)".to_string()),
            "{:?}",
            export.preview
        );

        match old_home {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        match old_up {
            Some(v) => unsafe { std::env::set_var("USERPROFILE", v) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&root);
    }
}
