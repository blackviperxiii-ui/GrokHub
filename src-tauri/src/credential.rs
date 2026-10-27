//! The shell's xAI API key, kept in the OS keychain under the cabin's
//! `GrokHub` service. The Grok CLI's own login files are never read.

pub const SERVICE: &str = grokhub_core::brand::INTERNAL_ID;
pub const ACCOUNT: &str = "shell-console-key";

/// Points chat at a local mock model server. Only plain-http loopback URLs are
/// accepted (`XaiClient::with_loopback_url`), so it can never move real traffic.
pub const TEST_URL_ENV: &str = "GROKHUB_SHELL_RESPONSES_URL";
/// The bearer sent to that mock server. It is not a credential.
pub const LOOPBACK_TEST_BEARER: &str = "loopback-test";

pub fn test_responses_url() -> Option<String> {
    std::env::var(TEST_URL_ENV)
        .ok()
        .or_else(phone_smoke_url)
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
}

/// Where ui/e2e/phone.mjs serves its mock model (`adb reverse` maps it on
/// Android; the iOS simulator shares the host's loopback).
pub const PHONE_SMOKE_URL: &str = "http://127.0.0.1:4610/v1/responses";

/// A phone gets no environment from the test runner, and Xcode's build drops
/// the job's environment, so a phone smoke build carries the mock URL as a
/// constant. Still loopback-only, as above.
fn phone_smoke_url() -> Option<String> {
    cfg!(feature = "phone-smoke").then(|| PHONE_SMOKE_URL.to_string())
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(keychain_message)
}

fn keychain_message(err: keyring::Error) -> String {
    format!("The system keychain is not available: {err}")
}

pub fn has_key() -> bool {
    test_responses_url().is_some() || load_key().is_ok()
}

pub fn load_key() -> Result<String, String> {
    match entry()?.get_password() {
        Ok(key) if !key.trim().is_empty() => Ok(key),
        Ok(_) | Err(keyring::Error::NoEntry) => Err(grokhub_core::XAI_NEED_SIGNIN.into()),
        Err(err) => Err(keychain_message(err)),
    }
}

pub fn save_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Paste an API key first.".into());
    }
    entry()?.set_password(key).map_err(keychain_message)
}

pub fn delete_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(keychain_message(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_key_is_refused_before_the_keychain() {
        assert_eq!(save_key("   "), Err("Paste an API key first.".to_string()));
    }

    #[cfg(not(feature = "phone-smoke"))]
    #[test]
    fn only_a_phone_smoke_build_carries_a_mock_url() {
        assert_eq!(phone_smoke_url(), None);
    }

    #[cfg(feature = "phone-smoke")]
    #[test]
    fn a_phone_smoke_build_points_chat_at_the_loopback_mock() {
        assert_eq!(
            phone_smoke_url(),
            Some("http://127.0.0.1:4610/v1/responses".to_string())
        );
    }
}
