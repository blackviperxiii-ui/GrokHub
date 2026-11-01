//! `show_panel`: validate and return a generative-UI [`PanelSpec`] (G1). No render or I/O.

use std::path::Path;

use grokhub_core::panel_spec::{panel_spec_json_schema, parse_panel_spec, PanelElement, PanelSpec};
use serde_json::{json, Value};

use super::ToolOutput;

pub const SHOW_PANEL: &str = "show_panel";

const CONFIG_NAME: &str = "generative_ui.toml";

static NOT_ENABLED: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    grokhub_core::brand::filled("show_panel is not enabled; set enabled = true in generative_ui.toml under the {product} config folder")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    pub enabled: bool,
}

pub fn config_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(CONFIG_NAME)
}

pub fn settings(config_dir: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(config_path(config_dir)) else {
        return Settings::default();
    };
    parse_file(&text)
}

pub fn enabled(config_dir: &Path) -> bool {
    settings(config_dir).enabled
}

fn parse_file(text: &str) -> Settings {
    let mut enabled = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("enabled") && value.trim().eq_ignore_ascii_case("true") {
            enabled = true;
        }
    }
    Settings { enabled }
}

pub fn schema() -> Value {
    let panel = panel_spec_json_schema();
    json!({
        "type": "function",
        "name": SHOW_PANEL,
        "description": "Emit a validated generative UI panel specification for the cabin to paint later. \
            Plain text only in panel fields; buttons name action ids (intents), never URLs or shell. \
            This tool does not render, write files, or run actions.",
        "parameters": {
            "type": panel["type"],
            "additionalProperties": panel["additionalProperties"],
            "properties": panel["properties"],
            "required": panel["required"],
            "$defs": panel["$defs"],
        }
    })
}

pub fn run(args: &Value) -> ToolOutput {
    if !enabled(&crate::perm::config_dir()) {
        return ToolOutput::err(*NOT_ENABLED);
    }
    let raw = match serde_json::to_string(args) {
        Ok(s) => s,
        Err(err) => return ToolOutput::err(format!("panel arguments are not JSON: {err}")),
    };
    let spec = match parse_panel_spec(&raw) {
        Ok(spec) => spec,
        Err(err) => return ToolOutput::err(format!("invalid panel spec: {err}")),
    };
    let panel_value = serde_json::to_value(&spec).expect("PanelSpec serializes");
    let wrapped = json!({ "panel": panel_value });
    let summary = summarize(&spec);
    let body = serde_json::to_string_pretty(&wrapped).unwrap_or_else(|_| wrapped.to_string());
    ToolOutput::ok(format!("{summary}\n\n{body}"))
}

fn summarize(spec: &PanelSpec) -> String {
    if let Some(title) = &spec.title {
        let t = title.trim();
        if !t.is_empty() {
            return format!("Panel: {t}");
        }
    }
    for el in &spec.elements {
        match el {
            PanelElement::Heading(h) => {
                let t = h.text.trim();
                if !t.is_empty() {
                    return format!("Panel: {t}");
                }
            }
            PanelElement::Text(t) => {
                let s = t.text.trim();
                if !s.is_empty() {
                    let short: String = s.chars().take(80).collect();
                    return format!("Panel: {short}");
                }
            }
            _ => {}
        }
    }
    "Panel".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{Gate, PermMode};
    use crate::perm::ConfigGuard;
    use crate::tools::registry;
    use crate::tools::{dispatch, schemas_for, ToolCtx};
    use serde_json::json;
    use std::path::Path;

    fn chat() -> Gate {
        Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: false,
        }
    }

    fn setup_config(dir: &Path, generative: bool, deferred: bool) -> ConfigGuard {
        if generative {
            std::fs::write(config_path(dir), "enabled = true\n").unwrap();
        } else {
            let _ = std::fs::remove_file(config_path(dir));
        }
        std::fs::write(
            registry::config_path(dir),
            format!("enabled = {deferred}\n"),
        )
        .unwrap();
        ConfigGuard::set(dir)
    }

    fn ctx(workspace: &Path) -> ToolCtx<'_> {
        ToolCtx {
            workspace,
            desktop: None,
            stop: &|| false,
            tasks: None,
            owner: None,
            session: Some("show-panel-test"),
        }
    }

    fn ci_status_panel() -> Value {
        json!({
            "title": "CI status",
            "elements": [
                { "type": "heading", "level": 2, "text": "Linux CI" },
                {
                    "type": "key_value",
                    "pairs": [
                        { "key": "Workflow", "value": "ci" },
                        { "key": "Branch", "value": "beta" }
                    ]
                },
                { "type": "progress", "label": "Tests", "value": 0.85 }
            ]
        })
    }

    fn build_errors_panel() -> Value {
        json!({
            "title": "Build errors",
            "elements": [
                { "type": "text", "text": "Two crates failed to compile." },
                {
                    "type": "list",
                    "ordered": false,
                    "items": ["error[E0425] in grokhub-agent", "error[E0308] in grokhub-core"]
                }
            ]
        })
    }

    fn compare_table_panel() -> Value {
        json!({
            "title": "Compare",
            "elements": [
                {
                    "type": "table",
                    "headers": ["Item", "Before", "After"],
                    "rows": [
                        ["latency", "120ms", "95ms"],
                        ["errors", "2", "0"]
                    ]
                }
            ]
        })
    }

    #[test]
    fn settings_default_off_and_file_enables() {
        assert!(!parse_file("").enabled);
        assert!(parse_file("enabled = true").enabled);
        let dir = crate::harness::test_dir("generative-ui-default");
        let _ = std::fs::remove_file(config_path(&dir));
        assert!(!settings(&dir).enabled);
        std::fs::write(config_path(&dir), "enabled = true\n").unwrap();
        assert!(settings(&dir).enabled);
    }

    #[test]
    fn absent_when_generative_ui_off() {
        let dir = crate::harness::test_dir("show-panel-absent");
        let workspace = dir.join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cfg = setup_config(&dir, false, false);
        let gate = chat();
        let tools = schemas_for(&gate);
        let names: Vec<_> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(!names.contains(&SHOW_PANEL));

        let _cfg2 = setup_config(&dir, false, true);
        registry::begin_turn("show-panel-test");
        let tools_deferred = schemas_for(&gate);
        let names_deferred: Vec<_> = tools_deferred
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(!names_deferred.contains(&SHOW_PANEL));
        let index = tools_deferred
            .iter()
            .find(|t| t["name"] == registry::LOAD_TOOL_SCHEMA)
            .and_then(|t| t["description"].as_str())
            .unwrap_or("");
        assert!(!index.contains(SHOW_PANEL));
    }

    #[test]
    fn dispatch_not_enabled_when_off() {
        let dir = crate::harness::test_dir("show-panel-dispatch-off");
        let workspace = dir.join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cfg = setup_config(&dir, false, false);
        let out = dispatch(&ctx(&workspace), SHOW_PANEL, &ci_status_panel().to_string());
        assert!(out.failed);
        assert!(out.text.contains("not enabled"));
    }

    #[test]
    fn listed_when_on_and_deferred_off() {
        let dir = crate::harness::test_dir("show-panel-listed");
        let _cfg = setup_config(&dir, true, false);
        let gate = chat();
        let tools = schemas_for(&gate);
        let names: Vec<_> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&SHOW_PANEL));
    }

    #[test]
    fn deferred_index_and_load_when_on() {
        let dir = crate::harness::test_dir("show-panel-deferred");
        let _cfg = setup_config(&dir, true, true);
        let gate = chat();
        registry::begin_turn("show-panel-deferred");
        let before = schemas_for(&gate);
        assert!(
            !before.iter().any(|t| t["name"] == SHOW_PANEL),
            "full schema must stay deferred"
        );
        let index = before
            .iter()
            .find(|t| t["name"] == registry::LOAD_TOOL_SCHEMA)
            .and_then(|t| t["description"].as_str())
            .unwrap_or("");
        assert!(index.contains(SHOW_PANEL));

        let stable_len = before.len();
        let out = registry::run_load_tool_schema(&json!({ "name": SHOW_PANEL }));
        assert!(!out.failed, "{}", out.text);
        assert!(out.text.contains("\"show_panel\""));

        let after = schemas_for(&gate);
        assert_eq!(after.len(), stable_len + 1);
        assert_eq!(after.last().unwrap()["name"], SHOW_PANEL);
        assert_eq!(&after[0..stable_len], &before[..]);
    }

    #[test]
    fn three_panel_shapes_validate_and_name_item() {
        let dir = crate::harness::test_dir("show-panel-roundtrip");
        let workspace = dir.join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cfg = setup_config(&dir, true, false);
        for (panel, want_name) in [
            (ci_status_panel(), "CI status"),
            (build_errors_panel(), "Build errors"),
            (compare_table_panel(), "Compare"),
        ] {
            let out = dispatch(&ctx(&workspace), SHOW_PANEL, &panel.to_string());
            assert!(!out.failed, "{}", out.text);
            assert!(out.text.contains(want_name), "{}", out.text);
            assert!(out.text.contains("\"panel\""));
            let marker_start = out.text.find('{').expect("json marker");
            let parsed: Value = serde_json::from_str(&out.text[marker_start..]).unwrap();
            assert!(parsed.get("panel").is_some());
            let round =
                parse_panel_spec(&serde_json::to_string(parsed.get("panel").unwrap()).unwrap())
                    .unwrap();
            let again = serde_json::to_value(&round).unwrap();
            assert_eq!(parsed["panel"], again);
        }
    }

    #[test]
    fn invalid_panel_fails_without_panic() {
        let dir = crate::harness::test_dir("show-panel-invalid");
        let workspace = dir.join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cfg = setup_config(&dir, true, false);

        let out = dispatch(
            &ctx(&workspace),
            SHOW_PANEL,
            r#"{"elements":[{"type":"chart","text":"nope"}]}"#,
        );
        assert!(out.failed);
        assert!(out.text.contains("invalid panel spec"));

        let out = dispatch(
            &ctx(&workspace),
            SHOW_PANEL,
            r#"{"title":"x","elements":[{"type":"text","text":"<b>html</b>"}]}"#,
        );
        assert!(out.failed);

        let out = dispatch(&ctx(&workspace), SHOW_PANEL, r#"{"title":"no elements"}"#);
        assert!(out.failed);
        assert!(out.text.contains("invalid panel spec"));
    }

    #[test]
    fn run_does_not_touch_workspace() {
        let dir = crate::harness::test_dir("show-panel-no-fs");
        let workspace = dir.join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cfg = setup_config(&dir, true, false);
        let before: Vec<_> = std::fs::read_dir(&workspace)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let _ = dispatch(&ctx(&workspace), SHOW_PANEL, &ci_status_panel().to_string());
        let after: Vec<_> = std::fs::read_dir(&workspace)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after);
    }
}
