use crate::channel::{Channel, ChannelTips};
use crate::host_plan::{explain_host_risk, host_risk, HostPlanStep, HostRisk};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Overlay update only. Never wipes `~/.config/GrokHub`.
pub fn is_grokhub_source(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
        && dir.join("scripts/install.sh").is_file()
        && dir.join("crates/grokhub-app").is_dir()
}

pub fn walk_up_source(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if is_grokhub_source(&cur) {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

pub fn discover_source(hints: &[PathBuf]) -> Option<PathBuf> {
    for h in hints {
        let p = h.as_path();
        if p.as_os_str().is_empty() {
            continue;
        }
        if is_grokhub_source(p) {
            return Some(p.to_path_buf());
        }
        if let Some(found) = walk_up_source(p) {
            return Some(found);
        }
    }
    None
}

fn host_quote_for(s: &str, windows: bool) -> String {
    if windows {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn git_probe_timeout() -> Duration {
    // Windows Defender / cold git.exe often exceeds 2s on the first probe.
    if cfg!(windows) {
        Duration::from_secs(8)
    } else {
        Duration::from_secs(2)
    }
}

fn git_stdout(source: &Path, args: &[&str]) -> Result<(bool, String), String> {
    git_stdout_within(source, args, git_probe_timeout())
}

/// `git_stdout` with its own timeout, for a local step that can take longer
/// than a probe (a checkout that rewrites the work tree).
fn git_stdout_within(
    source: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(bool, String), String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(source)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("git: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let mut out = String::new();
                if let Some(stdout) = child.stdout.take() {
                    let _ = stdout.take(4096).read_to_string(&mut out);
                }
                return Ok((st.success(), out));
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("git timed out — is the source clone reachable?".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(format!("git: {e}")),
        }
    }
}

fn git_head_branch(source: &Path) -> Result<String, String> {
    let (ok, out) = git_stdout(source, &["symbolic-ref", "-q", "--short", "HEAD"])?;
    if !ok {
        return Err("source clone is not on a branch — checkout main, then Update".into());
    }
    let branch = out.trim().to_string();
    if branch.is_empty() {
        return Err("source clone is not on a branch — checkout main, then Update".into());
    }
    Ok(branch)
}

fn git_origin_url(source: &Path) -> Result<String, String> {
    let (ok, out) = git_stdout(source, &["remote", "get-url", "origin"])?;
    if !ok {
        return Err("source clone has no origin — add origin, then Update".into());
    }
    Ok(out.trim().to_string())
}

fn git_rev_parse(source: &Path, rev: &str) -> Result<String, String> {
    let (ok, out) = git_stdout(source, &["rev-parse", "--verify", rev])?;
    if !ok {
        return Err(format!("missing {rev}"));
    }
    let sha = out.trim().to_string();
    if sha.is_empty() {
        return Err(format!("empty {rev}"));
    }
    Ok(sha)
}

/// Parse `git ls-remote` stdout for `refs/heads/beta` and `refs/heads/main`.
/// Missing refs yield empty strings.
pub fn parse_ls_remote_tips(stdout: &str) -> (String, String) {
    let mut beta = String::new();
    let mut main = String::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(sha) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        match name {
            "refs/heads/beta" => beta = sha.to_string(),
            "refs/heads/main" => main = sha.to_string(),
            _ => {}
        }
    }
    (beta, main)
}

/// Tip SHAs from cached remote-tracking refs (`origin/beta`, `origin/main`).
/// No network — use after a fetch/pull, or when ls-remote is unavailable.
pub fn remote_tracking_tips(source: &Path) -> Result<(String, String), String> {
    let beta = git_rev_parse(source, "origin/beta").unwrap_or_default();
    let main = git_rev_parse(source, "origin/main").unwrap_or_default();
    if beta.is_empty() && main.is_empty() {
        return Err("no origin/beta or origin/main in clone".into());
    }
    Ok((beta, main))
}

/// Labs Beta auto-off probe: fetch `beta` and `main` from `origin` into
/// `origin/beta` / `origin/main`, then read both tip SHAs and trees
/// (`origin/<branch>^{tree}`). Uses only the clone's existing `origin` remote.
/// Any fetch, timeout or ref error is `Err` — callers must not flip the
/// channel then, and there is no fallback to stale cached refs.
pub fn fetch_channel_tips(source: &Path) -> Result<ChannelTips, String> {
    if !is_grokhub_source(source) {
        return Err("not a GrokHub source tree".into());
    }
    // Tip fetch is not a Settings probe — allow longer than git_probe_timeout
    // so a busy Windows nextest / Defender pass does not flake the check.
    let (ok, _) = git_stdout_within(
        source,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "origin",
            "+refs/heads/beta:refs/remotes/origin/beta",
            "+refs/heads/main:refs/remotes/origin/main",
        ],
        Duration::from_secs(30),
    )?;
    if !ok {
        return Err("git fetch origin beta main failed".into());
    }
    Ok(ChannelTips {
        beta_sha: git_rev_parse(source, "refs/remotes/origin/beta")?,
        main_sha: git_rev_parse(source, "refs/remotes/origin/main")?,
        beta_tree: git_rev_parse(source, "refs/remotes/origin/beta^{tree}")?,
        main_tree: git_rev_parse(source, "refs/remotes/origin/main^{tree}")?,
    })
}

/// Labs Beta auto-off switch-back without a rebuild: move a clone that is on
/// `beta` onto `main` at `origin/main` (call after [`fetch_channel_tips`]).
/// Beta and main have the same tree then, so the next Update builds main.
/// Ok when the clone is already on `main`. Refuses, touching nothing, when the
/// clone is on another branch or detached, has uncommitted changes, or has a
/// local `main` with commits that are not on `origin/main`.
pub fn switch_clone_to_main(source: &Path) -> Result<(), String> {
    if !is_grokhub_source(source) {
        return Err("not a GrokHub source tree".into());
    }
    match git_head_branch(source)?.as_str() {
        "main" => return Ok(()),
        "beta" => {}
        other => {
            return Err(format!(
                "source clone is on {other}, not beta — checkout main by hand"
            ))
        }
    }
    let clean = git_stdout(source, &["diff", "--quiet"])?.0
        && git_stdout(source, &["diff", "--cached", "--quiet"])?.0;
    if !clean {
        return Err(format!(
            "error: {} has uncommitted changes; commit or stash them before --channel stable",
            source.display()
        ));
    }
    if git_stdout(
        source,
        &["show-ref", "--verify", "--quiet", "refs/heads/main"],
    )?
    .0 && !git_stdout(
        source,
        &[
            "merge-base",
            "--is-ancestor",
            "refs/heads/main",
            "refs/remotes/origin/main",
        ],
    )?
    .0
    {
        return Err(
            "local main has commits that are not on origin/main; sort that out by hand".into(),
        );
    }
    let (ok, _) = git_stdout_within(
        source,
        &["checkout", "-q", "-B", "main", "--track", "origin/main"],
        Duration::from_secs(30),
    )?;
    if !ok || git_head_branch(source)? != "main" {
        return Err("git checkout main failed — previous checkout kept".into());
    }
    Ok(())
}

/// Plan step the cabin runs itself: save a dirty or diverged clone onto a
/// `grokhub-backup/…` branch, then hard-reset to the channel tip.
pub const CLONE_RECOVER_STEP_PREFIX: &str = "grokhub: recover dirty or diverged clone · ";

/// Encode a recover step for the update plan runner.
pub fn clone_recover_step(source: &Path, channel: Channel) -> String {
    format!(
        "{CLONE_RECOVER_STEP_PREFIX}{} · {}",
        channel.as_str(),
        source.display()
    )
}

/// Parse [`clone_recover_step`]. `None` when the line is not a recover step.
pub fn parse_clone_recover_step(cmd: &str) -> Option<(Channel, PathBuf)> {
    let rest = cmd.strip_prefix(CLONE_RECOVER_STEP_PREFIX)?;
    let (ch, path) = rest.split_once(" · ")?;
    let channel = Channel::parse(ch)?;
    if path.is_empty() {
        return None;
    }
    Some((channel, PathBuf::from(path)))
}

fn backup_branch_name_now() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let rfc = crate::oauth::unix_ms_to_rfc3339(ms);
    let digits: String = rfc.chars().filter(|c| c.is_ascii_digit()).collect();
    let stamp = if digits.len() >= 14 {
        format!("{}-{}", &digits[..8], &digits[8..14])
    } else {
        ms.to_string()
    };
    format!("grokhub-backup/{stamp}")
}

fn git_is_dirty(source: &Path) -> Result<bool, String> {
    let (ok, out) = git_stdout(source, &["status", "--porcelain"])?;
    if !ok {
        return Err("git status failed".into());
    }
    Ok(!out.trim().is_empty())
}

/// Whether HEAD is not an ancestor of `tip` (diverged, or tip missing).
fn git_is_diverged_from(source: &Path, tip: &str) -> Result<bool, String> {
    if git_rev_parse(source, tip).is_err() {
        return Ok(false);
    }
    let (ok, _) = git_stdout(source, &["merge-base", "--is-ancestor", "HEAD", tip])?;
    Ok(!ok)
}

/// True when the clone has a dirty tree or a history that cannot fast-forward
/// onto the cached `origin/<branch>` tip. A missing remote-tracking tip is
/// not diverged — the pull step still fetches.
pub fn clone_needs_recover(source: &Path, channel: Channel) -> Result<bool, String> {
    let tip = format!("origin/{}", channel.branch());
    Ok(git_is_dirty(source)? || git_is_diverged_from(source, &tip)?)
}

fn fetch_channel_tip(source: &Path, channel: Channel) -> Result<(), String> {
    let want = channel.branch();
    let refspec = format!("+refs/heads/{want}:refs/remotes/origin/{want}");
    let (ok, _) = git_stdout_within(
        source,
        &["fetch", "--quiet", "origin", &refspec],
        Duration::from_secs(60),
    )?;
    if !ok {
        return Err(format!("git fetch origin {want} failed"));
    }
    Ok(())
}

/// Tip the recover step resets to — the same ref [`clone_needs_recover`] and
/// `pull --ff-only` use (`origin/main` / `origin/beta`). Never a release tag:
/// resetting to an old `vX.Y.Z` would leave a clone that keeps looking
/// diverged from `origin/main` on every later Update.
fn resolve_channel_tip(source: &Path, channel: Channel) -> Result<String, String> {
    let tip = format!("origin/{}", channel.branch());
    git_rev_parse(source, &tip)?;
    Ok(tip)
}

/// When the clone is dirty or diverged from the channel tip, commit the
/// current state onto a local `grokhub-backup/<YYYYMMDD-HHMMSS>` branch
/// (never pushed) and `reset --hard` to that tip. Returns the backup branch
/// name, or `None` when a plain fast-forward is enough.
pub fn recover_clone_if_needed(source: &Path, channel: Channel) -> Result<Option<String>, String> {
    if !is_grokhub_source(source) {
        return Err(format!(
            "{product} can't find its source folder (~/{id} or ~/.config/{id}/source).",
            product = crate::brand::product_name(),
            id = crate::brand::INTERNAL_ID
        ));
    }
    let want = channel.branch();
    let branch = git_head_branch(source)?;
    if branch != want {
        return Err(match channel {
            Channel::Stable => format!("source clone is on {branch} — checkout main, then Update"),
            Channel::Beta => format!(
                "source clone is on {branch} — run ./scripts/install.sh --user --channel beta, then Update"
            ),
        });
    }
    fetch_channel_tip(source, channel)?;
    let tip = resolve_channel_tip(source, channel)?;
    let dirty = git_is_dirty(source)?;
    let diverged = git_is_diverged_from(source, &tip)?;
    if !dirty && !diverged {
        return Ok(None);
    }
    let old_head = git_rev_parse(source, "HEAD")?;
    if dirty {
        let (ok, _) = git_stdout(source, &["add", "-A"])?;
        if !ok {
            return Err("git add failed while saving the clone".into());
        }
        let bak_msg = format!("GrokHub update backup before reset to {tip}");
        // Disable signing: a global commit.gpgsign=true must not abort recover.
        let (ok, _) = git_stdout_within(
            source,
            &[
                "-c",
                "user.email=cabin@local",
                "-c",
                "user.name=GrokHub",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--no-verify",
                "--no-gpg-sign",
                "-m",
                &bak_msg,
            ],
            Duration::from_secs(30),
        )?;
        if !ok {
            return Err("git commit failed while saving the clone".into());
        }
    }
    let mut bak = backup_branch_name_now();
    // A same-second retry: append a short suffix so `git branch` never clashes.
    for n in 0..8u32 {
        let name = if n == 0 {
            bak.clone()
        } else {
            format!("{bak}-{n}")
        };
        let (ok, _) = git_stdout(source, &["branch", &name, "HEAD"])?;
        if ok {
            bak = name;
            break;
        }
        if n == 7 {
            return Err(format!("could not create backup branch {bak}"));
        }
    }
    let (ok, _) = git_stdout_within(source, &["reset", "--hard", &tip], Duration::from_secs(60))?;
    if !ok {
        return Err(format!(
            "git reset --hard {tip} failed — prior state is on {bak} ({})",
            &old_head[..old_head.len().min(7)]
        ));
    }
    Ok(Some(bak))
}

/// Overlay pulls this GitHub remote until Cursor Origin is live.
pub const GITHUB_REMOTE_URL: &str = "https://github.com/blackviperxiii-ui/GrokHub.git";
/// Leftover Cursor Origin clone — retarget to GitHub.
pub const ORIGIN_REMOTE_URL: &str = "https://origin.cursor.com/viperxiii/GrokHub.git";
/// Public Latest release. Cabin notify compares `tag_name` to the running version.
pub const GITHUB_LATEST_API: &str =
    "https://api.github.com/repos/blackviperxiii-ui/GrokHub/releases/latest";

fn origin_norm(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_ascii_lowercase();
    if let Some(stripped) = u.strip_suffix(".git") {
        u = stripped.to_string();
    }
    if let Some(rest) = u.strip_prefix("git@") {
        rest.replace(':', "/")
    } else {
        u
    }
}

/// Same repo as `GITHUB_REMOTE_URL` (https or ssh). Not GrokHub-Windows.
pub fn canonical_github_origin(url: &str) -> bool {
    let u = origin_norm(url);
    u.ends_with("github.com/blackviperxiii-ui/grokhub")
}

/// Any origin that is not this repo — pin before pull so overlay never
/// fetches an unvalidated remote and then runs its install script.
pub fn origin_needs_retarget(url: &str) -> bool {
    !canonical_github_origin(url)
}

pub fn update_cmds(source: &Path) -> Result<Vec<String>, String> {
    update_cmds_on(source, cfg!(windows), Channel::Stable)
}

/// The plan for the channel in the install receipt: a beta clone pulls
/// `origin beta`, never `origin main`.
pub fn update_cmds_in(source: &Path, channel: Channel) -> Result<Vec<String>, String> {
    update_cmds_on(source, cfg!(windows), channel)
}

/// Same plan as `update_cmds`, for the host the caller named, so a Windows
/// test can still assert the Linux `install.sh` plan.
fn update_cmds_on(source: &Path, windows: bool, channel: Channel) -> Result<Vec<String>, String> {
    if !is_grokhub_source(source) {
        return Err(format!(
            "{product} can't find its source folder (~/{id} or ~/.config/{id}/source).",
            product = crate::brand::product_name(),
            id = crate::brand::INTERNAL_ID
        ));
    }
    let want = channel.branch();
    let branch = git_head_branch(source)?;
    if branch != want {
        return Err(match channel {
            Channel::Stable => format!("source clone is on {branch} — checkout main, then Update"),
            Channel::Beta => format!(
                "source clone is on {branch} — run ./scripts/install.sh --user --channel beta, then Update"
            ),
        });
    }
    let src = host_quote_for(&source.display().to_string(), windows);
    let mut cmds = Vec::new();
    match git_origin_url(source) {
        Ok(origin) if origin_needs_retarget(&origin) => {
            cmds.push(format!(
                "git -C {src} remote set-url origin {GITHUB_REMOTE_URL}"
            ));
        }
        Ok(_) => {}
        Err(_) => {
            cmds.push(format!(
                "git -C {src} remote add origin {GITHUB_REMOTE_URL}"
            ));
        }
    }
    if clone_needs_recover(source, channel)? {
        // Fetch + backup branch + hard reset; the cabin runs this step itself.
        cmds.push(clone_recover_step(source, channel));
    } else {
        cmds.push(format!("git -C {src} pull --ff-only origin {want}"));
    }
    cmds.push(overlay_install_cmd(source, &src, windows));
    Ok(cmds)
}

/// A real product checkout: GrokHub tree on `main`. Leftover `cursor/*`
/// (or any other branch) is not this — Windows Setup must still Update.
pub fn overlay_clone_usable(source: &Path) -> bool {
    overlay_clone_usable_in(source, Channel::Stable)
}

/// Same check for the receipt's channel (`beta` clone for a beta install).
pub fn overlay_clone_usable_in(source: &Path, channel: Channel) -> bool {
    is_grokhub_source(source) && git_head_branch(source).ok().as_deref() == Some(channel.branch())
}

/// Windows Setup users have no clone and no cargo. Download the latest zip.
/// A leftover agent checkout (wrong branch, detached HEAD) must not block that.
pub fn update_cmds_for(source: Option<&Path>) -> Result<Vec<String>, String> {
    update_cmds_for_host(source, cfg!(windows))
}

pub fn update_cmds_for_host(source: Option<&Path>, windows: bool) -> Result<Vec<String>, String> {
    update_cmds_for_host_in(source, windows, Channel::Stable)
}

/// An install without a usable clone takes its channel's latest signed
/// release: stable the latest release, beta the newest beta pre-release (on
/// Windows, also when a leftover clone is on the wrong branch). A clone keeps
/// building from source, the developer path.
pub fn update_cmds_for_host_in(
    source: Option<&Path>,
    windows: bool,
    channel: Channel,
) -> Result<Vec<String>, String> {
    match source {
        Some(src) => match update_cmds_on(src, windows, channel) {
            Ok(cmds) => Ok(cmds),
            Err(_) if windows => Ok(release_update_cmds()),
            Err(e) => Err(e),
        },
        None => Ok(release_update_cmds()),
    }
}

/// One step the cabin runs itself: download this host's asset from the
/// latest release, check it against the signed `SHA256SUMS`
/// ([`crate::release_verify`]), then swap the binaries in.
pub fn release_update_cmds() -> Vec<String> {
    vec![crate::release_verify::RELEASE_INSTALL_STEP.into()]
}

pub fn settings_update_note() -> &'static str {
    if cfg!(windows) {
        "Downloads the latest Windows release from GitHub (the newest beta pre-release on Beta), checks it against the signed SHA256SUMS, and swaps it into %LOCALAPPDATA%\\Programs\\GrokHub. A source clone on main overlays with install-windows.ps1 instead. Does not wipe %APPDATA%\\GrokHub."
    } else {
        "A source clone pulls its branch and builds. Without a clone, downloads the latest release from GitHub (the newest beta pre-release on Beta) and checks it against the signed SHA256SUMS. Does not wipe ~/.config/GrokHub."
    }
}

/// Cabin semver with optional `-beta.N` prerelease. Releases sort above any
/// prerelease of the same `major.minor.patch`; among betas, `N` is numeric
/// (`2.13.0-beta.2` < `2.13.0-beta.10` < `2.13.0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CabinSemver {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// `None` = release. `Some(n)` = `-beta.N` (bare `-beta` is `Some(0)`).
    pub beta: Option<u64>,
}

impl PartialOrd for CabinSemver {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CabinSemver {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.beta, other.beta) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(a), Some(b)) => a.cmp(&b),
            })
    }
}

/// `2.9.5`, `v2.9.5`, `2.13.0-beta.2`, or bare `2.13.0-beta` (`beta = 0`).
/// A trailing ` (…)` from release labels is ignored. Other suffixes after the
/// patch digits (for example `-alpha`) still parse as a release, as before.
pub fn parse_cabin_semver(raw: &str) -> Option<CabinSemver> {
    let t = raw.trim().trim_start_matches(['v', 'V']);
    let t = t.split_whitespace().next()?.trim();
    let (num, beta) = match t.rsplit_once("-beta") {
        Some((num, "")) => (num, Some(0)),
        Some((num, rest)) => {
            let n = rest.strip_prefix('.')?;
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            (num, Some(n.parse().ok()?))
        }
        None => (t, None),
    };
    let mut parts = num.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_tok = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let patch: String = patch_tok
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if patch.is_empty() || patch.len() != patch_tok.len() {
        // Unknown suffix on the patch token (not `-beta`, already split): keep
        // the old "ignore trailing junk" behavior only when digits prefix it.
        if patch.is_empty() {
            return None;
        }
        // `2.9.5-alpha` → release 2.9.5
        return Some(CabinSemver {
            major,
            minor,
            patch: patch.parse().ok()?,
            beta: None,
        });
    }
    Some(CabinSemver {
        major,
        minor,
        patch: patch.parse().ok()?,
        beta,
    })
}

/// True when `latest` is a newer install than `running`. A prerelease is never
/// offered to a stable (non-prerelease) install.
pub fn cabin_version_newer(latest: &str, running: &str) -> bool {
    match (parse_cabin_semver(latest), parse_cabin_semver(running)) {
        (Some(l), Some(r)) => {
            if l.beta.is_some() && r.beta.is_none() {
                return false;
            }
            l > r
        }
        _ => false,
    }
}

pub fn should_notify_cabin_update(running: &str, latest: Option<&str>) -> bool {
    latest.is_some_and(|tag| cabin_version_newer(tag, running))
}

/// Labs / updater auto-off by version: a beta install at `beta_running`
/// (`X.Y.Z-beta.N`) turns off when stable `main_version` has reached that
/// base (`X.Y.Z` or newer). Pure — tree-based [`crate::auto_off_step`] stays
/// the clone path.
pub fn beta_auto_off_for_versions(beta_running: &str, main_version: &str) -> bool {
    match (
        parse_cabin_semver(beta_running),
        parse_cabin_semver(main_version),
    ) {
        (Some(b), Some(m)) if b.beta.is_some() && m.beta.is_none() => {
            (m.major, m.minor, m.patch) >= (b.major, b.minor, b.patch)
        }
        _ => false,
    }
}

/// `tag_name` from `GET …/releases/latest`.
pub fn parse_github_latest_tag(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = v.get("tag_name").and_then(|x| x.as_str())?.trim();
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_string())
    }
}

pub fn cabin_update_notice(running: &str, latest: &str) -> String {
    format!(
        "{product} {latest} is on GitHub Latest (running {running}). Update overlays the cabin, then Restart.", product = crate::brand::product_name()
    )
}

/// GitHub Latest. Not launch-only.
pub const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(2 * 60 * 60);

pub fn update_check_due(last: Option<Instant>, now: Instant, every: Duration) -> bool {
    match last {
        None => true,
        Some(t) => now.saturating_duration_since(t) >= every,
    }
}

/// What the one Update control will run. Linux and Windows use the same cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatePending {
    None,
    Cabin,
}

/// Shared by the titlebar chip, Settings → Update, `/update`, and `grokhub --update`.
/// A missing or failed probe is not newer.
pub fn pending_from_versions(running_cabin: &str, cabin_latest: Option<&str>) -> UpdatePending {
    if should_notify_cabin_update(running_cabin, cabin_latest) {
        UpdatePending::Cabin
    } else {
        UpdatePending::None
    }
}

/// Titlebar chip. `None` hides the chip.
pub fn update_chip_label(pending: UpdatePending) -> Option<&'static str> {
    match pending {
        UpdatePending::None => None,
        UpdatePending::Cabin => Some("Update cabin"),
    }
}

/// Settings → Update title. Always a string so the button stays up for a manual check.
/// The titlebar chip still uses `update_chip_label` and hides when nothing is newer.
pub fn settings_update_label(pending: UpdatePending) -> &'static str {
    update_chip_label(pending).unwrap_or("Update")
}

/// `grokhub --update` on this install's channel. The release tag only tracks
/// stable, and beta never bumps the version, so a beta install always pulls
/// `origin beta` (a fast-forward that is a no-op when it is current).
pub fn pending_on_channel(pending: UpdatePending, channel: Channel) -> UpdatePending {
    match channel {
        Channel::Stable => pending,
        Channel::Beta => UpdatePending::Cabin,
    }
}

/// Settings / `/update` click. A missed probe still overlays the cabin.
pub fn pending_for_manual_update(_pending: UpdatePending) -> UpdatePending {
    UpdatePending::Cabin
}

/// Settings → Update hint. A click still overlays when the probe found nothing.
/// `update_hint` stays the pending-state / `grokhub --update` copy.
pub fn settings_update_hint(pending: UpdatePending) -> &'static str {
    match pending {
        UpdatePending::None => crate::brand::filled(
            "{product} looks current. Update still overlays so a missed probe can land.",
        ),
        other => update_hint(other),
    }
}

/// Pending-state copy. `None` is the `grokhub --update` exit line (no overlay).
pub fn update_hint(pending: UpdatePending) -> &'static str {
    match pending {
        UpdatePending::Cabin => "Updates the cabin, then Restart.",
        UpdatePending::None => crate::brand::filled("{product} is current."),
    }
}

pub fn cabin_overlay_step(cmd: &str) -> bool {
    cmd.contains("pull --ff-only")
        || cmd.contains("remote set-url")
        || cmd.contains("remote add")
        || cmd.contains("install.sh")
        || cmd.contains("install-windows.ps1")
        || cmd == crate::release_verify::RELEASE_INSTALL_STEP
        || crate::release_verify::parse_release_switch_step(cmd).is_some()
        || parse_clone_recover_step(cmd).is_some()
}

fn overlay_install_cmd(source: &Path, src_quoted: &str, windows: bool) -> String {
    if windows {
        // run_host is already PowerShell -Command. Do not nest powershell.exe -File
        // or the script's exit code is lost. Process Bypass is required because `&`
        // a .ps1 file is still subject to Restricted (Windows 10 default).
        let install = host_quote_for(
            &source
                .join("scripts")
                .join("install-windows.ps1")
                .display()
                .to_string(),
            true,
        );
        format!("Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force; & {install}")
    } else {
        format!("{src_quoted}/scripts/install.sh --user")
    }
}

pub fn update_plan_steps(cmds: Vec<String>) -> Vec<HostPlanStep> {
    cmds.into_iter()
        .map(|cmd| {
            let explain = if cmd.contains("pull --ff-only origin beta") {
                "fast-forward origin/beta — config stays".into()
            } else if cmd.contains("pull --ff-only") {
                "fast-forward origin/main — config stays".into()
            } else if let Some((channel, _)) = parse_clone_recover_step(&cmd) {
                match channel {
                    Channel::Beta => {
                        "save dirty or diverged work on a grokhub-backup/… branch, reset to origin/beta — config stays".into()
                    }
                    Channel::Stable => {
                        "save dirty or diverged work on a grokhub-backup/… branch, reset to origin/main — config stays".into()
                    }
                }
            } else if cmd.contains("remote set-url") || cmd.contains("remote add") {
                "point origin at GitHub — Cursor Origin is not live yet".into()
            } else if cmd == crate::release_verify::RELEASE_INSTALL_STEP {
                "download the latest release from GitHub, check its signed SHA256SUMS, swap it in — does not wipe config".into()
            } else if let Some(target) = crate::release_verify::parse_release_switch_step(&cmd) {
                match target {
                    Channel::Beta => "download the newest beta pre-release, check its signed SHA256SUMS, swap it in, then switch to beta — does not wipe config".into(),
                    Channel::Stable => "download the latest stable release, check its signed SHA256SUMS, swap it in, then switch to stable — does not wipe config".into(),
                }
            } else if cmd.contains("install.sh") || cmd.contains("install-windows.ps1") {
                if cfg!(windows) {
                    "overlay %LOCALAPPDATA%\\Programs\\GrokHub — does not wipe config".into()
                } else {
                    "overlay ~/.local/bin — does not wipe config".into()
                }
            } else {
                explain_host_risk(&cmd, host_risk(&cmd))
            };
            HostPlanStep {
                cmd,
                risk: HostRisk::Moderate,
                explain,
                checked: true,
            }
        })
        .collect()
}

pub fn update_wipes_config(cmds: &[String]) -> bool {
    cmds.iter().any(|c| {
        let l = c.to_ascii_lowercase();
        l.contains(".config/grokhub")
            && (l.contains("rm ") || l.contains("rm\t") || l.contains("rm -"))
    })
}

pub fn update_progress_pct(done_cmds: usize, total_cmds: usize) -> u8 {
    if total_cmds == 0 {
        return 100;
    }
    let pct = done_cmds.saturating_mul(100) / total_cmds;
    pct.min(100) as u8
}

pub fn update_step_label(cmd: &str) -> &'static str {
    if cmd.contains("pull --ff-only origin beta") {
        "Pulling origin/beta…"
    } else if cmd.contains("pull --ff-only") {
        "Pulling origin/main…"
    } else if parse_clone_recover_step(cmd).is_some() {
        "Saving prior clone, resetting…"
    } else if cmd.contains("remote set-url") || cmd.contains("remote add") {
        "Retargeting origin…"
    } else if cmd == crate::release_verify::RELEASE_INSTALL_STEP {
        "Downloading the verified release…"
    } else if let Some(target) = crate::release_verify::parse_release_switch_step(cmd) {
        match target {
            Channel::Beta => "Downloading the beta release…",
            Channel::Stable => "Downloading the stable release…",
        }
    } else if cmd.contains("install.sh") || cmd.contains("install-windows.ps1") {
        "Installing overlay…"
    } else {
        "Updating…"
    }
}

pub struct OverlayUpdateView {
    pub pct: u8,
    pub status: String,
    pub running: bool,
    pub posts_chat: bool,
    pub stay_on_update: bool,
    pub can_restart: bool,
}

fn overlay_view(pct: u8, status: String, running: bool, can_restart: bool) -> OverlayUpdateView {
    OverlayUpdateView {
        pct,
        status,
        running,
        posts_chat: false,
        stay_on_update: true,
        can_restart,
    }
}

pub fn overlay_update_begin(total_cmds: usize) -> OverlayUpdateView {
    overlay_view(
        update_progress_pct(0, total_cmds),
        "Updating…".into(),
        true,
        false,
    )
}

pub fn overlay_update_progress(
    done_cmds: usize,
    total_cmds: usize,
    label: &str,
) -> OverlayUpdateView {
    overlay_view(
        update_progress_pct(done_cmds, total_cmds),
        label.to_string(),
        true,
        false,
    )
}

pub fn overlay_update_finish(ok: bool, last_pct: u8) -> OverlayUpdateView {
    if ok {
        overlay_view(
            100,
            format!(
                "Update finished — restart {product}",
                product = crate::brand::product_name()
            ),
            false,
            true,
        )
    } else {
        overlay_view(last_pct, "Update failed".into(), false, false)
    }
}

pub fn overlay_update_can_restart(finished_ok: bool, running: bool) -> bool {
    finished_ok && !running
}

/// Prefer the user overlay binary so a running (deleted) inode is not relaunched.
pub fn restart_bin(home: Option<&str>, current_exe: Option<&str>) -> String {
    if cfg!(windows) {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let overlay = std::path::Path::new(&local)
                .join("Programs")
                .join("GrokHub")
                .join("grokhub.exe");
            if overlay.is_file() {
                return overlay.to_string_lossy().into_owned();
            }
        }
    } else if let Some(home) = home.map(str::trim).filter(|s| !s.is_empty()) {
        let overlay = std::path::Path::new(home)
            .join(".local")
            .join("bin")
            .join("grokhub");
        if overlay.is_file() {
            return overlay.to_string_lossy().into_owned();
        }
    }
    current_exe
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(if cfg!(windows) {
            "grokhub.exe"
        } else {
            "grokhub"
        })
        .to_string()
}

pub fn restart_argv(exe: &str, hidden: bool) -> Vec<String> {
    if hidden {
        vec![exe.to_string(), "--agent".into()]
    } else {
        vec![exe.to_string()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartAct {
    Systemd { units: Vec<String> },
    Spawn { argv: Vec<String> },
}

/// Restart hub if live, then spawn a new cabin. Grok Build owns computer-use;
/// do not restart ydotoold.
pub fn restart_acts(hub_unit: bool, _hands_unit: bool, exe: &str, hidden: bool) -> Vec<RestartAct> {
    let mut acts = Vec::new();
    let mut units = Vec::new();
    if hub_unit {
        units.push("grokhub-hub.service".into());
    }
    if !units.is_empty() {
        acts.push(RestartAct::Systemd { units });
    }
    acts.push(RestartAct::Spawn {
        argv: restart_argv(exe, hidden),
    });
    acts
}

/// Overlay stop targets: systemd MainPID of grokhub.service, then leftover cabin.pid.
/// Do not glob the process table by name — that matches this process.
pub fn overlay_stop_targets(main_pid: Option<u32>, cabin_pid: Option<u32>) -> Vec<u32> {
    let mut out = Vec::new();
    if let Some(pid) = main_pid.filter(|p| *p != 0) {
        out.push(pid);
    }
    if let Some(pid) = cabin_pid.filter(|p| *p != 0 && !out.contains(p)) {
        out.push(pid);
    }
    out
}

pub fn systemd_user_restart_args(units: &[String]) -> Vec<String> {
    let mut args = vec!["--user".into(), "restart".into()];
    args.extend(units.iter().cloned());
    args
}

pub fn systemd_user_stop_args(unit: &str) -> Vec<String> {
    vec!["--user".into(), "stop".into(), unit.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn assert_overlay_install(cmd: &str) {
        #[cfg(windows)]
        {
            assert!(
                cmd.contains("install-windows.ps1")
                    && cmd.contains("Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass")
                    && cmd.contains("& ")
                    && !cmd.contains("powershell.exe"),
                "Windows overlay must invoke the ps1 inside host PowerShell with process Bypass: {cmd}"
            );
        }
        #[cfg(unix)]
        {
            assert!(
                cmd.contains("install.sh") && cmd.contains("--user"),
                "{cmd}"
            );
        }
    }

    #[test]
    fn overlay_stop_uses_mainpid_and_cabin_pid_never_pgrep() {
        assert_eq!(overlay_stop_targets(None, None), Vec::<u32>::new());
        assert_eq!(overlay_stop_targets(Some(0), Some(0)), Vec::<u32>::new());
        assert_eq!(overlay_stop_targets(Some(42), None), vec![42]);
        assert_eq!(overlay_stop_targets(None, Some(7)), vec![7]);
        assert_eq!(overlay_stop_targets(Some(42), Some(42)), vec![42]);
        assert_eq!(overlay_stop_targets(Some(42), Some(7)), vec![42, 7]);
        let prod = include_str!("update.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("prod");
        assert!(
            !prod.contains("pgrep "),
            "overlay restart must never pgrep: {prod}"
        );
        let install = include_str!("../../../scripts/install.sh");
        assert!(
            !install.contains("pgrep grokhub"),
            "install.sh must never pgrep grokhub: {install}"
        );
    }

    #[test]
    fn source_and_overlay_plan() {
        let root = std::env::temp_dir().join(format!("grokhub-src-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        assert!(is_grokhub_source(&root));
        assert!(!is_grokhub_source(&std::env::temp_dir()));
        assert_eq!(
            walk_up_source(&root.join("crates/grokhub-app")),
            Some(root.clone())
        );
        assert_eq!(discover_source(&[root.join("crates")]), Some(root.clone()));
        let err = update_cmds(&root).unwrap_err();
        assert!(
            err.contains("not on a branch") || err.contains("main"),
            "{err}"
        );
        assert!(update_wipes_config(&["rm -rf ~/.config/GrokHub".into()]));
        assert!(update_cmds(Path::new("/tmp/not-grokhub")).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn git_probes_must_time_out() {
        let src = include_str!("update.rs");
        let git = src
            .split("fn git_stdout(")
            .nth(1)
            .and_then(|s| s.split("fn git_head_branch(").next())
            .expect("git_stdout");
        assert!(
            git.contains("try_wait") && !git.contains(".output()"),
            "Settings Update git must not hang the cabin: {git}"
        );
        assert!(
            git.contains("CREATE_NO_WINDOW") && git.contains("creation_flags"),
            "Windows git probes must not flash a console: {git}"
        );
        let head = src
            .split("fn git_head_branch(")
            .nth(1)
            .and_then(|s| s.split("fn git_origin_url(").next())
            .expect("git_head_branch");
        assert!(
            head.contains("git_stdout(") && !head.contains(".output()"),
            "git HEAD probe must time out: {head}"
        );
        let origin = src
            .split("fn git_origin_url(")
            .nth(1)
            .and_then(|s| s.split("pub const GITHUB_REMOTE_URL").next())
            .expect("git_origin_url");
        assert!(
            origin.contains("git_stdout(") && !origin.contains(".output()"),
            "git origin probe must time out: {origin}"
        );
    }

    fn seed_git_source(root: &std::path::Path, branch: &str) {
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        let run = |args: &[&str]| {
            let st = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .status()
                .unwrap();
            assert!(st.success(), "git {args:?}");
        };
        run(&["init", "-b", branch]);
        run(&["config", "user.email", "cabin@test"]);
        run(&["config", "user.name", "Cabin"]);
        run(&["add", "."]);
        run(&["commit", "-m", "seed"]);
    }

    #[test]
    fn a_beta_receipt_updates_from_origin_beta_and_never_pulls_main() {
        let root = std::env::temp_dir().join(format!("grokhub-src-beta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        seed_git_source(&root, "beta");
        std::process::Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/blackviperxiii-ui/GrokHub.git",
            ])
            .current_dir(&root)
            .status()
            .unwrap();
        let q = host_quote_for(&root.display().to_string(), cfg!(windows));
        let beta = update_cmds_in(&root, Channel::Beta).unwrap();
        assert_eq!(beta[0], format!("git -C {q} pull --ff-only origin beta"));
        assert_overlay_install(&beta[1]);
        assert!(!beta.iter().any(|c| c.contains("origin main")), "{beta:?}");
        assert!(overlay_clone_usable_in(&root, Channel::Beta));
        assert!(!overlay_clone_usable(&root));
        let plan = update_cmds_for_host_in(Some(&root), false, Channel::Beta).unwrap();
        assert_eq!(
            plan[0],
            format!(
                "git -C {} pull --ff-only origin beta",
                host_quote_for(&root.display().to_string(), false)
            )
        );
        // No clone: beta takes its newest pre-release, on either OS.
        assert_eq!(
            update_cmds_for_host_in(None, true, Channel::Beta).unwrap(),
            release_update_cmds()
        );
        assert_eq!(
            update_cmds_for_host_in(None, false, Channel::Beta).unwrap(),
            release_update_cmds()
        );
        assert_eq!(
            update_cmds_for_host_in(None, true, Channel::Stable).unwrap(),
            release_update_cmds()
        );
        // A stable receipt on the same beta clone refuses instead of pulling main into it.
        assert_eq!(
            update_cmds_in(&root, Channel::Stable).unwrap_err(),
            "source clone is on beta — checkout main, then Update"
        );
        std::process::Command::new("git")
            .args(["checkout", "-B", "main"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert_eq!(
            update_cmds_in(&root, Channel::Beta).unwrap_err(),
            "source clone is on main — run ./scripts/install.sh --user --channel beta, then Update"
        );
        let stable = update_cmds_in(&root, Channel::Stable).unwrap();
        assert_eq!(stable[0], format!("git -C {q} pull --ff-only origin main"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_requires_main_and_pulls_origin_main() {
        let root = std::env::temp_dir().join(format!("grokhub-src-main-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        seed_git_source(&root, "dev");
        let err = update_cmds(&root).unwrap_err();
        assert!(err.contains("main"), "{err}");
        assert!(err.contains("dev"), "{err}");
        std::process::Command::new("git")
            .args(["checkout", "-B", "main"])
            .current_dir(&root)
            .status()
            .unwrap();
        let no_origin = update_cmds(&root).unwrap();
        assert!(
            no_origin[0]
                .contains("remote add origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "{no_origin:?}"
        );
        assert!(
            no_origin[1].contains("pull --ff-only origin main"),
            "{no_origin:?}"
        );
        assert_overlay_install(&no_origin[2]);
        assert_eq!(
            no_origin.len(),
            3,
            "the plan ends at the overlay: {no_origin:?}"
        );
        std::process::Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://example.invalid/grokhub.git",
            ])
            .current_dir(&root)
            .status()
            .unwrap();
        let cmds = update_cmds(&root).unwrap();
        assert!(
            cmds[0]
                .contains("remote set-url origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "{cmds:?}"
        );
        assert!(cmds[1].contains("pull --ff-only origin main"), "{cmds:?}");
        assert_overlay_install(&cmds[2]);
        assert_eq!(cmds.len(), 3, "Update never runs grok update: {cmds:?}");
        assert!(!cmds.iter().any(|c| c.contains("grok update")), "{cmds:?}");
        assert!(!update_wipes_config(&cmds));
        let plan = update_plan_steps(cmds);
        assert!(plan[0].explain.contains("GitHub"), "{plan:?}");
        assert!(plan[1].explain.contains("origin/main"), "{plan:?}");
        assert!(plan[2].explain.contains("overlay"), "{plan:?}");
        assert_ne!(plan[0].explain, "read-only");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn overlay_progress_is_cmd_share() {
        assert_eq!(update_progress_pct(0, 2), 0);
        assert_eq!(update_progress_pct(1, 2), 50);
        assert_eq!(update_progress_pct(2, 2), 100);
        assert_eq!(update_progress_pct(3, 2), 100);
        assert_eq!(update_progress_pct(0, 0), 100);
        assert_eq!(update_progress_pct(1, 3), 33);
    }

    #[test]
    fn overlay_update_stays_on_settings_and_skips_chat() {
        let start = overlay_update_begin(2);
        assert_eq!(start.pct, 0);
        assert!(start.running);
        assert!(!start.posts_chat);
        assert!(start.stay_on_update);
        assert!(!start.can_restart);
        assert!(start.status.contains("Updating"));

        let pull =
            overlay_update_progress(1, 2, update_step_label("git pull --ff-only origin main"));
        assert_eq!(pull.pct, 50);
        assert!(pull.running);
        assert!(!pull.posts_chat);
        assert!(pull.stay_on_update);
        assert!(!pull.can_restart);
        assert!(pull.status.contains("Pulling"));

        let ok = overlay_update_finish(true, 50);
        assert_eq!(ok.pct, 100);
        assert!(!ok.running);
        assert!(!ok.posts_chat);
        assert!(ok.stay_on_update);
        assert!(ok.can_restart);
        assert!(ok.status.contains("restart"));
        assert!(!ok.status.contains("HOST_RESULT"));
        assert!(overlay_update_can_restart(true, false));
        assert!(!overlay_update_can_restart(true, true));
        assert!(!overlay_update_can_restart(false, false));

        let fail = overlay_update_finish(false, 50);
        assert_eq!(fail.pct, 50);
        assert!(!fail.running);
        assert!(!fail.posts_chat);
        assert!(fail.stay_on_update);
        assert!(!fail.can_restart);
        assert!(fail.status.contains("failed"));
        assert!(!fail.status.contains("HOST_RESULT"));
    }

    #[test]
    fn restart_prefers_overlay_bin_and_restarts_hub_then_cabin() {
        let root = std::env::temp_dir().join(format!(
            "grokhub-restart-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        #[cfg(unix)]
        {
            let bin = root.join(".local").join("bin").join("grokhub");
            fs::create_dir_all(bin.parent().unwrap()).unwrap();
            fs::write(&bin, "#!/bin/sh\n").unwrap();
            assert_eq!(
                restart_bin(Some(root.to_str().unwrap()), Some("/old/grokhub")),
                bin.to_string_lossy().to_string()
            );
            assert_eq!(restart_bin(None, Some("/opt/grokhub")), "/opt/grokhub");
        }
        #[cfg(windows)]
        {
            let prev = std::env::var_os("LOCALAPPDATA");
            std::env::set_var("LOCALAPPDATA", &root);
            let bin = root.join("Programs").join("GrokHub").join("grokhub.exe");
            fs::create_dir_all(bin.parent().unwrap()).unwrap();
            fs::write(&bin, b"MZ").unwrap();
            assert_eq!(
                restart_bin(None, Some(r"C:\old\grokhub.exe")),
                bin.to_string_lossy().to_string()
            );
            assert_eq!(
                restart_bin(None, Some(r"C:\opt\grokhub.exe")),
                bin.to_string_lossy().to_string(),
                "Windows overlay must win over current_exe"
            );
            match prev {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }
        assert_eq!(
            restart_argv("/opt/grokhub", false),
            vec!["/opt/grokhub".to_string()]
        );
        assert_eq!(
            restart_argv("/opt/grokhub", true),
            vec!["/opt/grokhub".to_string(), "--agent".into()]
        );
        assert_eq!(
            restart_acts(true, true, "/opt/grokhub", false),
            vec![
                RestartAct::Systemd {
                    units: vec!["grokhub-hub.service".into()]
                },
                RestartAct::Spawn {
                    argv: vec!["/opt/grokhub".into()]
                }
            ]
        );
        assert_eq!(
            restart_acts(true, false, "/opt/grokhub", true),
            vec![
                RestartAct::Systemd {
                    units: vec!["grokhub-hub.service".into()]
                },
                RestartAct::Spawn {
                    argv: vec!["/opt/grokhub".into(), "--agent".into()]
                }
            ]
        );
        assert_eq!(
            restart_acts(false, false, "/opt/grokhub", false),
            vec![RestartAct::Spawn {
                argv: vec!["/opt/grokhub".into()]
            }]
        );
        assert!(
            !restart_acts(true, true, "/opt/grokhub", false)
                .iter()
                .any(|a| match a {
                    RestartAct::Systemd { units } => units.iter().any(|u| u == "grokhub.service"),
                    RestartAct::Spawn { .. } => false,
                }),
            "cabin must spawn a new process, not systemctl restart grokhub.service"
        );
        assert_eq!(
            systemd_user_restart_args(&["ydotoold.service".into(), "grokhub-hub.service".into()]),
            vec![
                "--user",
                "restart",
                "ydotoold.service",
                "grokhub-hub.service"
            ]
        );
        assert_eq!(
            systemd_user_stop_args("grokhub-hub.service"),
            vec![
                "--user".to_string(),
                "stop".into(),
                "grokhub-hub.service".into()
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn beta_update_always_pulls_beta_and_stable_runs_only_what_is_newer() {
        use UpdatePending::*;
        assert_eq!(pending_on_channel(None, Channel::Beta), Cabin);
        assert_eq!(pending_on_channel(Cabin, Channel::Beta), Cabin);
        assert_eq!(pending_on_channel(None, Channel::Stable), None);
        assert_eq!(pending_on_channel(Cabin, Channel::Stable), Cabin);
        // A beta build carries the released version, so the tag compare says current.
        let same = pending_from_versions("2.10.92", Some("v2.10.92"));
        assert_eq!(same, None);
        assert_eq!(pending_on_channel(same, Channel::Beta), Cabin);
        assert_eq!(pending_from_versions("2.10.92", Some("v2.10.93")), Cabin);
        assert_eq!(pending_from_versions("2.10.92", Option::None), None);
        assert_eq!(pending_for_manual_update(None), Cabin);
        assert_eq!(update_chip_label(None), Option::None);
        assert_eq!(update_chip_label(Cabin), Some("Update cabin"));
        assert_eq!(settings_update_label(None), "Update");
        assert_eq!(update_hint(None), "Loadout is current.");
        assert_eq!(update_hint(Cabin), "Updates the cabin, then Restart.");
        assert_eq!(
            settings_update_hint(None),
            "Loadout looks current. Update still overlays so a missed probe can land."
        );
    }

    #[test]
    fn overlay_step_labels_name_pull_and_install() {
        assert_eq!(
            update_step_label("git -C '/x' pull --ff-only origin main"),
            "Pulling origin/main…"
        );
        assert_eq!(
            update_step_label("git -C '/x' pull --ff-only origin beta"),
            "Pulling origin/beta…"
        );
        let steps = update_plan_steps(vec![
            "git -C '/x' pull --ff-only origin beta".into(),
            "git -C '/x' pull --ff-only origin main".into(),
        ]);
        assert_eq!(steps[0].explain, "fast-forward origin/beta — config stays");
        assert_eq!(steps[1].explain, "fast-forward origin/main — config stays");
        assert_eq!(
            update_step_label("'/x/scripts/install.sh' --user"),
            "Installing overlay…"
        );
        assert_eq!(
            update_step_label(crate::release_verify::RELEASE_INSTALL_STEP),
            "Downloading the verified release…"
        );
        assert_eq!(update_step_label("echo hi"), "Updating…");
        assert_eq!(
            update_step_label("git -C '/x' remote set-url origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "Retargeting origin…"
        );
    }

    #[test]
    fn overlay_retargets_origin_clone_to_github() {
        assert!(origin_needs_retarget(
            "https://github.com/blackviperxiii-ui/Grok-Hub.git"
        ));
        assert!(origin_needs_retarget(ORIGIN_REMOTE_URL));
        assert!(!origin_needs_retarget(GITHUB_REMOTE_URL));
        assert!(!origin_needs_retarget(
            "git@github.com:blackviperxiii-ui/GrokHub.git"
        ));
        assert!(origin_needs_retarget(
            "https://github.com/blackviperxiii-ui/GrokHub-Windows.git"
        ));
        assert!(!canonical_github_origin(
            "https://github.com/blackviperxiii-ui/GrokHub-Windows.git"
        ));
        assert!(origin_needs_retarget("https://example.invalid/grokhub.git"));
        assert!(canonical_github_origin(GITHUB_REMOTE_URL));
        assert!(canonical_github_origin(
            "git@github.com:blackviperxiii-ui/GrokHub.git"
        ));

        let root = std::env::temp_dir().join(format!("grokhub-src-gh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        seed_git_source(&root, "main");
        std::process::Command::new("git")
            .args(["remote", "add", "origin", ORIGIN_REMOTE_URL])
            .current_dir(&root)
            .status()
            .unwrap();
        let cmds = update_cmds(&root).unwrap();
        assert!(
            cmds[0]
                .contains("remote set-url origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "{cmds:?}"
        );
        assert!(cmds[1].contains("pull --ff-only origin main"), "{cmds:?}");
        assert_overlay_install(&cmds[2]);
        assert_eq!(cmds.len(), 3, "{cmds:?}");
        let plan = update_plan_steps(cmds);
        assert!(plan[0].explain.contains("GitHub"), "{plan:?}");

        std::process::Command::new("git")
            .args([
                "remote",
                "set-url",
                "origin",
                "https://github.com/blackviperxiii-ui/GrokHub-Windows.git",
            ])
            .current_dir(&root)
            .status()
            .unwrap();
        let from_windows = update_cmds(&root).unwrap();
        assert!(
            from_windows[0]
                .contains("remote set-url origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "{from_windows:?}"
        );
        assert!(!from_windows
            .iter()
            .any(|c| c.contains("GrokHub-Windows.git")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn leftover_windows_clone_uses_github_zip() {
        let root =
            std::env::temp_dir().join(format!("grokhub-src-leftover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        seed_git_source(&root, "cursor/windows-icon-tray-7fe9");
        let err = update_cmds(&root).unwrap_err();
        assert!(
            err.contains("cursor/windows-icon-tray-7fe9") && err.contains("checkout main"),
            "{err}"
        );
        assert!(
            !overlay_clone_usable(&root),
            "a leftover cursor/* branch is not a product checkout"
        );
        let win = update_cmds_for_host(Some(&root), true).expect("Setup cabin must still Update");
        assert_eq!(win, release_update_cmds());
        assert_eq!(
            win,
            vec!["grokhub: install the verified release".to_string()]
        );
        assert!(!update_wipes_config(&win));
        assert!(
            !win.iter()
                .any(|c| c.contains("pull --ff-only") || c.contains("checkout main")),
            "leftover clone must not become the overlay plan: {win:?}"
        );
        let unix = update_cmds_for_host(Some(&root), false).unwrap_err();
        assert!(
            unix.contains("cursor/windows-icon-tray-7fe9"),
            "Linux still requires a main checkout: {unix}"
        );
        std::process::Command::new("git")
            .args(["checkout", "-B", "main"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(overlay_clone_usable(&root));
        let on_main = update_cmds_for_host(Some(&root), true).expect("main clone overlays");
        assert!(
            on_main
                .iter()
                .any(|c| c.contains("pull --ff-only origin main")),
            "a real main checkout still overlays: {on_main:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_install_without_a_clone_updates_from_the_verified_release() {
        let release = vec!["grokhub: install the verified release".to_string()];
        assert_eq!(
            update_cmds_for_host(None, true).expect("windows host, no clone"),
            release
        );
        assert_eq!(
            update_cmds_for_host(None, false).expect("linux tarball install, no clone"),
            release
        );
        assert_eq!(update_cmds_for(None).expect("this host, no clone"), release);
        assert!(!update_wipes_config(&release));
        let plan = update_plan_steps(release.clone());
        assert_eq!(
            plan[0].explain,
            "download the latest release from GitHub, check its signed SHA256SUMS, swap it in — does not wipe config"
        );
        let rel = crate::release_verify::RELEASE_INSTALL_STEP;
        assert!(
            crate::forbidden_reason(rel).is_none(),
            "the release step must pass the host command guard: {:?}",
            crate::forbidden_reason(rel)
        );
        let install_win = include_str!("../../../scripts/install-windows.ps1");
        assert!(
            install_win.contains("Overlay-Locked") && install_win.contains("Rename-Item"),
            "clone overlay must replace a running grokhub.exe: {install_win}"
        );
        assert_eq!(release_update_cmds(), vec![rel.to_string()]);
        for target in [Channel::Beta, Channel::Stable] {
            let switch = crate::release_verify::release_switch_step(target);
            assert!(crate::forbidden_reason(&switch).is_none(), "{switch}");
            assert!(cabin_overlay_step(&switch), "{switch}");
            assert!(
                !update_wipes_config(std::slice::from_ref(&switch)),
                "{switch}"
            );
        }
        assert_eq!(
            update_step_label("grokhub: switch to the beta release"),
            "Downloading the beta release…"
        );
        assert_eq!(
            update_plan_steps(vec!["grokhub: switch to the stable release".into()])[0].explain,
            "download the latest stable release, check its signed SHA256SUMS, swap it in, then switch to stable — does not wipe config"
        );
        assert!(
            settings_update_note().contains("SHA256SUMS"),
            "{}",
            settings_update_note()
        );
        assert!(
            !settings_update_note().contains("grok"),
            "{}",
            settings_update_note()
        );
        let win_install = include_str!("update.rs")
            .split("fn overlay_install_cmd(")
            .nth(1)
            .and_then(|s| s.split("pub fn update_plan_steps(").next())
            .expect("overlay_install_cmd");
        assert!(
            win_install.contains("Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass")
                && win_install.contains("& {install}")
                && !win_install.contains("-File {install}"),
            "clone overlay must Bypass Restricted without nesting powershell: {win_install}"
        );
    }

    #[test]
    fn notify_when_newer() {
        assert_eq!(
            parse_cabin_semver("2.9.5"),
            Some(CabinSemver {
                major: 2,
                minor: 9,
                patch: 5,
                beta: None
            })
        );
        assert_eq!(
            parse_cabin_semver("v2.9.6"),
            Some(CabinSemver {
                major: 2,
                minor: 9,
                patch: 6,
                beta: None
            })
        );
        assert_eq!(
            parse_cabin_semver("V2.10.0"),
            Some(CabinSemver {
                major: 2,
                minor: 10,
                patch: 0,
                beta: None
            })
        );
        assert!(cabin_version_newer("v2.9.6", "2.9.5"));
        assert!(cabin_version_newer("2.10.0", "v2.9.5"));
        assert!(!cabin_version_newer("v2.9.5", "2.9.5"));
        assert!(!cabin_version_newer("v2.9.4", "2.9.5"));
        assert!(!should_notify_cabin_update("2.9.5", None));
        assert!(!should_notify_cabin_update("2.9.5", Some("v2.9.5")));
        assert!(!should_notify_cabin_update("2.9.5", Some("v2.9.4")));
        assert!(should_notify_cabin_update("2.9.5", Some("v2.9.6")));
        assert!(should_notify_cabin_update("2.9.5", Some("2.10.0")));
        assert!(!should_notify_cabin_update("2.9.5", Some("not-a-version")));
        assert_eq!(
            parse_github_latest_tag(r#"{"tag_name":"v2.9.6","name":"GrokHub 2.9.6"}"#).as_deref(),
            Some("v2.9.6")
        );
        assert!(parse_github_latest_tag("{}").is_none());
        assert!(parse_github_latest_tag("not-json").is_none());
        let notice = cabin_update_notice("2.9.5", "v2.9.6");
        assert!(
            notice.contains("v2.9.6") && notice.contains("2.9.5"),
            "{notice}"
        );
        assert!(
            !notice.contains("http") && !notice.contains("github.com"),
            "notify is in-app, not a web page: {notice}"
        );
        assert_eq!(
            GITHUB_LATEST_API,
            "https://api.github.com/repos/blackviperxiii-ui/GrokHub/releases/latest"
        );
    }

    #[test]
    fn semver_prerelease_order_and_channel_rules() {
        let p = |s: &str| parse_cabin_semver(s).expect(s);
        assert!(p("2.13.0-beta.2") < p("2.13.0-beta.10"));
        assert!(p("2.13.0-beta.10") < p("2.13.0"));
        assert!(p("2.13.0") < p("2.13.1"));
        // Numeric prerelease, not lexicographic: .10 > .2
        assert!(cabin_version_newer("2.13.0-beta.10", "2.13.0-beta.2"));
        assert!(!cabin_version_newer("2.13.0-beta.2", "2.13.0-beta.10"));
        // Release beats any beta of the same base.
        assert!(cabin_version_newer("2.13.0", "2.13.0-beta.10"));
        assert!(cabin_version_newer("v2.13.0", "2.13.0-beta.2"));
        // Stable users are never offered a prerelease.
        assert!(!cabin_version_newer("2.13.1-beta.1", "2.13.0"));
        assert!(!cabin_version_newer("2.14.0-beta.5", "2.13.1"));
        assert!(!should_notify_cabin_update("2.13.1", Some("2.13.2-beta.1")));
        // Beta → newer beta and beta → release.
        assert!(should_notify_cabin_update(
            "2.13.0-beta.2",
            Some("2.13.0-beta.10")
        ));
        assert!(should_notify_cabin_update(
            "2.13.0-beta.10",
            Some("v2.13.0")
        ));
        // main → beta (Labs): a stable install does not take a prerelease via notify.
        assert!(!should_notify_cabin_update("2.13.0", Some("2.13.0-beta.1")));
        // Auto-off when main reaches the beta base, not before.
        assert!(beta_auto_off_for_versions("2.13.0-beta.2", "2.13.0"));
        assert!(beta_auto_off_for_versions("2.13.0-beta.10", "2.13.1"));
        assert!(!beta_auto_off_for_versions("2.13.0-beta.2", "2.12.9"));
        assert!(!beta_auto_off_for_versions("2.13.0", "2.13.0")); // already stable
        assert!(!beta_auto_off_for_versions(
            "2.13.0-beta.2",
            "2.13.0-beta.9"
        ));
        assert_eq!(
            p("2.13.0-beta"),
            CabinSemver {
                major: 2,
                minor: 13,
                patch: 0,
                beta: Some(0)
            }
        );
        assert_eq!(
            p("2.13.0-beta.2 (abc1234)"),
            CabinSemver {
                major: 2,
                minor: 13,
                patch: 0,
                beta: Some(2)
            }
        );
    }

    #[test]
    fn update_checks_every_two_hours() {
        assert_eq!(UPDATE_CHECK_EVERY, Duration::from_secs(2 * 60 * 60));
        let now = Instant::now();
        assert!(update_check_due(None, now, UPDATE_CHECK_EVERY));
        assert!(!update_check_due(Some(now), now, UPDATE_CHECK_EVERY));
        assert!(update_check_due(
            Some(now),
            now + Duration::from_secs(1),
            Duration::from_millis(1)
        ));
    }
}

#[cfg(test)]
mod channel_tip_tests {
    use super::*;

    /// Temp fixture: a bare `origin` with `main` and `beta` (both at a seed
    /// commit), a `work` clone that moves them, and a GrokHub-shaped `client`
    /// clone whose `origin/*` refs stay stale until it fetches.
    struct TipsFixture {
        base: PathBuf,
        work: PathBuf,
        client: PathBuf,
    }

    fn git_ok(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    impl TipsFixture {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir()
                .join(format!("grokhub-channel-tips-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            let (origin, work) = (base.join("origin.git"), base.join("work"));
            std::fs::create_dir_all(work.join("scripts")).unwrap();
            std::fs::create_dir_all(work.join("crates/grokhub-app")).unwrap();
            std::fs::write(work.join("Cargo.toml"), "[workspace]\n").unwrap();
            std::fs::write(work.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
            std::fs::write(work.join("crates/grokhub-app/lib.rs"), "\n").unwrap();
            git_ok(&base, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
            git_ok(&work, &["init", "-q", "-b", "main"]);
            git_ok(&work, &["config", "user.email", "cabin@test"]);
            git_ok(&work, &["config", "user.name", "Cabin"]);
            git_ok(&work, &["config", "commit.gpgsign", "false"]);
            git_ok(&work, &["add", "."]);
            git_ok(&work, &["commit", "-q", "-m", "seed"]);
            git_ok(
                &work,
                &["remote", "add", "origin", &origin.display().to_string()],
            );
            git_ok(&work, &["push", "-q", "origin", "main", "main:beta"]);
            git_ok(&base, &["clone", "-q", "origin.git", "client"]);
            let client = base.join("client");
            Self { base, work, client }
        }

        fn commit_on(&self, branch: &str, file: &str) {
            git_ok(
                &self.work,
                &["checkout", "-q", "-B", branch, &format!("origin/{branch}")],
            );
            std::fs::write(self.work.join(file), file).unwrap();
            git_ok(&self.work, &["add", "."]);
            git_ok(&self.work, &["commit", "-q", "-m", file]);
            git_ok(&self.work, &["push", "-q", "origin", branch]);
            git_ok(&self.work, &["fetch", "-q", "origin"]);
        }

        /// Squash promote beta → main, then merge-commit sync main → beta (#507 / #508).
        fn squash_promote_and_merge_sync(&self) {
            git_ok(&self.work, &["checkout", "-q", "-B", "main", "origin/main"]);
            git_ok(&self.work, &["merge", "-q", "--squash", "origin/beta"]);
            git_ok(&self.work, &["commit", "-q", "-m", "promote (squash)"]);
            git_ok(&self.work, &["checkout", "-q", "-B", "beta", "origin/beta"]);
            git_ok(
                &self.work,
                &[
                    "merge",
                    "-q",
                    "--no-ff",
                    "main",
                    "-m",
                    "sync main into beta",
                ],
            );
            git_ok(&self.work, &["push", "-q", "origin", "main", "beta"]);
            git_ok(&self.work, &["fetch", "-q", "origin"]);
        }
    }

    impl Drop for TipsFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn same_tree_different_sha_after_squash_and_merge_sync_is_caught_up() {
        let fx = TipsFixture::new("same-tree");
        fx.commit_on("beta", "feature.txt");
        // Opted in to Beta before the promote: the baseline is main's old tree.
        let before_promote = git_out(&fx.work, &["rev-parse", "origin/main^{tree}"]);
        fx.squash_promote_and_merge_sync();
        let tips = fetch_channel_tips(&fx.client).expect("fetch");
        // The fetch moved the stale client refs to the real tips.
        assert_eq!(
            tips.beta_sha,
            git_out(&fx.work, &["rev-parse", "origin/beta"])
        );
        assert_eq!(
            tips.main_sha,
            git_out(&fx.work, &["rev-parse", "origin/main"])
        );
        assert_ne!(
            tips.beta_sha, tips.main_sha,
            "squash + merge commit: SHAs differ"
        );
        assert_eq!(tips.beta_tree, tips.main_tree, "same code: trees match");
        assert_eq!(
            tips.beta_tree,
            git_out(&fx.work, &["rev-parse", "origin/beta^{tree}"])
        );
        assert!(crate::channel::beta_caught_up_to_main(&tips));
        assert_ne!(
            before_promote, tips.main_tree,
            "the promote moved main's tree"
        );
        assert_eq!(
            crate::channel::auto_off_step(Channel::Beta, &tips, Some(&before_promote)),
            crate::channel::AutoOffStep::SwitchToStable
        );
        // Opted in after the sync (beta == main already): record and stay on.
        assert_eq!(
            crate::channel::auto_off_step(Channel::Beta, &tips, None),
            crate::channel::AutoOffStep::RecordBaseline(tips.main_tree.clone())
        );
        assert_eq!(
            crate::channel::auto_off_step(Channel::Beta, &tips, Some(&tips.main_tree)),
            crate::channel::AutoOffStep::Stay
        );
    }

    #[test]
    fn different_trees_are_not_caught_up() {
        let fx = TipsFixture::new("diff-tree");
        fx.commit_on("beta", "feature.txt");
        fx.squash_promote_and_merge_sync();
        fx.commit_on("beta", "next-beta-pr.txt");
        let tips = fetch_channel_tips(&fx.client).expect("fetch");
        assert_ne!(tips.beta_sha, tips.main_sha);
        assert_ne!(tips.beta_tree, tips.main_tree);
        assert!(!crate::channel::beta_caught_up_to_main(&tips));
        let seed_tree = git_out(&fx.work, &["rev-parse", "origin/main~1^{tree}"]);
        assert_eq!(
            crate::channel::auto_off_step(Channel::Beta, &tips, Some(&seed_tree)),
            crate::channel::AutoOffStep::Stay
        );
    }

    #[test]
    fn same_sha_is_caught_up() {
        let fx = TipsFixture::new("same-sha");
        fx.commit_on("main", "hotfix.txt");
        // Fast-forward beta to main: one commit on both branches.
        git_ok(
            &fx.work,
            &["push", "-q", "origin", "origin/main:refs/heads/beta"],
        );
        let tips = fetch_channel_tips(&fx.client).expect("fetch");
        assert_eq!(tips.beta_sha, tips.main_sha);
        assert_eq!(
            tips.main_sha,
            git_out(&fx.work, &["rev-parse", "origin/main"])
        );
        assert!(crate::channel::beta_caught_up_to_main(&tips));
    }

    #[test]
    fn fetch_or_ref_errors_are_not_caught_up_and_skip_stale_refs() {
        let fx = TipsFixture::new("fetch-error");
        // Stale cached refs say "same commit" — a fallback to them would flip.
        let (b, m) = remote_tracking_tips(&fx.client).unwrap();
        assert_eq!(b, m);
        // Ref error: origin no longer has beta.
        git_ok(&fx.work, &["push", "-q", "origin", "--delete", "beta"]);
        assert!(fetch_channel_tips(&fx.client).is_err());
        // Fetch error: origin unreachable (offline).
        let gone = fx.base.join("gone.git").display().to_string();
        git_ok(&fx.client, &["remote", "set-url", "origin", &gone]);
        assert!(fetch_channel_tips(&fx.client).is_err());
        // Not a GrokHub clone.
        assert!(fetch_channel_tips(&fx.base).is_err());
    }

    #[test]
    fn switch_clone_to_main_moves_a_beta_clone_onto_origin_main() {
        let fx = TipsFixture::new("switch-main");
        git_ok(&fx.client, &["checkout", "-q", "beta"]);
        fx.commit_on("beta", "feature.txt");
        fx.squash_promote_and_merge_sync();
        let tips = fetch_channel_tips(&fx.client).expect("fetch");
        assert!(crate::channel::beta_caught_up_to_main(&tips));
        switch_clone_to_main(&fx.client).expect("switch");
        assert_eq!(
            git_out(&fx.client, &["symbolic-ref", "--short", "HEAD"]),
            "main"
        );
        assert_eq!(git_out(&fx.client, &["rev-parse", "HEAD"]), tips.main_sha);
        assert_eq!(
            git_out(
                &fx.client,
                &["rev-parse", "--abbrev-ref", "main@{upstream}"]
            ),
            "origin/main"
        );
        // The next stable Update can use this clone; a second call is a no-op.
        assert!(overlay_clone_usable_in(&fx.client, Channel::Stable));
        assert!(update_cmds_in(&fx.client, Channel::Stable).is_ok());
        switch_clone_to_main(&fx.client).expect("already on main");
    }

    #[test]
    fn switch_clone_to_main_refuses_dirty_other_branch_and_diverged_main() {
        let fx = TipsFixture::new("switch-refuse");
        git_ok(&fx.client, &["config", "user.email", "cabin@test"]);
        git_ok(&fx.client, &["config", "user.name", "Cabin"]);
        git_ok(&fx.client, &["config", "commit.gpgsign", "false"]);
        git_ok(&fx.client, &["checkout", "-q", "beta"]);
        // Dirty tree: refused, still on beta, edit kept.
        std::fs::write(fx.client.join("Cargo.toml"), "[workspace] # local edit\n").unwrap();
        let err = switch_clone_to_main(&fx.client).unwrap_err();
        assert!(err.contains("uncommitted changes"), "{err}");
        assert_eq!(
            git_out(&fx.client, &["symbolic-ref", "--short", "HEAD"]),
            "beta"
        );
        assert_eq!(
            std::fs::read_to_string(fx.client.join("Cargo.toml")).unwrap(),
            "[workspace] # local edit\n"
        );
        git_ok(&fx.client, &["checkout", "-q", "--", "Cargo.toml"]);
        // Local main with its own commit: refused.
        git_ok(&fx.client, &["checkout", "-q", "main"]);
        std::fs::write(fx.client.join("local.txt"), "local").unwrap();
        git_ok(&fx.client, &["add", "."]);
        git_ok(&fx.client, &["commit", "-q", "-m", "local only"]);
        let local_main = git_out(&fx.client, &["rev-parse", "main"]);
        git_ok(&fx.client, &["checkout", "-q", "beta"]);
        let err = switch_clone_to_main(&fx.client).unwrap_err();
        assert!(err.contains("not on origin/main"), "{err}");
        assert_eq!(
            git_out(&fx.client, &["symbolic-ref", "--short", "HEAD"]),
            "beta"
        );
        assert_eq!(git_out(&fx.client, &["rev-parse", "main"]), local_main);
        // Some other branch: refused.
        git_ok(&fx.client, &["checkout", "-q", "-b", "cabin/feature"]);
        assert!(switch_clone_to_main(&fx.client).is_err());
        assert!(switch_clone_to_main(&fx.base).is_err());
    }

    #[test]
    fn parse_ls_remote_tips_reads_beta_and_main() {
        let out = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/heads/beta\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/heads/main\n";
        assert_eq!(
            parse_ls_remote_tips(out),
            (
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into()
            )
        );
        assert_eq!(parse_ls_remote_tips(""), (String::new(), String::new()));
        assert_eq!(
            parse_ls_remote_tips("deadbeef refs/heads/beta\n"),
            ("deadbeef".into(), String::new())
        );
    }

    #[test]
    fn parse_clone_recover_step_reads_channel_and_path() {
        let step = clone_recover_step(Path::new("/tmp/GrokHub"), Channel::Beta);
        assert_eq!(
            parse_clone_recover_step(&step),
            Some((Channel::Beta, PathBuf::from("/tmp/GrokHub")))
        );
        assert!(parse_clone_recover_step("git pull").is_none());
        assert!(cabin_overlay_step(&step));
        assert_eq!(update_step_label(&step), "Saving prior clone, resetting…");
    }

    #[test]
    fn recover_diverged_clone_saves_old_head_on_backup_and_resets() {
        let fx = TipsFixture::new("recover-diverged");
        git_ok(&fx.client, &["config", "user.email", "cabin@test"]);
        git_ok(&fx.client, &["config", "user.name", "Cabin"]);
        git_ok(&fx.client, &["config", "commit.gpgsign", "false"]);
        // Advance origin/main past the client, then give the client its own commit.
        fx.commit_on("main", "upstream.txt");
        let tip = git_out(&fx.work, &["rev-parse", "origin/main"]);
        git_ok(&fx.client, &["checkout", "-q", "main"]);
        std::fs::write(fx.client.join("local-only.txt"), "mine").unwrap();
        git_ok(&fx.client, &["add", "."]);
        git_ok(&fx.client, &["commit", "-q", "-m", "local diverge"]);
        let old = git_out(&fx.client, &["rev-parse", "HEAD"]);
        assert!(clone_needs_recover(&fx.client, Channel::Stable).unwrap());
        let bak = recover_clone_if_needed(&fx.client, Channel::Stable)
            .expect("recover")
            .expect("backup branch");
        assert!(bak.starts_with("grokhub-backup/"), "{bak}");
        assert_eq!(git_out(&fx.client, &["rev-parse", bak.as_str()]), old);
        assert_eq!(git_out(&fx.client, &["rev-parse", "HEAD"]), tip);
        assert!(!clone_needs_recover(&fx.client, Channel::Stable).unwrap());
        let plan = update_cmds_in(&fx.client, Channel::Stable).unwrap();
        assert!(
            plan.iter()
                .any(|c| c.contains("pull --ff-only origin main")),
            "clean after recover uses pull: {plan:?}"
        );
        assert!(
            plan.iter().all(|c| parse_clone_recover_step(c).is_none()),
            "{plan:?}"
        );
    }

    #[test]
    fn recover_dirty_clone_keeps_changes_on_backup_branch() {
        let fx = TipsFixture::new("recover-dirty");
        git_ok(&fx.client, &["config", "user.email", "cabin@test"]);
        git_ok(&fx.client, &["config", "user.name", "Cabin"]);
        git_ok(&fx.client, &["config", "commit.gpgsign", "false"]);
        fx.commit_on("main", "upstream.txt");
        let tip = git_out(&fx.work, &["rev-parse", "origin/main"]);
        git_ok(&fx.client, &["fetch", "-q", "origin"]);
        git_ok(&fx.client, &["checkout", "-q", "main"]);
        // Still at seed tip locally until recover fetches; make the tree dirty.
        std::fs::write(fx.client.join("Cargo.toml"), "[workspace] # dirty\n").unwrap();
        std::fs::write(fx.client.join("untracked.txt"), "new").unwrap();
        assert!(clone_needs_recover(&fx.client, Channel::Stable).unwrap());
        let plan = update_cmds_in(&fx.client, Channel::Stable).unwrap();
        assert!(
            plan.iter().any(|c| parse_clone_recover_step(c).is_some()),
            "dirty plan uses recover step: {plan:?}"
        );
        let bak = recover_clone_if_needed(&fx.client, Channel::Stable)
            .expect("recover")
            .expect("backup");
        assert_eq!(git_out(&fx.client, &["rev-parse", "HEAD"]), tip);
        // Windows checkouts may rewrite LF → CRLF; assert the dirty edit is gone.
        let cargo = std::fs::read_to_string(fx.client.join("Cargo.toml"))
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(cargo, "[workspace]\n");
        assert!(!fx.client.join("untracked.txt").exists());
        // Backup tip has the saved dirty commit.
        let tree = git_out(&fx.client, &["ls-tree", "-r", "--name-only", bak.as_str()]);
        assert!(tree.contains("untracked.txt"), "{tree}");
        let show = git_out(&fx.client, &["show", &format!("{bak}:Cargo.toml")]);
        assert!(show.contains("dirty"), "{show}");
    }

    #[test]
    fn clean_clone_update_plan_stays_on_fast_forward() {
        let fx = TipsFixture::new("recover-clean");
        git_ok(&fx.client, &["checkout", "-q", "main"]);
        assert!(!clone_needs_recover(&fx.client, Channel::Stable).unwrap());
        assert_eq!(
            recover_clone_if_needed(&fx.client, Channel::Stable).unwrap(),
            None
        );
        let plan = update_cmds_in(&fx.client, Channel::Stable).unwrap();
        assert!(
            plan.iter()
                .any(|c| c.contains("pull --ff-only origin main")),
            "{plan:?}"
        );
        assert!(
            plan.iter().all(|c| parse_clone_recover_step(c).is_none()),
            "{plan:?}"
        );
        let branches = git_out(&fx.client, &["branch", "--list", "grokhub-backup/*"]);
        assert!(branches.trim().is_empty(), "no backup on clean: {branches}");
    }

    #[test]
    fn recover_stable_resets_to_origin_main_not_an_old_tag() {
        let fx = TipsFixture::new("recover-not-tag");
        git_ok(&fx.client, &["config", "user.email", "cabin@test"]);
        git_ok(&fx.client, &["config", "user.name", "Cabin"]);
        git_ok(&fx.client, &["config", "commit.gpgsign", "false"]);
        // An old release tag on the seed commit must not become the recover tip.
        git_ok(&fx.work, &["tag", "v2.10.0", "origin/main"]);
        git_ok(&fx.work, &["push", "-q", "origin", "v2.10.0"]);
        fx.commit_on("main", "newer-main.txt");
        let tip = git_out(&fx.work, &["rev-parse", "origin/main"]);
        let tag = git_out(&fx.work, &["rev-parse", "v2.10.0"]);
        assert_ne!(tip, tag, "main moved past the tag");
        git_ok(&fx.client, &["checkout", "-q", "main"]);
        std::fs::write(fx.client.join("local-only.txt"), "mine").unwrap();
        git_ok(&fx.client, &["add", "."]);
        git_ok(&fx.client, &["commit", "-q", "-m", "local diverge"]);
        // Fetch the tag into the client so resolve would see it if it looked.
        git_ok(&fx.client, &["fetch", "-q", "origin", "--tags"]);
        let bak = recover_clone_if_needed(&fx.client, Channel::Stable)
            .expect("recover")
            .expect("backup");
        assert!(bak.starts_with("grokhub-backup/"), "{bak}");
        assert_eq!(git_out(&fx.client, &["rev-parse", "HEAD"]), tip);
        assert_ne!(git_out(&fx.client, &["rev-parse", "HEAD"]), tag);
        assert!(!clone_needs_recover(&fx.client, Channel::Stable).unwrap());
    }

    #[test]
    fn recover_backup_commit_succeeds_when_gpgsign_is_on() {
        let fx = TipsFixture::new("recover-gpg");
        git_ok(&fx.client, &["config", "user.email", "cabin@test"]);
        git_ok(&fx.client, &["config", "user.name", "Cabin"]);
        // Global-style signing on — recover must still commit with --no-gpg-sign.
        git_ok(&fx.client, &["config", "commit.gpgsign", "true"]);
        fx.commit_on("main", "upstream.txt");
        let tip = git_out(&fx.work, &["rev-parse", "origin/main"]);
        git_ok(&fx.client, &["checkout", "-q", "main"]);
        std::fs::write(fx.client.join("dirty.txt"), "x").unwrap();
        let bak = recover_clone_if_needed(&fx.client, Channel::Stable)
            .expect("recover despite gpgsign")
            .expect("backup");
        assert_eq!(git_out(&fx.client, &["rev-parse", "HEAD"]), tip);
        let tree = git_out(&fx.client, &["ls-tree", "-r", "--name-only", bak.as_str()]);
        assert!(tree.contains("dirty.txt"), "{tree}");
    }

    #[test]
    fn single_branch_clone_can_switch_to_beta_and_back() {
        let fx = TipsFixture::new("single-branch");
        let single = fx.base.join("single");
        git_ok(
            &fx.base,
            &[
                "clone",
                "-q",
                "--single-branch",
                "-b",
                "main",
                "origin.git",
                "single",
            ],
        );
        git_ok(&single, &["config", "user.email", "cabin@test"]);
        git_ok(&single, &["config", "user.name", "Cabin"]);
        git_ok(&single, &["config", "commit.gpgsign", "false"]);
        // No local beta yet — same failure mode as a --single-branch user clone.
        assert!(!Command::new("git")
            .args(["show-ref", "--verify", "--quiet", "refs/heads/beta"])
            .current_dir(&single)
            .status()
            .unwrap()
            .success());
        // Mirror install.sh: add fetch refspec, fetch, checkout -B --track.
        git_ok(
            &single,
            &[
                "config",
                "--add",
                "remote.origin.fetch",
                "+refs/heads/beta:refs/remotes/origin/beta",
            ],
        );
        git_ok(
            &single,
            &[
                "fetch",
                "-q",
                "origin",
                "+refs/heads/beta:refs/remotes/origin/beta",
            ],
        );
        git_ok(
            &single,
            &["checkout", "-q", "-B", "beta", "--track", "origin/beta"],
        );
        assert_eq!(
            git_out(&single, &["symbolic-ref", "--short", "HEAD"]),
            "beta"
        );
        assert_eq!(
            git_out(&single, &["rev-parse", "--abbrev-ref", "beta@{upstream}"]),
            "origin/beta"
        );
        let tips = fetch_channel_tips(&single).expect("fetch tips");
        switch_clone_to_main(&single).expect("back to main");
        assert_eq!(
            git_out(&single, &["symbolic-ref", "--short", "HEAD"]),
            "main"
        );
        assert_eq!(git_out(&single, &["rev-parse", "HEAD"]), tips.main_sha);
    }

    #[cfg(unix)]
    #[test]
    fn install_sh_skips_sudo_when_packages_present_or_no_tty() {
        let base = std::env::temp_dir().join(format!(
            "grokhub-pkg-skip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let bin = base.join("bin");
        let cfg = base.join("cfg");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&cfg).unwrap();
        // Shim pacman: -Q succeeds (packages present); -S would write a marker.
        std::fs::write(
            bin.join("pacman"),
            "#!/bin/sh\nif [ \"$1\" = -Q ]; then exit 0; fi\necho PACMAN \"$*\" >>\"$GROKHUB_PKG_LOG\"\nexit 0\n",
        )
        .unwrap();
        std::fs::write(
            bin.join("sudo"),
            "#!/bin/sh\necho SUDO \"$*\" >>\"$GROKHUB_PKG_LOG\"\nexec \"$@\"\n",
        )
        .unwrap();
        // Shim id -u: as root the block installs without asking, but this
        // case is a normal user, also when the tests run as root.
        std::fs::write(
            bin.join("id"),
            "#!/bin/sh\nif [ \"$1\" = -u ]; then echo 1000; else exec /usr/bin/id \"$@\"; fi\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["pacman", "sudo", "id"] {
                let mut perms = std::fs::metadata(bin.join(name)).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(bin.join(name), perms).unwrap();
            }
        }
        // Extract and run only the package block via a tiny harness: dry-run
        // never reaches try_pkgs, so the test runs that section on its own.
        let harness = base.join("pkg-harness.sh");
        let install_text = include_str!("../../../scripts/install.sh");
        let start = install_text
            .find("PKG_SKIP_FILE=")
            .expect("PKG_SKIP_FILE in install.sh");
        let end = install_text[start..]
            .find("\nif command -v systemctl")
            .map(|i| start + i)
            .expect("end of package block");
        let block = &install_text[start..end];
        std::fs::write(
            &harness,
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\nGROKHUB_CONFIG=\"{cfg}\"\nexport GROKHUB_CONFIG\n{block}\n",
                cfg = cfg.display()
            ),
        )
        .unwrap();
        let log = base.join("pkg.log");
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        // Packages present → pacman -Q succeeds → no sudo.
        let out = Command::new("bash")
            .arg(&harness)
            .env("PATH", &path)
            .env("GROKHUB_PKG_LOG", &log)
            .env("HOME", &base)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !log.exists() || std::fs::read_to_string(&log).unwrap().is_empty(),
            "no sudo when packages present"
        );
        // Missing packages + no TTY → note, no sudo.
        std::fs::write(
            bin.join("pacman"),
            "#!/bin/sh\nif [ \"$1\" = -Q ]; then exit 1; fi\necho PACMAN \"$*\" >>\"$GROKHUB_PKG_LOG\"\nexit 0\n",
        )
        .unwrap();
        let _ = std::fs::remove_file(&log);
        let out = Command::new("bash")
            .arg(&harness)
            .env("PATH", &path)
            .env("GROKHUB_PKG_LOG", &log)
            .env("HOME", &base)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "stderr={stderr}");
        assert!(
            stdout.contains("pkgs: skipped (no TTY)") || stderr.contains("pkgs: skipped (no TTY)"),
            "stdout={stdout} stderr={stderr}"
        );
        assert!(
            !log.exists() || !std::fs::read_to_string(&log).unwrap().contains("SUDO"),
            "no sudo without TTY"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
