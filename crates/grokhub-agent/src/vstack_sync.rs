//! Runtime HTTPS sync for upstream `blackviperxiii-ui/vstack` skills.
//!
//! Fetches the latest commit metadata on `main` and downloads the verified
//! source tarball when upstream advances, falling back cleanly to the bundled
//! snapshot when offline or unreachable. Storage, archive validation, and
//! atomic swaps live in `grokhub_core::vstack_sync`.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use grokhub_core::vstack_sync::{
    active_synced_skills_dir, install_archive, match_github_tag_for_commit,
    parse_github_commit_sha, read_state, record_check_result, sync_due, VstackStatus,
    VSTACK_ARCHIVE_CAP, VSTACK_BRANCH, VSTACK_REPO_NAME, VSTACK_REPO_OWNER,
};

use crate::builtin_skills::{BUILTIN_SKILL_BODIES, BUILTIN_VSTACK_COMMIT, BUILTIN_VSTACK_TAG};
use crate::harness as hx;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const API_OVERALL_TIMEOUT: Duration = Duration::from_secs(30);
const ARCHIVE_OVERALL_TIMEOUT: Duration = Duration::from_secs(120);
const API_CAP: usize = 2 * 1024 * 1024;
const REDIRECTS_MAX: usize = 4;

const ALLOWED_HOSTS: &[&str] = &[
    "api.github.com",
    "github.com",
    "codeload.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

fn commit_api_url() -> String {
    format!(
        "https://api.github.com/repos/{VSTACK_REPO_OWNER}/{VSTACK_REPO_NAME}/commits/{VSTACK_BRANCH}"
    )
}

fn tags_api_url() -> String {
    format!("https://api.github.com/repos/{VSTACK_REPO_OWNER}/{VSTACK_REPO_NAME}/tags?per_page=10")
}

fn archive_url(commit: &str) -> String {
    format!("https://codeload.github.com/{VSTACK_REPO_OWNER}/{VSTACK_REPO_NAME}/tar.gz/{commit}")
}

/// Validates that a URL (or redirect target) is HTTPS on one of GitHub's
/// official hosts for `blackviperxiii-ui/vstack` and obeys Offline only +
/// EgressGuard.
pub fn vstack_url_ok(config_dir: &Path, url: &str, what: &str) -> Result<(), String> {
    grokhub_core::offline::refuse()?;
    let trimmed = url.trim();
    let rest = trimmed
        .strip_prefix("https://")
        .ok_or_else(|| format!("{what} must use HTTPS"))?;
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| format!("{what}: missing host"))?;
    if authority.contains('@') {
        return Err(format!("{what}: userinfo in URL is not allowed"));
    }
    let host = authority
        .split(':')
        .next()
        .unwrap_or(authority)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if grokhub_core::ssrf::host_name_blocked(&host)
        || !ALLOWED_HOSTS.iter().any(|allowed| host == *allowed)
    {
        return Err(format!("{what}: host {host} is not allowed"));
    }
    hx::guard_quiet(config_dir, &hx::EgressReq::new(trimmed, &[]))
}

fn guarded_get(
    config_dir: &Path,
    url: &str,
    accept: &str,
    cap: usize,
    overall: Duration,
    what: &str,
) -> Result<Vec<u8>, String> {
    grokhub_core::offline::refuse()?;
    vstack_url_ok(config_dir, url, what)?;
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: CONNECT_TIMEOUT,
        overall: Some(overall),
        redirects: 0,
        public_only: true,
        ..Default::default()
    });
    let check = |hop: &str| vstack_url_ok(config_dir, hop, what);
    let headers = [("user-agent", crate::USER_AGENT), ("accept", accept)];
    let resp =
        grokhub_core::http::get_following(&agent, url, &headers, REDIRECTS_MAX, what, &check)?;
    if resp.status() != 200 {
        return Err(format!("{what}: GitHub answered HTTP {}", resp.status()));
    }
    let mut body = Vec::new();
    resp.into_reader()
        .take((cap + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|e| format!("{what}: {e}"))?;
    if body.len() > cap {
        return Err(format!("{what}: response exceeded size limit"));
    }
    Ok(body)
}

/// Abstracted upstream source so tests can exercise full sync, updates,
/// deletions, and failure recovery hermetically without live network I/O.
pub trait UpstreamSource {
    fn head_commit(&self) -> Result<String, String>;
    fn tag_for_commit(&self, commit: &str) -> Option<String>;
    fn archive(&self, commit: &str) -> Result<Vec<u8>, String>;
}

struct GithubUpstream<'a> {
    config_dir: &'a Path,
}

impl UpstreamSource for GithubUpstream<'_> {
    fn head_commit(&self) -> Result<String, String> {
        let body = guarded_get(
            self.config_dir,
            &commit_api_url(),
            "application/vnd.github+json",
            API_CAP,
            API_OVERALL_TIMEOUT,
            "while checking vstack head commit",
        )?;
        parse_github_commit_sha(&body)
    }

    fn tag_for_commit(&self, commit: &str) -> Option<String> {
        let body = guarded_get(
            self.config_dir,
            &tags_api_url(),
            "application/vnd.github+json",
            API_CAP,
            API_OVERALL_TIMEOUT,
            "while checking vstack tags",
        )
        .ok()?;
        match_github_tag_for_commit(&body, commit)
    }

    fn archive(&self, commit: &str) -> Result<Vec<u8>, String> {
        guarded_get(
            self.config_dir,
            &archive_url(commit),
            "application/octet-stream",
            VSTACK_ARCHIVE_CAP,
            ARCHIVE_OVERALL_TIMEOUT,
            "while downloading vstack skills archive",
        )
    }
}

/// Current status of built-in/synced vstack skills for Settings → Code.
pub fn status(config_dir: &Path) -> VstackStatus {
    grokhub_core::vstack_sync::status(
        config_dir,
        BUILTIN_VSTACK_TAG,
        BUILTIN_VSTACK_COMMIT,
        BUILTIN_SKILL_BODIES.len(),
    )
}

/// Sync immediately from `blackviperxiii-ui/vstack` `main`, returning the
/// refreshed status. Never wipes existing synced skills if the check or
/// download fails.
pub fn sync_now(config_dir: &Path) -> Result<VstackStatus, String> {
    let now_ms = grokhub_core::now_ms();
    sync_with(config_dir, now_ms, &GithubUpstream { config_dir })
}

/// Background check if the periodic interval has elapsed and Offline only is
/// off. Returns `Ok(None)` when skipped.
pub fn sync_if_due(config_dir: &Path, now_ms: u64) -> Result<Option<VstackStatus>, String> {
    if !sync_due(config_dir, now_ms) || grokhub_core::offline::refuse().is_err() {
        return Ok(None);
    }
    sync_with(config_dir, now_ms, &GithubUpstream { config_dir }).map(Some)
}

/// Execute one sync cycle against `src`.
pub fn sync_with(
    config_dir: &Path,
    now_ms: u64,
    src: &dyn UpstreamSource,
) -> Result<VstackStatus, String> {
    if let Err(err) = grokhub_core::offline::refuse() {
        let msg = err.to_string();
        let _ = record_check_result(config_dir, now_ms, Some(msg.clone()));
        return Err(msg);
    }

    let commit = match src.head_commit() {
        Ok(sha) => sha,
        Err(err) => {
            let _ = record_check_result(config_dir, now_ms, Some(err.clone()));
            return Err(err);
        }
    };

    if !grokhub_core::vstack_sync::is_sha40(&commit) {
        let msg = "upstream returned invalid commit SHA".to_string();
        let _ = record_check_result(config_dir, now_ms, Some(msg.clone()));
        return Err(msg);
    }

    let up_to_date = active_synced_skills_dir(config_dir, BUILTIN_VSTACK_TAG).is_some()
        && read_state(config_dir).is_some_and(|s| s.commit == commit);
    if up_to_date {
        let _ = record_check_result(config_dir, now_ms, None);
        return Ok(status(config_dir));
    }

    let archive = match src.archive(&commit) {
        Ok(bytes) => bytes,
        Err(err) => {
            let _ = record_check_result(config_dir, now_ms, Some(err.clone()));
            return Err(err);
        }
    };

    let tag = src.tag_for_commit(&commit);
    if let Err(err) = install_archive(config_dir, &archive, &commit, tag, now_ms) {
        let _ = record_check_result(config_dir, now_ms, Some(err.clone()));
        return Err(err);
    }

    crate::skills::invalidate_discover_cache();
    Ok(status(config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;

    const SHA_1: &str = "11cca98b4a55da3118f51e749ac62faefbc21b3b";
    const SHA_2: &str = "22cca98b4a55da3118f51e749ac62faefbc21b3b";

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-agent-vstack-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        grokhub_core::vstack_sync::tar_gz_for_test(entries).unwrap()
    }

    struct FakeUpstream {
        commit: Result<String, String>,
        tag: Option<String>,
        archive: Result<Vec<u8>, String>,
        archive_calls: Cell<usize>,
    }

    impl UpstreamSource for FakeUpstream {
        fn head_commit(&self) -> Result<String, String> {
            self.commit.clone()
        }

        fn tag_for_commit(&self, _commit: &str) -> Option<String> {
            self.tag.clone()
        }

        fn archive(&self, _commit: &str) -> Result<Vec<u8>, String> {
            self.archive_calls.set(self.archive_calls.get() + 1);
            self.archive.clone()
        }
    }

    #[test]
    fn url_allowlist_pins_official_github_vstack_hosts() {
        let cfg = scratch("allow");
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        assert!(vstack_url_ok(&cfg, &commit_api_url(), "test").is_ok());
        assert!(vstack_url_ok(&cfg, &tags_api_url(), "test").is_ok());
        assert!(vstack_url_ok(&cfg, &archive_url(SHA_1), "test").is_ok());
        assert!(vstack_url_ok(&cfg, "http://api.github.com/x", "test").is_err());
        assert!(vstack_url_ok(&cfg, "https://evil.example.com/vstack.tar.gz", "test").is_err());
        assert!(vstack_url_ok(&cfg, "https://localhost/vstack.tar.gz", "test").is_err());
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn sync_with_installs_skips_unchanged_and_preserves_on_failure() {
        let cfg = scratch("sync-flow");
        let root1 = format!("blackviperxiii-ui-vstack-{SHA_1}");
        // Fake versions must be >= BUILTIN_VSTACK_TAG or status falls back to bundled.
        let ar1 = make_tar_gz(&[
            (&format!("{root1}/VERSION"), b"1.6.0\n"),
            (
                &format!("{root1}/skills/alpha/SKILL.md"),
                b"---\nname: alpha\ndescription: A.\n---\nA1\n",
            ),
            (
                &format!("{root1}/skills/beta/SKILL.md"),
                b"---\nname: beta\ndescription: B.\n---\nB1\n",
            ),
        ]);
        let src1 = FakeUpstream {
            commit: Ok(SHA_1.into()),
            tag: Some("v1.6.0".into()),
            archive: Ok(ar1),
            archive_calls: Cell::new(0),
        };
        let st1 = sync_with(&cfg, 100_000, &src1).unwrap();
        assert_eq!(st1.source, "synced");
        assert_eq!(st1.commit, SHA_1);
        assert_eq!(st1.skill_count, 2);
        assert_eq!(src1.archive_calls.get(), 1);

        // Same commit again skips downloading tarball while updating check timestamp.
        let src_same = FakeUpstream {
            commit: Ok(SHA_1.into()),
            tag: Some("v1.6.0".into()),
            archive: Err("should not download".into()),
            archive_calls: Cell::new(0),
        };
        let st_same = sync_with(&cfg, 200_000, &src_same).unwrap();
        assert_eq!(src_same.archive_calls.get(), 0);
        assert_eq!(st_same.last_check_ms, Some(200_000));
        assert_eq!(st_same.last_error, None);

        // New commit with network failure preserves existing alpha & beta.
        let src_err = FakeUpstream {
            commit: Ok(SHA_2.into()),
            tag: None,
            archive: Err("HTTP 503".into()),
            archive_calls: Cell::new(0),
        };
        assert!(sync_with(&cfg, 300_000, &src_err).is_err());
        let st_err = status(&cfg);
        assert_eq!(st_err.source, "synced");
        assert_eq!(st_err.commit, SHA_1);
        assert_eq!(st_err.last_error.as_deref(), Some("HTTP 503"));
        assert!(grokhub_core::vstack_sync::skills_dir(&cfg)
            .join("beta/SKILL.md")
            .is_file());

        // Advancing upstream commit updates alpha, drops beta, adds gamma.
        let root2 = format!("blackviperxiii-ui-vstack-{SHA_2}");
        let ar2 = make_tar_gz(&[
            (&format!("{root2}/VERSION"), b"1.7.0\n"),
            (
                &format!("{root2}/skills/alpha/SKILL.md"),
                b"---\nname: alpha\ndescription: A.\n---\nA2\n",
            ),
            (
                &format!("{root2}/skills/gamma/SKILL.md"),
                b"---\nname: gamma\ndescription: G.\n---\nG1\n",
            ),
        ]);
        let src2 = FakeUpstream {
            commit: Ok(SHA_2.into()),
            tag: Some("v1.7.0".into()),
            archive: Ok(ar2),
            archive_calls: Cell::new(0),
        };
        let st2 = sync_with(&cfg, 400_000, &src2).unwrap();
        assert_eq!(st2.version, "1.7.0");
        assert_eq!(st2.commit, SHA_2);
        assert_eq!(st2.last_error, None);
        assert!(!grokhub_core::vstack_sync::skills_dir(&cfg)
            .join("beta")
            .exists());
        assert!(grokhub_core::vstack_sync::skills_dir(&cfg)
            .join("gamma/SKILL.md")
            .is_file());

        let _ = fs::remove_dir_all(&cfg);
    }
}
