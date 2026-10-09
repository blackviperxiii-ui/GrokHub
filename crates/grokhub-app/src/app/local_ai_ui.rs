//! Router R3a in Settings: the "On-device model for background tasks" toggle
//! (`localModel`). The tick hands the flag to `route::local::set_enabled`, so
//! a flip takes effect without a restart. The route goes live only with a
//! runtime installed, and then only for background classes; chat never goes
//! local. With nothing installed the row says so and offers Set up, which
//! opens the setup wizard's on-device model step.

use grokhub_agent::route::local;

use super::*;

/// The toggle's hint, by state. It leads with On or Off (SB-07).
/// `downloaded` names a model file on disk that this build has no runtime for.
pub(super) fn local_model_hint(on: bool, installed: bool, downloaded: Option<&str>) -> String {
    match (on, installed, downloaded) {
        (false, _, _) => "Off. Background work uses your cloud model.".into(),
        (true, true, _) => "On. Dream, digests and summaries run on this device. Chat still uses your cloud model.".into(),
        (true, false, Some(name)) => {
            format!("On. {name} is downloaded, but this build can't run it yet, so background work uses your cloud model.")
        }
        (true, false, None) => "On, but no local model is installed. Background work uses your cloud model until one is set up.".into(),
    }
}

/// The quiet status line after a flip, naming the live tier when the route is live.
pub(super) fn local_model_status(on: bool, installed: bool, tier: &str) -> String {
    match (on, installed) {
        (false, _) => "On-device model for background tasks: off".into(),
        (true, true) => format!("On-device model for background tasks: on ({tier})"),
        (true, false) => "On-device model for background tasks: on, no local model installed".into(),
    }
}

impl Cabin {
    /// Save the toggle and hand it to the router now. A live route change
    /// refreshes the model list so the router picks the tier up (or drops it)
    /// with no prompt.
    pub(super) fn set_local_model(&mut self, on: bool) {
        if self.cfg.local_model == on {
            return;
        }
        self.cfg.local_model = on;
        self.persist_cfg();
        local::set_enabled(on);
        let installed = local::installed();
        if installed {
            self.refresh_models_now();
        }
        self.status = local_model_status(on, installed, local::live_tier());
    }

    /// The Settings → Cabin defaults rows: the toggle, and Set up while nothing is installed.
    pub(super) fn ui_local_model_rows(&mut self, ui: &mut egui::Ui) {
        let installed = local::installed();
        let mut on = self.cfg.local_model;
        let downloaded = self.downloaded_model_name();
        if crate::cards::settings_toggle(ui, "On-device model for background tasks", &local_model_hint(on, installed, downloaded), &mut on) {
            self.set_local_model(on);
        }
        if !installed && crate::cards::settings_action(ui, "Local model", &self.local_model_row_hint(), "Set up") {
            self.open_local_setup();
        }
    }

    /// Set up: the setup wizard's on-device model step.
    pub(super) fn open_local_setup(&mut self) {
        self.open_setup(super::setup_wizard::SetupStep::LocalAi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_and_status_name_each_state() {
        assert_eq!(local_model_hint(false, false, None), "Off. Background work uses your cloud model.");
        assert_eq!(local_model_hint(false, true, Some("Qwen 2.5 7B")), "Off. Background work uses your cloud model.");
        assert_eq!(local_model_hint(true, true, None), "On. Dream, digests and summaries run on this device. Chat still uses your cloud model.");
        assert_eq!(
            local_model_hint(true, false, Some("Qwen 2.5 7B")),
            "On. Qwen 2.5 7B is downloaded, but this build can't run it yet, so background work uses your cloud model."
        );
        assert_eq!(
            local_model_hint(true, false, None),
            "On, but no local model is installed. Background work uses your cloud model until one is set up."
        );
        assert_eq!(local_model_status(false, true, "local:small"), "On-device model for background tasks: off");
        assert_eq!(local_model_status(true, true, "local:tiny"), "On-device model for background tasks: on (local:tiny)");
        assert_eq!(local_model_status(true, false, "local:small"), "On-device model for background tasks: on, no local model installed");
    }

    #[test]
    fn the_toggle_saves_localmodel_and_routes_nothing_while_no_model_is_installed() {
        let root = std::env::temp_dir().join(format!("grokhub-local-ai-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("dir");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        assert!(!app.cfg.local_model);
        app.set_local_model(true);
        assert!(app.cfg.local_model);
        assert_eq!(app.status, "On-device model for background tasks: on, no local model installed");
        // This build has no runtime: the flag is set but the route stays off, and no refresh was asked for.
        assert!(!local::installed() && !local::enabled());
        assert!(!app.harness.router.force_table);
        let mut saved = String::new();
        for _ in 0..200 {
            saved = std::fs::read_to_string(root.join("app.json")).unwrap_or_default();
            if saved.contains("\"localModel\": true") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(saved.contains("\"localModel\": true"), "{saved}");
        // The same state again is a no-op; off says off.
        app.status.clear();
        app.set_local_model(true);
        assert_eq!(app.status, "");
        app.set_local_model(false);
        assert!(!app.cfg.local_model);
        assert_eq!(app.status, "On-device model for background tasks: off");
        app.open_local_setup();
        assert_eq!(app.setup.step, Some(super::super::setup_wizard::SetupStep::LocalAi));
        local::set_enabled(false);
        let _ = std::fs::remove_dir_all(root);
    }
}
