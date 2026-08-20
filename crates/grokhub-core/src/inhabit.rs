use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InhabitBundle {
    pub soul: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    pub goal: Option<String>,
    pub project_snapshot_id: Option<String>,
    pub from_id: Option<String>,
    pub from_name: Option<String>,
    #[serde(default)]
    pub to_id: Option<String>,
    #[serde(default)]
    pub to_name: Option<String>,
    pub at: Option<u64>,
}

pub fn can_inhabit(paired: bool, source_locked: bool, dest_idle: bool) -> bool {
    paired && source_locked && dest_idle
}

/// Pairing code or "sharing on" is not enough — a real peer must be present,
/// and the destination must not already be running a job.
pub fn inhabit_ready(peer_count: usize, dest_running: bool) -> bool {
    can_inhabit(peer_count > 0, true, !dest_running)
}

pub fn inhabit_bundle_usable(b: &InhabitBundle) -> bool {
    !b.soul.trim().is_empty()
        || !b.skill_ids.is_empty()
        || b.goal.as_deref().is_some_and(|g| !g.trim().is_empty())
}

/// Phones may stage a handoff; they must not consume the dest bundle.
/// Match device tokens, not substrings — "saxophone" is a cabin, not a phone.
pub fn inhabit_claim_allowed(peer_name: &str) -> bool {
    let n = peer_name.to_ascii_lowercase();
    !name_has_device_token(&n, "phone")
        && !name_has_device_token(&n, "android")
        && !name_has_device_token(&n, "iphone")
}

fn name_has_device_token(name: &str, token: &str) -> bool {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part == token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_roundtrip_and_gate() {
        let raw = serde_json::to_string(&InhabitBundle {
            soul: "voice".into(),
            skill_ids: vec!["flash-pi".into()],
            goal: Some("ship".into()),
            project_snapshot_id: None,
            from_id: Some("a".into()),
            from_name: Some("cabin".into()),
            to_id: None,
            to_name: None,
            at: Some(1),
        })
        .unwrap();
        let back: InhabitBundle = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.soul, "voice");
        assert_eq!(back.skill_ids, vec!["flash-pi".to_string()]);
        assert!(can_inhabit(true, true, true));
        assert!(!can_inhabit(true, false, true));
    }

    #[test]
    fn inhabit_requires_peers_and_idle_dest() {
        assert!(!inhabit_ready(0, false), "sharing with no peers is not paired");
        assert!(!inhabit_ready(1, true), "a busy dest must not take inhabit");
        assert!(inhabit_ready(1, false));
        assert!(!inhabit_bundle_usable(&InhabitBundle::default()));
        assert!(inhabit_bundle_usable(&InhabitBundle {
            soul: "stay kind".into(),
            ..Default::default()
        }));
        assert!(!inhabit_claim_allowed("Pixel phone"));
        assert!(!inhabit_claim_allowed("Android"));
        assert!(!inhabit_claim_allowed("iPhone"));
        assert!(inhabit_claim_allowed("cabin-2"));
        assert!(
            inhabit_claim_allowed("saxophone"),
            "substring phone must not block a cabin named saxophone"
        );
        assert!(
            inhabit_claim_allowed("headphones"),
            "headphones is not the phone device"
        );
    }
}
