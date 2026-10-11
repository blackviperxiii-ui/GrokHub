//! Signed release updates: pick the channel's GitHub release, find this
//! host's asset, and check it before anything is installed. `SHA256SUMS`
//! lists every asset's SHA-256 and `SHA256SUMS.minisig` signs that list
//! (see `.github/workflows/release.yml`). With a public key shipped, an
//! asset installs only when the signature verifies with that key and its
//! hash matches; anything else is refused with a named reason. No HTTP here.

use sha2::{Digest, Sha256};

use crate::Channel;

/// Newest first, drafts hidden for anonymous reads.
pub const GITHUB_RELEASES_API: &str = "https://api.github.com/repos/blackviperxiii-ui/GrokHub/releases?per_page=30";
pub const SUMS_NAME: &str = "SHA256SUMS";
pub const SIG_NAME: &str = "SHA256SUMS.minisig";

/// The release signing public key (`packaging/minisign.pub`). `None` until
/// the owner creates the key: until then updates check `SHA256SUMS` only.
/// `the_shipped_key_matches_packaging` fails when the file and this differ.
pub const RELEASE_PUBKEY: Option<&str> = None;

/// The update plan step the cabin runs itself (download, verify, install)
/// instead of handing it to a shell.
pub const RELEASE_INSTALL_STEP: &str = "grokhub: install the verified release";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub prerelease: bool,
    pub draft: bool,
    pub assets: Vec<ReleaseAsset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// "2.13.1" from `v2.13.1`, "2.13.1-beta (abc1234)" from `beta-v2.13.1-abc1234`.
    pub fn version(&self) -> String {
        if let Some(rest) = self.tag.strip_prefix("beta-v") {
            return match rest.rsplit_once('-') {
                Some((ver, sha)) => format!("{ver}-beta ({sha})"),
                None => format!("{rest}-beta"),
            };
        }
        self.tag.trim_start_matches('v').to_string()
    }
}

/// `GET …/releases` (a list) or one release object.
pub fn parse_releases(body: &str) -> Vec<Release> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let list = match v {
        serde_json::Value::Array(items) => items,
        one @ serde_json::Value::Object(_) => vec![one],
        _ => return Vec::new(),
    };
    list.iter()
        .filter_map(|r| {
            let tag = r.get("tag_name")?.as_str()?.trim().to_string();
            let assets = r
                .get("assets")
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            Some(ReleaseAsset {
                                name: x.get("name")?.as_str()?.to_string(),
                                url: x.get("browser_download_url")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Release {
                tag,
                prerelease: r.get("prerelease").and_then(|b| b.as_bool()).unwrap_or(false),
                draft: r.get("draft").and_then(|b| b.as_bool()).unwrap_or(false),
                assets,
            })
        })
        .collect()
}

/// Stable takes the newest full release (`v*`); beta the newest beta
/// pre-release (`beta-v*`). GitHub lists newest first.
pub fn pick_release(releases: &[Release], channel: Channel) -> Option<&Release> {
    releases.iter().find(|r| {
        !r.draft
            && match channel {
                Channel::Stable => !r.prerelease && r.tag.starts_with('v'),
                Channel::Beta => r.prerelease && r.tag.starts_with("beta-v"),
            }
    })
}

fn semver_name(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|r| r.strip_suffix(suffix))
        .is_some_and(|ver| {
            let parts: Vec<&str> = ver.split('.').collect();
            parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        })
}

/// The download this host installs: the Linux tarball or the Windows zip.
pub fn host_asset(release: &Release, windows: bool) -> Option<&ReleaseAsset> {
    let (prefix, suffix) = if windows { ("grokhub-windows-v", ".zip") } else { ("grokhub-linux-v", ".tar.gz") };
    release.assets.iter().find(|a| semver_name(&a.name, prefix, suffix))
}

/// Only this repo's release downloads, and the URL must end in the asset's name.
pub fn download_url_ok(url: &str, name: &str) -> bool {
    let from_repo = url.starts_with("https://github.com/blackviperxiii-ui/GrokHub/releases/download/");
    from_repo && url.ends_with(&format!("/{name}")) && !name.contains('/') && !name.contains("..")
}

/// Why an update was not installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NoRelease,
    NoAsset,
    BadUrl,
    NoSums,
    NoSignature,
    BadSignature,
    WrongKey,
    NotListed,
    HashMismatch,
    Download(String),
}

impl Refusal {
    pub fn reason(&self) -> String {
        match self {
            Self::NoRelease => "no release on this channel yet".into(),
            Self::NoAsset => "the release has no build for this computer".into(),
            Self::BadUrl => "the download link isn't from GrokHub's GitHub releases".into(),
            Self::NoSums => "the release has no SHA256SUMS".into(),
            Self::NoSignature => "the release isn't signed".into(),
            Self::BadSignature => "signature didn't verify".into(),
            Self::WrongKey => "it was signed with a different key".into(),
            Self::NotListed => "the download isn't listed in SHA256SUMS".into(),
            Self::HashMismatch => "the download's checksum didn't match".into(),
            Self::Download(e) => format!("download failed ({e})"),
        }
    }
}

/// "Update 2.10.98 not installed: signature didn't verify"
pub fn refusal_note(version: &str, why: &Refusal) -> String {
    if version.is_empty() {
        return format!("Update not installed: {}", why.reason());
    }
    format!("Update {version} not installed: {}", why.reason())
}

/// `sha256sum` lines: `<64 hex>  <name>` (or `*<name>` for binary mode).
pub fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let (hash, name) = l.trim_end().split_once(char::is_whitespace)?;
            let name = name.trim_start().trim_start_matches('*');
            (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) && !name.is_empty())
                .then(|| (hash.to_ascii_lowercase(), name.to_string()))
        })
        .collect()
}

/// Check one download. With a key: the signature over `sums` must verify
/// with it, then the asset's SHA-256 must match its `sums` line. With no key
/// shipped yet: the checksum alone.
pub fn verify_asset(pubkey: Option<&str>, sums: &str, sig: Option<&str>, name: &str, bytes: &[u8]) -> Result<(), Refusal> {
    if let Some(pk) = pubkey {
        let sig = sig.ok_or(Refusal::NoSignature)?;
        let pk = minisign_verify::PublicKey::decode(pk.trim()).map_err(|_| Refusal::BadSignature)?;
        let sig = minisign_verify::Signature::decode(sig.trim()).map_err(|_| Refusal::BadSignature)?;
        match pk.verify(sums.as_bytes(), &sig, false) {
            Ok(()) => {}
            Err(minisign_verify::Error::UnexpectedKeyId) => return Err(Refusal::WrongKey),
            Err(_) => return Err(Refusal::BadSignature),
        }
    }
    let want = parse_sums(sums).into_iter().find(|(_, n)| n == name).map(|(h, _)| h).ok_or(Refusal::NotListed)?;
    let got = hex::encode(Sha256::digest(bytes));
    if got != want {
        return Err(Refusal::HashMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway test key pair; its secret half was never kept.
    const PUB_A: &str = "untrusted comment: minisign public key CB53A10AE47051AE\nRWSuUXDkCqFTy8aCWU95WxgzjtEf7hIRvFsHGNTsMnUNJM52KlpBfGYS\n";
    const PUB_B: &str = "untrusted comment: minisign public key AD0F30500746C86A\nRWRqyEYHUDAPrZr+FD3YApeEe2tbffPN2QVpWR2VgOB3A8IWgN6OelDz\n";
    const SUMS: &str = "a82d46432d0f6baa57ebb1b0e8a03307df6724580cc316c763e61ae1bfe8cccf  grokhub-linux-v9.9.9.tar.gz\n8e527193e30f66d04ad8a6f13d97f4d577a00dec5a936ccb968ea06ac332399f  grokhub-windows-v9.9.9.zip\n";
    const SIG_A: &str = "untrusted comment: signature from minisign secret key\nRUSuUXDkCqFTywkM99kAdB59lHEjxXHCvwXejm2aIWEwZkvY8ryGGeQv1kP/DwbWoUiK6t0Ap4T2qIpC5ZvQ19xwurPwtbqKlgc=\ntrusted comment: grokhub v9.9.9 SHA256SUMS\noH4W8+yEHIBuwIbbjoh4i8fO4uZeWygt/BAkA1j6j+BIo9Yh2zTrjYEWrsRDRkBOp3OcE50kjKj9uZIDUDCnAw==\n";
    const LINUX: &[u8] = b"linux-asset";
    const LINUX_NAME: &str = "grokhub-linux-v9.9.9.tar.gz";

    #[test]
    fn a_signed_listed_asset_verifies_and_every_tamper_is_refused() {
        assert_eq!(verify_asset(Some(PUB_A), SUMS, Some(SIG_A), LINUX_NAME, LINUX), Ok(()));
        assert_eq!(verify_asset(Some(PUB_A), SUMS, Some(SIG_A), "grokhub-windows-v9.9.9.zip", b"windows-asset"), Ok(()));
        // Tampered asset.
        assert_eq!(verify_asset(Some(PUB_A), SUMS, Some(SIG_A), LINUX_NAME, b"linux-asset!"), Err(Refusal::HashMismatch));
        // Tampered list: the signature no longer covers it.
        let edited = SUMS.replacen("a82d", "0000", 1);
        assert_eq!(verify_asset(Some(PUB_A), &edited, Some(SIG_A), LINUX_NAME, LINUX), Err(Refusal::BadSignature));
        // Wrong key.
        assert_eq!(verify_asset(Some(PUB_B), SUMS, Some(SIG_A), LINUX_NAME, LINUX), Err(Refusal::WrongKey));
        // Missing .minisig.
        assert_eq!(verify_asset(Some(PUB_A), SUMS, None, LINUX_NAME, LINUX), Err(Refusal::NoSignature));
        // Garbage signature.
        assert_eq!(verify_asset(Some(PUB_A), SUMS, Some("not a signature"), LINUX_NAME, LINUX), Err(Refusal::BadSignature));
        // Not in the list.
        assert_eq!(verify_asset(Some(PUB_A), SUMS, Some(SIG_A), "grokhub-linux-v9.9.8.tar.gz", LINUX), Err(Refusal::NotListed));
        // No key shipped yet: the checksum alone decides.
        assert_eq!(verify_asset(None, SUMS, None, LINUX_NAME, LINUX), Ok(()));
        assert_eq!(verify_asset(None, SUMS, None, LINUX_NAME, b"other"), Err(Refusal::HashMismatch));
        assert_eq!(refusal_note("2.10.98", &Refusal::BadSignature), "Update 2.10.98 not installed: signature didn't verify");
        assert_eq!(refusal_note("", &Refusal::NoRelease), "Update not installed: no release on this channel yet");
    }

    fn release(tag: &str, prerelease: bool) -> String {
        format!(
            r#"{{"tag_name":"{tag}","prerelease":{prerelease},"draft":false,"assets":[
              {{"name":"grokhub-linux-v2.13.1.tar.gz","browser_download_url":"https://github.com/blackviperxiii-ui/GrokHub/releases/download/{tag}/grokhub-linux-v2.13.1.tar.gz"}},
              {{"name":"grokhub-windows-v2.13.1.zip","browser_download_url":"https://github.com/blackviperxiii-ui/GrokHub/releases/download/{tag}/grokhub-windows-v2.13.1.zip"}},
              {{"name":"GrokHub-Setup-2.13.1.exe","browser_download_url":"https://github.com/blackviperxiii-ui/GrokHub/releases/download/{tag}/GrokHub-Setup-2.13.1.exe"}},
              {{"name":"SHA256SUMS","browser_download_url":"https://github.com/blackviperxiii-ui/GrokHub/releases/download/{tag}/SHA256SUMS"}}]}}"#
        )
    }

    #[test]
    fn each_channel_picks_its_newest_release_and_this_hosts_asset() {
        let body = format!(
            "[{},{},{},{}]",
            release("beta-v2.13.1-abc1234", true),
            release("v2.13.1", false),
            release("beta-v2.13.0-0000000", true),
            release("v2.13.0", false)
        );
        let all = parse_releases(&body);
        assert_eq!(all.len(), 4);
        let stable = pick_release(&all, Channel::Stable).unwrap();
        assert_eq!((stable.tag.as_str(), stable.version()), ("v2.13.1", "2.13.1".to_string()));
        let beta = pick_release(&all, Channel::Beta).unwrap();
        assert_eq!((beta.tag.as_str(), beta.version()), ("beta-v2.13.1-abc1234", "2.13.1-beta (abc1234)".to_string()));
        assert_eq!(host_asset(stable, false).unwrap().name, "grokhub-linux-v2.13.1.tar.gz");
        assert_eq!(host_asset(stable, true).unwrap().name, "grokhub-windows-v2.13.1.zip");
        assert_eq!(stable.asset(SUMS_NAME).unwrap().name, "SHA256SUMS");
        assert_eq!(stable.asset(SIG_NAME), None);
        assert!(pick_release(&parse_releases(&format!("[{}]", release("v2.13.1", false))), Channel::Beta).is_none());
        assert_eq!(parse_releases(&release("v2.13.1", false)).len(), 1);
        assert!(parse_releases("not json").is_empty());
        let a = host_asset(stable, false).unwrap();
        assert!(download_url_ok(&a.url, &a.name));
        assert!(!download_url_ok("https://evil.example/grokhub-linux-v2.13.1.tar.gz", &a.name));
        assert!(!download_url_ok("https://github.com/blackviperxiii-ui/GrokHub/releases/download/v2.13.1/other.tar.gz", &a.name));
    }

    #[test]
    fn sums_lines_parse_in_text_and_binary_mode() {
        let parsed = parse_sums(&format!("{}{}\nnot a line\n", SUMS, "A82D46432D0F6BAA57EBB1B0E8A03307DF6724580CC316C763E61AE1BFE8CCCF *bin.zip"));
        assert_eq!(
            parsed,
            vec![
                ("a82d46432d0f6baa57ebb1b0e8a03307df6724580cc316c763e61ae1bfe8cccf".to_string(), LINUX_NAME.to_string()),
                ("8e527193e30f66d04ad8a6f13d97f4d577a00dec5a936ccb968ea06ac332399f".to_string(), "grokhub-windows-v9.9.9.zip".to_string()),
                ("a82d46432d0f6baa57ebb1b0e8a03307df6724580cc316c763e61ae1bfe8cccf".to_string(), "bin.zip".to_string()),
            ]
        );
    }

    #[test]
    fn the_shipped_key_matches_packaging() {
        let file = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/minisign.pub")).ok();
        assert_eq!(RELEASE_PUBKEY.map(str::trim), file.as_deref().map(str::trim), "packaging/minisign.pub and RELEASE_PUBKEY must match");
    }
}
