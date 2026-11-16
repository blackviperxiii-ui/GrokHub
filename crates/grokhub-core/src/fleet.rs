//! Machine cards for the Fleet page: each computer running the app reports
//! its CPU, memory and app version, and a hub holds the last card from each
//! paired computer (`POST /v1/fleet`, `GET /v1/fleet`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::Peer;

/// A card older than this shows its computer as asleep, dimmed, not hidden.
pub const STALE_MS: u64 = 3 * 60 * 1000;

/// Longest version text a card keeps.
const MAX_VERSION: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCard {
    pub id: String,
    pub name: String,
    /// CPU use since the last reading, 0 to 100; `None` before a second reading.
    pub cpu_pct: Option<u8>,
    pub ram_mb: Option<u64>,
    pub free_ram_mb: Option<u64>,
    pub app_version: String,
    /// When the card was made, in ms since the epoch.
    pub at_ms: u64,
}

impl MachineCard {
    /// Whether the card is fresh enough to call its computer online at `now`.
    pub fn online(&self, now: u64) -> bool {
        now.saturating_sub(self.at_ms) < STALE_MS
    }
}

/// The card a paired computer sent. Its id and name are the pairing's, never
/// the body's, and the time is the hub's, so a computer can't report as
/// another one or from the future.
pub fn card_from_peer(body: &Value, peer: &Peer, now: u64) -> Result<MachineCard, &'static str> {
    let version = body
        .get("appVersion")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or("appVersion is missing")?;
    if version.chars().count() > MAX_VERSION {
        return Err("appVersion is too long");
    }
    let mb = |key: &str| body.get(key).and_then(Value::as_u64);
    let cpu_pct = body
        .get("cpuPct")
        .and_then(Value::as_u64)
        .map(|p| p.min(100) as u8);
    let ram_mb = mb("ramMb");
    let free_ram_mb = mb("freeRamMb").map(|free| ram_mb.map_or(free, |total| free.min(total)));
    Ok(MachineCard {
        id: peer.id.clone(),
        name: peer.name.clone(),
        cpu_pct,
        ram_mb,
        free_ram_mb,
        app_version: version.to_string(),
        at_ms: now,
    })
}

/// `cards` with `card` in place of the last one from its computer.
pub fn keep_card(cards: &mut Vec<MachineCard>, card: MachineCard) {
    match cards.iter_mut().find(|c| c.id == card.id) {
        Some(old) => *old = card,
        None => cards.push(card),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn desk() -> Peer {
        Peer {
            id: "d-desk".into(),
            name: "Desk".into(),
            token: "t-desk".into(),
            last_seen: 0,
        }
    }

    #[test]
    fn a_card_takes_the_pairings_name_and_the_hubs_time() {
        let body = json!({
            "id": "d-other", "name": "Not me", "atMs": 9_999_999_999_999u64,
            "cpuPct": 37, "ramMb": 32_000, "freeRamMb": 12_500, "appVersion": " 3.6.0-beta.12 "
        });
        let card = card_from_peer(&body, &desk(), 1_000).unwrap();
        assert_eq!(
            card,
            MachineCard {
                id: "d-desk".into(),
                name: "Desk".into(),
                cpu_pct: Some(37),
                ram_mb: Some(32_000),
                free_ram_mb: Some(12_500),
                app_version: "3.6.0-beta.12".into(),
                at_ms: 1_000,
            }
        );
    }

    #[test]
    fn out_of_range_readings_are_clamped_and_a_card_needs_a_version() {
        let body =
            json!({ "cpuPct": 250, "ramMb": 8_000, "freeRamMb": 9_000, "appVersion": "3.6.0" });
        let card = card_from_peer(&body, &desk(), 5).unwrap();
        assert_eq!((card.cpu_pct, card.free_ram_mb), (Some(100), Some(8_000)));
        assert_eq!(
            card_from_peer(&json!({ "cpuPct": 5 }), &desk(), 5),
            Err("appVersion is missing")
        );
        assert_eq!(
            card_from_peer(&json!({ "appVersion": "x".repeat(65) }), &desk(), 5),
            Err("appVersion is too long")
        );
        let bare = card_from_peer(&json!({ "appVersion": "3.6.0" }), &desk(), 5).unwrap();
        assert_eq!(
            (bare.cpu_pct, bare.ram_mb, bare.free_ram_mb),
            (None, None, None)
        );
    }

    #[test]
    fn a_card_is_online_for_three_minutes() {
        let card = card_from_peer(&json!({ "appVersion": "3.6.0" }), &desk(), 10_000).unwrap();
        assert!(card.online(10_000));
        assert!(card.online(10_000 + 179_999));
        assert!(!card.online(10_000 + 180_000));
        assert!(
            card.online(0),
            "a clock behind the card's still reads online"
        );
    }

    #[test]
    fn a_new_card_replaces_only_its_own_computers() {
        let mut cards = Vec::new();
        let at = |n| card_from_peer(&json!({ "appVersion": "3.6.0" }), &desk(), n).unwrap();
        keep_card(&mut cards, at(1));
        let mut laptop = at(2);
        laptop.id = "d-laptop".into();
        keep_card(&mut cards, laptop);
        keep_card(&mut cards, at(3));
        let kept: Vec<(&str, u64)> = cards.iter().map(|c| (c.id.as_str(), c.at_ms)).collect();
        assert_eq!(kept, vec![("d-desk", 3), ("d-laptop", 2)]);
    }
}
