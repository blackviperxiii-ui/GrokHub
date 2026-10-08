//! What this credential may use: the plan tier and how the cabin signed in.
//! Absent from your own fresh list is `not_in_plan`, never a ghost strike.

use serde::{Deserialize, Serialize};

/// How the native engine is signed in (mirrors grokhub-agent's `AuthKind`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Credential {
    /// Not signed in: only the Grok Build plan list applies.
    #[default]
    None,
    /// Sign in with Grok (the SuperGrok pool).
    Plan,
    /// A console API key (API credits).
    ApiKey,
}

impl Credential {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Plan => "plan",
            Self::ApiKey => "api_key",
        }
    }
}

/// The plan as last seen. `tier` is `None` when no source reports one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entitlement {
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub credential: Credential,
}

/// The one plain notice a tier change gets, or `None` when nothing changed or
/// either side is unknown (an unknown tier is not a change).
pub fn tier_notice(old: Option<&str>, new: Option<&str>) -> Option<String> {
    match (old, new) {
        (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b) => Some(format!("Your Grok plan changed from {a} to {b}.")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_known_to_known_change_is_a_notice() {
        assert_eq!(tier_notice(Some("SuperGrok Heavy"), Some("SuperGrok")).as_deref(), Some("Your Grok plan changed from SuperGrok Heavy to SuperGrok."));
        assert_eq!(tier_notice(Some("SuperGrok"), Some("supergrok")), None);
        assert_eq!(tier_notice(None, Some("SuperGrok")), None);
        assert_eq!(tier_notice(Some("SuperGrok"), None), None);
    }
}
