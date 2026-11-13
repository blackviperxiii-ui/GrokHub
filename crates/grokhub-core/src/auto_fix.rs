//! Offer first: a card's fix waits for a tap unless its kind of fix is
//! switched on in Settings > Pulse. Every switch starts off, and a switch only
//! skips the tap. Anything that changes the computer still asks on its card.

use serde::{Deserialize, Serialize};

use crate::action_mode::{action_mode, offer_shape, ActionMode};
use crate::update_feed::UpdateCard;

/// A kind of fix a Pulse card can offer, each with its own switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FixType {
    /// Read a crashing program's crash history (read only).
    CrashCheck,
}

impl FixType {
    pub const ALL: [FixType; 1] = [FixType::CrashCheck];

    /// The switch's words in Settings.
    pub fn label(self) -> &'static str {
        match self {
            FixType::CrashCheck => "Check a crashing program's crash history",
        }
    }

    /// The line under the switch.
    pub fn note(self) -> &'static str {
        match self {
            FixType::CrashCheck => {
                "Read only. Runs when the card shows up, and what it finds goes on the card."
            }
        }
    }
}

/// Which kinds of fix may run without a tap. Empty, so all off, by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoFixes {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<FixType>,
}

impl AutoFixes {
    pub fn is_empty(&self) -> bool {
        self.on.is_empty()
    }

    pub fn is_on(&self, fix: FixType) -> bool {
        self.on.contains(&fix)
    }

    pub fn set(&mut self, fix: FixType, on: bool) {
        self.on.retain(|f| *f != fix);
        if on {
            self.on.push(fix);
            self.on.sort();
        }
    }
}

/// The offer that may run as this card arrives: the first one whose kind is
/// switched on and that runs quietly. A card that already has a receipt has
/// run once and does not run again.
pub fn auto_offer(card: &UpdateCard, auto: &AutoFixes) -> Option<usize> {
    let report = card.pulse.report.as_ref()?;
    if !report.receipts.is_empty() {
        return None;
    }
    report.offers.iter().position(|o| {
        o.fix_type.is_some_and(|f| auto.is_on(f))
            && action_mode(&offer_shape(o)) == ActionMode::Background
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_report::{CardReceipt, CardReport, FixOffer, OfferKind};
    use crate::update_feed::{blank_card, UpdateKind};

    fn crash_card(kind: OfferKind) -> UpdateCard {
        let mut card = blank_card("c".into(), UpdateKind::Suggestion, "T".into(), None, 1);
        card.pulse.report = Some(CardReport {
            found: "bluetoothd crashed 4 times today".into(),
            offers: vec![FixOffer {
                label: "Look into the crashes".into(),
                prompt: "Read its crash history.".into(),
                kind,
                fix_type: Some(FixType::CrashCheck),
            }],
            ..Default::default()
        });
        card
    }

    #[test]
    fn every_switch_starts_off_and_nothing_runs_untapped() {
        let off = AutoFixes::default();
        assert!(FixType::ALL.iter().all(|f| !off.is_on(*f)));
        assert_eq!(auto_offer(&crash_card(OfferKind::Look), &off), None);
        // An old config with no switches loads all off.
        let loaded: AutoFixes = serde_json::from_str("{}").unwrap();
        assert_eq!(loaded, off);
        assert_eq!(serde_json::to_string(&off).unwrap(), "{}");
    }

    #[test]
    fn a_switched_on_read_only_fix_runs_once_as_the_card_arrives() {
        let mut auto = AutoFixes::default();
        auto.set(FixType::CrashCheck, true);
        assert_eq!(
            serde_json::to_string(&auto).unwrap(),
            r#"{"on":["crashCheck"]}"#
        );
        assert_eq!(auto_offer(&crash_card(OfferKind::Look), &auto), Some(0));
        // An offer that needs a conversation still waits for the tap.
        assert_eq!(auto_offer(&crash_card(OfferKind::Chat), &auto), None);
        // Once it ran, it doesn't run again.
        let mut ran = crash_card(OfferKind::Look);
        ran.pulse
            .report
            .as_mut()
            .unwrap()
            .receipts
            .push(CardReceipt {
                at_ms: 2,
                what: "Checked bluetoothd's crash history".into(),
                result: vec![],
            });
        assert_eq!(auto_offer(&ran, &auto), None);
        // An offer of no kind never runs on its own.
        let mut plain = crash_card(OfferKind::Look);
        plain.pulse.report.as_mut().unwrap().offers[0].fix_type = None;
        assert_eq!(auto_offer(&plain, &auto), None);
        auto.set(FixType::CrashCheck, false);
        assert_eq!(auto, AutoFixes::default());
    }
}
