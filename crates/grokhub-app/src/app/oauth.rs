//! OAuth, profile name, and picture.

use super::*;


pub(super) struct OauthPhotoOut {
    tokens: Option<grokhub_core::XaiOAuthTokens>,
    url: String,
    image: Option<ColorImage>,
}


pub(super) struct ProfilePhotoOut {
    path: String,
    image: Option<ColorImage>,
}


pub(super) enum ProfilePick {
    Chosen(std::path::PathBuf),
    Cancelled,
    Failed(String),
}


/// Name and local picture the avatar menu and rail paint. The email is not a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AvatarMenu {
    pub(super) name: String,
    pub(super) picture_path: String,
}


pub(super) fn clip_profile_name(s: &str) -> String {
    s.trim().chars().take(PROFILE_NAME_MAX).collect()
}


pub(super) fn fallback_cabin_name(user_md: &str) -> String {
    let n = greeting_name(user_md, "");
    if n.is_empty() {
        "Grok".into()
    } else {
        n
    }
}


/// Saved name wins. An empty saved name keeps the OAuth name, then USER.md, then Grok.
/// The OAuth email is not a name.
pub(super) fn avatar_menu(
    saved_name: &str,
    saved_picture: &str,
    oauth_name: Option<&str>,
    user_md: &str,
) -> AvatarMenu {
    let saved = clip_profile_name(saved_name);
    let name = if !saved.is_empty() {
        saved
    } else if let Some(n) = oauth_name.map(str::trim).filter(|s| !s.is_empty()) {
        if greeting_name("", n).is_empty() {
            fallback_cabin_name(user_md)
        } else {
            n.to_string()
        }
    } else {
        fallback_cabin_name(user_md)
    };
    AvatarMenu {
        name,
        picture_path: saved_picture.trim().to_string(),
    }
}


pub(super) fn oauth_photo_image(bytes: &[u8]) -> Option<ColorImage> {
    let rgba = crate::oauth::avatar_rgba(bytes)?;
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}


/// A late picker result applies only when Remove has not bumped the token.
pub(super) fn profile_pick_current(active: u64, result: u64) -> bool {
    active != 0 && active == result
}


pub(super) fn next_pick_token(current: u64) -> u64 {
    let n = current.wrapping_add(1);
    if n == 0 {
        1
    } else {
        n
    }
}

/// Settings → Account Connect Grok row: title, hint, primary action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AccountConnectChrome {
    pub(super) title: &'static str,
    pub(super) hint: String,
    pub(super) action: &'static str,
    pub(super) connected: bool,
}

/// Identity line when OAuth tokens are present (email and/or Grok display name).
pub(super) fn account_connected_hint(tokens: &grokhub_core::XaiOAuthTokens) -> String {
    let email = tokens
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let name = tokens
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (name, email) {
        (Some(n), Some(e)) => format!("{n} · {e}"),
        (Some(n), None) => n.to_string(),
        (None, Some(e)) => e.to_string(),
        (None, None) => "Signed in with Grok.".into(),
    }
}

/// Connect Grok chrome for Settings → Account. Connected when access token is live.
pub(super) fn account_connect_chrome(
    oauth: Option<&grokhub_core::XaiOAuthTokens>,
) -> AccountConnectChrome {
    let Some(tokens) = oauth.filter(|t| !t.access_token.trim().is_empty()) else {
        return AccountConnectChrome {
            title: "Connect Grok",
            hint: "Device-code OAuth. GrokHub keeps this sign-in to itself."
                .into(),
            action: "Sign in with Grok",
            connected: false,
        };
    };
    AccountConnectChrome {
        title: "Connected",
        hint: account_connected_hint(tokens),
        action: "Sign out",
        connected: true,
    }
}

impl Cabin {

    /// Adopt secrets.json OAuth written by `grokhub --oauth` (or another process)
    /// while this cabin was already open. Does not overwrite an in-memory session.
    pub(super) fn pull_account_oauth_from_disk(&mut self) {
        if self.account_oauth_present() {
            return;
        }
        let disk = secrets::load();
        let Some(tok) = disk
            .oauth
            .filter(|t| !t.access_token.trim().is_empty())
        else {
            return;
        };
        self.secrets.oauth = Some(tok);
        if self.secrets.api_key.trim().is_empty() && !disk.api_key.trim().is_empty() {
            self.secrets.api_key = disk.api_key;
        }
        self.oauth_profile_tried = false;
    }

    pub(super) fn start_oauth(&mut self) {
        if self.oauth_start_rx.is_some()
            || self.oauth_pending.is_some()
            || self.oauth_poll_rx.is_some()
        {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.oauth_start_rx = Some(rx);
        self.status = "Starting Grok OAuth…".into();
        std::thread::spawn(move || {
            let _ = tx.send(crate::oauth::start_device());
        });
    }

    pub(super) fn poll_oauth(&mut self) {
        if let Some(rx) = self.oauth_start_rx.take() {
            match rx.try_recv() {
                Ok(Ok(start)) => {
                    let uri = start
                        .verification_uri_complete
                        .clone()
                        .unwrap_or_else(|| start.verification_uri.clone());
                    let _ = crate::oauth::open_browser(&uri);
                    self.status = format!(
                        "Grok OAuth code {} — approve in the browser",
                        start.user_code
                    );
                    let wait = start.interval.max(1);
                    self.oauth_pending = Some(start);
                    self.oauth_next_poll = Instant::now() + Duration::from_secs(wait);
                }
                Ok(Err(e)) => self.status = grokhub_core::oauth_error_status(e),
                Err(mpsc::TryRecvError::Empty) => {
                    self.oauth_start_rx = Some(rx);
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = "Grok OAuth failed to start".into();
                    return;
                }
            }
        }
        if let Some(rx) = self.oauth_poll_rx.take() {
            match rx.try_recv() {
                Ok(Ok(r)) => match r.status {
                    grokhub_core::PollStatus::Ready => {
                        if let Some(t) = r.tokens {
                            self.secrets.oauth = Some(t);
                            let io = self.persist_io.clone();
                            let secrets = self.secrets.clone();
                            std::thread::spawn(move || {
                                if let Ok(_g) = io.lock() {
                                    let _ = secrets::save(&secrets);
                                }
                            });
                            self.oauth_pending = None;
                            self.oauth_profile_tried = false;
                            self.oauth_photo = None;
                            self.oauth_photo_key.clear();
                            self.status = "Grok OAuth connected".into();
                            if self.cfg.grok_build_engine {
                                self.sync_cli_auth_from_oauth();
                            }
                            self.mark_get_started_done();
                        }
                    }
                    grokhub_core::PollStatus::Expired | grokhub_core::PollStatus::Denied => {
                        self.oauth_pending = None;
                        self.status = grokhub_core::oauth_error_status(
                            r.error.unwrap_or_else(|| "OAuth failed".into()),
                        );
                    }
                    status @ (grokhub_core::PollStatus::Pending
                    | grokhub_core::PollStatus::SlowDown) => {
                        if let Some(p) = self.oauth_pending.as_mut() {
                            if let Some(wait) =
                                grokhub_core::next_oauth_poll_secs(p.interval, status)
                            {
                                p.interval = wait;
                                self.oauth_next_poll = Instant::now() + Duration::from_secs(wait);
                            }
                        }
                    }
                },
                Ok(Err(e)) => self.status = grokhub_core::oauth_error_status(e),
                Err(mpsc::TryRecvError::Empty) => {
                    self.oauth_poll_rx = Some(rx);
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
            return;
        }
        let Some(p) = self.oauth_pending.clone() else {
            return;
        };
        if Instant::now() < self.oauth_next_poll {
            return;
        }
        let code = p.device_code.clone();
        let (tx, rx) = mpsc::channel();
        self.oauth_poll_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(crate::oauth::poll_device(&code));
        });
    }

    pub(super) fn clear_oauth_photo(&mut self) {
        self.oauth_photo = None;
        self.oauth_photo_key.clear();
        self.oauth_photo_rx = None;
        self.oauth_photo_busy = false;
        self.oauth_profile_tried = false;
    }

    pub(super) fn sign_out_oauth(&mut self) {
        self.secrets.oauth = None;
        self.imagine_pending = false;
        self.oauth_pending = None;
        self.oauth_start_rx = None;
        self.oauth_poll_rx = None;
        self.clear_oauth_photo();
        let io = self.persist_io.clone();
        let secrets = self.secrets.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = secrets::save(&secrets);
            }
        });
        self.status = "Signed out".into();
    }

    pub(super) fn poll_oauth_photo(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.oauth_photo_rx.take() {
            match rx.try_recv() {
                Ok(out) => {
                    self.oauth_photo_busy = false;
                    self.oauth_profile_tried = true;
                    if let Some(tokens) = out.tokens {
                        let changed = self.secrets.oauth.as_ref() != Some(&tokens);
                        self.secrets.oauth = Some(tokens);
                        if changed {
                            let io = self.persist_io.clone();
                            let secrets = self.secrets.clone();
                            std::thread::spawn(move || {
                                if let Ok(_g) = io.lock() {
                                    let _ = secrets::save(&secrets);
                                }
                            });
                        }
                    }
                    self.oauth_photo_key = out.url;
                    self.oauth_photo = out
                        .image
                        .map(|img| ctx.load_texture("oauth-avatar", img, TextureOptions::LINEAR));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.oauth_photo_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.oauth_photo_busy = false;
                }
            }
        }
        self.kick_oauth_photo();
    }

    pub(super) fn kick_oauth_photo(&mut self) {
        if self.oauth_photo_busy {
            return;
        }
        let Some(tok) = self.secrets.oauth.clone() else {
            if self.oauth_photo.is_some() || !self.oauth_photo_key.is_empty() {
                self.clear_oauth_photo();
            }
            return;
        };
        let url = tok
            .picture
            .as_ref()
            .and_then(|u| grokhub_core::trusted_profile_photo_url(u).ok())
            .unwrap_or_default();
        if !url.is_empty() && url == self.oauth_photo_key {
            return;
        }
        if url.is_empty() && self.oauth_profile_tried {
            return;
        }
        self.oauth_photo_busy = true;
        let (tx, rx) = mpsc::channel();
        self.oauth_photo_rx = Some(rx);
        std::thread::spawn(move || {
            let tokens = match crate::oauth::ensure_access(&tok) {
                Ok((_, t, _)) => crate::oauth::enrich_tokens(t),
                Err(_) => crate::oauth::enrich_tokens(tok),
            };
            let url = tokens
                .picture
                .as_ref()
                .and_then(|u| grokhub_core::trusted_profile_photo_url(u).ok())
                .unwrap_or_default();
            let bytes = if url.is_empty() {
                None
            } else {
                crate::oauth::fetch_profile_photo(&url, &tokens.access_token).ok()
            };
            let image = bytes.as_ref().and_then(|b| oauth_photo_image(b));
            let _ = tx.send(OauthPhotoOut {
                tokens: Some(tokens),
                url,
                image,
            });
        });
    }

    pub(super) fn pick_profile_picture(&mut self) {
        if self.profile_pick_rx.is_some() {
            self.status = "Choose a picture…".into();
            return;
        }
        let token = next_pick_token(self.profile_pick_token.load(Ordering::SeqCst));
        self.profile_pick_token.store(token, Ordering::SeqCst);
        let gate = Arc::clone(&self.profile_pick_token);
        let file_io = Arc::clone(&self.profile_file_io);
        let (tx, rx) = mpsc::channel();
        self.profile_pick_rx = Some(rx);
        self.status = "Choose a picture…".into();
        std::thread::spawn(move || {
            let out = match pick_file() {
                Some(p) => {
                    if !profile_pick_current(gate.load(Ordering::SeqCst), token) {
                        ProfilePick::Cancelled
                    } else if let Ok(_file) = file_io.lock() {
                        if !profile_pick_current(gate.load(Ordering::SeqCst), token) {
                            ProfilePick::Cancelled
                        } else {
                            match crate::oauth::install_profile_picture(&p) {
                                Ok(dest) => {
                                    if profile_pick_current(gate.load(Ordering::SeqCst), token) {
                                        ProfilePick::Chosen(dest)
                                    } else {
                                        let _ = std::fs::remove_file(&dest);
                                        ProfilePick::Cancelled
                                    }
                                }
                                Err(e) => ProfilePick::Failed(e),
                            }
                        }
                    } else {
                        ProfilePick::Cancelled
                    }
                }
                None => ProfilePick::Cancelled,
            };
            let _ = tx.send((token, out));
        });
    }

    pub(super) fn clear_profile_picture(&mut self) {
        let token = next_pick_token(self.profile_pick_token.load(Ordering::SeqCst));
        self.profile_pick_token.store(token, Ordering::SeqCst);
        self.profile_pick_rx = None;
        self.cfg.profile_picture.clear();
        self.profile_photo = None;
        self.profile_photo_key.clear();
        self.profile_photo_rx = None;
        self.profile_photo_busy = false;
        self.persist_cfg();
        let dest = config::config_dir().join("profile.png");
        let gate = Arc::clone(&self.profile_pick_token);
        let file_io = Arc::clone(&self.profile_file_io);
        std::thread::spawn(move || {
            let Ok(_file) = file_io.lock() else {
                return;
            };
            if profile_pick_current(gate.load(Ordering::SeqCst), token) {
                let _ = std::fs::remove_file(dest);
            }
        });
        self.status = "Saved".into();
    }

    pub(super) fn poll_profile_pick(&mut self) {
        let Some(rx) = self.profile_pick_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((token, ProfilePick::Chosen(path))) => {
                if profile_pick_current(self.profile_pick_token.load(Ordering::SeqCst), token) {
                    self.cfg.profile_picture = path.to_string_lossy().into_owned();
                    self.profile_photo = None;
                    self.profile_photo_key.clear();
                    self.persist_cfg();
                    self.status = "Saved".into();
                }
            }
            Ok((_, ProfilePick::Cancelled)) => {}
            Ok((token, ProfilePick::Failed(err))) => {
                if profile_pick_current(self.profile_pick_token.load(Ordering::SeqCst), token) {
                    self.status = err;
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.profile_pick_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn poll_profile_photo(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.profile_photo_rx.take() {
            match rx.try_recv() {
                Ok(out) => {
                    self.profile_photo_busy = false;
                    self.profile_photo_key = out.path;
                    self.profile_photo = out.image.map(|img| {
                        ctx.load_texture("profile-avatar", img, TextureOptions::LINEAR)
                    });
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.profile_photo_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.profile_photo_busy = false;
                }
            }
        }
        self.kick_profile_photo();
    }

    pub(super) fn kick_profile_photo(&mut self) {
        if self.profile_photo_busy {
            return;
        }
        let path = self.cfg.profile_picture.trim().to_string();
        if path.is_empty() {
            if self.profile_photo.is_some() || !self.profile_photo_key.is_empty() {
                self.profile_photo = None;
                self.profile_photo_key.clear();
            }
            return;
        }
        if path == self.profile_photo_key {
            return;
        }
        self.profile_photo_busy = true;
        let (tx, rx) = mpsc::channel();
        self.profile_photo_rx = Some(rx);
        std::thread::spawn(move || {
            let bytes = std::fs::metadata(&path)
                .ok()
                .filter(|m| m.len() > 0 && m.len() <= grokhub_core::IMAGE_FILE_CAP)
                .and_then(|_| std::fs::read(&path).ok())
                .filter(|b| (b.len() as u64) <= grokhub_core::IMAGE_FILE_CAP);
            let image = bytes.as_ref().and_then(|b| oauth_photo_image(b));
            let _ = tx.send(ProfilePhotoOut { path, image });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::XaiOAuthTokens;

    fn tok(access: &str, name: Option<&str>, email: Option<&str>) -> XaiOAuthTokens {
        XaiOAuthTokens {
            access_token: access.into(),
            name: name.map(str::to_string),
            email: email.map(str::to_string),
            connected_at: 1,
            ..Default::default()
        }
    }

    #[test]
    fn account_connect_chrome_signed_out() {
        let c = account_connect_chrome(None);
        assert!(!c.connected);
        assert_eq!(c.title, "Connect Grok");
        assert_eq!(c.action, "Sign in with Grok");
        assert!(c.hint.contains("Device-code OAuth"));
        let empty = tok("  ", None, None);
        let c2 = account_connect_chrome(Some(&empty));
        assert!(!c2.connected);
        assert_eq!(c2.action, "Sign in with Grok");
    }

    #[test]
    fn account_connect_chrome_signed_in_shows_identity_and_sign_out() {
        let t = tok("access", Some("GrokHub QA"), Some("grokhub-qa@agentmail.to"));
        let c = account_connect_chrome(Some(&t));
        assert!(c.connected);
        assert_eq!(c.title, "Connected");
        assert_eq!(c.action, "Sign out");
        assert_eq!(c.hint, "GrokHub QA · grokhub-qa@agentmail.to");
        assert_eq!(
            account_connected_hint(&tok("x", Some("Viper"), None)),
            "Viper"
        );
        assert_eq!(
            account_connected_hint(&tok("x", None, Some("a@b.co"))),
            "a@b.co"
        );
        assert_eq!(
            account_connected_hint(&tok("x", None, None)),
            "Signed in with Grok."
        );
    }
}
