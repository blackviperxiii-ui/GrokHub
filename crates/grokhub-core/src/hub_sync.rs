//! Hub snapshot build / merge. No secrets. Last-write-wins per record.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::HUB_KIND;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HubMemoryFile {
    pub name: String,
    pub content: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSnapshot {
    pub kind: String,
    pub from_device_id: String,
    pub from_device_name: String,
    pub exported_at: u64,
    pub threads: Vec<Value>,
    pub workboard: Value,
    pub skills: Vec<Value>,
    pub automations: Vec<Value>,
    pub memory_files: Vec<HubMemoryFile>,
}

pub fn is_hub_snapshot(v: &Value) -> bool {
    v.get("kind").and_then(|k| k.as_str()) == Some(HUB_KIND)
}

pub fn build_hub_snapshot(
    device_id: &str,
    device_name: &str,
    exported_at: u64,
    threads: Vec<Value>,
    workboard: Value,
    skills: Vec<Value>,
    automations: Vec<Value>,
    memory_files: Vec<HubMemoryFile>,
) -> HubSnapshot {
    let threads = threads
        .into_iter()
        .take(40)
        .map(|mut t| {
            if let Some(msgs) = t.get_mut("messages").and_then(|m| m.as_array_mut()) {
                let n = msgs.len();
                if n > 80 {
                    *msgs = msgs.split_off(n - 80);
                }
            }
            t
        })
        .collect();
    HubSnapshot {
        kind: HUB_KIND.into(),
        from_device_id: device_id.into(),
        from_device_name: device_name.into(),
        exported_at,
        threads,
        workboard,
        skills: skills.into_iter().take(80).collect(),
        automations: automations.into_iter().take(80).collect(),
        memory_files: memory_files
            .into_iter()
            .map(|f| HubMemoryFile {
                name: f.name.chars().take(80).collect(),
                content: f.content.chars().take(200_000).collect(),
                updated_at: f.updated_at,
            })
            .collect(),
    }
}

pub fn merge_hub_snapshots(local: &HubSnapshot, remote: &HubSnapshot) -> HubSnapshot {
    let mut files = std::collections::BTreeMap::new();
    for f in local.memory_files.iter().chain(remote.memory_files.iter()) {
        if f.name.is_empty() {
            continue;
        }
        let prev = files.get(&f.name);
        if prev.map(|p: &HubMemoryFile| f.updated_at >= p.updated_at).unwrap_or(true) {
            files.insert(f.name.clone(), f.clone());
        }
    }
    HubSnapshot {
        kind: HUB_KIND.into(),
        from_device_id: local.from_device_id.clone(),
        from_device_name: local.from_device_name.clone(),
        exported_at: local.exported_at.max(remote.exported_at),
        threads: merge_by_id(&local.threads, &remote.threads),
        workboard: merge_workboard(&local.workboard, &remote.workboard),
        skills: merge_by_id(&local.skills, &remote.skills),
        automations: merge_by_id(&local.automations, &remote.automations),
        memory_files: files.into_values().collect(),
    }
}

fn merge_workboard(local: &Value, remote: &Value) -> Value {
    let local_items = local.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let remote_items = remote.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    json!({ "items": merge_by_id(&local_items, &remote_items) })
}

fn merge_by_id(local: &[Value], remote: &[Value]) -> Vec<Value> {
    let mut map = std::collections::BTreeMap::new();
    for row in local.iter().chain(remote.iter()) {
        let Some(id) = row.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let t = row
            .get("updatedAt")
            .or_else(|| row.get("createdAt"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        match map.get(id) {
            Some((_, prev_t)) if *prev_t > t => {}
            _ => {
                map.insert(id.to_string(), (row.clone(), t));
            }
        }
    }
    map.into_values().map(|(v, _)| v).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_merge_last_write() {
        let local = build_hub_snapshot(
            "d1",
            "cabin",
            10,
            vec![json!({"id":"t1","updatedAt":1,"messages":[{"role":"user"}]})],
            json!({"items":[{"id":"w1","updatedAt":1}]}),
            vec![],
            vec![],
            vec![HubMemoryFile {
                name: "MEMORY.md".into(),
                content: "old".into(),
                updated_at: 1,
            }],
        );
        let remote = build_hub_snapshot(
            "d2",
            "pi",
            20,
            vec![json!({"id":"t1","updatedAt":9,"messages":[{"role":"assistant"}]})],
            json!({"items":[{"id":"w1","updatedAt":9}]}),
            vec![],
            vec![],
            vec![HubMemoryFile {
                name: "MEMORY.md".into(),
                content: "new".into(),
                updated_at: 9,
            }],
        );
        let m = merge_hub_snapshots(&local, &remote);
        assert_eq!(m.kind, HUB_KIND);
        assert_eq!(m.threads[0]["updatedAt"], 9);
        assert_eq!(m.memory_files[0].content, "new");
        assert!(is_hub_snapshot(&json!({"kind": HUB_KIND})));
    }
}
