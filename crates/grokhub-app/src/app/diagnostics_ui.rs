//! Settings → Labs → Diagnostics mode: one-click Desktop debug bundle.

use super::*;
use grokhub_core::{
    content_level_label, diagnostics_saved_status, diagnostics_zip_path, redact_for_diagnostics,
    redact_turn, turn_says, why_chat_vs_task, write_diagnostics_zip, DiagnosticsClassifier,
    DiagnosticsExport, DiagnosticsPlaybook, DiagnosticsPolicy, DiagnosticsToolCall,
    DiagnosticsTurn, DIAGNOSTICS_PREVIEW_LINES,
};
use std::time::SystemTime;

impl Cabin {
    pub(super) fn set_diagnostics_mode(&mut self, on: bool) {
        if self.cfg.diagnostics_mode == on {
            return;
        }
        self.cfg.diagnostics_mode = on;
        if !on {
            self.diagnostics_last_zip = None;
        }
        self.persist_cfg();
        self.status = "Saved".into();
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
            crate::cards::settings_note(
                ui,
                "Preview — each Save includes:",
            );
            for line in DIAGNOSTICS_PREVIEW_LINES {
                crate::cards::settings_note(ui, &format!("• {line}"));
            }
            if crate::cards::settings_action(
                ui,
                "Save diagnostics for this chat",
                "Writes loadout-diagnostics-DATE-TIME.zip to your Desktop.",
                "Save",
            ) {
                self.save_chat_diagnostics(None);
            }
            if self.diagnostics_last_zip.is_some()
                && crate::cards::ghost_pill(ui, "Open folder")
            {
                self.open_diagnostics_folder();
            }
        }
    }

    /// Open the Desktop folder that holds the last diagnostics zip.
    pub(super) fn open_diagnostics_folder(&mut self) {
        let Some(path) = self.diagnostics_last_zip.clone() else {
            self.status = "No diagnostics file yet".into();
            return;
        };
        let dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(path);
        if let Err(e) = crate::desktop::open_path(&dir.display().to_string()) {
            self.status = format!("Couldn't open folder: {e}");
        }
    }

    /// Whole chat (`None`) or one turn (`Some(turn)`). Writes a Desktop zip.
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
        let version = version
            .strip_prefix("GrokHub ")
            .unwrap_or(&version)
            .to_string();
        let messages: Vec<(String, String)> = if self.thread_idx < self.threads.len()
            && self.threads[self.thread_idx].id == thread.id
            && !self.messages.is_empty()
        {
            self.messages.as_ref().clone()
        } else {
            thread.messages.as_ref().clone()
        };
        let personality = self.effective_personality_id_for(&thread);
        let personality_explicit = self
            .picker
            .presets
            .iter()
            .find(|p| p.id == personality)
            .is_some_and(|p| p.explicit);
        let content_level =
            content_level_label(self.cfg.explicit_personalities, personality_explicit);
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
        let home = grokhub_core::user_home()
            .and_then(|p| p.into_os_string().into_string().ok());
        let home_ref = home.as_deref();

        let mut export =
            DiagnosticsExport::with_defaults(&version, &thread.id, &thread.title);
        export.service_status = self.diagnostics_service_status();
        export.recent_logs = self.diagnostics_recent_logs(home_ref);

        let turns = build_turn_pairs(&messages);
        for (turn_no, user_text, assistant_raw) in turns {
            if only_turn.is_some_and(|t| t != turn_no) {
                continue;
            }
            let brief = grokhub_agent::brain::brief_for_turn(&config_dir, &thread.id, turn_no);
            let spans = grokhub_agent::harness::read_spans(&config_dir, &thread.id)
                .unwrap_or_default()
                .into_iter()
                .filter(|s| s.turn == turn_no)
                .collect::<Vec<_>>();
            let run_key = format!("{}:{turn_no}", thread.id);
            let metrics = grokhub_core::run_metrics::find_run(&config_dir, &run_key);

            let mut turn = DiagnosticsTurn {
                turn: turn_no,
                user_text: user_text.clone(),
                final_reply: turn_says(&assistant_raw)
                    .unwrap_or_else(|| assistant_raw.clone()),
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
                    why_chat_vs_task: why_chat_vs_task(
                        line.brief.intent.as_str(),
                        &line.brief.by,
                    ),
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
            for span in &spans {
                started = started.min(span.ts_ms);
                finished = finished.max(span.ts_ms);
                let summary = redact_for_diagnostics(
                    &clip_chars(&span.result, 160),
                    home_ref,
                );
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
                turn.errors
                    .push(redact_for_diagnostics(&clip_chars(&assistant_raw, 240), home_ref));
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

        let at = SystemTime::now();
        let path = match diagnostics_zip_path(at) {
            Ok(p) => p,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        match write_diagnostics_zip(&path, &export) {
            Ok(()) => {
                self.diagnostics_last_zip = Some(path.clone());
                self.status = diagnostics_saved_status(&path);
            }
            Err(e) => self.status = format!("Couldn't save diagnostics: {e}"),
        }
    }

    fn effective_personality_id_for(&self, thread: &crate::threads::ChatThread) -> String {
        let id = if thread.personality.trim().is_empty() {
            self.cfg.personality.clone()
        } else {
            thread.personality.clone()
        };
        if id.trim().is_empty() {
            grokhub_core::personality::DEFAULT_ID.into()
        } else {
            id
        }
    }

    fn diagnostics_service_status(&self) -> Vec<String> {
        let mut lines: Vec<String> = grokhub_core::doctor_lines(
            self.has_key(),
            config::config_dir().join("memory").is_dir(),
            HUB_KIND,
        )
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

/// User/assistant pairs as (1-based turn, user text, assistant raw body).
fn build_turn_pairs(messages: &[(String, String)]) -> Vec<(u32, String, String)> {
    let mut out = Vec::new();
    let mut turn = 0u32;
    let mut pending_user: Option<String> = None;
    for (role, body) in messages {
        match role.as_str() {
            "user" => {
                if let Some(prev) = pending_user.take() {
                    turn += 1;
                    out.push((turn, prev, String::new()));
                }
                pending_user = Some(body.clone());
            }
            "assistant" => {
                if let Some(user) = pending_user.take() {
                    turn += 1;
                    out.push((turn, user, body.clone()));
                } else if turn > 0 {
                    // Continuation / repair reply: attach to last turn's raw if empty.
                    if let Some(last) = out.last_mut() {
                        if last.2.is_empty() {
                            last.2 = body.clone();
                        } else {
                            last.2.push_str("\n\n");
                            last.2.push_str(body);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(user) = pending_user {
        turn += 1;
        out.push((turn, user, String::new()));
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
    fn turn_pairs_number_user_assistant() {
        let msgs = vec![
            ("user".into(), "hi".into()),
            ("assistant".into(), "TURN_SAY:\nhey\n".into()),
            ("user".into(), "fix blender".into()),
            ("assistant".into(), "ok".into()),
        ];
        let pairs = build_turn_pairs(&msgs);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, 1);
        assert_eq!(pairs[0].1, "hi");
        assert!(pairs[0].2.contains("TURN_SAY:"));
        assert_eq!(pairs[1].0, 2);
        assert_eq!(pairs[1].1, "fix blender");
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
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);

        let mut cabin = Cabin::quiet_for_test();
        let mut thread = crate::threads::ChatThread::new("Hangout", false);
        thread.messages = std::sync::Arc::new(vec![
            ("user".into(), "hi there".into()),
            (
                "assistant".into(),
                "TURN_SAY:\nHey — what's up?\n".into(),
            ),
        ]);
        cabin.messages = thread.messages.clone();
        cabin.threads = vec![thread];
        cabin.thread_idx = 0;
        cabin.set_diagnostics_mode(true);
        cabin.save_chat_diagnostics(None);
        assert!(
            cabin.status.starts_with("Saved ") && cabin.status.contains("Open folder"),
            "status={}",
            cabin.status
        );
        let path = cabin.diagnostics_last_zip.expect("zip path");
        assert!(path.starts_with(&desk), "{path:?}");
        assert!(path.is_file());
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with("loadout-diagnostics-") && name.ends_with(".zip"),
            "{name}"
        );

        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match old_up {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&root);
    }
}
