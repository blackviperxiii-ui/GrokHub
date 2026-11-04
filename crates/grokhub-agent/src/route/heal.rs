//! Router R2a messages (§14.5): what the user hears when a model breaks,
//! disappears, is retired, or joins the plan. Pure: the registry events and
//! the registry after them go in, plain Home updates come out, each with an
//! incident key so it posts once ([`HealNotes`]). Quiet hours and the card
//! budget are applied by the cabin when it posts; nothing here sends an OS
//! notification. No message names a model: the router alone picks them, and
//! only a chat question about it is told which one answers.
//!
//! - A model degrades, rests, is retired, a ghost or redirected: nothing. The
//!   router moves on to another one by itself; a model that is healthy again
//!   ends its incidents ([`Heard::cleared`]).
//! - A plan change that removes models: one Home update.
//! - New models in your plan: one tell-only Home update once the table ranks them.
//! - No healthy route at all: the cabin's waiting card ([`super::wait`]).

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

/// One tell-only Home update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealMsg {
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
pub(super) fn candidates<'a>(
    reg: &'a Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    now_ms: u64,
) -> Vec<&'a str> {
    reg.models
        .keys()
        .map(String::as_str)
        .filter(|id| {
            policy::fits(
                reg,
                profiles,
                id,
                Fit::default(),
                &super::spend::Spend {
                    settings: super::spend::spend_settings(),
                    ..Default::default()
                },
                now_ms,
            )
        })
        .collect()
}

/// Who stands in for `from`: its redirect successor, then its family, newest first.
pub(super) fn stand_in(reg: &Registry, from: &str, ids: &[&str]) -> Option<String> {
    if let Some(s) = reg
        .get(from)
        .and_then(|r| r.served_as.clone())
        .filter(|s| ids.contains(&s.as_str()))
    {
        return Some(s);
    }
    policy::fallback_chain(from, ids).into_iter().next()
}

/// The plan-change update. `covered`: something left in the plan can take over.
pub fn plan_change_msg(notice: &str, covered: bool) -> HealMsg {
    let text = if covered {
        "Your plan no longer includes some of the AI models it had. Auto keeps going on the ones it still has."
    } else {
        "Your plan no longer includes some of the AI models it had, and none left in it can answer right now."
    };
    HealMsg {
        key: format!("tier:{notice}"),
        title: "Your plan changed".to_string(),
        text: text.to_string(),
    }
}

/// The messages a batch of registry events asks for.
pub fn heal_messages(
    events: &[RegistryEvent],
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    now_ms: u64,
) -> Heard {
    let mut heard = Heard::default();
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
        if *to == ModelState::Live
            && matches!(
                from,
                ModelState::Degraded | ModelState::Quarantined | ModelState::Probing
            )
        {
            heard.cleared.push(format!("unhealthy:{id}"));
            heard.cleared.push(format!("noroute:{id}"));
        }
    }
    let notice = events.iter().find_map(|e| match e {
        RegistryEvent::Notice { text } => Some(text.as_str()),
        _ => None,
    });
    let lost = events
        .iter()
        .any(|e| matches!(e, RegistryEvent::Removed { .. }));
    if let Some(notice) = notice.filter(|_| lost) {
        let covered = !candidates(reg, profiles, now_ms).is_empty();
        heard.msgs.push(plan_change_msg(notice, covered));
    }
    heard
}

/// The tell-only Home update for models the routing table ranked for the
/// first time: one update for the batch, naming none of them.
pub fn joined_messages(joined: &[String]) -> Vec<HealMsg> {
    if joined.is_empty() {
        return Vec::new();
    }
    let mut ids = joined.to_vec();
    ids.sort();
    ids.dedup();
    let text = if ids.len() == 1 {
        "Your plan has a new AI model. Auto uses it where it does best.".to_string()
    } else {
        format!(
            "Your plan has {} new AI models. Auto uses them where they do best.",
            ids.len()
        )
    };
    vec![HealMsg {
        key: format!("new:{}", ids.join(",")),
        title: "New in your plan".to_string(),
        text,
    }]
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
            let oldest = self
                .shown
                .iter()
                .min_by_key(|(_, t)| **t)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => self.shown.remove(&k),
                None => break,
            };
        }
        out
    }
}
