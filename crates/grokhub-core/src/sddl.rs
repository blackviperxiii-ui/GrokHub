//! SDDL DACL string helpers (no Windows API). Used by `win_acl` and cabin tests.

/// True when an SDDL DACL grants read/write to Everyone, Users, Authenticated Users,
/// Guests, or Anonymous. Owner / SYSTEM SIDs are fine (`OW` expands to `S-1-5-21-…`).
pub fn sddl_allows_world(sddl: &str) -> bool {
    sddl.split('(')
        .skip(1)
        .map(|ace| ace.split(')').next().unwrap_or(""))
        .filter(|ace| ace.starts_with('A') || ace.starts_with("OA"))
        .any(|ace| {
            let trustee = ace.rsplit(';').next().unwrap_or("");
            matches!(trustee, "WD" | "BU" | "AU" | "BG" | "AN")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_trustees_are_flagged_owner_and_system_are_not() {
        assert!(sddl_allows_world("D:P(A;;FR;;;WD)(A;;FA;;;OW)"));
        assert!(sddl_allows_world("D:(A;;FA;;;BU)"));
        assert!(sddl_allows_world("D:(A;;GR;;;AU)"));
        assert!(sddl_allows_world("D:(OA;;FA;;;BG)"));
        assert!(!sddl_allows_world("D:P(A;;FA;;;SY)(A;;FA;;;OW)"));
        assert!(!sddl_allows_world(
            "D:P(A;;FA;;;SY)(A;;FA;;;S-1-5-21-1-2-3-1001)"
        ));
        assert!(
            !sddl_allows_world("D:(D;;FA;;;WD)"),
            "a Deny ACE is not an Allow"
        );
    }
}
