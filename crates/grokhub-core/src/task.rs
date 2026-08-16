use serde::{Deserialize, Serialize};

use crate::uid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubTask {
    pub id: String,
    pub from_id: String,
    pub from_name: String,
    pub target_device_id: String,
    pub title: String,
    pub prompt: String,
    pub status: String,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receipts: Vec<Receipt>,
    #[serde(default)]
    pub result_claimed: bool,
}

impl HubTask {
    pub fn enqueue(
        from_id: &str,
        from_name: &str,
        target_device_id: &str,
        title: &str,
        prompt: &str,
        created_at: u64,
    ) -> Self {
        let title = {
            let t = title.trim();
            let t = if t.is_empty() { "Remote task" } else { t };
            t.chars().take(120).collect()
        };
        let prompt: String = prompt.chars().take(16_000).collect();
        Self {
            id: uid("task"),
            from_id: from_id.to_string(),
            from_name: from_name.to_string(),
            target_device_id: target_device_id.to_string(),
            title,
            prompt,
            status: "queued".into(),
            created_at,
            result: None,
            receipts: vec![],
            result_claimed: false,
        }
    }

    pub fn complete(&mut self, result: &str, receipts: Vec<Receipt>, status: Option<&str>) {
        self.status = if status
            .map(str::trim)
            .is_some_and(|s| s.eq_ignore_ascii_case("failed"))
        {
            "failed".into()
        } else {
            "done".into()
        };
        self.result = Some(result.chars().take(16_000).collect());
        self.receipts = receipts.into_iter().take(40).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_caps_and_complete_marks_done() {
        let prompt = "p".repeat(20_000);
        let mut t = HubTask::enqueue("from", "box", "dest", "", &prompt, 9);
        assert_eq!(t.title, "Remote task");
        assert_eq!(t.status, "queued");
        assert_eq!(t.prompt.len(), 16_000);
        t.complete("ok", vec![Receipt { cmd: "ls".into(), risk: None, code: Some(0), ms: Some(1) }], None);
        assert_eq!(t.status, "done");
        assert_eq!(t.result.as_deref(), Some("ok"));
        assert_eq!(t.receipts.len(), 1);
        t.complete("no", vec![], Some("failed"));
        assert_eq!(t.status, "failed");
    }

    #[test]
    fn complete_failed_is_case_insensitive() {
        let mut t = HubTask::enqueue("a", "b", "c", "Flash", "x", 0);
        t.complete("nope", vec![], Some("FAILED"));
        assert_eq!(t.status, "failed");
        t.complete("nope", vec![], Some(" failed\n"));
        assert_eq!(t.status, "failed");
        t.complete("ok", vec![], Some("done"));
        assert_eq!(t.status, "done");
    }
}
