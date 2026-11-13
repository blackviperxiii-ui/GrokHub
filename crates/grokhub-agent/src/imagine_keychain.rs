//! Imagine's own Sign in with Grok, by device code: the old cabin's "Use a
//! code", for the desktop shell. It asks for Imagine's scope only
//! (`IMAGINE_OAUTH_SCOPE`, no `grok-cli:access`) and keeps the tokens in the
//! system keychain under the cabin's own account (`XAI_OAUTH_ACCOUNT`), so a
//! sign-in made in the cabin still works here. The calls are
//! [`xai_keychain`](crate::xai_keychain)'s, through the same quiet egress
//! guard; nothing here writes a file or logs a code or token.
use std::sync::Mutex;

use grokhub_core::{
    delete_xai_oauth, imagine_access_usable, imagine_needs_refresh, imagine_oauth_preferred,
    imagine_tokens_from_xai, load_xai_oauth, merge_imagine_refresh, parse_token_json,
    ImagineTokens, OAuthAccountStore, IMAGINE_OAUTH_REFERRER, IMAGINE_OAUTH_SCOPE,
    IMAGINE_SIGN_IN_AGAIN, XAI_OAUTH_ACCOUNT, XAI_OAUTH_CLIENT_ID,
};

use crate::xai_keychain::{endpoints, post_form, CallError, Poll, SignIn, SignedIn, Started};

/// The system keychain, under the same service the cabin used.
pub struct Keychain;

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(grokhub_core::brand::INTERNAL_ID, account)
        .map_err(|e| grokhub_core::keychain_unavailable_message(&e.to_string()))
}

fn keychain_err(e: keyring::Error) -> String {
    grokhub_core::keychain_unavailable_message(&e.to_string())
}

impl OAuthAccountStore for Keychain {
    fn load_account(&self, account: &str) -> Result<Option<ImagineTokens>, String> {
        match entry(account)?.get_password() {
            Ok(raw) => Ok(serde_json::from_str(&raw).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_err(e)),
        }
    }

    fn save_account(&self, account: &str, tokens: &ImagineTokens) -> Result<(), String> {
        let raw = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
        entry(account)?.set_password(&raw).map_err(keychain_err)
    }

    fn delete_account(&self, account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_err(e)),
        }
    }
}

/// Asks xAI for a device code with Imagine's scope. The page and code go to the UI.
pub fn start(flow: &SignIn) -> Result<Started, String> {
    flow.start_with(&[
        ("scope", IMAGINE_OAUTH_SCOPE),
        ("referrer", IMAGINE_OAUTH_REFERRER),
    ])
}

/// Asks once whether the code was approved; on approval the tokens go to `store`.
pub fn poll(flow: &SignIn, store: &dyn OAuthAccountStore) -> Result<Poll, String> {
    flow.poll_with(&|tokens| {
        store.save_account(XAI_OAUTH_ACCOUNT, &imagine_tokens_from_xai(tokens))
    })
}

/// Who signed in to Imagine, or `None`. A keychain error reads as signed out.
pub fn signed_in(store: &dyn OAuthAccountStore) -> Option<SignedIn> {
    let tokens = load_xai_oauth(store).ok().flatten()?;
    imagine_oauth_preferred(&tokens, grokhub_core::now_ms()).then(|| SignedIn {
        name: None,
        email: tokens.email.filter(|e| !e.trim().is_empty()),
    })
}

/// Signs out of Imagine: both the current and the legacy account leave the keychain.
pub fn sign_out(flow: &SignIn, store: &dyn OAuthAccountStore) -> Result<(), String> {
    flow.cancel();
    delete_xai_oauth(store)
}

/// One refresh at a time, so a page reload and a Make don't both spend the refresh token.
static REFRESH: Mutex<()> = Mutex::new(());

/// The access token for an Imagine call, refreshed first when it is about to
/// run out. `None` when nobody signed in to Imagine. A refused refresh leaves
/// the stored tokens alone and says [`IMAGINE_SIGN_IN_AGAIN`].
pub fn bearer(store: &dyn OAuthAccountStore) -> Result<Option<String>, String> {
    let now = grokhub_core::now_ms();
    let Some(tokens) = load_xai_oauth(store)?.filter(|t| imagine_oauth_preferred(t, now)) else {
        return Ok(None);
    };
    if !imagine_needs_refresh(&tokens, now) && imagine_access_usable(&tokens, now) {
        return Ok(Some(tokens.access_token));
    }
    let _one = REFRESH.lock().unwrap_or_else(|e| e.into_inner());
    // Another call may have refreshed while this one waited.
    let tokens = load_xai_oauth(store)?.unwrap_or(tokens);
    let now = grokhub_core::now_ms();
    if !imagine_needs_refresh(&tokens, now) && imagine_access_usable(&tokens, now) {
        return Ok(Some(tokens.access_token));
    }
    let refresh = tokens
        .refresh_token
        .clone()
        .filter(|t| !t.trim().is_empty())
        .ok_or(IMAGINE_SIGN_IN_AGAIN)?;
    let ends = endpoints().map_err(String::from)?;
    let answer = post_form(
        &ends.token,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", XAI_OAUTH_CLIENT_ID),
        ],
    );
    let v = match answer {
        Ok((true, v)) => v,
        Ok((false, _)) => return Err(IMAGINE_SIGN_IN_AGAIN.into()),
        Err(CallError::NoAnswer(e) | CallError::Other(e)) => return Err(e),
    };
    let next = parse_token_json(&v, grokhub_core::now_ms())
        .map_err(|_| IMAGINE_SIGN_IN_AGAIN.to_string())?;
    let merged = merge_imagine_refresh(&tokens, imagine_tokens_from_xai(&next));
    store.save_account(XAI_OAUTH_ACCOUNT, &merged)?;
    Ok(Some(merged.access_token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xai_keychain::tests::{mock, use_base};
    use grokhub_core::XAI_OAUTH_LEGACY_ACCOUNT;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Mem(Mutex<HashMap<String, ImagineTokens>>);

    impl OAuthAccountStore for Mem {
        fn load_account(&self, account: &str) -> Result<Option<ImagineTokens>, String> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn save_account(&self, account: &str, tokens: &ImagineTokens) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(account.to_string(), tokens.clone());
            Ok(())
        }
        fn delete_account(&self, account: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    fn config(name: &str) -> (std::path::PathBuf, crate::perm::ConfigGuard) {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-imagine-oauth-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let guard = crate::perm::ConfigGuard::set(&dir);
        (dir, guard)
    }

    const START: &str = r#"{"device_code":"dev-1","user_code":"ABCD-EFGH","verification_uri":"https://accounts.x.ai/device","expires_in":600,"interval":5}"#;
    const PENDING: &str = r#"{"error":"authorization_pending"}"#;
    const TOKENS: &str =
        r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600,"token_type":"Bearer"}"#;
    const USERINFO: &str = r#"{"name":"Sam Rivera","email":"sam@example.com"}"#;

    fn stored(access: &str, expires_at: u64) -> ImagineTokens {
        ImagineTokens {
            access_token: access.into(),
            refresh_token: Some("rt-old".into()),
            expires_at: Some(expires_at),
            id_token: None,
            email: Some("sam@example.com".into()),
            connected_at: 10,
        }
    }

    #[test]
    fn a_code_asks_for_imagine_scope_and_keeps_the_tokens_in_the_cabin_account() {
        let (_dir, _cfg) = config("flow");
        let server = mock(vec![
            ("/device", 200, START),
            ("/token", 400, PENDING),
            ("/token", 200, TOKENS),
            ("/userinfo", 200, USERINFO),
        ]);
        let _base = use_base(&server.base);
        let flow = SignIn::new();
        let store = Mem::default();
        let started = start(&flow).expect("start");
        assert_eq!(started.user_code, "ABCD-EFGH");
        assert_eq!(poll(&flow, &store), Ok(Poll::Pending { interval: 5 }));
        assert_eq!(signed_in(&store), None);
        assert_eq!(
            poll(&flow, &store),
            Ok(Poll::Ready {
                signed_in: SignedIn {
                    name: Some("Sam Rivera".into()),
                    email: Some("sam@example.com".into())
                }
            })
        );
        let kept = store
            .load_account(XAI_OAUTH_ACCOUNT)
            .unwrap()
            .expect("kept");
        assert_eq!(kept.access_token, "at-1");
        assert_eq!(kept.refresh_token.as_deref(), Some("rt-1"));
        assert_eq!(kept.email.as_deref(), Some("sam@example.com"));
        assert_eq!(bearer(&store), Ok(Some("at-1".into())));
        assert_eq!(
            signed_in(&store),
            Some(SignedIn {
                name: None,
                email: Some("sam@example.com".into())
            })
        );
        let seen = server.seen.lock().unwrap();
        let device = &seen.iter().find(|(p, _)| p == "/device").unwrap().1;
        assert!(device.starts_with("client_id="), "{device}");
        assert!(
            device.ends_with(
                "&scope=openid%20profile%20email%20offline_access%20api%3Aaccess&referrer=grokhub"
            ),
            "{device}"
        );
        assert!(!device.contains("grok-cli"), "{device}");
    }

    #[test]
    fn an_old_token_is_refreshed_and_saved_and_a_refused_one_says_sign_in_again() {
        let (_dir, _cfg) = config("refresh");
        let server = mock(vec![
            ("/token", 200, TOKENS),
            ("/token", 400, r#"{"error":"invalid_grant"}"#),
        ]);
        let _base = use_base(&server.base);
        let store = Mem::default();
        store
            .save_account(XAI_OAUTH_ACCOUNT, &stored("at-old", 1))
            .unwrap();
        assert_eq!(bearer(&store), Ok(Some("at-1".into())));
        let kept = store.load_account(XAI_OAUTH_ACCOUNT).unwrap().unwrap();
        assert_eq!(kept.access_token, "at-1");
        assert_eq!(kept.refresh_token.as_deref(), Some("rt-1"));
        assert_eq!(kept.email.as_deref(), Some("sam@example.com"));
        {
            let seen = server.seen.lock().unwrap();
            assert_eq!(seen.len(), 1);
            assert!(seen[0].1.contains("refresh_token=rt-old"), "{}", seen[0].1);
        }
        store
            .save_account(XAI_OAUTH_ACCOUNT, &stored("at-old", 1))
            .unwrap();
        assert_eq!(bearer(&store), Err(IMAGINE_SIGN_IN_AGAIN.to_string()));
        assert_eq!(
            store
                .load_account(XAI_OAUTH_ACCOUNT)
                .unwrap()
                .unwrap()
                .access_token,
            "at-old"
        );
    }

    #[test]
    fn a_cabin_sign_in_is_used_and_sign_out_removes_both_accounts() {
        let store = Mem::default();
        assert_eq!(bearer(&store), Ok(None));
        let live = stored("at-cabin", grokhub_core::now_ms() + 3_600_000);
        store.save_account(XAI_OAUTH_LEGACY_ACCOUNT, &live).unwrap();
        assert_eq!(bearer(&store), Ok(Some("at-cabin".into())));
        store.save_account(XAI_OAUTH_LEGACY_ACCOUNT, &live).unwrap();
        sign_out(&SignIn::new(), &store).unwrap();
        assert!(store.0.lock().unwrap().is_empty());
        assert_eq!(signed_in(&store), None);
        assert_eq!(bearer(&store), Ok(None));
    }

    #[test]
    fn the_source_keeps_tokens_out_of_files_and_the_grok_cli() {
        let src = include_str!("imagine_keychain.rs");
        for banned in [
            concat!("File::", "create"),
            concat!("secrets", ".json"),
            concat!("auth", ".json"),
            concat!("grok_cli_", "key"),
            concat!("tracing::", "info!"),
            concat!("eprint", "ln!"),
        ] {
            assert!(!src.contains(banned), "{banned}");
        }
    }
}
