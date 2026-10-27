//! Steam wishlist sale alerts (`grokhub_core::steam_wishlist`), opt-in under
//! Settings → Pulse → Sources. The SteamID is sealed with the private-data
//! key in `{config}/steam-id.sealed` and only ever leaves in the wishlist
//! request to Steam. The check runs once a day with the source check, off
//! the UI thread, and posts one card per sale that clears the threshold.

use std::path::Path;

use super::*;
use grokhub_core::amr::Sealer as _;
use grokhub_core::pulse_sources::{Fetched, SourceKind, SourceReq};
use grokhub_core::steam_wishlist::{self as sw, Alert, Threshold, Wishlist};

const ID_FILE: &str = "steam-id.sealed";
const ID_AAD: &str = "grokhub-steam-id:v1";

#[derive(Default)]
pub(super) struct WishState {
    list: Option<Wishlist>,
    rx: Option<mpsc::Receiver<WishRun>>,
    draft: String,
    /// Whether a sealed SteamID is on disk; read once.
    has_id: Option<bool>,
    /// Steam accounts signed in on this PC; read once.
    local: Option<Vec<u64>>,
    pub note: String,
}

struct WishRun {
    list: Wishlist,
    alerts: Vec<Alert>,
    error: Option<String>,
}

fn seal_steam_id(dir: &Path, id: u64) -> Result<(), String> {
    let sealed = grokhub_agent::harness::LearnedVault::new(dir).seal(ID_AAD, &id.to_string())?;
    crate::config::atomic_write(&dir.join(ID_FILE), sealed.as_bytes())
}

fn open_steam_id(dir: &Path) -> Option<u64> {
    let sealed = std::fs::read_to_string(dir.join(ID_FILE)).ok()?;
    grokhub_agent::harness::LearnedVault::new(dir)
        .open(ID_AAD, sealed.trim())
        .ok()?
        .parse()
        .ok()
}

/// One keyless GET through the source fetcher (public-only, egress-guarded).
fn get(url: &str) -> Result<String, String> {
    let req = SourceReq {
        kind: SourceKind::Rss,
        url: url.to_string(),
        name: sw::SOURCE_NAME.to_string(),
        topic: None,
    };
    match super::pulse_sources_ui::fetch_source(&req, None) {
        Fetched::Body { body, .. } => Ok(body),
        Fetched::NotModified => Err("no reply".into()),
        Fetched::Failed(why) => Err(why),
    }
}

/// The whole check, given a GET. Never puts the SteamID in an error.
fn run_check(
    mut list: Wishlist,
    steam_id: u64,
    allowed: usize,
    now: u64,
    get: &mut dyn FnMut(&str) -> Result<String, String>,
) -> WishRun {
    let failed = |list: Wishlist, why: String| WishRun {
        list,
        alerts: Vec::new(),
        error: Some(why),
    };
    let ids = match get(&sw::wishlist_url(steam_id)).map(|b| sw::parse_wishlist(&b)) {
        Ok(Some(ids)) => ids,
        Ok(None) => return failed(list, "Steam's wishlist reply wasn't readable".into()),
        Err(why) => return failed(list, format!("wishlist: {why}")),
    };
    let mut prices = std::collections::BTreeMap::new();
    for chunk in ids.chunks(sw::PRICE_BATCH) {
        match get(&sw::prices_url(chunk)) {
            Ok(body) => prices.extend(sw::parse_prices(&body)),
            Err(why) => return failed(list, format!("prices: {why}")),
        }
    }
    let mut alerts = list.check(&ids, &prices, allowed, now);
    for alert in alerts.iter_mut().filter(|a| a.name.is_empty()) {
        if let Some(name) = get(&sw::name_url(alert.app_id))
            .ok()
            .and_then(|b| sw::parse_name(&b, alert.app_id))
        {
            list.note_name(alert.app_id, &name);
            alert.name = name;
        }
    }
    WishRun {
        list,
        alerts,
        error: None,
    }
}

impl Cabin {
    fn wish_now(&mut self) -> &mut Wishlist {
        self.sources
            .wish
            .list
            .get_or_insert_with(|| sw::load(&crate::config::config_dir()))
    }

    fn save_wish(&mut self) {
        let list = self.wish_now().clone();
        if let Err(err) = sw::save(&crate::config::config_dir(), &list) {
            self.status = err;
        }
    }

    fn wish_has_id(&mut self) -> bool {
        *self
            .sources
            .wish
            .has_id
            .get_or_insert_with(|| crate::config::config_dir().join(ID_FILE).exists())
    }

    /// With the source check: the daily wishlist check, when on and due.
    pub(super) fn kick_wishlist(&mut self, now: u64) {
        if self.sources.wish.rx.is_some() || !self.wish_now().due(now) {
            return;
        }
        let dir = crate::config::config_dir();
        let Some(steam_id) = open_steam_id(&dir) else {
            return;
        };
        let bias =
            grokhub_core::pulse_sources::load_cache(&super::pulse_sources_ui::sources_cache_path())
                .source_bias
                .get(sw::SOURCE_NAME)
                .copied()
                .unwrap_or(0);
        let list = self.wish_now().clone();
        let (tx, rx) = mpsc::channel();
        self.sources.wish.rx = Some(rx);
        self.sources.wish.note = "Checking your wishlist…".into();
        std::thread::spawn(move || {
            let run = run_check(list, steam_id, sw::alerts_allowed(bias), now, &mut get);
            let _ = tx.send(run);
        });
    }

    pub(super) fn poll_wishlist(&mut self) {
        let Some(rx) = self.sources.wish.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(run) => self.finish_wishlist(run),
            Err(mpsc::TryRecvError::Empty) => self.sources.wish.rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sources.wish.note = "The wishlist check stopped before it finished.".into();
            }
        }
    }

    fn finish_wishlist(&mut self, run: WishRun) {
        // Settings may have changed while the check ran; the user's choice wins.
        let (on, threshold) = {
            let now = self.wish_now();
            (now.on, now.threshold)
        };
        if let Some(why) = run.error {
            self.sources.wish.note = format!("Wishlist check failed: {why}.");
            return;
        }
        if !on {
            return;
        }
        let mut list = run.list;
        list.threshold = threshold;
        let at = list.last_check_ms;
        let mut added = 0;
        for alert in &run.alerts {
            if grokhub_core::post_update(&mut self.updates, alert.card(at)) {
                added += 1;
            }
        }
        if added > 0 {
            self.persist_updates();
        }
        self.sources.wish.note = if list.private {
            String::new()
        } else if added == 0 {
            format!(
                "Checked {} wishlisted games. No new sales.",
                list.apps.len()
            )
        } else {
            format!(
                "Checked {} wishlisted games. {added} on sale.",
                list.apps.len()
            )
        };
        self.sources.wish.list = Some(list);
        self.save_wish();
    }

    fn set_wish_on(&mut self, on: bool) {
        if on {
            self.wish_now().on = true;
        } else {
            self.wish_now().turn_off();
            self.sources.wish.note.clear();
        }
        self.save_wish();
    }

    fn save_steam_id(&mut self, id: u64) {
        match seal_steam_id(&crate::config::config_dir(), id) {
            Ok(()) => {
                self.sources.wish.has_id = Some(true);
                self.sources.wish.draft.clear();
                self.wish_now().last_check_ms = 0;
                self.save_wish();
                self.kick_wishlist(now_ms());
            }
            Err(err) => self.sources.wish.note = format!("Couldn't save the SteamID: {err}"),
        }
    }

    fn forget_steam_id(&mut self) {
        let _ = std::fs::remove_file(crate::config::config_dir().join(ID_FILE));
        self.sources.wish.has_id = Some(false);
        self.set_wish_on(false);
    }

    /// Settings → Pulse → Sources: the wishlist toggle, SteamID and threshold.
    pub(super) fn ui_wishlist_source(&mut self, ui: &mut egui::Ui) {
        let mut on = self.wish_now().on;
        if ui
            .checkbox(
                &mut on,
                RichText::new("Alert me when wishlisted games go on sale")
                    .size(crate::theme::FONT_BODY),
            )
            .changed()
        {
            self.set_wish_on(on);
        }
        if !on {
            ui.add_space(6.0);
            return;
        }
        ui.indent("pulse-wishlist", |ui| {
            crate::cards::help_text(
                ui,
                "Once a day, from your public Steam wishlist and Steam's store prices. Your SteamID is sealed on this PC and only sent to Steam.",
            );
            if self.wish_has_id() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("SteamID saved")
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::muted()),
                    );
                    if crate::cards::ghost_pill(ui, "Check now") {
                        self.wish_now().last_check_ms = 0;
                        self.kick_wishlist(now_ms());
                    }
                    if crate::cards::ghost_pill(ui, "Forget") {
                        self.forget_steam_id();
                    }
                });
            } else {
                self.ui_steam_id_entry(ui);
            }
            let list = self.wish_now();
            let mut pick = Threshold::ALL
                .iter()
                .position(|t| *t == list.threshold)
                .unwrap_or(0);
            let before = pick;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Alert on").size(crate::theme::FONT_TIP));
                egui::ComboBox::from_id_salt("wishlist-threshold")
                    .selected_text(Threshold::ALL[pick].label())
                    .show_ui(ui, |ui| {
                        for (i, t) in Threshold::ALL.iter().enumerate() {
                            ui.selectable_value(&mut pick, i, t.label());
                        }
                    });
            });
            if pick != before {
                self.wish_now().threshold = Threshold::ALL[pick];
                self.save_wish();
            }
            if self.wish_now().private {
                crate::cards::settings_note(
                    ui,
                    "Your Steam wishlist looks private or empty. In Steam, set Privacy → Game details to Public to get alerts.",
                );
            }
            if !self.sources.wish.note.is_empty() {
                crate::cards::help_text(ui, &self.sources.wish.note.clone());
            }
        });
        ui.add_space(6.0);
    }

    fn ui_steam_id_entry(&mut self, ui: &mut egui::Ui) {
        let local = self
            .sources
            .wish
            .local
            .get_or_insert_with(|| {
                grokhub_core::steam::detect_root()
                    .map(|root| sw::local_accounts(&root))
                    .unwrap_or_default()
            })
            .clone();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.sources.wish.draft)
                    .password(true)
                    .hint_text("SteamID64 or /profiles/ link")
                    .desired_width(220.0),
            );
            if crate::cards::ghost_pill(ui, "Save") {
                match sw::parse_steam_id(&self.sources.wish.draft) {
                    Ok(id) => self.save_steam_id(id),
                    Err(why) => self.sources.wish.note = why,
                }
            }
            if let [id] = local.as_slice() {
                if crate::cards::ghost_pill(ui, "Use the account on this PC") {
                    self.save_steam_id(*id);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_ID: u64 = 76_561_197_960_278_073;

    fn replies(url: &str) -> Result<String, String> {
        if url.contains("GetWishlist") {
            Ok(
                r#"{"response":{"items":[{"appid":11,"priority":0},{"appid":22,"priority":1}]}}"#
                    .into(),
            )
        } else if url.contains("price_overview") {
            Ok(r#"{"11":{"success":true,"data":{"price_overview":{"initial":2000,"final":1000,"discount_percent":50,"initial_formatted":"$20.00","final_formatted":"$10.00"}}},
                   "22":{"success":true,"data":{"price_overview":{"initial":1500,"final":1500,"discount_percent":0,"initial_formatted":"$15.00","final_formatted":"$15.00"}}}}"#.into())
        } else if url.contains("appids=11&filters=basic") {
            Ok(r#"{"11":{"success":true,"data":{"name":"Fake Quest"}}}"#.into())
        } else {
            Err(format!("unexpected {url}"))
        }
    }

    #[test]
    fn a_check_reads_the_wishlist_prices_and_names_once() {
        let mut asked = Vec::new();
        let list = Wishlist {
            on: true,
            ..Default::default()
        };
        let run = run_check(list, FAKE_ID, 3, 9, &mut |url| {
            asked.push(url.to_string());
            replies(url)
        });
        assert_eq!(run.error, None);
        assert_eq!(run.alerts.len(), 1);
        assert_eq!(run.alerts[0].card(9).title, "Fake Quest is 50% off");
        assert_eq!(run.list.apps[&11].name, "Fake Quest");
        assert_eq!(asked.len(), 3, "{asked:?}");
        // The next day: the same sale doesn't alert and the name isn't fetched.
        asked.clear();
        let again = run_check(run.list, FAKE_ID, 3, 10, &mut |url| {
            asked.push(url.to_string());
            replies(url)
        });
        assert!(again.alerts.is_empty());
        assert_eq!(asked.len(), 2);
    }

    #[test]
    fn the_steam_id_is_sealed_and_fails_closed_without_the_key() {
        use grokhub_agent::harness as hx;
        let root = crate::config::test_config_root("wishlist-seal");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        hx::use_key_store_for(&root, std::sync::Arc::new(hx::MemoryKeyStore::new()));
        seal_steam_id(&root, FAKE_ID).unwrap();
        let on_disk = std::fs::read_to_string(root.join(ID_FILE)).unwrap();
        assert!(!on_disk.contains(&FAKE_ID.to_string()));
        assert_eq!(open_steam_id(&root), Some(FAKE_ID));
        hx::use_key_store_for(
            &root,
            std::sync::Arc::new(hx::MemoryKeyStore::unavailable()),
        );
        assert_eq!(open_steam_id(&root), None);
        assert!(seal_steam_id(&root, FAKE_ID).is_err());
    }

    #[test]
    fn a_failed_check_never_names_the_steam_id() {
        let run = run_check(Wishlist::default(), FAKE_ID, 3, 9, &mut |_| {
            Err("HTTP 500".into())
        });
        let why = run.error.unwrap();
        assert_eq!(why, "wishlist: HTTP 500");
        assert!(!why.contains(&FAKE_ID.to_string()));
    }

    #[test]
    fn alerts_post_only_while_on_and_keep_the_users_threshold() {
        let root = crate::config::test_config_root("wishlist-post");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        cabin.sources.wish.list = Some(Wishlist {
            on: true,
            threshold: Threshold::Off50,
            ..Default::default()
        });
        let run = run_check(
            Wishlist {
                on: true,
                ..Default::default()
            },
            FAKE_ID,
            3,
            9,
            &mut replies,
        );
        let before = cabin.updates.len();
        cabin.finish_wishlist(run);
        assert_eq!(cabin.updates.len(), before + 1);
        assert_eq!(cabin.wish_now().threshold, Threshold::Off50);
        assert_eq!(
            cabin.sources.wish.note,
            "Checked 2 wishlisted games. 1 on sale."
        );
        cabin.sources.wish.list = Some(Wishlist::default());
        let off = run_check(
            Wishlist {
                on: true,
                ..Default::default()
            },
            FAKE_ID,
            3,
            9,
            &mut replies,
        );
        let before = cabin.updates.len();
        cabin.finish_wishlist(off);
        assert_eq!(cabin.updates.len(), before, "turned off mid-check");
        assert_eq!(sw::load(&root).threshold, Threshold::Off50);
    }
}
