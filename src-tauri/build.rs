use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Newest stable `vMAJOR.MINOR.PATCH` tag, by semver (the cabin's rule in
/// `crates/grokhub-app/build.rs`).
fn newest_stable_tag(tags: &str) -> Option<&str> {
    let mut best: Option<((u64, u64, u64), &str)> = None;
    for tag in tags.lines().map(str::trim) {
        let Some(rest) = tag.strip_prefix('v') else {
            continue;
        };
        let parts: Vec<&str> = rest.split('.').collect();
        let [maj, min, patch] = parts[..] else {
            continue;
        };
        let (Ok(a), Ok(b), Ok(c)) = (maj.parse(), min.parse(), patch.parse()) else {
            continue;
        };
        if best.is_none_or(|(k, _)| (a, b, c) > k) {
            best = Some(((a, b, c), tag));
        }
    }
    best.map(|(_, t)| t)
}

/// The shell's channel and beta number, the way the cabin labels its own
/// builds: `GROKHUB_CHANNEL` wins, else a `beta` checkout builds as beta, and
/// N counts commits since the newest stable tag (0 without tags). The shell
/// updater (src/shell_update.rs) compares releases against this version.
fn build_info(manifest: &Path) {
    println!("cargo:rerun-if-env-changed=GROKHUB_CHANNEL");
    let root = manifest.join("..");
    let version = root.join("VERSION");
    println!("cargo:rerun-if-changed={}", version.display());
    let base = std::fs::read_to_string(&version)
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let branch = git(&root, &["symbolic-ref", "-q", "--short", "HEAD"]).unwrap_or_default();
    let channel = match std::env::var("GROKHUB_CHANNEL")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("beta") => "beta",
        Some("stable") => "stable",
        _ if branch == "beta" => "beta",
        _ => "stable",
    };
    let n = if channel == "beta" {
        git(&root, &["tag", "-l", "v*"])
            .and_then(|tags| {
                let tag = newest_stable_tag(&tags)?.to_string();
                git(&root, &["rev-list", "--count", &format!("{tag}..HEAD")])
            })
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    } else {
        0
    };
    println!("cargo:rustc-env=GROKHUB_SHELL_BASE={base}");
    println!("cargo:rustc-env=GROKHUB_SHELL_CHANNEL={channel}");
    println!("cargo:rustc-env=GROKHUB_SHELL_BETA_N={n}");
    for w in ["HEAD", "packed-refs", "logs/HEAD"] {
        if let Some(p) = git(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-path", w],
        ) {
            if Path::new(&p).exists() {
                println!("cargo:rerun-if-changed={p}");
            }
        }
    }
}

fn main() {
    build_info(Path::new(env!("CARGO_MANIFEST_DIR")));
    tauri_build::build()
}
