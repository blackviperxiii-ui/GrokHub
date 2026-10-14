//! Release-train helpers for GrokHub.
//!
//! Feature PRs add a fragment under `changelog.d/`. A promotion or hotfix
//! owns the version files via `cargo xtask bump <X.Y.Z>`. CI calls
//! `cargo xtask bump --check` so every copy of the version stays in lockstep.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let root = workspace_root();
    match args.as_slice() {
        [cmd] if cmd == "bump" => {
            eprintln!("usage: cargo xtask bump --check | cargo xtask bump <X.Y.Z>");
            process::exit(2);
        }
        [cmd, flag] if cmd == "bump" && flag == "--check" => {
            if let Err(e) = bump_check(&root) {
                eprintln!("xtask bump --check: {e}");
                process::exit(1);
            }
            println!("xtask bump --check: ok ({})", read_version(&root).unwrap());
        }
        [cmd, ver] if cmd == "bump" => {
            if let Err(e) = bump_apply(&root, ver) {
                eprintln!("xtask bump {ver}: {e}");
                process::exit(1);
            }
            println!("xtask bump: wrote {ver} and folded changelog.d into CHANGELOG.md");
        }
        _ => {
            eprintln!("usage: cargo xtask bump --check | cargo xtask bump <X.Y.Z>");
            process::exit(2);
        }
    }
}

fn workspace_root() -> PathBuf {
    if let Ok(manifest) = env::var("CARGO_MANIFEST_DIR") {
        let xtask = PathBuf::from(manifest);
        if let Some(parent) = xtask.parent() {
            return parent.to_path_buf();
        }
    }
    env::current_dir().expect("cwd")
}

fn read_version(root: &Path) -> Result<String, String> {
    let raw = fs::read_to_string(root.join("VERSION")).map_err(|e| format!("VERSION: {e}"))?;
    let v = raw.trim().to_string();
    if v.is_empty() || !is_semver(&v) {
        return Err(format!("VERSION is not semver: {v:?}"));
    }
    Ok(v)
}

fn is_semver(v: &str) -> bool {
    let parts: Vec<_> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn bump_check(root: &Path) -> Result<(), String> {
    let version = read_version(root)?;
    check_cargo_toml(root, &version)?;
    check_contains(
        root,
        "packaging/PKGBUILD",
        &format!("pkgver={version}"),
    )?;
    check_contains(
        root,
        "packaging/aur/PKGBUILD",
        &format!("pkgver={version}"),
    )?;
    check_contains(
        root,
        "packaging/windows/grokhub.iss",
        &format!("#define MyAppVersion \"{version}\""),
    )?;
    check_contains(
        root,
        "crates/grokhub-app/src/cli.rs",
        &format!("assert_eq!(env!(\"CARGO_PKG_VERSION\"), \"{version}\");"),
    )?;
    check_reference(root, &version)?;
    check_cargo_lock(root, &version)?;
    Ok(())
}

fn check_cargo_toml(root: &Path, version: &str) -> Result<(), String> {
    let text = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    let needle = format!("version = \"{version}\"");
    // First version under [workspace.package] — require it appears before [workspace.dependencies]
    let pkg = text
        .split("[workspace.package]")
        .nth(1)
        .and_then(|s| s.split("[workspace.dependencies]").next())
        .ok_or_else(|| "Cargo.toml missing [workspace.package]".to_string())?;
    if !pkg.contains(&needle) {
        return Err(format!(
            "Cargo.toml [workspace.package] version is not {version}"
        ));
    }
    Ok(())
}

fn check_contains(root: &Path, rel: &str, needle: &str) -> Result<(), String> {
    let text = fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    if !text.contains(needle) {
        return Err(format!("{rel} missing {needle:?}"));
    }
    Ok(())
}

fn check_reference(root: &Path, version: &str) -> Result<(), String> {
    let text =
        fs::read_to_string(root.join("docs/REFERENCE.md")).map_err(|e| format!("REFERENCE.md: {e}"))?;
    check_reference_text(&text, version)
}

/// Headline + both Latest cells use `**vX.Y.Z**`; `--version` examples use
/// `GrokHub X.Y.Z-beta…` and `GrokHub X.Y.Z (main…)`.
fn check_reference_text(text: &str, version: &str) -> Result<(), String> {
    let bold = format!("**v{version}**");
    let bold_n = text.matches(&bold).count();
    if bold_n < 3 {
        return Err(format!(
            "docs/REFERENCE.md: expected >=3 {bold:?} (headline + Latest rows), found {bold_n}"
        ));
    }
    for (needle, label) in [
        (
            format!("grokhub-linux-v{version}.tar.gz"),
            "Linux tarball",
        ),
        (format!("GrokHub-Setup-{version}.exe"), "Windows Setup"),
        (
            format!("grokhub-windows-v{version}.zip"),
            "Windows zip",
        ),
        (
            format!("GrokHub {version}-beta."),
            "--version beta example",
        ),
        (
            format!("GrokHub {version} (main"),
            "--version main example",
        ),
    ] {
        if !text.contains(&needle) {
            return Err(format!("docs/REFERENCE.md missing {label}: {needle:?}"));
        }
    }
    Ok(())
}

fn check_cargo_lock(root: &Path, version: &str) -> Result<(), String> {
    let text = fs::read_to_string(root.join("Cargo.lock")).map_err(|e| e.to_string())?;
    for name in [
        "grokhub-app",
        "grokhub-agent",
        "grokhub-core",
        "grokhub-hub",
    ] {
        let block = format!("name = \"{name}\"\nversion = \"{version}\"");
        if !text.contains(&block) {
            return Err(format!(
                "Cargo.lock package {name} is not version {version}"
            ));
        }
    }
    Ok(())
}

fn bump_apply(root: &Path, new_version: &str) -> Result<(), String> {
    if !is_semver(new_version) {
        return Err(format!("not semver X.Y.Z: {new_version}"));
    }
    let old = read_version(root)?;
    if old == new_version {
        return Err(format!("already at {new_version}; nothing to bump"));
    }
    bump_check(root).map_err(|e| format!("refusing to bump while --check fails: {e}"))?;

    write_str(root, "VERSION", &format!("{new_version}\n"))?;
    replace_workspace_version(root, &old, new_version)?;
    replace_in(
        root,
        "packaging/PKGBUILD",
        &format!("pkgver={old}"),
        &format!("pkgver={new_version}"),
    )?;
    replace_in(
        root,
        "packaging/aur/PKGBUILD",
        &format!("pkgver={old}"),
        &format!("pkgver={new_version}"),
    )?;
    replace_in(
        root,
        "packaging/windows/grokhub.iss",
        &format!("#define MyAppVersion \"{old}\""),
        &format!("#define MyAppVersion \"{new_version}\""),
    )?;
    replace_in(
        root,
        "crates/grokhub-app/src/cli.rs",
        &format!("assert_eq!(env!(\"CARGO_PKG_VERSION\"), \"{old}\");"),
        &format!("assert_eq!(env!(\"CARGO_PKG_VERSION\"), \"{new_version}\");"),
    )?;
    replace_reference(root, &old, new_version)?;
    replace_cargo_lock(root, &old, new_version)?;
    fold_changelog(root, new_version)?;
    bump_check(root)?;
    Ok(())
}

fn write_str(root: &Path, rel: &str, body: &str) -> Result<(), String> {
    fs::write(root.join(rel), body).map_err(|e| format!("write {rel}: {e}"))
}

fn replace_in(root: &Path, rel: &str, from: &str, to: &str) -> Result<(), String> {
    let path = root.join(rel);
    let text = fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
    if !text.contains(from) {
        return Err(format!("{rel} missing {from:?}"));
    }
    let next = text.replacen(from, to, 1);
    fs::write(&path, next).map_err(|e| format!("write {rel}: {e}"))
}

fn replace_workspace_version(root: &Path, old: &str, new: &str) -> Result<(), String> {
    let path = root.join("Cargo.toml");
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let (head, rest) = text
        .split_once("[workspace.package]")
        .ok_or_else(|| "Cargo.toml missing [workspace.package]".to_string())?;
    let (pkg, tail) = rest
        .split_once("[workspace.dependencies]")
        .ok_or_else(|| "Cargo.toml missing [workspace.dependencies]".to_string())?;
    let from = format!("version = \"{old}\"");
    let to = format!("version = \"{new}\"");
    if !pkg.contains(&from) {
        return Err("Cargo.toml [workspace.package] version mismatch".into());
    }
    let pkg = pkg.replacen(&from, &to, 1);
    let next = format!("{head}[workspace.package]{pkg}[workspace.dependencies]{tail}");
    fs::write(&path, next).map_err(|e| e.to_string())
}

fn replace_reference(root: &Path, old: &str, new: &str) -> Result<(), String> {
    let path = root.join("docs/REFERENCE.md");
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    // Refuse a partial bump: every lockstep token must already be present.
    check_reference_text(&text, old)?;
    let mut next = text;
    // Longer / more specific tokens first so `GrokHub X.Y.Z-beta.` is not
    // rewritten only as a prefix of a looser pattern.
    for (from, to) in [
        (format!("**v{old}**"), format!("**v{new}**")),
        (
            format!("grokhub-linux-v{old}.tar.gz"),
            format!("grokhub-linux-v{new}.tar.gz"),
        ),
        (
            format!("GrokHub-Setup-{old}.exe"),
            format!("GrokHub-Setup-{new}.exe"),
        ),
        (
            format!("grokhub-windows-v{old}.zip"),
            format!("grokhub-windows-v{new}.zip"),
        ),
        (
            format!("GrokHub {old}-beta."),
            format!("GrokHub {new}-beta."),
        ),
        (
            format!("GrokHub {old} (main"),
            format!("GrokHub {new} (main"),
        ),
        // Remaining plain `GrokHub X.Y.Z` (no-git build line, etc.).
        (format!("GrokHub {old}"), format!("GrokHub {new}")),
    ] {
        if !next.contains(&from) {
            // Plain `GrokHub {old}` may already be gone after the beta/main
            // replacements when those were the only occurrences — only the
            // required tokens above are mandatory (checked via check_reference_text).
            if from == format!("GrokHub {old}") {
                continue;
            }
            return Err(format!("docs/REFERENCE.md missing required token {from:?}"));
        }
        next = next.replace(&from, &to);
    }
    check_reference_text(&next, new)?;
    fs::write(&path, next).map_err(|e| e.to_string())
}

fn replace_cargo_lock(root: &Path, old: &str, new: &str) -> Result<(), String> {
    let path = root.join("Cargo.lock");
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut next = text.clone();
    for name in [
        "grokhub-app",
        "grokhub-agent",
        "grokhub-core",
        "grokhub-hub",
    ] {
        let from = format!("name = \"{name}\"\nversion = \"{old}\"");
        let to = format!("name = \"{name}\"\nversion = \"{new}\"");
        if !next.contains(&from) {
            return Err(format!("Cargo.lock missing {name} @ {old}"));
        }
        next = next.replacen(&from, &to, 1);
    }
    fs::write(&path, next).map_err(|e| e.to_string())
}

fn fold_changelog(root: &Path, new_version: &str) -> Result<(), String> {
    let path = root.join("CHANGELOG.md");
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let fragments = collect_fragments(root)?;
    let unreleased = extract_unreleased_body(&text)?;
    let mut bullets = String::new();
    for line in unreleased.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            continue;
        }
        bullets.push_str(t);
        bullets.push('\n');
    }
    for frag in &fragments {
        for line in frag.lines() {
            let t = line.trim_end();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let line = if t.starts_with('-') {
                t.to_string()
            } else {
                format!("- {t}")
            };
            bullets.push_str(&line);
            bullets.push('\n');
        }
    }
    if bullets.is_empty() {
        bullets.push_str("- (no changelog fragments)\n");
    }
    let today = today_utc();
    let section = format!(
        "## {new_version} — {today}\n\n{bullets}\n- Linux: `grokhub-linux-v{new_version}.tar.gz` and AUR `pkgver={new_version}`.\n- Windows: `GrokHub-Setup-{new_version}.exe` and `grokhub-windows-v{new_version}.zip`.\n"
    );
    let rest = strip_unreleased(&text)?;
    let next = format!("# Changelog\n\n## Unreleased\n\n{section}\n{rest}");
    fs::write(&path, next).map_err(|e| e.to_string())?;
    for name in fragment_names(root)? {
        fs::remove_file(root.join("changelog.d").join(&name))
            .map_err(|e| format!("remove changelog.d/{name}: {e}"))?;
    }
    Ok(())
}

fn collect_fragments(root: &Path) -> Result<Vec<String>, String> {
    let mut names = fragment_names(root)?;
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let body = fs::read_to_string(root.join("changelog.d").join(&name))
            .map_err(|e| format!("changelog.d/{name}: {e}"))?;
        out.push(body);
    }
    Ok(out)
}

fn fragment_names(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("changelog.d");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "README.md" || !name.ends_with(".md") {
            continue;
        }
        names.push(name);
    }
    Ok(names)
}

fn extract_unreleased_body(text: &str) -> Result<String, String> {
    let after = text
        .split_once("## Unreleased")
        .map(|(_, rest)| rest)
        .ok_or_else(|| "CHANGELOG.md missing ## Unreleased".to_string())?;
    let body = after
        .split("\n## ")
        .next()
        .ok_or_else(|| "CHANGELOG.md: empty after Unreleased".to_string())?;
    Ok(body.trim_start_matches('\n').to_string())
}

fn strip_unreleased(text: &str) -> Result<String, String> {
    let after = text
        .split_once("## Unreleased")
        .map(|(_, rest)| rest)
        .ok_or_else(|| "CHANGELOG.md missing ## Unreleased".to_string())?;
    let rest = after
        .find("\n## ")
        .map(|i| &after[i + 1..])
        .ok_or_else(|| "CHANGELOG.md: no version section after Unreleased".to_string())?;
    Ok(rest.to_string())
}

fn today_utc() -> String {
    // Prefer SOURCE_DATE_EPOCH / date CLI; fall back to a fixed placeholder the
    // release owner can edit. Avoids pulling chrono into xtask.
    if let Ok(epoch) = env::var("SOURCE_DATE_EPOCH") {
        if let Ok(secs) = epoch.parse::<i64>() {
            if let Some(ymd) = civil_date_from_unix(secs) {
                return ymd;
            }
        }
    }
    if let Ok(out) = process::Command::new("date").args(["-u", "+%Y-%m-%d"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.len() == 10 {
                return s;
            }
        }
    }
    "YYYY-MM-DD".into()
}

fn civil_date_from_unix(secs: i64) -> Option<String> {
    // Days since Unix epoch → civil date (Howard Hinnant algorithm).
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_accepts_plain_triples() {
        assert!(is_semver("3.0.2"));
        assert!(is_semver("2.11.0"));
        assert!(!is_semver("3.0.2-beta.1"));
        assert!(!is_semver("v3.0.2"));
        assert!(!is_semver(""));
    }

    #[test]
    fn civil_date_known_epoch() {
        // 2026-10-14 00:00:00 UTC
        assert_eq!(
            civil_date_from_unix(1_791_936_000).as_deref(),
            Some("2026-10-14")
        );
    }

    fn sample_reference(version: &str) -> String {
        format!(
            "**v{version}** — notes\n\
             | Linux | `grokhub-linux-v{version}.tar.gz` | **v{version}** |\n\
             | Windows | `GrokHub-Setup-{version}.exe`, `grokhub-windows-v{version}.zip` | **v{version}** |\n\
             grokhub --version # GrokHub {version}-beta.3 (beta @ abc) or GrokHub {version} (main @ def)\n\
             A build without git shows just `GrokHub {version}`.\n"
        )
    }

    #[test]
    fn check_reference_requires_bold_count_and_version_examples() {
        let ok = sample_reference("3.0.2");
        assert!(check_reference_text(&ok, "3.0.2").is_ok());

        // Only two **v…** cells — must fail (headline + Latest need three).
        let two_bold = ok.replacen("**v3.0.2**", "v3.0.2", 1);
        let err = check_reference_text(&two_bold, "3.0.2").unwrap_err();
        assert!(
            err.contains("expected >=3"),
            "expected bold-count error, got {err}"
        );

        // Stale --version beta example.
        let stale_beta = ok.replace("GrokHub 3.0.2-beta.", "GrokHub 9.9.9-beta.");
        let err = check_reference_text(&stale_beta, "3.0.2").unwrap_err();
        assert!(
            err.contains("--version beta example"),
            "expected beta example error, got {err}"
        );

        // Stale --version main example.
        let stale_main = ok.replace("GrokHub 3.0.2 (main", "GrokHub 9.9.9 (main");
        let err = check_reference_text(&stale_main, "3.0.2").unwrap_err();
        assert!(
            err.contains("--version main example"),
            "expected main example error, got {err}"
        );
    }
}
