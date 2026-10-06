//! Empty-home cabin pulse gate.

use super::*;

pub(super) fn should_paint_pulse(empty_chat: bool, scratch: bool, signed_in: bool) -> bool {
    empty_chat && !scratch && signed_in
}

impl Cabin {
    pub(super) fn pulse_should_paint(&self) -> bool {
        should_paint_pulse(
            self.messages.is_empty(),
            self.scratch(),
            self.cabin_signed_in(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_skips_scratch_and_full_thread() {
        assert!(should_paint_pulse(true, false, true));
        assert!(!should_paint_pulse(false, false, true));
        assert!(!should_paint_pulse(true, true, true));
        assert!(!should_paint_pulse(true, false, false));
    }
}
