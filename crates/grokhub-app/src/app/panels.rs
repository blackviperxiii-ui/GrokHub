//! Phase 1 controlled generative UI panels (CI status, build errors, compare table).
//! Renderer only: actions are returned as [`PanelAction`] intents for approval-gated callers.
#![allow(dead_code)] // API surface for G2; cabin wiring is a follow-up card.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, RichText, Ui};
use serde::{Deserialize, Serialize};

use crate::ui_kit::{badge, card_frame, BadgeTone, CardVariant};

/// Off by default until Settings owns a toggle (G3).
pub static PANELS_UI_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn panels_enabled() -> bool {
    PANELS_UI_ENABLED.load(Ordering::Relaxed)
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

#[derive(Debug, Clone)]
enum PanelBody {
    Spec(PanelSpec),
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
}

/// Live panels keyed by id (upsert updates in place).
#[derive(Debug, Default)]
pub struct PanelStore {
    order: Vec<String>,
    entries: HashMap<String, PanelEntry>,
}

impl PanelStore {
    pub fn upsert(&mut self, id: impl Into<String>, spec_json: &str) {
        let id = id.into();
        let body = parse_panel_json(spec_json);
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.body = body;
            entry.dismissed = false;
        } else {
            self.order.push(id.clone());
            self.entries.insert(
                id,
                PanelEntry {
                    body,
                    pinned: false,
                    dismissed: false,
                },
            );
        }
    }

    pub fn dismiss(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.dismissed = true;
        }
    }

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

    pub fn is_pinned(&self, id: &str) -> bool {
        self.entries.get(id).map(|e| e.pinned).unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.entries.values().filter(|e| !e.dismissed).count()
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
    match serde_json::from_value::<PanelSpec>(value) {
        Ok(spec) => PanelBody::Spec(spec),
        Err(e) => PanelBody::Fallback(format!("Unsupported panel spec: {e}")),
    }
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
        PanelBody::Fallback(_) => format!("Panel {id}"),
    }
}

/// Paint visible panels and collect action intents. No I/O or tool calls.
pub fn show(ui: &mut Ui, store: &mut PanelStore) -> Vec<PanelAction> {
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

fn paint_build_errors(ui: &mut Ui, be: &BuildErrors, panel_id: &str, actions: &mut Vec<PanelAction>) {
    for err in &be.errors {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{}:{}", err.file, err.line))
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::blend_color(crate::theme::fg(), crate::theme::bg(), 0.25)),
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

fn paint_compare_table(ui: &mut Ui, ct: &CompareTable, panel_id: &str, actions: &mut Vec<PanelAction>) {
    if ct.headers.is_empty() {
        ui.label(
            RichText::new("(empty table)")
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::fg()),
        );
        return;
    }
    egui::Grid::new(ui.id().with("compare"))
        .spacing(egui::vec2(crate::theme::SPACING_12, crate::theme::SPACING_4))
        .show(ui, |ui| {
            for h in &ct.headers {
                ui.label(
                    RichText::new(h)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::fg()),
                );
            }
            ui.end_row();
            for row in &ct.rows {
                for cell in row {
                    ui.label(
                        RichText::new(cell)
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::fg()),
                    );
                }
                ui.end_row();
            }
        });
    paint_action_row(ui, &ct.actions, panel_id, actions);
}

fn paint_action_row(ui: &mut Ui, buttons: &[PanelButton], panel_id: &str, actions: &mut Vec<PanelAction>) {
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

    fn headless_show(store: &mut PanelStore) -> (Vec<egui::epaint::ClippedShape>, Vec<PanelAction>) {
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
            actions.iter().any(|a| a.panel_id == "ci" && a.action == "rerun_ci"),
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
}
