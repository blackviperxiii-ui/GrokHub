//! Steam wishlist sale alerts for Pulse. Opt-in. Once a day the cabin reads
//! the user's public wishlist (`IWishlistService/GetWishlist`, no key) and
//! the wishlisted games' prices from the store (`appdetails` with
//! `price_overview`, no key), then posts a Pulse card for each game whose
//! sale clears the user's threshold. The SteamID is sealed on disk by the
//! caller and never kept here; this file holds only prefs and what each game
//! cost when it was last seen.
//!
//! "Lowest" is the lowest price seen by this PC (the regular price until a
//! sale is seen), since no free, keyless price-history source exists.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "pulse-steam-wishlist.json";
pub const SOURCE_NAME: &str = "Steam wishlist";
/// One wishlist and price check a day.
pub const CHECK_EVERY_MS: u64 = 24 * 60 * 60 * 1000;
/// Alerts posted by one check before the feedback score moves it.
pub const ALERTS_PER_CHECK: i32 = 3;
/// Most alerts one check can post however much the user likes them.
pub const ALERTS_MAX: i32 = 5;
/// App ids per `appdetails` price call.
pub const PRICE_BATCH: usize = 50;
/// SteamID64 of account id 0.
const ID64_BASE: u64 = 76_561_197_960_265_728;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Threshold {
    #[default]
    AnySale,
    Off25,
    Off50,
    Lowest,
}

impl Threshold {
    pub const ALL: [Threshold; 4] = [Self::AnySale, Self::Off25, Self::Off50, Self::Lowest];

    pub fn label(self) -> &'static str {
        match self {
            Self::AnySale => "Any sale",
            Self::Off25 => "25% off or more",
            Self::Off50 => "50% off or more",
            Self::Lowest => "Lowest price seen",
        }
    }

    fn clears(self, discount: u32, new_low: bool) -> bool {
        match self {
            Self::AnySale => discount > 0,
            Self::Off25 => discount >= 25,
            Self::Off50 => discount >= 50,
            Self::Lowest => discount > 0 && new_low,
        }
    }
}

/// What one wishlisted game cost when it was last seen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seen {
    #[serde(default)]
    pub name: String,
    /// Lowest final price seen, in the store currency's cents.
    #[serde(default)]
    pub low: Option<u64>,
    /// The sale already alerted ("final:discount"); cleared when the sale ends.
    #[serde(default)]
    pub alerted: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wishlist {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub threshold: Threshold,
    #[serde(default)]
    pub last_check_ms: u64,
    /// The last check found the wishlist private or empty.
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub apps: BTreeMap<u32, Seen>,
}

/// One game's store price.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Price {
    pub initial: u64,
    pub final_price: u64,
    pub discount: u32,
    pub initial_text: String,
    pub final_text: String,
}

/// A sale worth a Pulse card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub app_id: u32,
    pub name: String,
    pub price: Price,
    pub new_low: bool,
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

pub fn load(config_dir: &Path) -> Wishlist {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, list: &Wishlist) -> Result<(), String> {
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path(config_dir), &text)
}

/// A SteamID64 from what the user typed: the 17-digit id or a
/// `steamcommunity.com/profiles/<id>` link. Custom `/id/<name>` links need a
/// Web API key to resolve, so they are refused with a hint.
pub fn parse_steam_id(text: &str) -> Result<u64, String> {
    let text = text.trim().trim_end_matches('/');
    if text.contains("/id/") {
        return Err("Custom profile links can't be read without a key. Paste the 17-digit SteamID64 or a /profiles/ link.".into());
    }
    let digits = text.rsplit('/').next().unwrap_or(text);
    match digits.parse::<u64>() {
        Ok(id) if digits.len() == 17 && id > ID64_BASE => Ok(id),
        _ => Err("That isn't a SteamID64 (17 digits, starting 7656119).".into()),
    }
}

/// The SteamID64 for a Steam `userdata/<account id>` folder name.
pub fn id_from_account(account: &str) -> Option<u64> {
    let account = account.parse::<u32>().ok().filter(|a| *a > 0)?;
    Some(ID64_BASE + u64::from(account))
}

/// The accounts that signed in to Steam on this PC, as SteamID64s.
pub fn local_accounts(steam_root: &Path) -> Vec<u64> {
    let Ok(dirs) = std::fs::read_dir(steam_root.join("userdata")) else {
        return Vec::new();
    };
    let mut ids: Vec<u64> = dirs
        .flatten()
        .filter_map(|d| id_from_account(&d.file_name().to_string_lossy()))
        .collect();
    ids.sort_unstable();
    ids
}

pub fn wishlist_url(steam_id: u64) -> String {
    format!("https://api.steampowered.com/IWishlistService/GetWishlist/v1/?steamid={steam_id}")
}

pub fn prices_url(app_ids: &[u32]) -> String {
    let ids: Vec<String> = app_ids.iter().map(u32::to_string).collect();
    format!(
        "https://store.steampowered.com/api/appdetails?appids={}&filters=price_overview",
        ids.join(",")
    )
}

pub fn name_url(app_id: u32) -> String {
    format!("https://store.steampowered.com/api/appdetails?appids={app_id}&filters=basic")
}

pub fn store_url(app_id: u32) -> String {
    format!("https://store.steampowered.com/app/{app_id}/")
}

pub fn capsule_url(app_id: u32) -> String {
    format!("https://cdn.akamai.steamstatic.com/steam/apps/{app_id}/header.jpg")
}

/// The wishlisted app ids, highest priority first. `None` for a body that
/// isn't the wishlist reply; empty for a private or empty wishlist.
pub fn parse_wishlist(body: &str) -> Option<Vec<u32>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let resp = v.get("response")?;
    let mut items: Vec<(u64, u32)> = resp
        .get("items")
        .and_then(|i| i.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    let id = u32::try_from(it.get("appid")?.as_u64()?).ok()?;
                    let priority = it.get("priority").and_then(|p| p.as_u64()).unwrap_or(0);
                    Some((priority, id))
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_unstable();
    Some(items.into_iter().map(|(_, id)| id).collect())
}

/// Prices from an `appdetails?filters=price_overview` reply. Free and
/// unreleased games have no price and are left out.
pub fn parse_prices(body: &str) -> BTreeMap<u32, Price> {
    let mut out = BTreeMap::new();
    let Ok(serde_json::Value::Object(apps)) = serde_json::from_str::<serde_json::Value>(body)
    else {
        return out;
    };
    for (id, app) in apps {
        let Ok(id) = id.parse::<u32>() else {
            continue;
        };
        let Some(p) = app.pointer("/data/price_overview") else {
            continue;
        };
        let num = |k: &str| p.get(k).and_then(|v| v.as_u64());
        let text = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let (Some(initial), Some(final_price)) = (num("initial"), num("final")) else {
            continue;
        };
        out.insert(
            id,
            Price {
                initial,
                final_price,
                discount: u32::try_from(num("discount_percent").unwrap_or(0)).unwrap_or(0),
                initial_text: text("initial_formatted"),
                final_text: text("final_formatted"),
            },
        );
    }
    out
}

/// The game's name from an `appdetails?filters=basic` reply.
pub fn parse_name(body: &str, app_id: u32) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let name = v.pointer(&format!("/{app_id}/data/name"))?.as_str()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// How many alerts one check may post: the base, moved by the feedback score
/// the user gave this source's posts (Not interested lowers it, Save raises it).
pub fn alerts_allowed(source_bias: i32) -> usize {
    usize::try_from((ALERTS_PER_CHECK + source_bias).clamp(0, ALERTS_MAX)).unwrap_or(0)
}

impl Wishlist {
    pub fn due(&self, now_ms: u64) -> bool {
        self.on && now_ms.saturating_sub(self.last_check_ms) >= CHECK_EVERY_MS
    }

    /// Fold one check into what was seen and return the sales to alert,
    /// deepest discount first, at most `allowed`. A sale alerts once; it can
    /// alert again only after it ends or its price drops further. Games that
    /// left the wishlist are forgotten.
    pub fn check(
        &mut self,
        wishlist: &[u32],
        prices: &BTreeMap<u32, Price>,
        allowed: usize,
        now_ms: u64,
    ) -> Vec<Alert> {
        self.last_check_ms = now_ms;
        self.private = wishlist.is_empty();
        self.apps.retain(|id, _| wishlist.contains(id));
        let mut alerts = Vec::new();
        for id in wishlist {
            let Some(price) = prices.get(id) else {
                continue;
            };
            let seen = self.apps.entry(*id).or_default();
            let low = seen.low.unwrap_or(price.initial);
            let new_low = price.final_price < low;
            seen.low = Some(low.min(price.final_price));
            if price.discount == 0 {
                seen.alerted = None;
                continue;
            }
            let key = format!("{}:{}", price.final_price, price.discount);
            if seen.alerted.as_deref() == Some(key.as_str())
                || !self.threshold.clears(price.discount, new_low)
            {
                continue;
            }
            alerts.push(Alert {
                app_id: *id,
                name: seen.name.clone(),
                price: price.clone(),
                new_low,
            });
        }
        alerts.sort_by(|a, b| {
            b.price
                .discount
                .cmp(&a.price.discount)
                .then(a.app_id.cmp(&b.app_id))
        });
        alerts.truncate(allowed);
        for alert in &alerts {
            if let Some(seen) = self.apps.get_mut(&alert.app_id) {
                seen.alerted = Some(format!(
                    "{}:{}",
                    alert.price.final_price, alert.price.discount
                ));
            }
        }
        alerts
    }

    /// Remember a game's name so later checks don't fetch it again.
    pub fn note_name(&mut self, app_id: u32, name: &str) {
        if let Some(seen) = self.apps.get_mut(&app_id) {
            seen.name = name.to_string();
        }
    }

    /// Off: forget everything but the threshold.
    pub fn turn_off(&mut self) {
        *self = Wishlist {
            threshold: self.threshold,
            ..Default::default()
        };
    }
}

impl Alert {
    pub fn title(&self) -> String {
        let name = if self.name.is_empty() {
            format!("App {}", self.app_id)
        } else {
            self.name.clone()
        };
        format!("{name} is {}% off", self.price.discount)
    }

    pub fn body(&self) -> String {
        let mut body = format!(
            "{} (was {}) on Steam.",
            self.price.final_text, self.price.initial_text
        );
        if self.new_low {
            body.push_str(" The lowest price seen yet.");
        }
        body
    }

    /// The Feed card: Save / Not interested act on it like any source post.
    pub fn card(&self, now_ms: u64) -> crate::UpdateCard {
        let source_id = format!(
            "source:steam-wishlist:{}:{}",
            self.app_id, self.price.final_price
        );
        let mut card =
            crate::update_feed::digest_card(&source_id, &self.title(), &self.body(), now_ms);
        card.citations = vec![store_url(self.app_id)];
        card.pulse.source_name = Some(SOURCE_NAME.to_string());
        card.pulse.image_urls = vec![capsule_url(self.app_id)];
        card.pulse.because = Some("On your wishlist".to_string());
        card
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake ids only: account 12345 and made-up app ids.
    const FAKE_ID: u64 = 76_561_197_960_278_073;

    fn price(initial: u64, final_price: u64, discount: u32) -> Price {
        Price {
            initial,
            final_price,
            discount,
            initial_text: format!("${}.{:02}", initial / 100, initial % 100),
            final_text: format!("${}.{:02}", final_price / 100, final_price % 100),
        }
    }

    fn ids(alerts: &[Alert]) -> Vec<u32> {
        alerts.iter().map(|a| a.app_id).collect()
    }

    #[test]
    fn steam_ids_come_from_a_number_a_profile_link_or_the_pc() {
        assert_eq!(parse_steam_id(" 76561197960278073 "), Ok(FAKE_ID));
        assert_eq!(
            parse_steam_id("https://steamcommunity.com/profiles/76561197960278073/"),
            Ok(FAKE_ID)
        );
        assert!(parse_steam_id("https://steamcommunity.com/id/someone")
            .unwrap_err()
            .starts_with("Custom profile links"));
        assert!(parse_steam_id("12345").is_err());
        assert_eq!(id_from_account("12345"), Some(FAKE_ID));
        assert_eq!(id_from_account("0"), None);
        assert_eq!(id_from_account("anonymous"), None);
        assert_eq!(
            wishlist_url(FAKE_ID),
            "https://api.steampowered.com/IWishlistService/GetWishlist/v1/?steamid=76561197960278073"
        );
        assert_eq!(
            prices_url(&[10, 20]),
            "https://store.steampowered.com/api/appdetails?appids=10,20&filters=price_overview"
        );
    }

    #[test]
    fn fixture_replies_parse_to_ids_prices_and_names() {
        let wish = r#"{"response":{"items":[
            {"appid":300,"priority":2,"date_added":1},
            {"appid":100,"priority":0,"date_added":1},
            {"appid":200,"priority":1,"date_added":1}]}}"#;
        assert_eq!(parse_wishlist(wish), Some(vec![100, 200, 300]));
        assert_eq!(parse_wishlist(r#"{"response":{}}"#), Some(vec![]));
        assert_eq!(parse_wishlist("<html>"), None);
        let prices = r#"{
            "100":{"success":true,"data":{"price_overview":{"currency":"USD","initial":1999,"final":999,"discount_percent":50,"initial_formatted":"$19.99","final_formatted":"$9.99"}}},
            "200":{"success":true,"data":[]},
            "300":{"success":false}}"#;
        let got = parse_prices(prices);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got.get(&100),
            Some(&Price {
                initial: 1999,
                final_price: 999,
                discount: 50,
                initial_text: "$19.99".into(),
                final_text: "$9.99".into(),
            })
        );
        assert_eq!(
            parse_name(
                r#"{"100":{"success":true,"data":{"name":" Fake Quest "}}}"#,
                100
            ),
            Some("Fake Quest".into())
        );
        assert_eq!(parse_name(r#"{"100":{"success":false}}"#, 100), None);
    }

    #[test]
    fn the_threshold_picks_which_sales_alert() {
        let wish = [1, 2, 3, 4];
        let prices: BTreeMap<u32, Price> = [
            (1, price(1000, 900, 10)),
            (2, price(1000, 700, 30)),
            (3, price(1000, 400, 60)),
            (4, price(1000, 1000, 0)),
        ]
        .into();
        let run = |t: Threshold| {
            let mut list = Wishlist {
                on: true,
                threshold: t,
                ..Default::default()
            };
            ids(&list.check(&wish, &prices, 5, 1))
        };
        assert_eq!(run(Threshold::AnySale), vec![3, 2, 1]);
        assert_eq!(run(Threshold::Off25), vec![3, 2]);
        assert_eq!(run(Threshold::Off50), vec![3]);
        // First sight: the regular price is the low, so every sale is a new low.
        assert_eq!(run(Threshold::Lowest), vec![3, 2, 1]);
    }

    #[test]
    fn a_sale_alerts_once_and_again_only_when_it_drops_or_returns() {
        let mut list = Wishlist {
            on: true,
            ..Default::default()
        };
        let day = CHECK_EVERY_MS;
        let on_sale: BTreeMap<u32, Price> = [(7, price(2000, 1500, 25))].into();
        let first = list.check(&[7], &on_sale, 3, day);
        assert_eq!(ids(&first), vec![7]);
        assert!(first[0].new_low);
        assert_eq!(
            ids(&list.check(&[7], &on_sale, 3, 2 * day)),
            Vec::<u32>::new(),
            "same sale"
        );
        let deeper: BTreeMap<u32, Price> = [(7, price(2000, 1000, 50))].into();
        let again = list.check(&[7], &deeper, 3, 3 * day);
        assert_eq!((ids(&again), again[0].new_low), (vec![7], true));
        let over: BTreeMap<u32, Price> = [(7, price(2000, 2000, 0))].into();
        assert!(list.check(&[7], &over, 3, 4 * day).is_empty());
        assert_eq!(list.apps[&7].alerted, None);
        assert_eq!(list.apps[&7].low, Some(1000));
        // The same 25% sale returns: it alerts, but it is not a new low.
        let back = list.check(&[7], &on_sale, 3, 5 * day);
        assert_eq!((ids(&back), back[0].new_low), (vec![7], false));
        list.threshold = Threshold::Lowest;
        list.apps.get_mut(&7).unwrap().alerted = None;
        assert!(
            list.check(&[7], &on_sale, 3, 6 * day).is_empty(),
            "not below 1000"
        );
    }

    #[test]
    fn the_cap_follows_feedback_and_private_lists_say_so() {
        assert_eq!(alerts_allowed(0), 3);
        assert_eq!(alerts_allowed(-2), 1);
        assert_eq!(alerts_allowed(-4), 0);
        assert_eq!(alerts_allowed(4), 5);
        let prices: BTreeMap<u32, Price> = (1..=6)
            .map(|i| (i, price(1000, 1000 - u64::from(i) * 100, i * 10)))
            .collect();
        let mut list = Wishlist {
            on: true,
            ..Default::default()
        };
        let wish: Vec<u32> = (1..=6).collect();
        assert_eq!(
            ids(&list.check(&wish, &prices, alerts_allowed(-2), 1)),
            vec![6]
        );
        assert!(!list.private);
        assert!(list.check(&[], &prices, 3, 2).is_empty());
        assert!(list.private);
        assert!(list.apps.is_empty(), "left the wishlist, forgotten");
        assert!(!list.due(2 + CHECK_EVERY_MS - 1));
        assert!(list.due(2 + CHECK_EVERY_MS));
        list.turn_off();
        assert!(!list.due(u64::MAX));
    }

    #[test]
    fn an_alert_card_links_the_store_with_its_art_and_price() {
        let alert = Alert {
            app_id: 4242,
            name: "Fake Quest".into(),
            price: price(1999, 999, 50),
            new_low: true,
        };
        let card = alert.card(5_000);
        assert_eq!(card.title, "Fake Quest is 50% off");
        assert_eq!(
            card.body.as_deref(),
            Some("$9.99 (was $19.99) on Steam. The lowest price seen yet.")
        );
        assert_eq!(card.source_id, "source:steam-wishlist:4242:999");
        assert_eq!(
            card.citations,
            vec!["https://store.steampowered.com/app/4242/".to_string()]
        );
        assert_eq!(card.pulse.source_name.as_deref(), Some("Steam wishlist"));
        assert_eq!(
            card.pulse.image_urls,
            vec!["https://cdn.akamai.steamstatic.com/steam/apps/4242/header.jpg".to_string()]
        );
        assert!(crate::pulse_sources::is_source_post(&card));
        let mut feed = Vec::new();
        assert!(crate::post_update(&mut feed, card));
    }
}
