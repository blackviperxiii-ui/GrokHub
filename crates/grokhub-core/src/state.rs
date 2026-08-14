use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::frame::{store_frame, PresenceFrame};
use crate::inhabit::InhabitBundle;
use crate::pair::{make_pair_code, normalize_code, PAIR_TTL_MS};
use crate::task::{HubTask, Receipt};
use crate::{new_token, now_ms, uid};

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
        id: &str,
        result: &str,
        receipts: Vec<Receipt>,
        status: Option<&str>,
    ) -> Option<HubTask> {
        let t = self.inbox.iter_mut().find(|t| t.id == id)?;
        t.complete(result, receipts, status);
        Some(t.clone())
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

    pub fn ack_inbox(&mut self, id: &str, peer_id: &str) {
        if let Some(t) = self.inbox.iter_mut().find(|t| t.id == id && t.target_device_id == peer_id) {
            t.status = "acked".into();
        }
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

pub fn state_for_disk(st: &HubState) -> HubState {
    let mut out = st.clone();
    out.last_frame = None;
    out
}

pub fn save_hub_state(path: &std::path::Path, st: &HubState) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let disk = state_for_disk(st);
    let s = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())
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
        st.complete_task(&task.id, "blocked", vec![], Some("failed"));
        let results = st.claim_results(&peer.id);
        assert_eq!(results[0].status, "failed");
        assert!(st.claim_results(&peer.id).is_empty());
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
        save_hub_state(&path, &st).expect("save");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("SECRETFRAME"));
        let loaded = load_hub_state(&path).expect("load");
        assert_eq!(loaded.device_id, st.device_id);
        assert!(loaded.last_frame.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
