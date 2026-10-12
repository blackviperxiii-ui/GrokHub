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

/// Newest stable `vMAJOR.MINOR.PATCH` tag (no prerelease suffix), by semver.
fn newest_stable_tag(tags: &str) -> Option<&str> {
    let mut best: Option<(u64, u64, u64, &str)> = None;
    for tag in tags.lines().map(str::trim).filter(|t| !t.is_empty()) {
        let Some(rest) = tag.strip_prefix('v') else {
            continue;
        };
        let mut parts = rest.split('.');
        let (Some(maj), Some(min), Some(patch_tok)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if parts.next().is_some() || !patch_tok.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let (Ok(major), Ok(minor), Ok(patch)) = (
            maj.parse::<u64>(),
            min.parse::<u64>(),
            patch_tok.parse::<u64>(),
        ) else {
            continue;
        };
        let key = (major, minor, patch);
        match best {
            Some((a, b, c, _)) if key <= (a, b, c) => {}
            _ => best = Some((major, minor, patch, tag)),
        }
    }
    best.map(|(_, _, _, t)| t)
}

/// Beta N = commits on HEAD since the newest stable `vX.Y.Z` tag (`git
/// rev-list --count <tag>..HEAD`). Deterministic on any clone that has that
/// tag. Shallow clones without tags fall back to `0` (`X.Y.Z-beta.0`); fetch
/// tags (`git fetch --tags`) for the real count.
fn beta_n(root: &Path) -> u64 {
    let tags = match git(root, &["tag", "-l", "v*"]) {
        Some(t) => t,
        None => return 0,
    };
    let Some(tag) = newest_stable_tag(&tags) else {
        return 0;
    };
    let range = format!("{tag}..HEAD");
    git(root, &["rev-list", "--count", &range])
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Channel, branch, short SHA, and beta N for `grokhub --version`, read from
/// git at build time. `GROKHUB_CHANNEL=beta|stable` (set by `scripts/install.sh
/// --channel`) wins; otherwise a `beta` branch builds as beta. The Cargo
/// version is never touched. Beta labels are `X.Y.Z-beta.N` (see `beta_n`).
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
    let n = if channel == "beta" { beta_n(&root) } else { 0 };
    println!("cargo:rustc-env=GROKHUB_BUILD_CHANNEL={channel}");
    println!("cargo:rustc-env=GROKHUB_BUILD_BRANCH={branch}");
    println!("cargo:rustc-env=GROKHUB_BUILD_SHA={sha}");
    println!("cargo:rustc-env=GROKHUB_BUILD_BETA_N={n}");
    // Rebuild the label when HEAD or tags move. The reflog is appended on every
    // commit, checkout, and pull, so it catches a pull that writes a loose ref
    // after `git gc` packed the branch ref. `packed-refs` covers tag updates.
    let mut watch = vec![
        "HEAD".to_string(),
        "packed-refs".to_string(),
        "logs/HEAD".to_string(),
    ];
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
