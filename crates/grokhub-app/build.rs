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

/// Embed `assets/personalities/<id>/{1..5}.png` so dropping Grok Bot's final
/// set in place rebuilds with no Rust edits.
fn embed_personality_photos(manifest: &Path) {
    let root = manifest.join("assets/personalities");
    println!("cargo:rerun-if-changed={}", root.display());
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let dest = out.join("personality_photos.rs");
    let mut arms = String::new();
    let mut count = 0u32;
    if root.is_dir() {
        let mut ids: Vec<_> = std::fs::read_dir(&root)
            .expect("read personalities assets")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        ids.sort();
        for id in ids {
            let dir = root.join(&id);
            println!("cargo:rerun-if-changed={}", dir.display());
            for n in 1u8..=5 {
                let file = dir.join(format!("{n}.png"));
                println!("cargo:rerun-if-changed={}", file.display());
                if !file.is_file() {
                    continue;
                }
                // Absolute path: this file lives in OUT_DIR, not next to the PNGs.
                let abs = file
                    .canonicalize()
                    .unwrap_or(file)
                    .to_string_lossy()
                    .replace('\\', "/");
                arms.push_str(&format!(
                    "        (\"{id}\", {n}) => Some(include_bytes!(\"{abs}\") as &'static [u8]),\n"
                ));
                count += 1;
            }
        }
    }
    // No PNGs yet: plain `None` (avoid clippy::match_single_binding on `_ => None`).
    // Photos land later; keep the API without dead_code / unused-arg noise until then.
    let body = if count == 0 {
        "    let _ = (id, slot);\n    None\n".to_string()
    } else {
        format!(
            "    match (id, slot) {{\n\
         {arms}        _ => None,\n\
    }}\n"
        )
    };
    let src = format!(
        "/// Built-in Nyx photo slots for the Personality picker showcase.\n\
         /// Generated by build.rs from `assets/personalities/<id>/1..5.png`.\n\
         #[allow(dead_code)]\n\
         pub(super) fn personality_photo_png(id: &str, slot: u8) -> Option<&'static [u8]> {{\n\
         {body}}}\n\
         #[allow(dead_code)]\n\
         pub(super) const PERSONALITY_PHOTO_COUNT: u32 = {count};\n"
    );
    std::fs::write(&dest, src).expect("write personality_photos.rs");
}

fn main() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    build_info(&manifest);
    embed_personality_photos(&manifest);
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
