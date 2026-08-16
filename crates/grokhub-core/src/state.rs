use crate::frame::{store_frame, PresenceFrame};
use crate::inhabit::InhabitBundle;
use crate::pair::{make_pair_code, normalize_code, PAIR_TTL_MS};
use crate::task::{HubTask, Receipt};
use crate::{new_token, now_ms, uid};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub const HUB_KIND: &str = "grokhub-hub-v1";
pub const DEFAULT_PORT: u16 = 18766;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairCode {
    pub code: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub token: String,
    #[serde(default)]
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubState {
    pub device_id: String,
    pub device_name: String,
    pub sharing: bool,
    pub port: u16,
    pub pair: Option<PairCode>,
    pub peers: Vec<Peer>,
    pub inbox: Vec<HubTask>,
    pub snapshot: Option<Value>,
    pub last_incoming_at: u64,
    pub inhabit: Option<InhabitBundle>,
    #[serde(skip)]
    pub last_frame: Option<PresenceFrame>,
    /// Console API key for duplex Voice minting. Never written to hub-state.json.
    #[serde(skip)]
    pub console_api_key: String,
    /// Cabin injects xAI `POST /realtime/client_secrets`. Tests stub this.
    #[serde(skip)]
    pub mint_realtime: Option<MintRealtimeFn>,
}

/// Mint an ephemeral realtime client secret with a console API key.
#[derive(Clone)]
pub struct MintRealtimeFn(pub Arc<dyn Fn(&str) -> Result<Value, String> + Send + Sync>);

impl std::fmt::Debug for MintRealtimeFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MintRealtimeFn")
    }
}

impl HubState {
    pub fn empty() -> Self {
        Self {
            device_id: uid("d"),
            device_name: hostname(),
            sharing: false,
            port: DEFAULT_PORT,
            pair: None,
            peers: vec![],
            inbox: vec![],
            snapshot: None,
            last_incoming_at: 0,
            inhabit: None,
            last_frame: None,
            console_api_key: String::new(),
            mint_realtime: None,
        }
    }

    pub fn rotate_pair(&mut self) -> PairCode {
        let p = PairCode {
            code: make_pair_code(),
            expires_at: now_ms() + PAIR_TTL_MS,
        };
        self.pair = Some(p.clone());
        p
    }

    pub fn pair_with(&mut self, code: &str, device_id: &str, device_name: &str) -> Result<Peer, PairError> {
        let want = self
            .pair
            .as_ref()
            .filter(|p| p.expires_at >= now_ms())
            .map(|p| normalize_code(&p.code))
            .unwrap_or_default();
        if want.is_empty() {
            return Err(PairError::NoCode);
        }
        if normalize_code(code) != want {
            return Err(PairError::Mismatch);
        }
        let id = if device_id.trim().is_empty() {
            uid("d")
        } else {
            device_id.trim().to_string()
        };
        let name: String = {
            let n = device_name.trim();
            let n = if n.is_empty() { "Computer" } else { n };
            n.chars().take(48).collect()
        };
        let token = new_token();
        if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
            p.name = name;
            p.token = token;
            p.last_seen = now_ms();
            let out = p.clone();
            self.pair = None;
            return Ok(out);
        }
        let peer = Peer {
            id,
            name,
            token,
            last_seen: now_ms(),
        };
        self.peers.push(peer.clone());
        self.pair = None;
        Ok(peer)
    }

    pub fn peer_for_token(&self, token: &str) -> Option<&Peer> {
        if token.is_empty() {
            return None;
        }
        self.peers.iter().find(|p| p.token == token)
    }

    pub fn peer_for_token_mut(&mut self, token: &str) -> Option<&mut Peer> {
        if token.is_empty() {
            return None;
        }
        self.peers.iter_mut().find(|p| p.token == token)
    }

    pub fn enqueue_task(&mut self, from: &Peer, target: &str, title: &str, prompt: &str) -> Result<HubTask, String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Task prompt is empty.".into());
        }
        let target = if target.trim().is_empty() {
            self.device_id.as_str()
        } else {
            target.trim()
        };
        let task = HubTask::enqueue(&from.id, &from.name, target, title, prompt, now_ms());
        self.inbox.insert(0, task.clone());
        self.inbox.truncate(80);
        Ok(task)
    }

    pub fn get_task(&self, id: &str, peer_id: &str) -> Option<&HubTask> {
        self.inbox.iter().find(|t| {
            t.id == id && (t.from_id == peer_id || t.target_device_id == peer_id)
        })
    }

    pub fn complete_task(
        &mut self,
        peer_id: &str,
        id: &str,
        result: &str,
        receipts: Vec<Receipt>,
        status: Option<&str>,
    ) -> Result<HubTask, CompleteError> {
        if !self.inbox.iter().any(|t| t.id == id) {
            return Err(CompleteError::NotFound);
        }
        let t = self
            .inbox
            .iter_mut()
            .find(|t| t.id == id && t.target_device_id == peer_id)
            .ok_or(CompleteError::Forbidden)?;
        t.complete(result, receipts, status);
        Ok(t.clone())
    }

    pub fn take_next_queued(&mut self, peer_id: &str) -> Option<HubTask> {
        let t = self
            .inbox
            .iter_mut()
            .find(|t| t.status == "queued" && t.target_device_id == peer_id)?;
        t.status = "claimed".into();
        Some(t.clone())
    }

    pub fn claim_inbox(&mut self, peer_id: &str) -> Vec<HubTask> {
        let mut out = vec![];
        for t in &mut self.inbox {
            if t.status == "queued" && t.target_device_id == peer_id {
                t.status = "claimed".into();
                out.push(t.clone());
            }
        }
        out
    }

    pub fn queued_for(&self, peer_id: &str) -> Vec<HubTask> {
        self.inbox
            .iter()
            .filter(|t| t.status == "queued" && t.target_device_id == peer_id)
            .cloned()
            .collect()
    }

    pub fn ack_inbox(&mut self, id: &str, peer_id: &str) -> Result<(), CompleteError> {
        if !self.inbox.iter().any(|t| t.id == id) {
            return Err(CompleteError::NotFound);
        }
        let t = self
            .inbox
            .iter_mut()
            .find(|t| t.id == id && t.target_device_id == peer_id)
            .ok_or(CompleteError::Forbidden)?;
        if t.status == "done" || t.status == "failed" {
            return Ok(());
        }
        t.status = "acked".into();
        Ok(())
    }

    pub fn enqueue_local(&mut self, title: &str, prompt: &str) -> Result<HubTask, String> {
        let from = Peer {
            id: self.device_id.clone(),
            name: self.device_name.clone(),
            token: String::new(),
            last_seen: now_ms(),
        };
        let target = self.device_id.clone();
        self.enqueue_task(&from, &target, title, prompt)
    }

    pub fn claim_results(&mut self, peer_id: &str) -> Vec<HubTask> {
        let mut out = vec![];
        for t in &mut self.inbox {
            if t.from_id == peer_id
                && (t.status == "done" || t.status == "failed")
                && !t.result_claimed
            {
                t.result_claimed = true;
                out.push(t.clone());
            }
        }
        out
    }

    pub fn store_inhabit(&mut self, mut bundle: InhabitBundle, from: &Peer) {
        bundle.from_id = Some(from.id.clone());
        bundle.from_name = Some(from.name.clone());
        bundle.at = Some(now_ms());
        self.inhabit = Some(bundle);
    }

    pub fn claim_inhabit(&mut self) -> Option<InhabitBundle> {
        self.inhabit.take()
    }

    pub fn store_frame(&mut self, data_url: &str) {
        if let Some(f) = store_frame(data_url, now_ms()) {
            self.last_frame = Some(f);
        }
    }

    pub fn put_snapshot(&mut self, snap: Value) -> Result<(), String> {
        let kind = snap.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if kind != HUB_KIND {
            return Err("Not a GrokHub hub snapshot.".into());
        }
        self.snapshot = Some(match (&self.snapshot, crate::hub_sync::is_hub_snapshot(&snap)) {
            (Some(local_v), true) => {
                if let (Ok(local), Ok(remote)) = (
                    serde_json::from_value::<crate::hub_sync::HubSnapshot>(local_v.clone()),
                    serde_json::from_value::<crate::hub_sync::HubSnapshot>(snap.clone()),
                ) {
                    serde_json::to_value(crate::hub_sync::merge_hub_snapshots(&local, &remote))
                        .unwrap_or(snap)
                } else {
                    snap
                }
            }
            _ => snap,
        });
        self.last_incoming_at = now_ms();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairError {
    NoCode,
    Mismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompleteError {
    NotFound,
    Forbidden,
}

pub fn state_for_disk(st: &HubState) -> HubState {
    let mut out = st.clone();
    out.last_frame = None;
    out.console_api_key.clear();
    out.mint_realtime = None;
    out
}

pub fn save_hub_state(path: &std::path::Path, st: &HubState) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let disk = state_for_disk(st);
    let s = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
    let tmp = path.with_file_name(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("hub-state.json")
    ));
    std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn load_hub_state(path: &std::path::Path) -> Option<HubState> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut st: HubState = serde_json::from_str(&raw).ok()?;
    st.last_frame = None;
    Some(st)
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "This computer".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_then_task() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        let peer = st.pair_with(&code, "phone", "Pixel").unwrap();
        assert!(!peer.token.is_empty());
        assert!(st.pair.is_none());
        let task = st
            .enqueue_task(&peer, &st.device_id.clone(), "Flash", "flash the pi")
            .unwrap();
        assert_eq!(st.get_task(&task.id, &peer.id).unwrap().prompt, "flash the pi");
        st.complete_task(&st.device_id.clone(), &task.id, "blocked", vec![], Some("failed"))
            .expect("hub target may complete");
        let results = st.claim_results(&peer.id);
        assert_eq!(results[0].status, "failed");
        assert!(st.claim_results(&peer.id).is_empty());
    }

    #[test]
    fn foreign_peer_cannot_complete_hub_task() {
        let mut st = HubState::empty();
        let phone_code = st.rotate_pair().code;
        let phone = st.pair_with(&phone_code, "phone", "Pixel").unwrap();
        let other_code = st.rotate_pair().code;
        let other = st.pair_with(&other_code, "other", "Laptop").unwrap();
        let hub_id = st.device_id.clone();
        let task = st
            .enqueue_task(&phone, &hub_id, "Flash", "flash the pi")
            .unwrap();
        assert_eq!(
            st.complete_task(&other.id, &task.id, "nope", vec![], Some("done"))
                .unwrap_err(),
            CompleteError::Forbidden
        );
        assert_eq!(st.get_task(&task.id, &phone.id).unwrap().status, "queued");
        let done = st
            .complete_task(&hub_id, &task.id, "flashed", vec![], Some("done"))
            .expect("target completes");
        assert_eq!(done.status, "done");
        assert_eq!(
            st.complete_task(&hub_id, "missing-id", "x", vec![], None)
                .unwrap_err(),
            CompleteError::NotFound
        );
    }

    #[test]
    fn persist_omits_frame() {
        let dir = std::env::temp_dir().join(format!("grokhub-hub-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hub-state.json");
        let mut st = HubState::empty();
        st.last_frame = Some(crate::PresenceFrame {
            data_url: "data:image/jpeg;base64,SECRETFRAME".into(),
            at: 9,
        });
        st.console_api_key = "xai-should-not-persist".into();
        save_hub_state(&path, &st).expect("save");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("SECRETFRAME"));
        assert!(!raw.contains("xai-should-not-persist"));
        let loaded = load_hub_state(&path).expect("load");
        assert_eq!(loaded.device_id, st.device_id);
        assert!(loaded.last_frame.is_none());
        assert!(loaded.console_api_key.is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "hub-state.json holds pair tokens");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_and_wrong_pair_codes() {
        let mut st = HubState::empty();
        st.pair = Some(PairCode {
            code: "ABC-234".into(),
            expires_at: 1,
        });
        assert_eq!(
            st.pair_with("ABC-234", "phone", "Pixel").unwrap_err(),
            PairError::NoCode
        );
        st.pair = Some(PairCode {
            code: "ABC-234".into(),
            expires_at: now_ms() + 60_000,
        });
        assert_eq!(
            st.pair_with("ZZZ-999", "phone", "Pixel").unwrap_err(),
            PairError::Mismatch
        );
        assert!(st.pair.is_some(), "a mismatch must leave the code live");
    }

    #[test]
    fn task_is_hidden_from_other_peers() {
        let mut st = HubState::empty();
        let phone_code = st.rotate_pair().code;
        let phone = st.pair_with(&phone_code, "phone", "Pixel").unwrap();
        let other_code = st.rotate_pair().code;
        let other = st.pair_with(&other_code, "other", "Laptop").unwrap();
        let hub_id = st.device_id.clone();
        assert!(st.enqueue_task(&phone, &hub_id, "Flash", "   ").is_err());
        let task = st
            .enqueue_task(&phone, &hub_id, "Flash", "flash the pi")
            .unwrap();
        assert!(st.get_task(&task.id, &phone.id).is_some());
        assert!(
            st.get_task(&task.id, &other.id).is_none(),
            "another paired box must not read this task"
        );
        assert_eq!(st.queued_for(&other.id).len(), 0);
        assert_eq!(
            st.ack_inbox("missing", &phone.id).unwrap_err(),
            CompleteError::NotFound
        );
        assert_eq!(
            st.ack_inbox(&task.id, &other.id).unwrap_err(),
            CompleteError::Forbidden
        );
        st.ack_inbox(&task.id, &hub_id).expect("target acks");
        assert_eq!(st.get_task(&task.id, &phone.id).unwrap().status, "acked");
        let done = st
            .enqueue_task(&phone, &hub_id, "Done", "finish me")
            .unwrap();
        st.complete_task(&hub_id, &done.id, "ok", vec![], Some("done"))
            .unwrap();
        st.ack_inbox(&done.id, &hub_id).expect("ack after complete");
        assert_eq!(
            st.get_task(&done.id, &phone.id).unwrap().status,
            "done",
            "ack must not hide a completed result from GET /v1/results"
        );
        for i in 0..90 {
            st.enqueue_local("local", &format!("do {i}")).unwrap();
        }
        assert!(st.inbox.len() <= 80);
    }

    #[test]
    fn task_title_and_prompt_are_capped() {
        let t = HubTask::enqueue("a", "b", "c", "", &"y".repeat(20_000), 1);
        assert_eq!(t.title, "Remote task");
        assert_eq!(t.prompt.chars().count(), 16_000);
        let titled = HubTask::enqueue("a", "b", "c", &"x".repeat(200), "ok", 1);
        assert_eq!(titled.title.chars().count(), 120);
        let mut done = titled.clone();
        done.complete("ok", vec![], None);
        assert_eq!(done.status, "done");
        let mut failed = titled;
        failed.complete("nope", vec![], Some("failed"));
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.result.as_deref(), Some("nope"));
    }
}
