//! How the app runs an action a card offers: quietly in the background, in a
//! live dialog that shows each step, or in the card's chat.

use crate::card_report::{FixOffer, OfferKind};

/// Where an action runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionMode {
    /// Read only, or one quick step that is easy to undo: runs quietly and
    /// leaves a receipt on the card.
    Background,
    /// Changes the computer in more than one step, asks for a hard approval,
    /// or takes a while: a dialog with each step as it runs, Cancel and Undo.
    Live,
    /// Needs a choice or a conversation.
    Chat,
}

/// What an action does, as far as choosing its mode goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActionShape {
    /// It changes something on the computer (a service, a file, a setting).
    pub changes_system: bool,
    /// Steps it takes.
    pub steps: usize,
    /// A step needs a hard approval (admin, a package, a driver).
    pub hard: bool,
    /// It can't go on without the person picking or saying something.
    pub needs_choice: bool,
    /// It is expected to run longer than a few seconds.
    pub long: bool,
}

pub fn action_mode(shape: &ActionShape) -> ActionMode {
    if shape.needs_choice {
        ActionMode::Chat
    } else if shape.hard || shape.long || (shape.changes_system && shape.steps > 1) {
        ActionMode::Live
    } else {
        ActionMode::Background
    }
}

/// The shape of a card's fix offer.
pub fn offer_shape(offer: &FixOffer) -> ActionShape {
    match offer.kind {
        OfferKind::Look => ActionShape {
            steps: 1,
            ..Default::default()
        },
        OfferKind::Chat => ActionShape {
            needs_choice: true,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(changes_system: bool, steps: usize, hard: bool, long: bool) -> ActionShape {
        ActionShape {
            changes_system,
            steps,
            hard,
            needs_choice: false,
            long,
        }
    }

    #[test]
    fn each_action_runs_where_its_stakes_put_it() {
        use ActionMode::*;
        let cases = [
            // Reading crash history: quiet, a receipt on the card.
            (shape(false, 3, false, false), Background),
            // One quick reversible change (restart a service).
            (shape(true, 1, false, false), Background),
            // Several changes: show each step.
            (shape(true, 2, false, false), Live),
            // An admin step, even alone.
            (shape(true, 1, true, false), Live),
            // A long read (a full scan) still shows its progress.
            (shape(false, 1, false, true), Live),
        ];
        for (s, mode) in cases {
            assert_eq!(action_mode(&s), mode, "{s:?}");
        }
        let choose = ActionShape {
            needs_choice: true,
            changes_system: true,
            steps: 4,
            hard: true,
            long: true,
        };
        assert_eq!(action_mode(&choose), Chat);
    }

    #[test]
    fn a_look_offer_runs_quietly_and_an_ask_goes_to_the_chat() {
        let look = FixOffer {
            label: "Look into the crashes".into(),
            prompt: "Read its crash history.".into(),
            kind: OfferKind::Look,
            fix_type: None,
        };
        assert_eq!(action_mode(&offer_shape(&look)), ActionMode::Background);
        let ask = FixOffer {
            kind: OfferKind::Chat,
            fix_type: None,
            ..look
        };
        assert_eq!(action_mode(&offer_shape(&ask)), ActionMode::Chat);
    }
}
