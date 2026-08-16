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
    pub at: Option<u64>,
}

pub fn can_inhabit(paired: bool, source_locked: bool, dest_idle: bool) -> bool {
    paired && source_locked && dest_idle
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
            at: Some(1),
        })
        .unwrap();
        let back: InhabitBundle = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.soul, "voice");
        assert_eq!(back.skill_ids, vec!["flash-pi".to_string()]);
        assert!(can_inhabit(true, true, true));
        assert!(!can_inhabit(true, false, true));
    }
}
