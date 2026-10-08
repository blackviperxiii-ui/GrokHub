//! Who pays for a call to a model: inside the plan, on API credits, or unknown.
//! The onboarding probe only runs on `included` routes.

use serde::{Deserialize, Serialize};

use super::entitlement::Credential;
use super::record::{ModelRecord, SourceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    /// Inside the signed-in plan's pool.
    Included,
    /// Billed per token (API credits).
    Metered,
    Unknown,
}

impl CostClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Included => "included",
            Self::Metered => "metered",
            Self::Unknown => "unknown",
        }
    }
}

/// The cost class of a direct (native engine) call to `rec` with `cred`.
/// A plan sign-in is included only for a model the plan's own list has.
pub fn cost_class(cred: Credential, rec: Option<&ModelRecord>) -> CostClass {
    let Some(rec) = rec else {
        return CostClass::Unknown;
    };
    match cred {
        Credential::ApiKey => CostClass::Metered,
        Credential::Plan if rec.state.routable() && rec.sources.contains(&SourceKind::XaiApi) => CostClass::Included,
        _ => CostClass::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_registry::record::{ModelMeta, ModelState};

    #[test]
    fn plan_listed_is_included_key_is_metered_else_unknown() {
        let mut r = ModelRecord { meta: ModelMeta::bare("grok-4.7"), sources: vec![SourceKind::XaiApi], ..ModelRecord::default() };
        assert_eq!(cost_class(Credential::Plan, Some(&r)), CostClass::Included);
        assert_eq!(cost_class(Credential::ApiKey, Some(&r)), CostClass::Metered);
        assert_eq!(cost_class(Credential::None, Some(&r)), CostClass::Unknown);
        assert_eq!(cost_class(Credential::Plan, None), CostClass::Unknown);
        r.state = ModelState::NotInPlan;
        assert_eq!(cost_class(Credential::Plan, Some(&r)), CostClass::Unknown);
        r.state = ModelState::Live;
        r.sources = vec![SourceKind::GrokBuild];
        assert_eq!(cost_class(Credential::Plan, Some(&r)), CostClass::Unknown);
    }
}
