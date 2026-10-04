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

/// Channel, branch, and short SHA for `grokhub --version`, read from git at
/// build time. `GROKHUB_CHANNEL=beta|stable` (set by `scripts/install.sh
/// --channel`) wins; otherwise a `beta` branch builds as beta. The Cargo
/// version is never touched, so the lockstep version stays 2.10.x.
fn build_info(manifest: &Path) {
    println!("cargo:rerun-if-env-changed=GROKHUB_CHANNEL");
    let root = manifest.join("../..");
    let branch = git(&root, &["symbolic-ref", "-q", "--short", "HEAD"])
        .or_else(|| git(&root, &["describe", "--tags", "--exact-match", "HEAD"]))
        .or_else(|| git(&root, &["rev-parse", "--short=7", "HEAD"]).map(|_| "detached".into()))
        .unwrap_or_default();
    let sha = git(&root, &["rev-parse", "--short=7", "HEAD"]).unwrap_or_default();
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
    println!("cargo:rustc-env=GROKHUB_BUILD_CHANNEL={channel}");
    println!("cargo:rustc-env=GROKHUB_BUILD_BRANCH={branch}");
    println!("cargo:rustc-env=GROKHUB_BUILD_SHA={sha}");
    // Rebuild the label when HEAD moves (branch switch or new commit).
    let mut watch = vec!["HEAD".to_string(), "packed-refs".to_string()];
    if let Some(r) = git(&root, &["symbolic-ref", "-q", "HEAD"]) {
        watch.push(r);
    }
    for w in watch {
        if let Some(p) = git(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-path", &w],
        ) {
            if Path::new(&p).exists() {
                println!("cargo:rerun-if-changed={p}");
            }
        }
    }
}

fn main() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    build_info(&manifest);
    let ico = manifest.join("../../packaging/windows/grokhub.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico.to_str().expect("grokhub.ico path"));
    res.set("ProductName", "GrokHub");
    res.set("FileDescription", "GrokHub");
    res.compile().expect("embed grokhub.ico into grokhub.exe");
}
