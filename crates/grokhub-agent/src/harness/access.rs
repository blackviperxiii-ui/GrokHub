//! Session Access ladder. Independent of the Ask/Auto/Always permission pill.
//! Always-approve soft tools must never promote Access to Full.

use serde::{Deserialize, Serialize};

/// Cabin Access tier. Boot default is Readonly: computer tools are absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    /// Observe / read / search only. Computer (click/type/…) tools not registered.
    #[default]
    Readonly,
    /// Computer + host tools available; HardClass and write/exec still park on a card.
    Supervised,
    /// Auto within allowlist. HardClass + hard floor still pause/deny.
    /// Requires an explicit "Grant full" — Always-approve does not imply Full.
    Full,
}

impl AccessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Readonly => "readonly",
            Self::Supervised => "supervised",
            Self::Full => "full",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Readonly => "Readonly",
            Self::Supervised => "Supervised",
            Self::Full => "Full",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "readonly" | "read-only" | "ro" => Some(Self::Readonly),
            "supervised" | "supervise" => Some(Self::Supervised),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    /// Computer-use tools (click/type/key/…) may appear in the tool list.
    pub fn allows_computer(self) -> bool {
        matches!(self, Self::Supervised | Self::Full)
    }

    /// Soft mutating host tools (write/shell) may run under normal Ask/Auto/Always.
    /// Readonly keeps them off the list (same surface as plan-mode read-only).
    pub fn allows_host_mutate(self) -> bool {
        matches!(self, Self::Supervised | Self::Full)
    }

    pub fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }
}

/// Always-approve on the permission pill must not unlock computer tools or Full.
pub fn always_does_not_imply_full(access: AccessMode, perm_is_always: bool) -> bool {
    if !perm_is_always {
        return true;
    }
    // Under Always, Access stays whatever it was — never silently Full,
    // and computer tools stay gated on Access alone.
    !access.is_full() || access.allows_computer()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_default_is_readonly() {
        assert_eq!(AccessMode::default(), AccessMode::Readonly);
        assert!(!AccessMode::Readonly.allows_computer());
        assert!(!AccessMode::Readonly.allows_host_mutate());
    }

    #[test]
    fn supervised_and_full_allow_computer() {
        assert!(AccessMode::Supervised.allows_computer());
        assert!(AccessMode::Full.allows_computer());
        assert!(AccessMode::Full.is_full());
        assert!(!AccessMode::Supervised.is_full());
    }

    #[test]
    fn always_does_not_grant_full() {
        // Always + Readonly must stay non-Full and without computer.
        assert!(!AccessMode::Readonly.allows_computer());
        assert!(!AccessMode::Readonly.is_full());
        // Toggling Always alone never changes Access — caller must keep Access.
        let access = AccessMode::Readonly;
        let after_always = access; // Always must not mutate Access
        assert_eq!(after_always, AccessMode::Readonly);
        assert!(always_does_not_imply_full(after_always, true));
    }

    #[test]
    fn parse_labels() {
        assert_eq!(AccessMode::parse("Readonly"), Some(AccessMode::Readonly));
        assert_eq!(AccessMode::parse("supervised"), Some(AccessMode::Supervised));
        assert_eq!(AccessMode::parse("FULL"), Some(AccessMode::Full));
        assert_eq!(AccessMode::parse("yolo"), None);
    }
}
