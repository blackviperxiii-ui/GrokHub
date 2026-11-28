//! Settings → Account → Priority service tier. Off until switched on.
//!
//! The switch is `priorityServiceTier` in `app.json`. When on, chat
//! `/v1/responses` calls send `service_tier: "priority"`. Default omits the
//! field. This is not the router's premium cost class (that path would
//! change billing).

use std::path::Path;

use serde::Serialize;
use serde_json::json;

pub const KEY: &str = grokhub_agent::PRIORITY_SERVICE_TIER_KEY;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub on: bool,
}

pub fn is_on(config_dir: &Path) -> bool {
    grokhub_core::app_json::bool_or(config_dir, KEY, false)
}

pub fn view(config_dir: &Path) -> View {
    View {
        on: is_on(config_dir),
    }
}

pub fn set(config_dir: &Path, on: bool) -> Result<View, String> {
    grokhub_core::app_json::patch(config_dir, &[(KEY, json!(on))])?;
    Ok(view(config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grokhub-priority-tier-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn off_by_default_and_the_key_is_literal() {
        let d = dir("off");
        assert_eq!(KEY, "priorityServiceTier");
        assert!(!is_on(&d));
        assert_eq!(view(&d), View { on: false });
        assert!(set(&d, true).unwrap().on);
        assert!(is_on(&d));
        let raw = std::fs::read_to_string(d.join("app.json")).unwrap();
        assert!(raw.contains("\"priorityServiceTier\": true"), "{raw}");
        assert!(!set(&d, false).unwrap().on);
        let _ = std::fs::remove_dir_all(&d);
    }
}
