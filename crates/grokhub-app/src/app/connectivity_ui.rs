//! Install grokhub-core connectivity and mirror live state into the agent router.

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
pub(super) fn tick_connectivity(now_ms: u64) {
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
