//! Settings → AI models → "Models Auto may use": per-model preferences.
//!
//! One row per model Auto can see: the xAI chat catalog, the on-device tiers
//! the registry lists, and every provider model you gave a preference. Each
//! row says where Auto may send the model on its own (everything, chats,
//! coding and plans, background work, never) and, for a model that takes
//! one, the reasoning effort Auto sends it. "Add a provider model" lists the
//! models of providers you allowed. The router reads the map through
//! `prefs::set_model_prefs` on the tick; a pinned default model and "Use for
//! your chats" still win over a preference.

use grokhub_agent::route::prefs::{ModelPrefs, ModelUse};
use grokhub_core::model_registry::Registry;

use super::*;

pub(super) const SECTION: &str = "Models Auto may use";
const SECTION_HINT: &str = "Where Auto may send each model on its own. A pinned default model and \"Use for your chats\" always win over these.";
const XAI_HINT: &str = "Default lets Auto decide from its routing table and your plan.";
const PROVIDER_HINT: &str =
    "Default is only when you pick it. Allow a class and Auto ranks it with the rest.";
const LOCAL_HINT: &str =
    "Default follows Local AI's own rules (background work, fallback, Local AI for chats).";
const EFFORT_HINT: &str =
    "The reasoning effort Auto sends this model. Auto picks per step by default.";
const EFFORT_LABELS: [(&str, &str); 5] = [
    ("Auto picks the effort", ""),
    ("Low", "low"),
    ("Medium", "medium"),
    ("High", "high"),
    ("Very high", "xhigh"),
];
const ADD_ROW: &str = "Add a provider model";
const ADD_HINT: &str =
    "A model from a provider you allowed. It starts as \"Everything\"; change it on its row.";
const ADD_NONE: &str = "Pick a model…";

/// The rows the section paints: (model id, is a provider model). xAI chat
/// catalog first, then the on-device tiers the registry lists, then every
/// provider model with a preference, alphabetical.
pub(super) fn pref_rows(prefs: &ModelPrefs, reg: &Registry) -> Vec<(String, bool)> {
    let mut rows: Vec<(String, bool)> = super::settings::cabin_default_models()
        .into_iter()
        .filter(|(id, _)| !id.is_empty())
        .map(|(id, _)| (id.to_string(), false))
        .collect();
    for tier in grokhub_agent::route::local::TIERS {
        if reg.get(tier).is_some() {
            rows.push((tier.to_string(), false));
        }
    }
    let mut provider: Vec<String> = prefs
        .ids()
        .filter(|id| !rows.iter().any(|(r, _)| r == id))
        .filter(|id| id.contains('/'))
        .map(str::to_string)
        .collect();
    provider.sort();
    rows.extend(provider.into_iter().map(|id| (id, true)));
    rows
}

/// "Grok 4.7 · grok-4.7" for a catalog model, "On-device model · local:small"
/// for a tier, the id for a provider model.
pub(super) fn pref_title(id: &str) -> String {
    if let Some((_, label)) = super::settings::cabin_default_models()
        .into_iter()
        .find(|(k, _)| *k == id)
    {
        return format!("{label} · {id}");
    }
    if grokhub_agent::route::local::is_local(id) {
        return format!("On-device model · {id}");
    }
    id.to_string()
}

fn pref_hint(id: &str, new_provider: bool) -> &'static str {
    if new_provider {
        PROVIDER_HINT
    } else if grokhub_agent::route::local::is_local(id) {
        LOCAL_HINT
    } else {
        XAI_HINT
    }
}

/// The model takes a reasoning effort: its listing names one, or xAI's
/// reasoning families do.
pub(super) fn takes_effort(id: &str, reg: &Registry) -> bool {
    reg.get(id)
        .and_then(|r| r.meta.efforts.as_ref())
        .is_some_and(|m| !m.is_empty())
        || grokhub_core::accepts_reasoning_effort(id)
}

impl Cabin {
    /// Settings → AI models, under "How Auto picks".
    pub(super) fn ui_model_pref_rows(&mut self, ui: &mut egui::Ui) {
        crate::ui_kit::section_header(ui, SECTION, None);
        crate::cards::settings_note(ui, SECTION_HINT);
        let dir = crate::config::config_dir();
        let (reg, _) = grokhub_agent::route::live::snapshot(&dir);
        let rows = pref_rows(&self.cfg.model_prefs, &reg);
        let mut changed = false;
        for (id, new_provider) in &rows {
            ui.push_id(("model-pref", id.as_str()), |ui| {
                let labels: Vec<String> = ModelUse::ALL
                    .iter()
                    .map(|u| u.label(*new_provider).to_string())
                    .collect();
                let current = self.cfg.model_prefs.use_of(id);
                let picked = current.label(*new_provider).to_string();
                if let Some(i) = crate::cards::settings_dropdown(
                    ui,
                    &pref_title(id),
                    pref_hint(id, *new_provider),
                    &picked,
                    &labels,
                ) {
                    let next = ModelUse::ALL[i.min(ModelUse::ALL.len() - 1)];
                    if next != current {
                        self.cfg.model_prefs.set_use(id, next);
                        changed = true;
                    }
                }
                if takes_effort(id, &reg) {
                    let labels: Vec<String> =
                        EFFORT_LABELS.iter().map(|(l, _)| l.to_string()).collect();
                    let cur = self.cfg.model_prefs.effort(id).unwrap_or("");
                    let picked = EFFORT_LABELS
                        .iter()
                        .find(|(_, v)| *v == cur)
                        .map_or(labels[0].clone(), |(l, _)| l.to_string());
                    if let Some(i) = crate::cards::settings_dropdown(
                        ui,
                        &format!("{id} · effort"),
                        EFFORT_HINT,
                        &picked,
                        &labels,
                    ) {
                        let next = EFFORT_LABELS.get(i).map_or("", |(_, v)| *v);
                        if next != cur {
                            self.cfg.model_prefs.set_effort(id, next);
                            changed = true;
                        }
                    }
                }
                if *new_provider
                    && crate::cards::settings_action(
                        ui,
                        &format!("Forget {id}"),
                        "Back to \"only when you pick it\".",
                        "Remove",
                    )
                {
                    self.cfg.model_prefs.remove(id);
                    changed = true;
                }
            });
        }
        let choices: Vec<String> = self
            .provider_model_choices()
            .into_iter()
            .filter(|m| self.cfg.model_prefs.use_of(m) == ModelUse::Default)
            .collect();
        if !choices.is_empty() {
            let mut labels = vec![ADD_NONE.to_string()];
            labels.extend(choices.iter().map(|m| super::provider_ui::pick_label(m)));
            if let Some(i) =
                crate::cards::settings_dropdown(ui, ADD_ROW, ADD_HINT, ADD_NONE, &labels)
            {
                if let Some(m) = i.checked_sub(1).and_then(|i| choices.get(i)) {
                    self.cfg.model_prefs.set_use(m, ModelUse::Any);
                    changed = true;
                }
            }
        }
        if changed {
            self.persist_cfg();
            self.sync_provider_pick();
            self.status = "Saved".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_list_the_catalog_then_local_tiers_then_provider_preferences() {
        let mut prefs = ModelPrefs::default();
        prefs.set_use("openrouter/zeta/big", ModelUse::Chat);
        prefs.set_use("openrouter/alpha/small", ModelUse::Any);
        prefs.set_effort("grok-4.7", "high");
        let reg = Registry::default();
        let rows = pref_rows(&prefs, &reg);
        let catalog: Vec<String> = super::super::settings::cabin_default_models()
            .into_iter()
            .filter(|(id, _)| !id.is_empty())
            .map(|(id, _)| id.to_string())
            .collect();
        assert!(!catalog.is_empty());
        let (head, tail) = rows.split_at(catalog.len());
        assert_eq!(
            head.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
            catalog
        );
        assert!(head.iter().all(|(_, p)| !p));
        // No local tier in an empty registry; provider preferences alphabetical, marked.
        assert_eq!(
            tail,
            &[
                ("openrouter/alpha/small".to_string(), true),
                ("openrouter/zeta/big".to_string(), true),
            ]
        );
        assert!(pref_title("grok-4.7").ends_with(" · grok-4.7"));
        assert_eq!(pref_title("local:small"), "On-device model · local:small");
        assert_eq!(
            pref_title("openrouter/alpha/small"),
            "openrouter/alpha/small"
        );
        assert!(takes_effort("grok-4.7", &reg));
        assert!(!takes_effort("openrouter/alpha/small", &reg));
        assert_eq!(EFFORT_LABELS[4], ("Very high", "xhigh"));
    }

    #[test]
    fn the_section_paints_every_catalog_model_with_its_default_choice() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("model-prefs-rows");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        app.cfg
            .model_prefs
            .set_use("openrouter/vendor/model", ModelUse::Chat);
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let out = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
            app.ui_model_pref_rows(ui);
        });
        let texts: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect();
        for want in [
            SECTION,
            "Default (Auto decides)",
            "openrouter/vendor/model",
            "Chats",
            "Remove",
            "Auto picks the effort",
        ] {
            assert!(
                texts.iter().any(|t| t == want),
                "missing {want:?} in {texts:?}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
