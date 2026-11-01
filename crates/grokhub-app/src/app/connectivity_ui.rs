//! Install grokhub-core connectivity and mirror live state into the agent router,
//! and the Offline only chip (LU-13) that stands in for the network chip.

use eframe::egui;
use grokhub_core::connectivity::ConnectivityUiPhase;

/// The chip while Offline only is on: the user's choice, not an outage.
pub(super) const OFFLINE_ONLY_CHIP: &str = "Offline only";
/// The chip's hover: where the switch lives.
pub(super) const OFFLINE_ONLY_TIP: &str =
    "You turned on Offline only. Turn it off in Settings → AI models.";

/// The chip's main label: the Offline only switch wins over what the network says.
pub(super) fn chip_label(offline_only: bool, phase: ConnectivityUiPhase) -> &'static str {
    if offline_only {
        OFFLINE_ONLY_CHIP
    } else {
        phase.label()
    }
}

/// While Offline only is on, paint its chip (and the on-device model's paused
/// badge) in place of the network chip and return `true`.
pub(super) fn paint_offline_only_chip(ui: &mut egui::Ui) -> bool {
    use crate::ui_kit::{badge, BadgeTone};
    if !grokhub_core::offline::enabled() {
        return false;
    }
    let label = chip_label(true, ConnectivityUiPhase::OfflineLocal);
    badge(ui, label, BadgeTone::Accent).on_hover_text(OFFLINE_ONLY_TIP);
    if grokhub_agent::route::local::switched_on() && grokhub_agent::route::local_gov::paused() {
        badge(
            ui,
            grokhub_agent::route::local::LOCAL_PAUSED_MSG,
            BadgeTone::Warning,
        );
    }
    true
}

#[cfg(test)]
pub(super) fn tick_connectivity(_now_ms: u64) {}

#[cfg(not(test))]
mod live {
    use std::sync::{Arc, Mutex, OnceLock};

    use grokhub_core::chat::XAI_BASE;
    use grokhub_core::connectivity::{default_http_client, Connectivity};

    fn engine() -> Arc<Mutex<Connectivity>> {
        static SLOT: OnceLock<Arc<Mutex<Connectivity>>> = OnceLock::new();
        SLOT.get_or_init(|| {
            let probe = format!("{}/models", XAI_BASE.trim_end_matches('/'));
            let arc = Connectivity::default()
                .with_probe_url(probe)
                .install_global();
            grokhub_agent::route::connectivity::sync_from_core();
            arc
        })
        .clone()
    }

    /// Heartbeat: debounce, re-probe when offline, and push state into `route::connectivity`.
    pub fn tick_connectivity(now_ms: u64) {
        let arc = engine();
        let Ok(mut g) = arc.lock() else {
            return;
        };
        if let Some(changed) = g.tick(now_ms) {
            grokhub_agent::route::connectivity::on_core_state_changed(changed);
        } else {
            grokhub_agent::route::connectivity::sync_from_core();
        }
        if g.probe_due(now_ms) {
            if let Ok(client) = default_http_client() {
                let _ = g.run_probe_if_due(&client, now_ms);
                grokhub_agent::route::connectivity::sync_from_core();
            }
        }
    }
}

#[cfg(not(test))]
pub(super) use live::tick_connectivity;

#[cfg(test)]
mod tests {
    use super::*;

    fn painted(on: bool) -> (bool, String) {
        let _forced = grokhub_core::offline::force_for_thread(on);
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let mut shown = false;
        let out = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                shown = paint_offline_only_chip(ui);
            });
        });
        let mut text = String::new();
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                text.push_str(t.galley.text());
                text.push('\n');
            }
        }
        (shown, text)
    }

    #[test]
    fn the_chip_reads_offline_only_while_the_switch_is_on() {
        assert_eq!(
            chip_label(true, ConnectivityUiPhase::Online),
            "Offline only"
        );
        assert_eq!(
            chip_label(true, ConnectivityUiPhase::OfflineLocal),
            "Offline only"
        );
        assert_eq!(
            chip_label(false, ConnectivityUiPhase::OfflineLocal),
            "Offline (local engine)"
        );
        assert_eq!(chip_label(false, ConnectivityUiPhase::Online), "Online");
        assert_eq!(
            OFFLINE_ONLY_TIP,
            "You turned on Offline only. Turn it off in Settings → AI models."
        );
        let (shown, text) = painted(true);
        assert!(shown);
        // The paused badge may follow when another test left the governor paused.
        assert_eq!(text.lines().next(), Some("Offline only"));
        assert!(!text.contains("Offline (local engine)"), "{text}");
        let (shown, text) = painted(false);
        assert!(!shown, "the network chip paints instead");
        assert_eq!(text, "");
    }
}
