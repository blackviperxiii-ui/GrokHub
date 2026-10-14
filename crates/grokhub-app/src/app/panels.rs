//! Phase 1 controlled generative UI panels (CI status, build errors, compare table).
//! Renderer only: actions are returned as [`PanelAction`] intents for approval-gated callers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use grokhub_core::panel_spec::{
    parse_panel_spec as parse_core_panel_spec, PanelElement as CoreElement, PanelSpec as CoreSpec,
};
use serde::Deserialize;
use serde::Serialize;

use crate::ui_kit::{badge, card_frame, BadgeTone, CardVariant};

/// Off unless `generative_ui.toml` sets `enabled = true` (no Settings toggle yet).
pub static PANELS_UI_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn panels_enabled() -> bool {
    PANELS_UI_ENABLED.load(Ordering::Relaxed)
}

/// Read G2 `generative_ui.toml` `enabled` and set [`PANELS_UI_ENABLED`].
pub fn apply_generative_ui_setting(config_dir: &Path) {
    PANELS_UI_ENABLED.store(read_generative_ui_enabled(config_dir), Ordering::Relaxed);
}

fn read_generative_ui_enabled(config_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(config_dir.join("generative_ui.toml")) else {
        return false;
    };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("enabled") && value.trim().eq_ignore_ascii_case("true") {
            return true;
        }
    }
    false
}

/// User intent from a panel control; the host runs tools after approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelAction {
    pub panel_id: String,
    pub action: String,
}

/// Phase 1 controlled panel spec (JSON `type` discriminant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PanelSpec {
    CiStatus(CiStatus),
    BuildErrors(BuildErrors),
    CompareTable(CompareTable),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiStatus {
    pub pr_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub checks: Vec<CiCheck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<PanelButton>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiCheck {
    pub name: String,
    pub state: CiCheckState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiCheckState {
    Pass,
    Fail,
    Pending,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildErrors {
    pub target: String,
    pub errors: Vec<BuildErrorLine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<PanelButton>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildErrorLine {
    pub file: String,
    pub line: u32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompareTable {
    pub subject: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<PanelButton>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelButton {
    pub label: String,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq)]
enum PanelBody {
    /// G3 controlled tagged forms (`ci_status` / `build_errors` / `compare_table`).
    Spec(PanelSpec),
    /// Live G1/G2 agent shape from `grokhub_core::panel_spec` (`title` + `elements`).
    Core(CoreSpec),
    Fallback(String),
}

struct PaintPanel {
    id: String,
    header: String,
    pinned: bool,
    body: PanelBody,
}

#[derive(Debug)]
struct PanelEntry {
    body: PanelBody,
    pinned: bool,
    dismissed: bool,
    /// Tool call id (or message id) that owns this panel in the chat thread.
    anchor: String,
}

/// Live panels keyed by id (upsert updates in place).
#[derive(Debug, Default)]
pub struct PanelStore {
    /// Open chat thread this store is bound to; switching clears panels.
    thread_id: String,
    order: Vec<String>,
    entries: HashMap<String, PanelEntry>,
}

impl PanelStore {
    /// Insert or replace a panel by id (tests and direct hosts).
    #[allow(dead_code)]
    pub fn upsert(&mut self, id: impl Into<String>, spec_json: &str) {
        self.upsert_anchored(id, "", spec_json);
    }

    fn upsert_anchored(&mut self, id: impl Into<String>, anchor: &str, spec_json: &str) {
        let id = id.into();
        let body = parse_panel_json(spec_json);
        if let Some(entry) = self.entries.get_mut(&id) {
            // Same body (re-drain / re-emit) must not revive a dismissed panel.
            if entry.body != body {
                entry.body = body;
                entry.dismissed = false;
            }
            if !anchor.is_empty() {
                entry.anchor = anchor.to_string();
            }
        } else {
            self.order.push(id.clone());
            self.entries.insert(
                id,
                PanelEntry {
                    body,
                    pinned: false,
                    dismissed: false,
                    anchor: anchor.to_string(),
                },
            );
        }
    }

    /// Drop all panels when the open chat changes.
    pub fn clear(&mut self) {
        self.order.clear();
        self.entries.clear();
    }

    /// Bind panels to `thread_id`; clears the store when the thread changes.
    pub fn bind_thread(&mut self, thread_id: &str) {
        if self.thread_id != thread_id {
            self.clear();
            self.thread_id = thread_id.to_string();
        }
    }

    /// Parse a G2 `show_panel` tool result (`{"panel": …}` marker). Same id updates
    /// in place. Returns false when the result is not a panel marker.
    pub fn upsert_from_tool_result(&mut self, call_id: &str, result: &str) -> bool {
        let Some((opt_id, spec_json)) = extract_panel_marker(result) else {
            return false;
        };
        let id = opt_id.unwrap_or_else(|| call_id.to_string());
        self.upsert_anchored(id, call_id, &spec_json);
        true
    }

    #[cfg(test)]
    fn anchor_of(&self, id: &str) -> Option<&str> {
        self.entries.get(id).map(|e| e.anchor.as_str())
    }

    pub fn dismiss(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.dismissed = true;
        }
    }

    /// Drop every unpinned panel (Settings / slash later).
    #[allow(dead_code)]
    pub fn dismiss_all(&mut self) {
        for id in self.order.clone() {
            let pinned = self.entries.get(&id).map(|e| e.pinned).unwrap_or(false);
            if pinned {
                continue;
            }
            self.entries.remove(&id);
        }
        self.order.retain(|id| self.entries.contains_key(id));
    }

    pub fn pin(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.pinned = true;
            entry.dismissed = false;
        }
    }

    pub fn unpin(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.pinned = false;
        }
    }

    #[allow(dead_code)]
    pub fn is_pinned(&self, id: &str) -> bool {
        self.entries.get(id).map(|e| e.pinned).unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.entries.values().filter(|e| !e.dismissed).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[cfg(test)]
    fn pr_number(&self, id: &str) -> Option<u32> {
        self.entries.get(id).and_then(|e| match &e.body {
            PanelBody::Spec(PanelSpec::CiStatus(ci)) => Some(ci.pr_number),
            _ => None,
        })
    }
}

fn parse_panel_json(json: &str) -> PanelBody {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return PanelBody::Fallback("Panel spec is empty.".into());
    }
    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(e) => return PanelBody::Fallback(format!("Invalid panel JSON: {e}")),
    };
    // G3 controlled tagged forms used by cabin fixtures / tests.
    if let Some(ty) = value.get("type").and_then(|v| v.as_str()) {
        if matches!(ty, "ci_status" | "build_errors" | "compare_table") {
            return match serde_json::from_value::<PanelSpec>(value) {
                Ok(spec) => PanelBody::Spec(spec),
                Err(e) => PanelBody::Fallback(format!("Unsupported panel spec: {e}")),
            };
        }
    }
    // Live G1/G2 agent shape: `{ "title"?, "elements": [...] }`.
    if value.get("elements").is_some() {
        return match parse_core_panel_spec(trimmed) {
            Ok(spec) => PanelBody::Core(spec),
            Err(e) => PanelBody::Fallback(format!("Unsupported panel spec: {e}")),
        };
    }
    PanelBody::Fallback("Unsupported panel spec: missing type or elements".into())
}

/// Pull the G2 `{"panel": …}` marker out of a tool result body (summary + JSON).
fn extract_panel_marker(result: &str) -> Option<(Option<String>, String)> {
    let mut search = result;
    while let Some(rel) = search.find('{') {
        let mut de = serde_json::Deserializer::from_str(&search[rel..]);
        if let Ok(v) = serde_json::Value::deserialize(&mut de) {
            if let Some(panel) = v.get("panel") {
                let id = panel
                    .get("id")
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let json = serde_json::to_string(panel).ok()?;
                return Some((id, json));
            }
        }
        search = &search[rel + 1..];
    }
    None
}

fn header_title(id: &str, body: &PanelBody) -> String {
    match body {
        PanelBody::Spec(PanelSpec::CiStatus(ci)) => {
            if let Some(t) = &ci.title {
                format!("PR #{} — {}", ci.pr_number, t)
            } else {
                format!("PR #{}", ci.pr_number)
            }
        }
        PanelBody::Spec(PanelSpec::BuildErrors(be)) => be.target.clone(),
        PanelBody::Spec(PanelSpec::CompareTable(ct)) => ct.subject.clone(),
        PanelBody::Core(spec) => core_header_title(id, spec),
        PanelBody::Fallback(_) => format!("Panel {id}"),
    }
}

fn core_header_title(id: &str, spec: &CoreSpec) -> String {
    if let Some(title) = &spec.title {
        let t = title.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    for el in &spec.elements {
        if let CoreElement::Heading(h) = el {
            let t = h.text.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    format!("Panel {id}")
}

/// Paint visible panels and collect action intents. No I/O or tool calls.
pub fn show(ui: &mut Ui, store: &mut PanelStore) -> Vec<PanelAction> {
    show_filtered(ui, store, None)
}

/// Paint panels anchored to a tool call / message id.
pub fn show_for_anchor(ui: &mut Ui, store: &mut PanelStore, anchor: &str) -> Vec<PanelAction> {
    show_filtered(ui, store, Some(anchor))
}

fn show_filtered(
    ui: &mut Ui,
    store: &mut PanelStore,
    only_anchor: Option<&str>,
) -> Vec<PanelAction> {
    if !panels_enabled() {
        return Vec::new();
    }
    let mut actions = Vec::new();
    let panels: Vec<PaintPanel> = store
        .order
        .iter()
        .filter_map(|id| {
            store.entries.get(id).and_then(|entry| {
                if entry.dismissed {
                    return None;
                }
                if let Some(want) = only_anchor {
                    if entry.anchor != want {
                        return None;
                    }
                }
                Some(PaintPanel {
                    id: id.clone(),
                    header: header_title(id, &entry.body),
                    pinned: entry.pinned,
                    body: entry.body.clone(),
                })
            })
        })
        .collect();
    for panel in panels {
        card_frame(ui, CardVariant::Outlined, true, |ui| {
            paint_header(ui, &panel.header, panel.pinned, &panel.id, store);
            ui.add_space(crate::theme::SPACING_8);
            match &panel.body {
                PanelBody::Spec(spec) => {
                    paint_spec(ui, spec, &panel.id, &mut actions);
                }
                PanelBody::Core(spec) => {
                    paint_core(ui, spec, &panel.id, &mut actions);
                }
                PanelBody::Fallback(msg) => {
                    ui.label(
                        RichText::new(msg.as_str())
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::fg()),
                    );
                }
            }
        });
        ui.add_space(crate::theme::SPACING_8);
    }
    actions
}

fn paint_header(ui: &mut Ui, title: &str, pinned: bool, panel_id: &str, store: &mut PanelStore) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .font(crate::theme::title_font(crate::theme::TYPE_LABEL))
                .color(crate::theme::fg()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let pin_label = if pinned { "Unpin" } else { "Pin" };
            if header_btn(ui, pin_label).clicked() {
                if pinned {
                    store.unpin(panel_id);
                } else {
                    store.pin(panel_id);
                }
            }
            if header_btn(ui, "Dismiss").clicked() {
                store.dismiss(panel_id);
            }
        });
    });
}

fn header_btn(ui: &mut Ui, label: &str) -> egui::Response {
    crate::theme::felt_label_button(
        ui,
        label,
        egui::Color32::TRANSPARENT,
        crate::theme::fg(),
        crate::theme::RADIUS_SM,
        egui::vec2(crate::theme::MIN_HIT, crate::theme::MIN_HIT * 0.85),
        Some(egui::Stroke::new(1.0, crate::theme::border())),
        false,
    )
}

fn paint_spec(ui: &mut Ui, spec: &PanelSpec, panel_id: &str, actions: &mut Vec<PanelAction>) {
    match spec {
        PanelSpec::CiStatus(ci) => paint_ci_status(ui, ci, panel_id, actions),
        PanelSpec::BuildErrors(be) => paint_build_errors(ui, be, panel_id, actions),
        PanelSpec::CompareTable(ct) => paint_compare_table(ui, ct, panel_id, actions),
    }
}

fn paint_core(ui: &mut Ui, spec: &CoreSpec, panel_id: &str, actions: &mut Vec<PanelAction>) {
    for el in &spec.elements {
        match el {
            CoreElement::Heading(h) => {
                ui.label(
                    RichText::new(&h.text)
                        .font(crate::theme::title_font(crate::theme::TYPE_LABEL))
                        .color(crate::theme::fg()),
                );
            }
            CoreElement::Text(t) => {
                ui.label(
                    RichText::new(&t.text)
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::fg()),
                );
            }
            CoreElement::List(list) => {
                for (i, item) in list.items.iter().enumerate() {
                    let bullet = if list.ordered {
                        format!("{}. {item}", i + 1)
                    } else {
                        format!("• {item}")
                    };
                    ui.label(
                        RichText::new(bullet)
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::fg()),
                    );
                }
            }
            CoreElement::Table(table) => {
                if table.headers.is_empty() {
                    continue;
                }
                paint_string_table(
                    ui,
                    ("core-table", panel_id),
                    &table.headers,
                    &table.rows,
                );
            }
            CoreElement::KeyValue(kv) => {
                for pair in &kv.pairs {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{}:", pair.key))
                                .size(crate::theme::TYPE_CAPTION)
                                .color(crate::theme::muted()),
                        );
                        ui.label(
                            RichText::new(&pair.value)
                                .size(crate::theme::TYPE_BODY)
                                .color(crate::theme::fg()),
                        );
                    });
                }
            }
            CoreElement::Button(btn) => {
                if action_btn(ui, &btn.label).clicked() {
                    actions.push(PanelAction {
                        panel_id: panel_id.to_string(),
                        action: btn.action.clone(),
                    });
                }
            }
            CoreElement::Input(input) => {
                let value = input.value.as_deref().unwrap_or("");
                ui.label(
                    RichText::new(format!("{}: {value}", input.label))
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::fg()),
                );
            }
            CoreElement::Progress(p) => {
                let pct = (p.value.clamp(0.0, 1.0) * 100.0).round() as i32;
                ui.label(
                    RichText::new(format!("{} — {pct}%", p.label))
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::fg()),
                );
            }
        }
        ui.add_space(crate::theme::SPACING_4);
    }
}

fn paint_ci_status(ui: &mut Ui, ci: &CiStatus, panel_id: &str, actions: &mut Vec<PanelAction>) {
    for check in &ci.checks {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&check.name)
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::fg()),
            );
            let (label, tone) = ci_badge(check.state);
            badge(ui, label, tone);
        });
    }
    paint_action_row(ui, &ci.actions, panel_id, actions);
}

fn ci_badge(state: CiCheckState) -> (&'static str, BadgeTone) {
    match state {
        CiCheckState::Pass => ("Pass", BadgeTone::Accent),
        CiCheckState::Fail => ("Fail", BadgeTone::Warning),
        CiCheckState::Pending => ("Pending", BadgeTone::Neutral),
        CiCheckState::Skipped => ("Skipped", BadgeTone::Neutral),
    }
}

fn paint_build_errors(
    ui: &mut Ui,
    be: &BuildErrors,
    panel_id: &str,
    actions: &mut Vec<PanelAction>,
) {
    for err in &be.errors {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{}:{}", err.file, err.line))
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::blend_color(
                        crate::theme::fg(),
                        crate::theme::bg(),
                        0.25,
                    )),
            );
            ui.label(
                RichText::new(&err.message)
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::fg()),
            );
            if let Some(action) = &err.action {
                if action_btn(ui, "Jump").clicked() {
                    actions.push(PanelAction {
                        panel_id: panel_id.to_string(),
                        action: action.clone(),
                    });
                }
            }
        });
    }
    paint_action_row(ui, &be.actions, panel_id, actions);
}

fn paint_compare_table(
    ui: &mut Ui,
    ct: &CompareTable,
    panel_id: &str,
    actions: &mut Vec<PanelAction>,
) {
    if ct.headers.is_empty() {
        ui.label(
            RichText::new("(empty table)")
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::fg()),
        );
        return;
    }
    paint_string_table(ui, ("compare", panel_id), &ct.headers, &ct.rows);
    paint_action_row(ui, &ct.actions, panel_id, actions);
}

/// Embedded string table via egui_extras TableBuilder (no inner vscroll).
fn paint_string_table(
    ui: &mut Ui,
    id_salt: impl egui::AsIdSalt,
    headers: &[String],
    rows: &[Vec<String>],
) {
    let cols = headers.len();
    if cols == 0 {
        return;
    }
    let header_h = crate::theme::TYPE_CAPTION + crate::theme::SPACING_4;
    let row_h = crate::theme::TYPE_BODY + crate::theme::SPACING_4;
    let mut builder = TableBuilder::new(ui)
        .id_salt(id_salt)
        .vscroll(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    for i in 0..cols {
        let col = if i + 1 == cols {
            Column::remainder()
        } else {
            Column::auto()
        };
        builder = builder.column(col);
    }
    builder
        .header(header_h, |mut header| {
            for h in headers {
                header.col(|ui| {
                    ui.label(
                        RichText::new(h)
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::fg()),
                    );
                });
            }
        })
        .body(|mut body| {
            for row in rows {
                body.row(row_h, |mut table_row| {
                    for i in 0..cols {
                        let cell = row.get(i).map(String::as_str).unwrap_or("");
                        table_row.col(|ui| {
                            ui.label(
                                RichText::new(cell)
                                    .size(crate::theme::TYPE_BODY)
                                    .color(crate::theme::fg()),
                            );
                        });
                    }
                });
            }
        });
}

fn paint_action_row(
    ui: &mut Ui,
    buttons: &[PanelButton],
    panel_id: &str,
    actions: &mut Vec<PanelAction>,
) {
    if buttons.is_empty() {
        return;
    }
    ui.add_space(crate::theme::SPACING_8);
    ui.horizontal_wrapped(|ui| {
        for btn in buttons {
            if action_btn(ui, &btn.label).clicked() {
                actions.push(PanelAction {
                    panel_id: panel_id.to_string(),
                    action: btn.action.clone(),
                });
            }
        }
    });
}

fn action_btn(ui: &mut Ui, label: &str) -> egui::Response {
    crate::theme::felt_label_button(
        ui,
        label,
        crate::theme::surface(),
        crate::theme::fg(),
        crate::theme::RADIUS_MD,
        egui::vec2(crate::theme::MIN_HIT * 1.4, crate::theme::MIN_HIT),
        Some(egui::Stroke::new(1.0, crate::theme::border())),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Pos2, Rect, Shape};
    use std::sync::atomic::Ordering;

    fn enable_panels() {
        PANELS_UI_ENABLED.store(true, Ordering::Relaxed);
    }

    fn disable_panels() {
        PANELS_UI_ENABLED.store(false, Ordering::Relaxed);
    }

    fn headless_show(
        store: &mut PanelStore,
    ) -> (Vec<egui::epaint::ClippedShape>, Vec<PanelAction>) {
        let ctx = crate::ui_kit::test_ctx(true);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480.0, 720.0))),
            ..Default::default()
        };
        let mut actions = Vec::new();
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                actions = show(ui, store);
            });
        });
        (out.shapes, actions)
    }

    fn text_in_shapes(shapes: &[egui::epaint::ClippedShape], needle: &str) -> bool {
        shapes.iter().any(|c| {
            if let Shape::Text(t) = &c.shape {
                t.galley.text().contains(needle)
            } else {
                false
            }
        })
    }

    fn ci_json(pr: u32) -> String {
        format!(
            r#"{{
  "type": "ci_status",
  "pr_number": {pr},
  "checks": [{{"name": "linux", "state": "pass"}}, {{"name": "windows", "state": "fail"}}],
  "actions": [{{"label": "Re-run", "action": "rerun_ci"}}]
}}"#,
            pr = pr
        )
    }

    fn build_errors_json() -> &'static str {
        r#"{
  "type": "build_errors",
  "target": "grokhub-app",
  "errors": [{"file": "src/main.rs", "line": 12, "message": "expected `;`", "action": "jump_line"}],
  "actions": []
}"#
    }

    fn compare_json() -> &'static str {
        r#"{
  "type": "compare_table",
  "subject": "beta vs main",
  "headers": ["Branch", "SHA"],
  "rows": [["beta", "abc"], ["main", "def"]],
  "actions": [{"label": "Open diff", "action": "open_diff"}]
}"#
    }

    #[test]
    fn upsert_updates_in_place() {
        let mut store = PanelStore::default();
        store.upsert("a", &ci_json(1));
        store.upsert("a", &ci_json(2));
        assert_eq!(store.len(), 1);
        assert_eq!(store.pr_number("a"), Some(2));
    }

    #[test]
    fn dismiss_hides_panel() {
        let mut store = PanelStore::default();
        store.upsert("a", &ci_json(9));
        store.dismiss("a");
        assert_eq!(store.len(), 0);
        enable_panels();
        let (shapes, _) = headless_show(&mut store);
        assert!(!text_in_shapes(&shapes, "PR #9"));
        disable_panels();
    }

    #[test]
    fn pinned_survives_dismiss_all_and_updates() {
        let mut store = PanelStore::default();
        store.upsert("keep", &ci_json(10));
        store.pin("keep");
        store.upsert("drop", &ci_json(11));
        store.dismiss_all();
        assert!(store.entries.contains_key("keep"));
        assert!(!store.entries.contains_key("drop"));
        store.upsert("keep", &ci_json(12));
        assert!(store.is_pinned("keep"));
        assert_eq!(store.pr_number("keep"), Some(12));
    }

    #[test]
    fn ci_status_renders_header_headless() {
        enable_panels();
        let mut store = PanelStore::default();
        store.upsert("ci", &ci_json(756));
        let (shapes, _) = headless_show(&mut store);
        assert!(text_in_shapes(&shapes, "PR #756"));
        disable_panels();
    }

    #[test]
    fn build_errors_renders_header_headless() {
        enable_panels();
        let mut store = PanelStore::default();
        store.upsert("be", build_errors_json());
        let (shapes, _) = headless_show(&mut store);
        assert!(text_in_shapes(&shapes, "grokhub-app"));
        assert!(text_in_shapes(&shapes, "expected `;`"));
        disable_panels();
    }

    #[test]
    fn compare_table_renders_header_headless() {
        enable_panels();
        let mut store = PanelStore::default();
        store.upsert("ct", compare_json());
        let (shapes, _) = headless_show(&mut store);
        assert!(text_in_shapes(&shapes, "beta vs main"));
        disable_panels();
    }

    #[test]
    fn invalid_spec_falls_back_to_text() {
        enable_panels();
        let mut store = PanelStore::default();
        store.upsert("bad", r#"{"type":"chart","nope":1}"#);
        let (shapes, _) = headless_show(&mut store);
        assert!(text_in_shapes(&shapes, "Unsupported panel spec"));
        disable_panels();
    }

    #[test]
    fn action_click_yields_panel_action_without_side_effects() {
        enable_panels();
        let mut store = PanelStore::default();
        store.upsert("ci", &ci_json(42));
        let ctx = crate::ui_kit::test_ctx(true);
        let mut actions_first = Vec::new();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480.0, 720.0))),
            ..Default::default()
        };
        crate::theme::test_pass(&ctx, raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                actions_first = show(ui, &mut store);
            });
        });
        assert!(actions_first.is_empty());
        let click_pos = Pos2::new(72.0, 168.0);
        let raw_click = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480.0, 720.0))),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let mut actions = Vec::new();
        crate::theme::test_pass(&ctx, raw_click, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                actions = show(ui, &mut store);
            });
        });
        assert!(
            actions
                .iter()
                .any(|a| a.panel_id == "ci" && a.action == "rerun_ci"),
            "expected rerun_ci action, got {actions:?}"
        );
        disable_panels();
    }

    #[test]
    fn setting_off_renders_nothing() {
        disable_panels();
        let mut store = PanelStore::default();
        store.upsert("ci", &ci_json(1));
        let (shapes, actions) = headless_show(&mut store);
        assert!(actions.is_empty());
        assert!(!text_in_shapes(&shapes, "PR #1"));
    }

    fn g2_marker(pr: u32) -> String {
        format!("Panel: CI status\n\n{{\n  \"panel\": {}\n}}", ci_json(pr))
    }

    #[test]
    fn upsert_from_tool_result_adds_and_updates() {
        let mut store = PanelStore::default();
        assert!(store.upsert_from_tool_result("call-1", &g2_marker(756)));
        assert_eq!(store.len(), 1);
        assert_eq!(store.pr_number("call-1"), Some(756));
        assert_eq!(store.anchor_of("call-1"), Some("call-1"));
        assert!(store.upsert_from_tool_result("call-1", &g2_marker(757)));
        assert_eq!(store.len(), 1);
        assert_eq!(store.pr_number("call-1"), Some(757));
    }

    #[test]
    fn upsert_from_tool_result_uses_panel_id_when_present() {
        let mut store = PanelStore::default();
        let body = "{\n  \"panel\": {\n    \"id\": \"panel-x\",\n    \"type\": \"ci_status\",\n    \"pr_number\": 9,\n    \"checks\": [],\n    \"actions\": []\n  }\n}";
        // id on panel is not part of controlled PanelSpec → fallback body, still upserts.
        assert!(store.upsert_from_tool_result("call-z", body));
        assert_eq!(store.len(), 1);
        assert!(store.entries.contains_key("panel-x"));
        assert_eq!(store.anchor_of("panel-x"), Some("call-z"));
    }

    #[test]
    fn upsert_from_tool_result_rejects_non_panel() {
        let mut store = PanelStore::default();
        assert!(!store.upsert_from_tool_result("c", "ok, wrote file"));
        assert!(!store.upsert_from_tool_result("c", r#"{"status":"ok"}"#));
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn upsert_from_tool_result_malformed_panel_is_fallback() {
        enable_panels();
        let mut store = PanelStore::default();
        let body = r#"{"panel": {"type":"chart","nope":1}}"#;
        assert!(store.upsert_from_tool_result("bad", body));
        assert_eq!(store.len(), 1);
        let (shapes, _) = headless_show(&mut store);
        assert!(text_in_shapes(&shapes, "Unsupported panel spec"));
        disable_panels();
    }

    #[test]
    fn show_for_anchor_filters() {
        enable_panels();
        let mut store = PanelStore::default();
        store.upsert_from_tool_result("a", &g2_marker(1));
        store.upsert_from_tool_result("b", &g2_marker(2));
        let ctx = crate::ui_kit::test_ctx(true);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480.0, 720.0))),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = show_for_anchor(ui, &mut store, "a");
            });
        });
        assert!(text_in_shapes(&out.shapes, "PR #1"));
        assert!(!text_in_shapes(&out.shapes, "PR #2"));
        disable_panels();
    }

    fn g1_core_marker() -> &'static str {
        r#"Panel: CI status

{
  "panel": {
    "title": "Linux CI",
    "elements": [
      { "type": "heading", "level": 2, "text": "Checks" },
      {
        "type": "key_value",
        "pairs": [
          { "key": "Workflow", "value": "ci" },
          { "key": "Branch", "value": "beta" }
        ]
      },
      { "type": "button", "label": "Re-run", "action": "rerun_ci" }
    ]
  }
}"#
    }

    #[test]
    fn live_g1_core_panel_renders_title() {
        enable_panels();
        let mut store = PanelStore::default();
        assert!(store.upsert_from_tool_result("live-1", g1_core_marker()));
        assert_eq!(store.len(), 1);
        let (shapes, _) = headless_show(&mut store);
        assert!(text_in_shapes(&shapes, "Linux CI"));
        assert!(text_in_shapes(&shapes, "Workflow"));
        disable_panels();
    }

    #[test]
    fn dismissed_survives_same_body_reupsert() {
        enable_panels();
        let mut store = PanelStore::default();
        assert!(store.upsert_from_tool_result("call-1", &g2_marker(756)));
        store.dismiss("call-1");
        assert_eq!(store.len(), 0);
        assert!(store.upsert_from_tool_result("call-1", &g2_marker(756)));
        assert_eq!(store.len(), 0, "same body must not clear dismissed");
        let (shapes, _) = headless_show(&mut store);
        assert!(!text_in_shapes(&shapes, "PR #756"));
        assert!(store.upsert_from_tool_result("call-1", &g2_marker(757)));
        assert_eq!(store.len(), 1, "changed body revives the panel");
        disable_panels();
    }

    #[test]
    fn bind_thread_clears_panels() {
        let mut store = PanelStore::default();
        store.bind_thread("t1");
        assert!(store.upsert_from_tool_result("c", &g2_marker(1)));
        assert_eq!(store.len(), 1);
        store.bind_thread("t1");
        assert_eq!(store.len(), 1, "same thread keeps panels");
        store.bind_thread("t2");
        assert!(store.is_empty());
    }
}
