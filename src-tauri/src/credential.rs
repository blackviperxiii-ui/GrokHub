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

fn keychain_message(err: keyring::Error) -> String {
    format!("The system keychain is not available: {err}")
}

fn backend() -> grokhub_agent::keyring_blob::OsBackend {
    grokhub_agent::keyring_blob::OsBackend::new(SERVICE, keychain_message)
}

pub fn has_key() -> bool {
    test_responses_url().is_some() || load_key().is_ok()
}

pub fn load_key() -> Result<String, String> {
    match grokhub_agent::keyring_blob::get(&backend(), ACCOUNT)? {
        Some(key) if !key.trim().is_empty() => Ok(key),
        _ => Err(grokhub_core::XAI_NEED_SIGNIN.into()),
    }
}

pub fn save_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Paste an API key first.".into());
    }
    // Chunk when a pasted key exceeds Windows Credential Manager's limit.
    grokhub_agent::keyring_blob::set(&backend(), ACCOUNT, key)
}

pub fn delete_key() -> Result<(), String> {
    grokhub_agent::keyring_blob::delete(&backend(), ACCOUNT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::keyring_blob::{self, Backend, PLATFORM_UTF16_LIMIT};
    use std::collections::HashMap;
    use std::sync::Mutex;

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

    #[derive(Default)]
    struct Mem(Mutex<HashMap<String, String>>);

    impl Backend for Mem {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(account.to_string(), value.to_string());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    #[test]
    fn a_console_key_over_the_windows_limit_round_trips_through_chunking() {
        let store = Mem::default();
        let secret: String = "K".repeat(PLATFORM_UTF16_LIMIT + 64);
        keyring_blob::set(&store, ACCOUNT, &secret).unwrap();
        assert_eq!(
            keyring_blob::get(&store, ACCOUNT).unwrap().as_deref(),
            Some(secret.as_str())
        );
        keyring_blob::delete(&store, ACCOUNT).unwrap();
        assert!(keyring_blob::get(&store, ACCOUNT).unwrap().is_none());
    }
}
