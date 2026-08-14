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
