//! Empty-home cabin pulse gate and Pulse/Feed kit paint surface.

use super::*;

#[allow(unused_imports)]
pub(super) use feed_ui::{
    kit_daily_drop_badge, kit_pulse_feed_post, kit_pulse_ideas_row, kit_skeleton_row,
    paint_feed_empty_prompt, paint_ideas_empty_prompt,
};

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

    #[test]
    fn pulse_reexports_feed_kit_paint() {
        let pulse = include_str!("pulse.rs");
        assert!(
            pulse.contains("kit_pulse_feed_post") && pulse.contains("paint_feed_empty_prompt"),
            "pulse.rs surfaces feed_ui kit helpers"
        );
    }
}
