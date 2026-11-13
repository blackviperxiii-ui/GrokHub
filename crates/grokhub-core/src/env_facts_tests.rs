use super::*;

const DAY: u64 = 24 * 60 * 60 * 1000;

fn ws(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gh-env-facts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_check_confirms_contradicts_and_retires() {
    let mut s = Store::default();
    let at = |v: &'static str| Some((v, "Cargo.toml"));
    assert_eq!(
        s.check(Kind::TestRunner, "ws:1", at("cargo test"), DAY),
        (Outcome::New, true)
    );
    // Same reading within a day: confirmed, nothing to save.
    assert_eq!(
        s.check(Kind::TestRunner, "ws:1", at("cargo test"), DAY + 5),
        (Outcome::Confirmed, false)
    );
    assert_eq!(
        s.check(Kind::TestRunner, "ws:1", at("cargo test"), 3 * DAY),
        (Outcome::Confirmed, true)
    );
    // A check that can't tell changes nothing.
    assert_eq!(
        s.check(Kind::TestRunner, "ws:1", None, 4 * DAY),
        (Outcome::Unknown, false)
    );
    let (out, _) = s.check(Kind::TestRunner, "ws:1", at("cargo nextest run"), 5 * DAY);
    assert_eq!(
        out,
        Outcome::Contradicted {
            old: "cargo test".into()
        }
    );
    let old = s.facts.iter().find(|f| f.value == "cargo test").unwrap();
    assert_eq!(old.retired_ms, Some(5 * DAY));
    assert_eq!(old.contradicted_by.as_deref(), Some("cargo nextest run"));
    let shown = s.shown("ws:1", 5 * DAY);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].value, "cargo nextest run");
    // Other projects don't see it; a stale fact stops showing.
    assert!(s.shown("ws:2", 5 * DAY).is_empty());
    assert!(s
        .shown("ws:1", 5 * DAY + (MAX_AGE_DAYS + 1) * DAY)
        .is_empty());
}

#[test]
fn project_files_name_the_commands() {
    let rust = ws("rust");
    std::fs::write(rust.join("Cargo.toml"), "[package]\n").unwrap();
    assert_eq!(detect_test_runner(&rust).unwrap().0, "cargo test");
    std::fs::create_dir_all(rust.join(".config")).unwrap();
    std::fs::write(rust.join(".config/nextest.toml"), "").unwrap();
    assert_eq!(detect_test_runner(&rust).unwrap().0, "cargo nextest run");
    assert_eq!(detect_build_command(&rust).unwrap().0, "cargo build");

    let js = ws("js");
    std::fs::write(
        js.join("package.json"),
        r#"{"scripts":{"test":"vitest run","build":"vite build"}}"#,
    )
    .unwrap();
    assert_eq!(detect_test_runner(&js).unwrap().0, "npm test");
    std::fs::write(js.join("pnpm-lock.yaml"), "").unwrap();
    assert_eq!(detect_test_runner(&js).unwrap().0, "pnpm run test");
    assert_eq!(detect_build_command(&js).unwrap().0, "pnpm run build");

    let placeholder = ws("placeholder");
    std::fs::write(
        placeholder.join("package.json"),
        r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
    )
    .unwrap();
    assert_eq!(detect_test_runner(&placeholder), None);

    let py = ws("py");
    std::fs::write(py.join("pyproject.toml"), "[tool.pytest.ini_options]\n").unwrap();
    std::fs::write(py.join("uv.lock"), "").unwrap();
    assert_eq!(detect_test_runner(&py).unwrap().0, "uv run pytest");
    for d in [rust, js, placeholder, py] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn observing_a_project_saves_only_changes_and_builds_the_block() {
    let cfg = ws("cfg");
    let proj = ws("proj");
    std::fs::write(proj.join("go.mod"), "module x\n").unwrap();
    let facts = observe_workspace(&cfg, &proj, DAY);
    assert_eq!(facts.len(), 2);
    assert_eq!(
        observe_machine(
            &cfg,
            Kind::Gpu,
            Some(("NVIDIA GeForce RTX 4070", "hardware probe")),
            DAY
        ),
        Outcome::New
    );
    let facts = observe_workspace(&cfg, &proj, DAY + 1);
    let block = prompt_block(&facts);
    assert!(
        block.contains("- Tests: `go test ./...` (from go.mod)"),
        "{block}"
    );
    assert!(block.contains("- GPU: NVIDIA GeForce RTX 4070"), "{block}");
    assert!(!block.contains("2026"), "no dates in the block");
    // The project changes: the old facts retire.
    std::fs::remove_file(proj.join("go.mod")).unwrap();
    std::fs::write(proj.join("Cargo.toml"), "[package]\n").unwrap();
    let facts = observe_workspace(&cfg, &proj, 2 * DAY);
    assert!(facts.iter().any(|f| f.value == "cargo test"));
    assert!(load(&cfg)
        .facts
        .iter()
        .any(|f| f.value == "go test ./..." && !f.active()));
    assert_eq!(prompt_block(&[]), "");
    for d in [cfg, proj] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn the_gpu_reading_prefers_the_biggest_discrete_card() {
    use crate::local_profile::{Gpu, GpuVendor};
    let gpu = |name: &str, vram_mb, integrated| Gpu {
        vendor: GpuVendor::Other,
        name: name.into(),
        vram_mb,
        free_vram_mb: None,
        integrated,
        unified: false,
        driver: String::new(),
    };
    let both = [
        gpu("Intel UHD", 0, true),
        gpu("RTX 4070", 12 * 1024, false),
        gpu("GTX 1650", 4096, false),
    ];
    assert_eq!(gpu_reading(&both).as_deref(), Some("RTX 4070, 12 GB VRAM"));
    assert_eq!(
        gpu_reading(&[gpu("Intel UHD", 0, true)]).as_deref(),
        Some("Intel UHD (integrated)")
    );
    assert_eq!(gpu_reading(&[gpu(" ", 4096, false)]), None);
    assert_eq!(gpu_reading(&[]), None);
}
