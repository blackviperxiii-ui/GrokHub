//! The router's settings for a host that shares the GrokHub config folder but
//! not the cabin's Settings screens (the Tauri shell). The cabin's `app.json`
//! holds Local AI's switch, the typo tidy, OpenRouter's data policy and the
//! spend limits; [`sync`] reads them there and hands them to the router, as
//! the cabin does from its own Settings. Providers you added live in
//! `providers.json`, which the router reads itself.
//!
//! Nothing here picks a model or an effort. A chat engine starts at
//! [`chat_start`], and the router picks every step's provider, model and
//! effort in [`super::live::stream_routed`].

use std::path::Path;

use serde_json::Value;

use super::providers::{self, Routing};
use super::spend::{self, SpendSettings};
use grokhub_core::model_registry::cost_class::DEFAULT_CEILING_USD_PER_M;

/// The cabin's settings file in the config folder.
pub const APP_JSON: &str = "app.json";

/// A bigger `app.json` is not read; the defaults apply.
const APP_JSON_CAP: u64 = 4 * 1024 * 1024;

/// What the router takes from `app.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct HostRouting {
    /// Local AI may take turns (`localModel`). Only an explicit `false` turns
    /// it off: with no cabin settings, setting up Local AI is the switch. It
    /// routes nothing until a local model is installed.
    pub local_model: bool,
    /// The on-device typo tidy (`promptCleanup`), only while Local AI is on.
    pub prompt_cleanup: bool,
    /// OpenRouter's data policy (`providerRouting`).
    pub routing: Routing,
    /// The weekly cap and the $/M price ceiling (`weeklySpendCapUsd`,
    /// `priceCeilingUsdPerM`).
    pub spend: SpendSettings,
}

impl Default for HostRouting {
    fn default() -> Self {
        Self {
            local_model: true,
            prompt_cleanup: false,
            routing: Routing::default(),
            spend: SpendSettings::default(),
        }
    }
}

/// A non-negative dollar amount, or `None`.
fn dollars(raw: &Value, key: &str) -> Option<f64> {
    raw.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// The router's settings in `config_dir`'s `app.json`. A missing, oversized or
/// unreadable file, or a value this build can't read, gives that default.
pub fn read(config_dir: &Path) -> HostRouting {
    let path = config_dir.join(APP_JSON);
    let raw = std::fs::metadata(&path)
        .ok()
        .filter(|m| m.len() <= APP_JSON_CAP)
        .and_then(|_| std::fs::read_to_string(&path).ok())
        .and_then(|text| {
            // A file saved with a byte-order mark still reads.
            serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok()
        })
        .unwrap_or(Value::Null);
    let flag = |key: &str| raw.get(key).and_then(Value::as_bool);
    let local_model = flag("localModel").unwrap_or(true);
    HostRouting {
        local_model,
        prompt_cleanup: local_model && flag("promptCleanup").unwrap_or(false),
        routing: raw
            .get("providerRouting")
            .and_then(|v| serde_json::from_value::<Routing>(v.clone()).ok())
            .unwrap_or_default(),
        spend: SpendSettings {
            weekly_cap_usd: dollars(&raw, "weeklySpendCapUsd").unwrap_or(0.0),
            ceiling_usd_per_m: dollars(&raw, "priceCeilingUsdPerM")
                .unwrap_or(DEFAULT_CEILING_USD_PER_M),
        },
    }
}

/// Hands `s` to the router. A changed spend limit forgets the cached week.
pub fn apply(s: &HostRouting) {
    super::local::set_enabled(s.local_model);
    super::local::set_prompt_cleanup(s.local_model && s.prompt_cleanup);
    providers::set_routing(s.routing.clone());
    if spend::spend_settings() != s.spend {
        spend::set_spend(s.spend);
        super::budget::forget_week();
    }
}

/// [`read`] then [`apply`]: call it before each chat turn, so a change in the
/// cabin's Settings applies to the next message.
pub fn sync(config_dir: &Path) -> HostRouting {
    let s = read(config_dir);
    apply(&s);
    s
}

/// The model and effort a chat engine starts at. The router replaces both on
/// every step of a routed class; they only stand for an unlisted one.
pub fn chat_start() -> (String, Option<String>) {
    (
        crate::DEFAULT_MODEL.to_string(),
        super::live::start_effort(super::live::DEFAULT_CLASS),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-route-host-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    fn with_app_json(name: &str, raw: &str) -> HostRouting {
        let dir = temp_dir(name);
        std::fs::write(dir.join(APP_JSON), raw).expect("write");
        let read = read(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        read
    }

    #[test]
    fn no_cabin_settings_turn_local_ai_on_and_keep_the_default_limits() {
        let dir = temp_dir("missing");
        assert_eq!(
            read(&dir),
            HostRouting {
                local_model: true,
                prompt_cleanup: false,
                routing: Routing {
                    data: String::new()
                },
                spend: SpendSettings {
                    weekly_cap_usd: 0.0,
                    ceiling_usd_per_m: DEFAULT_CEILING_USD_PER_M,
                },
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(with_app_json("junk", "{not json").local_model);
        assert!(with_app_json("other", r#"{"closeToTray":false}"#).local_model);
    }

    #[test]
    fn the_cabin_settings_reach_the_router() {
        let read = with_app_json(
            "set",
            "\u{feff}{\"localModel\":true,\"promptCleanup\":true,\
             \"providerRouting\":{\"data\":\"zdr\",\"sort\":\"price\"},\
             \"weeklySpendCapUsd\":25,\"priceCeilingUsdPerM\":12}",
        );
        assert_eq!(
            read,
            HostRouting {
                local_model: true,
                prompt_cleanup: true,
                routing: Routing { data: "zdr".into() },
                spend: SpendSettings {
                    weekly_cap_usd: 25.0,
                    ceiling_usd_per_m: 12.0,
                },
            }
        );
    }

    #[test]
    fn local_ai_switched_off_in_the_cabin_stays_off_and_takes_the_tidy_with_it() {
        let read = with_app_json("off", r#"{"localModel":false,"promptCleanup":true}"#);
        assert_eq!((read.local_model, read.prompt_cleanup), (false, false));
    }

    #[test]
    fn values_this_build_cannot_read_load_as_their_defaults() {
        let read = with_app_json(
            "bad",
            r#"{"localModel":"yes","providerRouting":"zdr","weeklySpendCapUsd":-5,"priceCeilingUsdPerM":"cheap"}"#,
        );
        assert_eq!(
            read,
            HostRouting {
                local_model: true,
                prompt_cleanup: false,
                routing: Routing {
                    data: String::new()
                },
                spend: SpendSettings {
                    weekly_cap_usd: 0.0,
                    ceiling_usd_per_m: DEFAULT_CEILING_USD_PER_M,
                },
            }
        );
    }

    #[test]
    fn a_chat_starts_at_the_routers_base_model_and_the_default_class_effort() {
        assert_eq!(
            chat_start(),
            ("grok-4.7".to_string(), Some("medium".to_string()))
        );
    }
}
