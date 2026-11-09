//! Guard for the one spacing scale (docs/design/spacing.md): every literal gap in
//! the cabin sits on it, so pages line up and nothing drifts back to 6s and 10s.
//! Fix a failure with `scripts/spacing_snap.py rust <file>`.

use super::{page_margin_for, NARROW_WINDOW_W, PAGE_MARGIN, PAGE_MARGIN_NARROW};
use std::path::Path;

/// Spacing steps in px. 0 and 1 (hairlines) are allowed too.
const SCALE: [f32; 14] = [
    0.0, 1.0, 2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0, 40.0, 48.0, 64.0, 80.0, 96.0,
];

fn on_scale(v: f32) -> bool {
    let a = v.abs();
    a > 96.0 || SCALE.contains(&a)
}

/// The numbers inside `call(` ... `)` when they are plain literals.
fn literal_args(src: &str, call: &str) -> Vec<(usize, Vec<f32>)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find(call) {
        let start = from + i + call.len();
        from = start;
        let Some(end) = src[start..].find(')') else {
            break;
        };
        let args = &src[start..start + end];
        let nums: Option<Vec<f32>> = args
            .split(',')
            .map(|a| a.trim().trim_end_matches("f32").parse::<f32>().ok())
            .collect();
        if let Some(nums) = nums {
            let line = src[..start].matches('\n').count() + 1;
            out.push((line, nums));
        }
    }
    out
}

/// `item_spacing.x = 6.0;` style assignments.
fn literal_assigns(src: &str) -> Vec<(usize, f32)> {
    let mut out = Vec::new();
    for (n, line) in src.lines().enumerate() {
        for axis in ["item_spacing.x", "item_spacing.y"] {
            if let Some(i) = line.find(axis) {
                let rest = line[i + axis.len()..].trim_start();
                if let Some(v) = rest.strip_prefix('=') {
                    if let Ok(v) = v.trim().trim_end_matches(';').trim().parse::<f32>() {
                        out.push((n + 1, v));
                    }
                }
            }
        }
    }
    out
}

fn off_scale_in(src: &str) -> Vec<String> {
    let mut bad = Vec::new();
    for call in [
        "add_space(",
        "Margin::same(",
        "Margin::symmetric(",
        "item_spacing = egui::vec2(",
    ] {
        for (line, nums) in literal_args(src, call) {
            if nums.iter().any(|v| !on_scale(*v)) {
                bad.push(format!("line {line}: {call}{nums:?})"));
            }
        }
    }
    for (line, v) in literal_assigns(src) {
        if !on_scale(v) {
            bad.push(format!("line {line}: item_spacing = {v}"));
        }
    }
    bad
}

fn walk(dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("read src dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "theme_spacing_tests.rs" {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read source");
            for b in off_scale_in(&src) {
                out.push(format!("{}: {b}", path.display()));
            }
        }
    }
}

#[test]
fn every_cabin_gap_is_on_the_spacing_scale() {
    let mut bad = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut bad);
    assert!(
        bad.is_empty(),
        "off-scale spacing (run scripts/spacing_snap.py rust):\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_guard_catches_off_scale_gaps() {
    let src = "ui.add_space(6.0);\nlet m = egui::Margin::symmetric(10, 4);\nui.spacing_mut().item_spacing.x = 5.0;\nui.add_space(8.0);";
    let bad = off_scale_in(src);
    assert_eq!(bad.len(), 3, "{bad:?}");
    assert!(bad[0].starts_with("line 1: add_space("));
    assert!(bad[1].starts_with("line 2: Margin::symmetric("));
    assert_eq!(bad[2], "line 3: item_spacing = 5");
    assert!(off_scale_in("ui.add_space(x * 2.0); ui.add_space(theme::SPACING_8);").is_empty());
}

#[test]
fn pages_tighten_on_narrow_windows() {
    assert_eq!(PAGE_MARGIN, 24.0);
    assert_eq!(PAGE_MARGIN_NARROW, 16.0);
    assert_eq!(NARROW_WINDOW_W, 900.0);
    assert_eq!(page_margin_for(1920.0), 24.0);
    assert_eq!(page_margin_for(900.0), 24.0);
    assert_eq!(page_margin_for(899.0), 16.0);
    assert_eq!(page_margin_for(640.0), 16.0);
}
