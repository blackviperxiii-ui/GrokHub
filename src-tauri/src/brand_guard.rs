//! The shell takes every visible product name from `brand/brand.json`
//! (`grokhub_core::brand`), so a rename there reaches every string.

use std::path::Path;

/// Literals that may still say GrokHub, and why.
const ALLOWED: &[&str] = &[];

fn names_in_literals(src: &str, name: &str) -> Vec<String> {
    src.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//"))
        .filter(|line| {
            line.split('"')
                .skip(1)
                .step_by(2)
                .any(|lit| lit.contains(name))
        })
        .filter(|line| !ALLOWED.iter().any(|ok| line.contains(ok)))
        .map(str::to_string)
        .collect()
}

#[test]
fn shell_strings_take_the_name_from_brand_json() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("src dir") {
        let path = entry.expect("entry").path();
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Tests quote what the shell shows, and this file names what it bans.
        if !file.ends_with(".rs") || file.ends_with("_tests.rs") || file == "brand_guard.rs" {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read");
        for name in ["GrokHub", grokhub_core::brand::product_name()] {
            for line in names_in_literals(&src, name) {
                hits.push(format!("{file}: {line}"));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "hardcoded product name, use grokhub_core::brand:\n{}",
        hits.join("\n")
    );
}

#[test]
fn guard_catches_a_hardcoded_name() {
    assert_eq!(
        names_in_literals(
            "let t = \"Show GrokHub\";\n// GrokHub in a comment\n",
            "GrokHub"
        ),
        vec!["let t = \"Show GrokHub\";".to_string()]
    );
    // The models folder label takes the brand now, so it is caught too.
    assert_eq!(
        names_in_literals("Source::Folder => \"GrokHub models\",", "GrokHub").len(),
        1
    );
}

#[test]
fn tray_and_errors_use_the_brand_name() {
    assert_eq!(
        crate::remote::refused(),
        "The web app can't do that. Use Loadout on your computer."
    );
    assert_eq!(crate::credential::SERVICE, "GrokHub");
}
