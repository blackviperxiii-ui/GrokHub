//! Round 3 item 36: keeps Microsoft's vulnerable driver blocklist cached so
//! the pre-check can flag a fan, RGB or monitoring tool whose driver is on
//! it. Windows only, once a launch at most, and only when the cache is a
//! month old or missing. Each hop goes through the egress guard, stays on
//! Microsoft's own download hosts, and dials public addresses only.

#[cfg(not(test))]
use std::io::Read;

use grokhub_core::driver_check::BLOCKLIST_HOSTS;
#[cfg(not(test))]
use grokhub_core::driver_check::{self, Blocklist, BLOCKLIST_CAP, BLOCKLIST_URL};

/// Fetch the blocklist in the background when it is due. Tests never fetch.
#[cfg(not(test))]
pub fn refresh_if_due() {
    if !cfg!(windows) {
        return;
    }
    if grokhub_core::offline::refuse().is_err() {
        return;
    }
    let config = crate::config::config_dir();
    let now = grokhub_core::now_ms() / 1000;
    if !driver_check::due(&config, now) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("driver-blocklist".into())
        .spawn(move || {
            if let Ok(bytes) = fetch() {
                if let Ok(names) = driver_check::names_from_zip(&bytes) {
                    let _ = driver_check::save(
                        &config,
                        &Blocklist {
                            fetched_at: now,
                            names,
                        },
                    );
                }
            }
        });
}

fn allowed(url: &str) -> bool {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .is_some_and(|host| BLOCKLIST_HOSTS.contains(&host.to_ascii_lowercase().as_str()))
}

/// The zip, following up to four redirects by hand, each one checked.
#[cfg(not(test))]
fn fetch() -> Result<Vec<u8>, String> {
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        redirects: 0,
        overall: Some(std::time::Duration::from_secs(60)),
        public_only: true,
        ..Default::default()
    });
    let mut url = BLOCKLIST_URL.to_string();
    for _ in 0..5 {
        if !allowed(&url) || !grokhub_core::public_http_url(&url) {
            return Err("off Microsoft's download hosts".into());
        }
        crate::xai::egress_ok(&url, &[])?;
        let resp = agent
            .get(&url)
            .any_status()
            .call()
            .map_err(|e| e.to_string())?;
        match resp.status() {
            200 => {
                let mut buf = Vec::new();
                resp.into_reader()
                    .take(BLOCKLIST_CAP)
                    .read_to_end(&mut buf)
                    .map_err(|e| e.to_string())?;
                return Ok(buf);
            }
            301 | 302 | 303 | 307 | 308 => {
                url = resp
                    .header("location")
                    .ok_or("a redirect with no location")?
                    .to_string();
            }
            code => return Err(format!("HTTP {code}")),
        }
    }
    Err("too many redirects".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_microsoft_download_hosts_over_https() {
        assert!(super::allowed("https://aka.ms/VulnerableDriverBlockList"));
        assert!(super::allowed(
            "https://download.microsoft.com/download/x/VulnerableDriverBlockList.zip"
        ));
        assert!(!super::allowed("http://download.microsoft.com/x.zip"));
        assert!(!super::allowed(
            "https://download.microsoft.com.evil.example/x.zip"
        ));
        assert!(!super::allowed("https://example.com/?u=https://aka.ms/"));
    }
}
