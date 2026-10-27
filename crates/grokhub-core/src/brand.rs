//! The product's name, tagline, companion name and About link label, read from
//! `brand/brand.json` at the repo root. The cabin, the shell and the web UI
//! (`ui/src/brand.ts`) all read that one file, so a rename is one line there.

use serde::Deserialize;
use std::sync::LazyLock;

/// Folder, keychain and app-id name. Never follows the brand, so a rename
/// can't move anyone's settings, sign-ins or installed copy.
pub const INTERNAL_ID: &str = "GrokHub";

const BRAND_JSON: &str = include_str!("../../../brand/brand.json");

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Brand {
    pub product_name: String,
    pub tagline: String,
    pub companion_name: String,
    pub support_label: String,
}

fn parse(raw: &str) -> Option<Brand> {
    let brand: Brand = serde_json::from_str(raw).ok()?;
    let filled = [
        &brand.product_name,
        &brand.tagline,
        &brand.companion_name,
        &brand.support_label,
    ]
    .iter()
    .all(|s| !s.trim().is_empty());
    filled.then_some(brand)
}

static BRAND: LazyLock<Brand> = LazyLock::new(|| {
    // A broken brand.json fails `brand_json_parses` in CI; this only keeps a
    // hand-edited build from showing blanks.
    parse(BRAND_JSON).unwrap_or_else(|| Brand {
        product_name: INTERNAL_ID.to_string(),
        tagline: String::new(),
        companion_name: String::new(),
        support_label: "Support the developer".to_string(),
    })
});

/// The name people see: window titles, tray, About, welcome, notifications.
pub fn product_name() -> &'static str {
    &BRAND.product_name
}

pub fn tagline() -> &'static str {
    &BRAND.tagline
}

/// The companion's name, shown wherever the companion speaks or is shown.
pub fn companion_name() -> &'static str {
    &BRAND.companion_name
}

/// The quiet link in Settings → About, shown only once a support URL is set.
pub fn support_label() -> &'static str {
    &BRAND.support_label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_json_parses() {
        assert_eq!(
            parse(BRAND_JSON),
            Some(Brand {
                product_name: "Loadout".into(),
                tagline: "Gear up. Level up.".into(),
                companion_name: "Nyx".into(),
                support_label: "Support the developer".into(),
            })
        );
        assert_eq!(product_name(), "Loadout");
        assert_eq!(tagline(), "Gear up. Level up.");
        assert_eq!(companion_name(), "Nyx");
        assert_eq!(support_label(), "Support the developer");
    }

    #[test]
    fn rename_is_one_field() {
        let renamed = BRAND_JSON.replace("\"Loadout\"", "\"Namehere\"");
        assert_eq!(
            parse(&renamed).map(|b| b.product_name),
            Some("Namehere".into())
        );
    }

    #[test]
    fn blank_or_broken_brand_is_refused() {
        assert_eq!(parse("{}"), None);
        assert_eq!(parse("not json"), None);
        assert_eq!(
            parse(r#"{"productName":" ","tagline":"t","companionName":"c","supportLabel":"s"}"#),
            None
        );
        assert_eq!(
            parse(r#"{"productName":"p","tagline":"t","companionName":"c","supportLabel":""}"#),
            None
        );
    }

    #[test]
    fn internal_id_stays_put() {
        assert_eq!(INTERNAL_ID, "GrokHub");
    }
}
