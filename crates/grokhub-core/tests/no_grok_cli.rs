//! Guard for the break from the Grok Build CLI.
//!
//! GrokHub runs its own engine and OAuth. The one exception is
//! `crates/grokhub-app/src/google_mcp_via_cli.rs`, which may look for an
//! installed CLI so Google's MCP connectors can use it. This test fails when:
//! - non-test Rust code outside that module spawns, locates, installs or
//!   updates the CLI, or reads its files,
//! - packaging or the install/release scripts install or bundle the CLI,
//! - a deleted CLI installer comes back.
//!
//! Hermetic: reads files under the repo root only.

use std::fs;
use std::path::{Path, PathBuf};

/// The only Rust file allowed to look for the CLI.
const ALLOWED: &str = "crates/grokhub-app/src/google_mcp_via_cli.rs";

/// Text that only CLI plumbing needs.
const RUST_BANNED: &[&str] = &[
    "Command::new(\"grok",
    "\"grok.exe\"",
    "GROKHUB_GROK",
    ".grok/bin",
    ".grok\\\\bin",
    "grok update",
    "x.ai/cli/install",
    "install-grok",
];

/// Text that means a package or script installs or ships the CLI.
const PACKAGING_BANNED: &[&str] = &[
    "install-grok",
    "x.ai/cli",
    "grok-build-public-artifacts",
    "grok.exe",
    "GROK_CHANNEL",
    "grok update",
    ".grok/bin",
    ".grok\\bin",
];

const PACKAGING_FILES: &[&str] = &[
    "packaging/PKGBUILD",
    "packaging/aur/PKGBUILD",
    "packaging/aur/.SRCINFO",
    "packaging/aur/grokhub.install",
    "packaging/windows/grokhub.iss",
    "scripts/install.sh",
    "scripts/install-windows.ps1",
    "scripts/make-release-bundle.sh",
    "scripts/make-windows-release.ps1",
];

const DELETED: &[&str] = &[
    "scripts/install-grok-cli.sh",
    "scripts/grok-windows-artifact.ps1",
    "packaging/windows/install-grok-alpha.ps1",
    "crates/grokhub-acp",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Test files and in-file test modules may name the CLI to assert it is gone.
fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    rel.contains("/tests/") || name == "tests.rs" || name.ends_with("_tests.rs")
}

/// The file up to its first `#[cfg(test)]`, where the test module starts.
fn shipped_part(src: &str) -> &str {
    src.find("#[cfg(test)]").map_or(src, |at| &src[..at])
}

fn rust_hits(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let mut hits = Vec::new();
    for path in files {
        let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        if rel == ALLOWED || is_test_file(&rel) || rel.contains("/target") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read source");
        for (n, line) in shipped_part(&src).lines().enumerate() {
            for pat in RUST_BANNED {
                if line.contains(pat) {
                    hits.push(format!("{rel}:{}: {pat}", n + 1));
                }
            }
        }
    }
    hits
}

fn packaging_hits(root: &Path) -> Vec<String> {
    let mut hits = Vec::new();
    for rel in PACKAGING_FILES {
        let src = fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        for (n, line) in src.lines().enumerate() {
            for pat in PACKAGING_BANNED {
                if line.contains(pat) {
                    hits.push(format!("{rel}:{}: {pat}", n + 1));
                }
            }
        }
    }
    hits
}

#[test]
fn only_the_google_module_looks_for_the_grok_cli() {
    let root = repo_root();
    let hits = rust_hits(&root);
    assert!(hits.is_empty(), "CLI plumbing outside {ALLOWED}:\n{}", hits.join("\n"));
    let allowed = fs::read_to_string(root.join(ALLOWED)).expect("google module");
    assert!(
        allowed.contains("GROKHUB_GROK") && !shipped_part(&allowed).contains("Command::new"),
        "the Google module only locates the CLI and never runs it"
    );
}

#[test]
fn packaging_never_installs_the_grok_cli() {
    let root = repo_root();
    let hits = packaging_hits(&root);
    assert!(hits.is_empty(), "packaging still ships the Grok Build CLI:\n{}", hits.join("\n"));
    for rel in DELETED {
        assert!(!root.join(rel).exists(), "{rel} was deleted with the CLI and must stay gone");
    }
}

#[test]
fn guard_flags_a_cli_spawn_and_skips_test_modules() {
    assert_eq!(shipped_part("fn a() {}\n#[cfg(test)]\nmod tests { grok update }"), "fn a() {}\n");
    assert!(is_test_file("crates/grokhub-app/src/app/tests.rs"));
    assert!(is_test_file("crates/grokhub-agent/src/route/r2a_tests.rs"));
    assert!(!is_test_file("crates/grokhub-app/src/main.rs"));
    let line = "    let child = std::process::Command::new(\"grok\").arg(\"-p\");";
    assert_eq!(RUST_BANNED.iter().filter(|p| line.contains(*p)).count(), 1);
}
