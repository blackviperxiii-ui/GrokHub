//! Router R2a messages (§14.5): what the user hears when a model breaks,
//! disappears, is retired, or joins the plan. Pure: the registry events and
//! the registry after them go in, plain messages come out, each with a tier
//! and an incident key so it posts once ([`HealNotes`]). Quiet hours and the
//! card budget are applied by the cabin when it posts; nothing here sends an
//! OS notification.
//!
//! - Effort changes: nothing (chip hover and `/why` carry them).
//! - A model degrades or rests and its fallback works: nothing, unless it is
//!   your pin. Then one Work-tree row, once per incident.
//! - A pin that is retired, a ghost or redirected: the fallback, plus one
//!   Home update. The pin itself stays saved (`app.json` is the Access
//!   settings file, which the ledger never writes) and shows "(retired)".
//! - A plan change that removes models: one Home update.
//! - A new model in your plan: one tell-only Home update once the table ranks it.
//! - No healthy route: a pause, with a needs-attention line and a card.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::store::{models_dir, read_json, write_json};
use grokhub_core::model_registry::{ModelState, Registry, RegistryEvent};

use super::policy::{self, Fit};

pub const HEAL_NOTES_FILE: &str = "heal_notes.json";
/// Incident keys remembered.
pub const HEAL_NOTES_CAP: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// One quiet row in the Work tree.
    WorkRow,
    /// One tell-only Home update.
    HomeUpdate,
    /// The step paused: a needs-attention line plus a card with options.
    Pause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealMsg {
    pub tier: Tier,
    /// Posts once per key.
    pub key: String,
    pub title: String,
    pub text: String,
}

/// What a batch of events says, and which incidents ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Heard {
    pub msgs: Vec<HealMsg>,
    /// Incident keys whose model is healthy again: they may post next time.
    pub cleared: Vec<String>,
}

/// The models that could take an everyday step now.
fn candidates<'a>(reg: &'a Registry, profiles: &BTreeMap<String, ModelProfile>, now_ms: u64) -> Vec<&'a str> {
    reg.models.keys().map(String::as_str).filter(|id| policy::fits(reg, profiles, id, Fit::default(), &super::spend::Spend { settings: super::spend::spend_settings(), ..Default::default() }, now_ms)).collect()
}

/// Who stands in for `from`: its redirect successor, then its family, newest first.
fn stand_in(reg: &Registry, from: &str, ids: &[&str]) -> Option<String> {
    if let Some(s) = reg.get(from).and_then(|r| r.served_as.clone()).filter(|s| ids.contains(&s.as_str())) {
        return Some(s);
    }
    policy::fallback_chain(from, ids).into_iter().next()
}

/// The pause card's text: what failed, what was tried, the options.
pub fn pause_msg(model: &str, failed: &str, tried: &[String]) -> HealMsg {
    let tried = if tried.is_empty() { "No other model in your plan can take over.".to_string() } else { format!("I tried {}.", tried.join(", ")) };
    HealMsg {
        tier: Tier::Pause,
        key: format!("noroute:{model}"),
        title: "Paused: no model in your plan is answering".into(),
        text: format!("{model}: {failed} {tried} You can wait for it to come back, refresh the model list, or pick another model in Settings."),
    }
}

/// The messages a batch of registry events asks for. `pin` is the user's
/// pinned model (empty is Auto).
pub fn heal_messages(events: &[RegistryEvent], reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, pin: &str, now_ms: u64) -> Heard {
    let pin = pin.trim();
    let ids = candidates(reg, profiles, now_ms);
    let mut heard = Heard::default();
    let tier_notice = events.iter().find_map(|e| match e {
        RegistryEvent::Notice { text } => Some(text.clone()),
        _ => None,
    });
    // One batch can move a model twice (a first 404 rests it, the second makes
    // it a ghost): only where it ended up speaks.
    let mut last: BTreeMap<&str, (ModelState, ModelState)> = BTreeMap::new();
    for ev in events {
        if let RegistryEvent::State { id, from, to, .. } = ev {
            let first_from = last.get(id.as_str()).map(|(f, _)| *f).unwrap_or(*from);
            last.insert(id, (first_from, *to));
        }
    }
    for (id, (from, to)) in &last {
        let id = *id;
        if *to == ModelState::Live && matches!(from, ModelState::Degraded | ModelState::Quarantined | ModelState::Probing) {
            heard.cleared.push(format!("unhealthy:{id}"));
            heard.cleared.push(format!("noroute:{id}"));
        }
        if pin.is_empty() || id != pin {
            continue;
        }
        let fallback = stand_in(reg, id, &ids);
        let failed = reg.get(id).map(|r| r.reason.clone()).unwrap_or_default();
        match (to, &fallback) {
            (ModelState::Degraded | ModelState::Quarantined, Some(f)) => heard.msgs.push(HealMsg {
                tier: Tier::WorkRow,
                key: format!("unhealthy:{id}"),
                title: format!("{id} isn't answering"),
                text: format!("{id} isn't answering, so I'm using {f} for now. Your pick is saved."),
            }),
            (ModelState::Retired | ModelState::Pruned, Some(f)) => heard.msgs.push(HealMsg {
                tier: Tier::HomeUpdate,
                key: format!("retired:{id}"),
                title: format!("{id} was retired by xAI"),
                text: format!("{id} was retired by xAI, so I'm using {f} where you picked it. Your pick shows (retired) until you choose another in Settings."),
            }),
            (ModelState::Ghost, Some(f)) => heard.msgs.push(HealMsg {
                tier: Tier::HomeUpdate,
                key: format!("ghost:{id}"),
                title: format!("{id} is gone from xAI"),
                text: format!("{id} is still listed but no longer answers, so I'm using {f} where you picked it. Your pick is saved."),
            }),
            (ModelState::Redirected, Some(f)) => heard.msgs.push(HealMsg {
                tier: Tier::HomeUpdate,
                key: format!("redirected:{id}"),
                title: format!("{id} now answers as {f}"),
                text: format!("xAI points {id} at {f} now, so I'm using {f} where you picked it. Your pick is saved."),
            }),
            (ModelState::NotInPlan, Some(f)) if tier_notice.is_none() => heard.msgs.push(HealMsg {
                tier: Tier::HomeUpdate,
                key: format!("plan:{id}"),
                title: "Your plan changed".into(),
                text: format!("Your plan changed, so {id} isn't included anymore. I'll use {f}."),
            }),
            // A degraded pin with nothing healthy to stand in keeps answering, slowly.
            (ModelState::Live | ModelState::Probing | ModelState::NotInPlan | ModelState::Degraded, _) => {}
            // An alias with nothing listed behind it keeps answering under its own name (2.10.97).
            (ModelState::Redirected, None) => {}
            (_, None) => {
                let all: Vec<&str> = reg.models.keys().map(String::as_str).collect();
                heard.msgs.push(pause_msg(id, &failed, &policy::fallback_chain(id, &all)));
            }
        }
    }
    if let Some(notice) = tier_notice {
        let removed: Vec<&String> = events
            .iter()
            .filter_map(|e| match e {
                RegistryEvent::Removed { id } => Some(id),
                _ => None,
            })
            .collect();
        let lost = removed.iter().find(|id| id.as_str() == pin).or(removed.first());
        if let Some(lost) = lost {
            let instead = stand_in(reg, lost, &ids).or_else(|| ids.first().map(|s| s.to_string()));
            let text = match instead {
                Some(f) => format!("Your plan changed, so {lost} isn't included anymore. I'll use {f}."),
                None => format!("Your plan changed, so {lost} isn't included anymore."),
            };
            heard.msgs.push(HealMsg { tier: Tier::HomeUpdate, key: format!("tier:{notice}"), title: "Your plan changed".into(), text });
        }
    }
    heard
}

/// The tell-only Home update for models the routing table ranked for the first time.
pub fn joined_messages(joined: &[String]) -> Vec<HealMsg> {
    joined
        .iter()
        .map(|m| HealMsg {
            tier: Tier::HomeUpdate,
            key: format!("new:{m}"),
            title: format!("{m} is now in your plan"),
            text: format!("{m} is now in your plan. Auto will use it where it does best. /why has the details."),
        })
        .collect()
}

/// Incidents already told, so each posts once. `models/heal_notes.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealNotes {
    #[serde(default)]
    pub shown: BTreeMap<String, u64>,
}

pub fn heal_notes_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(HEAL_NOTES_FILE)
}

impl HealNotes {
    pub fn load(config_dir: &Path) -> Self {
        read_json(&heal_notes_path(config_dir)).unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) {
        let _ = write_json(&heal_notes_path(config_dir), self);
    }

    /// Keep the messages not told yet and remember them; ended incidents may tell again.
    pub fn admit(&mut self, heard: Heard, now_ms: u64) -> Vec<HealMsg> {
        for key in &heard.cleared {
            self.shown.remove(key);
        }
        let mut out = Vec::new();
        for m in heard.msgs {
            if self.shown.contains_key(&m.key) || out.iter().any(|o: &HealMsg| o.key == m.key) {
                continue;
            }
            self.shown.insert(m.key.clone(), now_ms);
            out.push(m);
        }
        while self.shown.len() > HEAL_NOTES_CAP {
            let oldest = self.shown.iter().min_by_key(|(_, t)| **t).map(|(k, _)| k.clone());
            match oldest {
                Some(k) => self.shown.remove(&k),
                None => break,
            };
        }
        out
    }
}

/// The tag the Settings model list shows beside a model, if any.
pub fn settings_tag(reg: &Registry, id: &str) -> Option<&'static str> {
    match reg.get(id)?.state {
        ModelState::NotInPlan => Some("Not in your plan"),
        ModelState::Degraded | ModelState::Quarantined | ModelState::Ghost => Some("Not answering"),
        _ => None,
    }
}

/// Retired and pruned models leave the Settings list, unless one is your pin.
pub fn settings_hidden(reg: &Registry, id: &str, pin: &str) -> bool {
    id != pin.trim() && reg.get(id).is_some_and(|r| r.state.tombstone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::model_registry::{CallStatus, Listing, ModelMeta, Observation, SourceKind};

    #[test]
    fn settings_tags_and_hides_by_state() {
        let mut reg = Registry::default();
        let mut old = ModelMeta::bare("grok-4.3");
        old.context_length = Some(1);
        old.deprecated = Some(true);
        let mut live = ModelMeta::bare("grok-4.7");
        live.context_length = Some(1);
        reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, vec![old, live])], &[], 1);
        assert_eq!(settings_tag(&reg, "grok-4.7"), None);
        assert_eq!(settings_tag(&reg, "grok-9"), None);
        assert!(settings_hidden(&reg, "grok-4.3", ""));
        assert!(!settings_hidden(&reg, "grok-4.3", "grok-4.3"), "your retired pin stays visible");
        assert!(!settings_hidden(&reg, "grok-4.7", ""));
        let fail = |ts_ms| Observation { model: "grok-4.7".into(), ts_ms, status: CallStatus::Error, latency_ms: 0, served_model: None, endpoint_ok: true, reasoning_tokens: 0, cost_ticks: 0, http: 500 };
        reg.fold(&[fail(10), fail(11), fail(12)]);
        assert_eq!(settings_tag(&reg, "grok-4.7"), Some("Not answering"));
        let mut fresh = ModelMeta::bare("grok-4.7");
        fresh.context_length = Some(1);
        reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, vec![fresh])], &[], 20);
        assert_eq!(reg.get("grok-4.3").map(|r| r.state), Some(ModelState::NotInPlan));
        assert_eq!(settings_tag(&reg, "grok-4.3"), Some("Not in your plan"));
    }
}
