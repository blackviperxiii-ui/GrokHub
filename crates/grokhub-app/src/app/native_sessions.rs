//! History rows for native sessions, merged with read-only Grok CLI sessions.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use super::*;

#[derive(Clone)]
struct HistoryCache {
    gen: u64,
    at: u64,
    rows: Vec<grokhub_agent::HistoryRow>,
}

#[derive(Clone, Default)]
struct RenameDraft {
    id: String,
    text: String,
}

enum NativeHistAct {
    Open(grokhub_agent::HistoryRow),
    Delete(String),
    Fork(String),
    Export(String),
    SaveRename(String, String),
}

/// CLI sessions from `discover_session_files_in`, then native JSONL sessions.
pub(super) fn merged_native_history(cli_home: Option<&Path>) -> Vec<grokhub_agent::HistoryRow> {
    let cli = match cli_home {
        Some(home) => grokhub_acp::discover_session_files_in(home),
        None => grokhub_acp::discover_session_files(),
    };
    let native = grokhub_agent::list_sessions();
    grokhub_agent::merge_history(&cli, &native)
}

fn cached_merged_rows(ui: &egui::Ui) -> Vec<grokhub_agent::HistoryRow> {
    let gen = grokhub_agent::history_generation();
    let now = now_ms();
    let id = egui::Id::new("native-history-cache");
    if let Some(hit) = ui.data(|data| data.get_temp::<HistoryCache>(id)) {
        if hit.gen == gen && now.saturating_sub(hit.at) < 2_000 {
            return hit.rows;
        }
    }
    let rows = merged_native_history(None);
    ui.data_mut(|data| {
        data.insert_temp(
            id,
            HistoryCache {
                gen,
                at: now,
                rows: rows.clone(),
            },
        );
    });
    rows
}

impl Cabin {
    /// Extra History section. Absent unless Settings → Labs native engine is on,
    /// so the CLI history page stays as it is.
    pub(super) fn paint_native_history_merge(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.native_engine {
            return;
        }
        let bound: HashSet<String> = self
            .threads
            .iter()
            .filter_map(|thread| thread.grok_session.clone())
            .filter(|id| !id.trim().is_empty())
            .collect();
        let query = self.history_q.trim().to_ascii_lowercase();
        let mut rows = cached_merged_rows(ui);
        rows.retain(|row| !bound.contains(&row.id));
        if !query.is_empty() {
            rows.retain(|row| {
                row.title.to_ascii_lowercase().contains(&query)
                    || row.id.to_ascii_lowercase().contains(&query)
            });
        }
        if rows.is_empty() {
            return;
        }
        ui.add_space(16.0);
        crate::cards::section_label(ui, "Sessions");
        ui.label(
            egui::RichText::new(
                "Native sessions resume in the native engine. Grok CLI sessions stay read-only.",
            )
            .size(12.0)
            .color(crate::theme::subtle()),
        );
        let draft_id = egui::Id::new("native-history-rename");
        let mut draft = ui
            .data(|data| data.get_temp::<RenameDraft>(draft_id))
            .unwrap_or_default();
        let mut act: Option<NativeHistAct> = None;
        for row in &rows {
            let kind = if row.read_only {
                "Grok CLI".to_string()
            } else {
                let usage = grokhub_agent::usage_label(
                    row.input_tokens,
                    row.output_tokens,
                    row.reasoning_tokens,
                    row.cost_in_usd_ticks,
                    &row.meter,
                );
                if usage.is_empty() {
                    "Native".to_string()
                } else {
                    format!("Native · {usage}")
                }
            };
            let add = if row.read_only { None } else { Some("Delete") };
            match crate::cards::grok_tile(
                ui,
                crate::icons::TileIcon::Chat,
                &row.title,
                &kind,
                add,
                false,
            ) {
                crate::cards::TileHit::Body => act = Some(NativeHistAct::Open(row.clone())),
                crate::cards::TileHit::Add => act = Some(NativeHistAct::Delete(row.id.clone())),
                crate::cards::TileHit::None => {}
            }
            if row.native && !row.read_only {
                if draft.id == row.id {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut draft.text).desired_width(220.0));
                        if crate::cards::ghost_pill(ui, "Save") {
                            act = Some(NativeHistAct::SaveRename(
                                row.id.clone(),
                                draft.text.clone(),
                            ));
                        }
                        if crate::cards::ghost_pill(ui, "Cancel") {
                            draft = RenameDraft::default();
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if crate::cards::ghost_pill(ui, "Fork") {
                            act = Some(NativeHistAct::Fork(row.id.clone()));
                        }
                        if crate::cards::ghost_pill(ui, "Export") {
                            act = Some(NativeHistAct::Export(row.id.clone()));
                        }
                        if crate::cards::ghost_pill(ui, "Rename") {
                            draft = RenameDraft {
                                id: row.id.clone(),
                                text: row.title.clone(),
                            };
                        }
                    });
                }
            }
            ui.add_space(6.0);
        }
        if matches!(act, Some(NativeHistAct::SaveRename(_, _))) {
            draft = RenameDraft::default();
        }
        ui.data_mut(|data| data.insert_temp(draft_id, draft));
        if let Some(act) = act {
            self.apply_native_hist_act(act);
        }
    }

    fn apply_native_hist_act(&mut self, act: NativeHistAct) {
        match act {
            NativeHistAct::Open(row) => {
                if row.native && !row.read_only {
                    self.open_native_history(&row.id);
                } else {
                    self.open_cli_history_row(&row);
                }
            }
            NativeHistAct::Delete(id) => self.delete_native_history(&id),
            NativeHistAct::Fork(id) => self.fork_native_history(&id),
            NativeHistAct::Export(id) => self.export_native_history(&id),
            NativeHistAct::SaveRename(id, title) => self.rename_native_history(&id, &title),
        }
    }

    pub(super) fn show_native_usage(&mut self, info: &grokhub_agent::SessionInfo) {
        self.grok_usage.input_tokens = info.usage.input_tokens;
        self.grok_usage.output_tokens = info.usage.output_tokens;
        self.grok_usage.reasoning_tokens = info.usage.reasoning_tokens;
        self.grok_usage.total_tokens = info
            .usage
            .input_tokens
            .saturating_add(info.usage.output_tokens)
            .saturating_add(info.usage.reasoning_tokens);
        self.grok_usage.cost_in_usd_ticks = info.usage.cost_in_usd_ticks;
        self.grok_usage.meter = info.meter.clone();
        let input = info.input();
        let has_usage = info.usage.input_tokens > 0
            || info.usage.output_tokens > 0
            || info.usage.reasoning_tokens > 0
            || info.usage.cost_in_usd_ticks != 0;
        if !input.is_empty() || has_usage {
            self.grok_usage.context_tokens_used = grokhub_agent::estimate_input_tokens(&input);
            self.grok_usage.context_window_tokens =
                grokhub_agent::context_length_for(&info.model, &[]);
        }
    }

    pub(super) fn sync_native_title_from_store(&mut self) {
        let Some(id) = self
            .acp
            .as_ref()
            .map(|handle| handle.session_id.clone())
            .filter(|id| id.starts_with("native-"))
        else {
            return;
        };
        let Ok(info) = grokhub_agent::load_session(&id) else {
            return;
        };
        let Some(idx) = self.threads.iter().position(|thread| {
            thread.native && thread.grok_session.as_deref() == Some(id.as_str())
        }) else {
            return;
        };
        if self.threads[idx].title_locked {
            return;
        }
        let title = info.title.trim();
        if title.is_empty() || title == "Chat" || title == id {
            return;
        }
        if self.threads[idx].title != title {
            self.threads[idx].title = title.to_string();
            self.persist();
        }
    }

    fn open_cli_history_row(&mut self, row: &grokhub_agent::HistoryRow) {
        if !self
            .grok_sessions
            .iter()
            .any(|session| session.id == row.id)
        {
            self.grok_sessions.push(grokhub_acp::GrokSession {
                id: row.id.clone(),
                title: row.title.clone(),
                path: row.path.clone(),
                cwd: row.cwd.clone(),
                cabin: false,
            });
        }
        self.open_grok_session(&row.id);
        if let Some(thread) = self
            .threads
            .iter_mut()
            .find(|thread| thread.grok_session.as_deref() == Some(row.id.as_str()))
        {
            thread.native = false;
        }
    }

    fn open_native_history(&mut self, id: &str) {
        let Ok(info) = grokhub_agent::load_session(id) else {
            self.status = "Session file is missing".into();
            return;
        };
        self.open_native_info(&info);
    }

    fn open_native_info(&mut self, info: &grokhub_agent::SessionInfo) {
        let pairs = grokhub_agent::transcript_pairs(info);
        if let Some(idx) = self
            .threads
            .iter()
            .position(|thread| thread.grok_session.as_deref() == Some(info.id.as_str()))
        {
            if let Some(thread) = self.threads.get_mut(idx) {
                thread.native = true;
                thread.grok_show_pending = false;
                if thread.messages.is_empty() && !pairs.is_empty() {
                    thread.messages = Arc::new(pairs);
                }
                if !thread.title_locked
                    && (thread.title.trim().is_empty() || thread.title == "Chat")
                {
                    let title = info.title.trim();
                    if !title.is_empty() && title != "Chat" {
                        thread.title = title.to_string();
                    }
                }
            }
            let same = idx == self.thread_idx;
            self.apply_switch_thread(idx);
            if same {
                if let Some(thread) = self.threads.get(idx) {
                    self.messages = thread.messages.clone();
                }
                self.pin_chat_tail();
            }
            self.prepare_native_resume(&info.id);
            self.show_native_usage(info);
            self.nav = Nav::Chat;
            self.composer_want_focus = true;
            return;
        }
        let title = {
            let title = info.title.trim();
            if title.is_empty() {
                "Chat".to_string()
            } else {
                title.to_string()
            }
        };
        let mut created = ChatThread::new(&title, false);
        created.native = true;
        created.grok_session = Some(info.id.clone());
        created.grok_cwd = Some(info.cwd.clone()).filter(|cwd| !cwd.trim().is_empty());
        created.title_locked = info.renamed;
        created.accessed_ms = now_ms();
        if !pairs.is_empty() {
            created.messages = Arc::new(pairs);
        }
        self.threads.push(created);
        let idx = self.threads.len() - 1;
        self.apply_switch_thread(idx);
        self.prepare_native_resume(&info.id);
        self.show_native_usage(info);
        self.nav = Nav::Chat;
        self.status = format!("Opened {title}");
        self.composer_want_focus = true;
        self.persist();
    }

    fn prepare_native_resume(&mut self, id: &str) {
        if self
            .acp
            .as_ref()
            .is_some_and(|handle| handle.session_id != id)
        {
            self.acp = None;
        }
    }

    fn delete_native_history(&mut self, id: &str) {
        let thread_ids: Vec<String> = self
            .threads
            .iter()
            .filter(|thread| thread.grok_session.as_deref() == Some(id))
            .map(|thread| thread.id.clone())
            .collect();
        if self
            .acp
            .as_ref()
            .is_some_and(|handle| handle.session_id == id)
        {
            self.halt_in_flight();
            self.acp = None;
        }
        for thread_id in &thread_ids {
            self.drop_bg_runs_for(thread_id);
        }
        match grokhub_agent::delete_session(id) {
            Ok(()) => self.status = "Deleted session".into(),
            Err(err) => self.status = format!("Delete failed: {err}"),
        }
    }

    fn fork_native_history(&mut self, id: &str) {
        match grokhub_agent::fork_session(id) {
            Ok(info) => {
                let title = info.title.clone();
                self.open_native_info(&info);
                self.status = format!("Forked {title}");
            }
            Err(err) => self.status = format!("Fork failed: {err}"),
        }
    }

    fn export_native_history(&mut self, id: &str) {
        let md = match grokhub_agent::export_markdown(id) {
            Ok(md) => md,
            Err(err) => {
                self.status = format!("Export failed: {err}");
                return;
            }
        };
        let dest = self.export_dest(&format!("{id}.md"));
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&dest, md) {
            Ok(()) => self.status = format!("Exported {}", dest.display()),
            Err(err) => self.status = format!("Export failed: {err}"),
        }
    }

    fn rename_native_history(&mut self, id: &str, title: &str) {
        match grokhub_agent::rename_session(id, title) {
            Ok(()) => {
                let shown = grokhub_agent::local_title(title);
                if let Some(thread) = self
                    .threads
                    .iter_mut()
                    .find(|thread| thread.native && thread.grok_session.as_deref() == Some(id))
                {
                    thread.title = shown.clone();
                    thread.title_locked = true;
                }
                self.status = format!("Renamed {shown}");
                self.persist();
            }
            Err(err) => self.status = format!("Rename failed: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_history_merge_keeps_cli_rows_read_only() {
        let cli_id = "01a01b0f-7e06-74b1-8f22-5236c9d57d45";
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("gh-cli-home-{}-{stamp}", std::process::id()));
        let cfg =
            std::env::temp_dir().join(format!("gh-native-merge-{}-{stamp}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(home.join("sessions").join(cli_id)).unwrap();
        let plan = home.join("sessions").join(cli_id).join("plan.md");
        let bytes = b"# Night cabin\n\n## User\n\nhello\n";
        std::fs::write(&plan, bytes).unwrap();
        let before = std::fs::read(&plan).unwrap();
        let _guard = grokhub_agent::perm::ConfigGuard::set(&cfg);
        let item = grokhub_agent::InputItem::Message {
            role: "user".into(),
            content: vec![grokhub_agent::ContentPart::InputText("fix the dock".into())],
        };
        let usage = grokhub_agent::Usage {
            input_tokens: 3,
            output_tokens: 1,
            reasoning_tokens: 0,
            cost_in_usd_ticks: 9,
        };
        grokhub_agent::record_turn(
            "native-merge",
            "/work",
            "grok-4.7",
            &[item],
            &usage,
            "SuperGrok pool",
            "fix the dock",
        )
        .unwrap();
        let rows = merged_native_history(Some(&home));
        let cli = rows.iter().find(|row| row.id == cli_id).expect("cli row");
        assert_eq!(cli.title, "Night cabin");
        assert_eq!(cli.path.as_deref(), Some(plan.as_path()));
        assert_eq!(cli.cwd, None);
        assert!(cli.read_only);
        assert!(!cli.native);
        grokhub_agent::rename_session("native-merge", "Renamed native").unwrap();
        let rows = merged_native_history(Some(&home));
        let cli = rows
            .iter()
            .find(|row| row.id == cli_id)
            .expect("cli row stays");
        assert_eq!(cli.title, "Night cabin");
        assert_eq!(cli.path.as_deref(), Some(plan.as_path()));
        assert_eq!(cli.cwd, None);
        assert!(cli.read_only);
        assert!(!cli.native);
        assert_eq!(std::fs::read(&plan).unwrap(), before);
        let native = rows
            .iter()
            .find(|row| row.id == "native-merge")
            .expect("native row");
        assert!(native.native);
        assert!(!native.read_only);
        assert_eq!(native.title, "Renamed native");
        assert_eq!(native.cost_in_usd_ticks, 9);
        let here = include_str!("native_sessions.rs");
        assert!(here.contains("discover_session_files_in"));
        assert!(here.contains("read_only"));
        assert!(here.contains("delete_session"));
        assert!(here.contains("cfg.native_engine"));
        let pages = include_str!("pages.rs");
        assert!(pages.contains("paint_native_history_merge"));
        drop(_guard);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn manual_compact_slash_targets_native_threads_only() {
        let slash = include_str!("slash.rs");
        let compact = slash
            .split("Slash::Compact =>")
            .nth(1)
            .and_then(|src| src.split("Slash::Skill").next())
            .expect("Compact");
        let native_at = compact
            .find("native_compact_if_current")
            .expect("native hook");
        let cli_at = compact
            .find("send_grok_slash(\"/compact\")")
            .expect("cli compact");
        assert!(native_at < cli_at, "{compact}");
        assert!(
            compact.contains("compact_keep_start_from") && !compact.contains("content.clone()"),
            "{compact}"
        );
        assert!(
            compact.contains("stamp_current_access") || compact.contains("accessed_ms"),
            "{compact}"
        );
        let engine = include_str!("native_engine.rs");
        let body = engine
            .split("fn native_compact_if_current")
            .nth(1)
            .and_then(|src| src.split("fn kick_native_turn").next())
            .expect("native_compact_if_current");
        assert!(body.contains("manual_compact_targets_native"));
        assert!(body.contains("self.cfg.native_engine"));
        assert!(body.contains("ensure_native_engine"));
        assert!(body.contains("\"Compacting…\""));
        assert!(!body.contains("self.running = true"));
        let ensure_at = body.find("ensure_native_engine").unwrap();
        let job_at = body.find("chat_job_thread").unwrap();
        assert!(ensure_at < job_at, "{body}");
        assert!(grokhub_agent::manual_compact_targets_native(true, true));
        assert!(!grokhub_agent::manual_compact_targets_native(false, true));
        assert!(!grokhub_agent::manual_compact_targets_native(true, false));
    }
}
