//! Guard for the one type scale (docs/design/type.md): every literal text size
//! in the cabin is a step of `TYPE_SCALE`, so text doesn't drift back to 13s
//! and 15s. Fix a failure with `scripts/type_snap.py rust <file>`.

use super::{
    headline_for, line_height_for, LH_PROSE, TYPE_BADGE, TYPE_BODY, TYPE_CAPTION, TYPE_DISPLAY,
    TYPE_HEADLINE, TYPE_MONO, TYPE_PROSE, TYPE_SECTION,
};
use std::path::Path;

/// Every allowed text size, smallest first. Same steps as the shell's `--font-*`.
const TYPE_SCALE: [f32; 7] = [
    TYPE_BADGE,
    TYPE_CAPTION,
    TYPE_BODY,
    TYPE_PROSE,
    TYPE_SECTION,
    TYPE_HEADLINE,
    TYPE_DISPLAY,
];

fn on_scale(v: f32) -> bool {
    TYPE_SCALE.contains(&v)
}

fn literal(arg: &str) -> Option<f32> {
    arg.trim()
        .trim_end_matches("f32")
        .trim_end_matches('_')
        .parse::<f32>()
        .ok()
}

/// Off-scale literal sizes in `.size(N)`, `FontId::proportional(N)`,
/// `FontId::monospace(N)`, `FontId::new(N, ..)` and font-size consts.
fn off_scale_in(src: &str) -> Vec<String> {
    let mut bad = Vec::new();
    for (n, line) in src.lines().enumerate() {
        for call in [
            ".size(",
            "FontId::proportional(",
            "FontId::monospace(",
            "FontId::new(",
        ] {
            let mut rest = line;
            while let Some(i) = rest.find(call) {
                rest = &rest[i + call.len()..];
                let arg = rest.split([')', ',']).next().unwrap_or("");
                if let Some(v) = literal(arg).filter(|v| !on_scale(*v)) {
                    bad.push(format!("line {}: {call}{v})", n + 1));
                }
            }
        }
        // `const TITLE_FONT: f32 = 15.0;` and friends.
        let t = line
            .trim_start()
            .trim_start_matches("pub ")
            .trim_start_matches("(super) ");
        if let Some(decl) = t.strip_prefix("const ") {
            let name = decl.split(':').next().unwrap_or("");
            let sized = ["FONT", "TYPE_", "TEXT_SIZE", "LABEL_SIZE", "HERO"]
                .iter()
                .any(|k| name.contains(k));
            if sized && decl.contains(": f32 =") {
                let val = decl.rsplit('=').next().unwrap_or("").trim_end_matches(';');
                if let Some(v) = literal(val).filter(|v| !on_scale(*v)) {
                    bad.push(format!("line {}: const {name} = {v}", n + 1));
                }
            }
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
            if name == "theme_type_tests.rs" {
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
fn every_cabin_text_size_is_on_the_type_scale() {
    let mut bad = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut bad);
    assert!(
        bad.is_empty(),
        "off-scale text size (run scripts/type_snap.py rust):\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_guard_catches_off_scale_sizes() {
    let src = "RichText::new(t).size(13.0);\nlet f = FontId::proportional(15.0);\nconst ROW_FONT: f32 = 11.0;\nlet ok = FontId::new(14.0, FontFamily::Proportional);";
    let bad = off_scale_in(src);
    assert_eq!(
        bad,
        vec![
            "line 1: .size(13)".to_string(),
            "line 2: FontId::proportional(15)".to_string(),
            "line 3: const ROW_FONT = 11".to_string(),
        ]
    );
    assert!(off_scale_in("t.size(theme::TYPE_BODY); FontId::proportional(0.72 * s);").is_empty());
}

#[test]
fn the_scale_matches_the_shell() {
    assert_eq!(TYPE_SCALE, [10.0, 12.0, 14.0, 16.0, 20.0, 24.0, 32.0]);
    assert_eq!(TYPE_CAPTION, 12.0);
    assert_eq!(TYPE_BODY, 14.0);
    assert_eq!(TYPE_PROSE, 16.0);
    assert_eq!(TYPE_MONO, 14.0);
    assert_eq!(LH_PROSE, TYPE_PROSE * 1.5);
}

#[test]
fn every_step_has_a_line_height_on_the_4px_grid() {
    let want = [
        (12.0, 16.0),
        (14.0, 20.0),
        (16.0, 24.0),
        (20.0, 28.0),
        (24.0, 32.0),
        (32.0, 40.0),
    ];
    for (size, lh) in want {
        assert_eq!(line_height_for(size), lh, "size {size}");
    }
    assert_eq!(line_height_for(48.0), 72.0);
}

#[test]
fn page_titles_step_down_on_narrow_windows() {
    assert_eq!(headline_for(1920.0), TYPE_HEADLINE);
    assert_eq!(headline_for(900.0), 24.0);
    assert_eq!(headline_for(899.0), TYPE_SECTION);
    assert_eq!(headline_for(640.0), 20.0);
}
