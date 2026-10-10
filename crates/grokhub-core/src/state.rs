use crate::frame::{store_frame, PresenceFrame};
use crate::inhabit::InhabitBundle;
use crate::pair::{ct_eq, make_pair_code, normalize_code, PAIR_TTL_MS};
use crate::task::{HubTask, Receipt};
use crate::{new_token, now_ms, uid};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::io::Read;
use std::sync::Arc;

const HUB_STATE_CAP: u64 = 8 * 1024 * 1024;

pub const HUB_KIND: &str = "grokhub-hub-v1";
pub const DEFAULT_PORT: u16 = 18766;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairCode {
    pub code: String,
    pub expires_at: u64,
    /// Wrong guesses so far. The code burns at `PAIR_MAX_TRIES` so a 30-bit code cannot be
    /// ground down by a client that is free to guess thousands of times a second.
    #[serde(default)]
    pub tries: u32,
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
    #[serde(default, serialize_with = "ser_arc_value", deserialize_with = "de_arc_value")]
    pub snapshot: Option<Arc<Value>>,
    pub last_incoming_at: u64,
    pub inhabit: Option<InhabitBundle>,
    #[serde(skip)]
    pub last_frame: Option<Arc<PresenceFrame>>,
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
            tries: 0,
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
        if !ct_eq(normalize_code(code).as_bytes(), want.as_bytes()) {
            // Burn the code once guessing looks like grinding. Leaving it live let a
            // caller try the whole keyspace inside the 15 minute TTL.
            if let Some(p) = self.pair.as_mut() {
                p.tries = p.tries.saturating_add(1);
                if p.tries >= crate::pair::PAIR_MAX_TRIES {
                    self.pair = None;
                }
            }
            return Err(PairError::Mismatch);
        }
        let id = if device_id.trim().is_empty() {
            uid("d")
        } else {
            device_id.trim().to_string()
        };
        // Every authorization check downstream keys off these ids, and the hub's own id is
        // handed out by `/v1/pair` and `/v1/status`. A peer allowed to claim it could read
        // tasks addressed to the hub and forge their completion.
        if id == self.device_id {
            return Err(PairError::ReservedId);
        }
        // Phone pairing is gone (#533). Checked live too, not only when hub-state.json
        // loads: a phone paired since the last restart could claim inhabit bundles.
        if is_phone_name(device_name) {
            return Err(PairError::Phone);
        }
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

    /// Phone pairing is gone (#533), so a phone peer from an older
    /// `hub-state.json` loses its token: it could still read and write the
    /// snapshot, read the frame and claim inhabit bundles. Its queued tasks
    /// go too; nobody is left to read their results. Same name test the old
    /// inhabit guard used.
    pub fn drop_phone_peers(&mut self) {
        let phones: Vec<String> = self.peers.iter().filter(|p| is_phone_name(&p.name)).map(|p| p.id.clone()).collect();
        if phones.is_empty() {
            return;
        }
        self.peers.retain(|p| !phones.contains(&p.id));
        self.inbox.retain(|t| t.status != "queued" || !phones.contains(&t.from_id));
    }

    pub fn peer_for_token(&self, token: &str) -> Option<&Peer> {
        if token.is_empty() {
            return None;
        }
        // Same constant-time compare as the pairing code: no early exit on a matching prefix.
        self.peers
            .iter()
            .find(|p| ct_eq(p.token.as_bytes(), token.as_bytes()))
    }

    pub fn peer_for_token_mut(&mut self, token: &str) -> Option<&mut Peer> {
        if token.is_empty() {
            return None;
        }
        self.peers
            .iter_mut()
            .find(|p| ct_eq(p.token.as_bytes(), token.as_bytes()))
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

    /// After a crash, claimed rows have no live worker. Put them back in line.
    pub fn requeue_claimed_for(&mut self, peer_id: &str) -> u32 {
        let mut n = 0;
        for t in &mut self.inbox {
            if t.status == "claimed" && t.target_device_id == peer_id {
                t.status = "queued".into();
                n += 1;
            }
        }
        n
    }

    /// `/send` queues work for this computer. `drain_inbox` claims it when chat is free.
    pub fn enqueue_local(&mut self, title: &str, prompt: &str) -> Result<HubTask, String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Task prompt is empty.".into());
        }
        let id = self.device_id.clone();
        let task = HubTask::enqueue(&id, &self.device_name, &id, title, prompt, now_ms());
        self.inbox.insert(0, task.clone());
        self.inbox.truncate(80);
        Ok(task)
    }

    pub fn store_inhabit(&mut self, mut bundle: InhabitBundle, from: &Peer) {
        bundle.from_id = Some(from.id.clone());
        bundle.from_name = Some(from.name.clone());
        bundle.at = Some(now_ms());
        self.inhabit = Some(bundle);
    }

    /// The staged bundle, for the peer it names by id only. A name is chosen by the
    /// peer itself, so a second peer paired under the target's name must not match,
    /// and a bundle with no destination goes to nobody.
    pub fn claim_inhabit(&mut self, peer: &Peer) -> Option<InhabitBundle> {
        if is_phone_name(&peer.name) {
            return None;
        }
        let hit = self.inhabit.as_ref()?;
        if hit.to_id.as_deref().is_some_and(|id| !id.is_empty() && id == peer.id) {
            self.inhabit.take()
        } else {
            None
        }
    }

    pub fn store_frame(&mut self, data_url: &str) {
        if let Some(f) = store_frame(data_url, now_ms()) {
            self.last_frame = Some(Arc::new(f));
        }
    }

    /// JPEG already parsed off the hub lock — just the Arc bump.
    pub fn install_frame(&mut self, frame: PresenceFrame) {
        self.last_frame = Some(Arc::new(frame));
    }

    pub fn put_snapshot(&mut self, snap: Value) -> Result<(), String> {
        let merged = merge_put_snapshot(self.snapshot.as_deref(), snap)?;
        self.snapshot = Some(Arc::new(merged));
        self.last_incoming_at = now_ms();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairError {
    NoCode,
    Mismatch,
    /// The request asked to be paired under an id the hub reserves for itself.
    ReservedId,
    /// A phone, Android or iPhone name: phone pairing is gone (#533).
    Phone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompleteError {
    NotFound,
    Forbidden,
}

/// Drop `pending_hub_task` only when the inbox row is gone or completed.
pub fn clear_pending_after_complete(err: Option<CompleteError>) -> bool {
    match err {
        None => true,
        Some(CompleteError::NotFound) => true,
        Some(CompleteError::Forbidden) => false,
    }
}

/// Merge a peer PUT without holding hub.lock() across from_value of 8MB JSON.
pub fn merge_put_snapshot(local: Option<&Value>, snap: Value) -> Result<Value, String> {
    let kind = snap.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if kind != HUB_KIND {
        return Err("Not a GrokHub hub snapshot.".into());
    }
    Ok(match (local, crate::hub_sync::is_hub_snapshot(&snap)) {
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
    })
}

/// serde has no `rc` feature here — wrap Value in Arc without changing hub-state.json.
fn ser_arc_value<S: Serializer>(v: &Option<Arc<Value>>, s: S) -> Result<S::Ok, S::Error> {
    v.as_deref().serialize(s)
}

fn de_arc_value<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Arc<Value>>, D::Error> {
    Option::<Value>::deserialize(d).map(|o| o.map(Arc::new))
}

pub fn state_for_disk(st: &HubState) -> HubState {
    HubState {
        device_id: st.device_id.clone(),
        device_name: st.device_name.clone(),
        sharing: st.sharing,
        port: st.port,
        pair: st.pair.clone(),
        peers: st.peers.clone(),
        inbox: st.inbox.clone(),
        snapshot: st.snapshot.clone(),
        last_incoming_at: st.last_incoming_at,
        inhabit: st.inhabit.clone(),
        last_frame: None,
    }
}

pub fn save_hub_state(path: &std::path::Path, st: &HubState) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let disk = state_for_disk(st);
    let s = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
    // The cabin and the standalone `grokhub-hub` daemon both persist this path. A temp
    // name derived only from the destination means they interleave writes into one file
    // and rename a torn result into place, so keep the pid in the name.
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("hub-state.json"),
        std::process::id()
    ));
    // Every peer's bearer token is in `s`, so the temp must be private before the bytes
    // land, not after the rename.
    write_private_synced(&tmp, s.as_bytes()).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
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

fn write_private_synced(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let _ = std::fs::remove_file(path);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    // Without the fsync a power loss can leave a zero-length file that the rename already
    // published, which reads back as "no peers" and unpairs every device.
    f.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_hub_state(path: &std::path::Path) -> Option<HubState> {
    let file = std::fs::File::open(path).ok()?;
    let mut raw = String::new();
    let read = file
        .take(HUB_STATE_CAP.saturating_add(1))
        .read_to_string(&mut raw)
        .ok()
        .filter(|n| (*n as u64) <= HUB_STATE_CAP);
    // Callers fall back to an empty state, which the next persist tick writes back over
    // this file. Move a state we cannot parse aside so the pairings stay recoverable.
    let Some(_) = read else {
        quarantine_hub_state(path);
        return None;
    };
    match serde_json::from_str::<HubState>(&raw) {
        Ok(mut st) => {
            st.last_frame = None;
            st.drop_phone_peers();
            Some(st)
        }
        Err(_) if raw.trim().is_empty() => None,
        Err(_) => {
            quarantine_hub_state(path);
            None
        }
    }
}

/// A peer name that says phone, Android or iPhone as a whole word.
pub fn is_phone_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.split(|c: char| !c.is_ascii_alphanumeric()).any(|part| matches!(part, "phone" | "android" | "iphone"))
}

fn quarantine_hub_state(path: &std::path::Path) {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let stamp = crate::now_ms();
    let _ = std::fs::rename(path, path.with_file_name(format!("{name}.corrupt-{stamp}")));
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
    fn pair_then_local_task() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        let peer = st.pair_with(&code, "d-laptop", "Laptop").unwrap();
        assert!(!peer.token.is_empty());
        assert!(st.pair.is_none());
        let hub_id = st.device_id.clone();
        let task = st.enqueue_local("Flash", "flash the pi").unwrap();
        assert_eq!(task.from_id, hub_id);
        assert_eq!(task.target_device_id, hub_id);
        assert_eq!(task.prompt, "flash the pi");
        assert_eq!(
            st.complete_task(&peer.id, &task.id, "nope", vec![], Some("done"))
                .unwrap_err(),
            CompleteError::Forbidden,
            "a paired computer must not complete this computer's task"
        );
        assert_eq!(st.inbox[0].status, "queued");
        let done = st
            .complete_task(&hub_id, &task.id, "blocked", vec![], Some("failed"))
            .expect("this computer completes its own task");
        assert_eq!(done.status, "failed");
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
        st.last_frame = Some(Arc::new(crate::PresenceFrame {
            data_url: "data:image/jpeg;base64,SECRETFRAME".into(),
            at: 9,
        }));
        save_hub_state(&path, &st).expect("save");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("SECRETFRAME"));
        let loaded = load_hub_state(&path).expect("load");
        assert_eq!(loaded.device_id, st.device_id);
        assert!(loaded.last_frame.is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "hub-state.json holds pair tokens");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_for_disk_skips_the_live_frame_clone() {
        let src = include_str!("state.rs");
        let disk = src
            .split("pub fn state_for_disk(")
            .nth(1)
            .and_then(|s| s.split("pub fn save_hub_state(").next())
            .expect("state_for_disk");
        assert!(
            !disk.contains("st.clone()") && disk.contains("last_frame: None"),
            "persist must not clone a 400KB cabin frame then throw it away: {disk}"
        );
    }

    #[test]
    fn hub_snapshot_and_frame_are_arc_so_get_clone_is_a_refcount() {
        let src = include_str!("state.rs");
        let hub = src
            .split("pub struct HubState")
            .nth(1)
            .and_then(|s| s.split("impl HubState").next())
            .expect("HubState");
        assert!(
            hub.contains("snapshot: Option<Arc<Value>>") && hub.contains("ser_arc_value"),
            "GET /v1/snapshot must clone an Arc, not 8MB of JSON under hub.lock(): {hub}"
        );
        assert!(
            hub.contains("last_frame: Option<Arc<PresenceFrame>>"),
            "GET /v1/frame must clone an Arc, not a 400KB JPEG under hub.lock(): {hub}"
        );
        let put = src
            .split("pub fn put_snapshot")
            .nth(1)
            .and_then(|s| s.split("pub fn state_for_disk").next())
            .expect("put_snapshot");
        assert!(
            put.contains("Arc::new") && put.contains("merge_put_snapshot"),
            "put_snapshot must store Arc so persist/GET do not deep-clone under the lock: {put}"
        );
        let merge = src
            .split("pub fn merge_put_snapshot")
            .nth(1)
            .and_then(|s| s.split("fn ser_arc_value").next())
            .expect("merge_put_snapshot");
        assert!(
            merge.contains("from_value") && merge.contains("merge_hub_snapshots"),
            "PUT merge must LWW peer threads off the hub lock: {merge}"
        );
        let install = src
            .split("pub fn install_frame")
            .nth(1)
            .and_then(|s| s.split("pub fn put_snapshot").next())
            .expect("install_frame");
        assert!(
            install.contains("Arc::new") && !install.contains("store_frame("),
            "install_frame must not re-decode the JPEG under hub.lock(): {install}"
        );
    }

    #[test]
    fn inhabit_claim_matches_named_peer() {
        let mut st = HubState::empty();
        let a_code = st.rotate_pair().code;
        let a = st.pair_with(&a_code, "a", "cabin-a").unwrap();
        let b_code = st.rotate_pair().code;
        let b = st.pair_with(&b_code, "b", "cabin-b").unwrap();
        st.store_inhabit(
            InhabitBundle {
                soul: "stay".into(),
                to_id: Some(b.id.clone()),
                to_name: Some(b.name.clone()),
                ..Default::default()
            },
            &a,
        );
        assert!(
            st.claim_inhabit(&a).is_none(),
            "the source cabin must not consume a bundle aimed at someone else"
        );
        assert!(st.inhabit.is_some());
        let got = st.claim_inhabit(&b).expect("dest");
        assert_eq!(got.to_id.as_deref(), Some(b.id.as_str()));
        assert!(st.inhabit.is_none());
    }

    #[test]
    fn expired_and_wrong_pair_codes() {
        let mut st = HubState::empty();
        st.pair = Some(PairCode {
            code: "ABC-234".into(),
            expires_at: 1,
            tries: 0,
        });
        assert_eq!(
            st.pair_with("ABC-234", "d-laptop", "Laptop").unwrap_err(),
            PairError::NoCode
        );
        st.pair = Some(PairCode {
            code: "ABC-234".into(),
            expires_at: now_ms() + 60_000,
            tries: 0,
        });
        assert_eq!(
            st.pair_with("ZZZ-999", "d-laptop", "Laptop").unwrap_err(),
            PairError::Mismatch
        );
        assert!(st.pair.is_some(), "a mismatch must leave the code live");
    }

    #[test]
    fn pair_code_burns_after_repeated_wrong_guesses() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        // Six characters over a 32 symbol alphabet is ~30 bits. Left live for the full
        // 15 minute TTL a caller can guess thousands of times a second, so the code has
        // to stop accepting attempts long before the keyspace is in reach.
        for i in 1..crate::pair::PAIR_MAX_TRIES {
            assert_eq!(
                st.pair_with("ZZZ-999", "attacker", "Laptop").unwrap_err(),
                PairError::Mismatch
            );
            assert!(st.pair.is_some(), "still live after {i} wrong guesses");
        }
        assert_eq!(
            st.pair_with("ZZZ-999", "attacker", "Laptop").unwrap_err(),
            PairError::Mismatch
        );
        assert!(st.pair.is_none(), "the code must burn at PAIR_MAX_TRIES");
        assert_eq!(
            st.pair_with(&code, "d-laptop", "Laptop").unwrap_err(),
            PairError::NoCode,
            "even the real code is dead once it burned; the host rotates a new one"
        );

        // A correct guess never counts against the budget.
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        for _ in 0..(crate::pair::PAIR_MAX_TRIES * 3) {
            let code = st.rotate_pair().code;
            assert!(st.pair_with(&code, "d-laptop", "Laptop").is_ok());
        }
        let _ = code;
    }

    #[test]
    fn peer_cannot_pair_as_the_hub() {
        let mut st = HubState::empty();
        let hub_id = st.device_id.clone();
        let code = st.rotate_pair().code;
        // `/v1/pair` and `/v1/status` both hand out the hub id, and every authorization
        // check downstream keys off these ids. A peer that could claim the hub's id would
        // pose as the hub to every other paired computer.
        assert_eq!(
            st.pair_with(&code, &hub_id, "Impostor").unwrap_err(),
            PairError::ReservedId
        );
        assert!(st.peers.is_empty(), "the impostor must not be registered");
        assert!(st.pair.is_some(), "a reserved id is not a wrong code");
        assert!(
            st.pair_with(&code, " ", "Laptop").is_ok(),
            "an ordinary device still pairs"
        );

        // Re-pairing the same real device is still allowed to rotate its token.
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        let first = st.pair_with(&code, "d-laptop", "Laptop").unwrap();
        let code = st.rotate_pair().code;
        let again = st.pair_with(&code, "d-laptop", "Laptop").unwrap();
        assert_eq!(st.peers.len(), 1, "re-pairing must not duplicate the peer");
        assert_ne!(first.token, again.token, "re-pairing rotates the token");
    }

    #[test]
    fn pair_code_compare_is_constant_time() {
        use crate::pair::ct_eq;
        assert!(ct_eq(b"ABC234", b"ABC234"));
        assert!(!ct_eq(b"ABC234", b"ABC235"));
        assert!(!ct_eq(b"ABC234", b"ZZZ999"));
        assert!(!ct_eq(b"ABC234", b"ABC2345"), "length must not match");
        assert!(!ct_eq(b"", b"ABC234"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn local_task_queue_claims_and_requeues() {
        let mut st = HubState::empty();
        let hub_id = st.device_id.clone();
        assert!(st.enqueue_local("Flash", "   ").is_err());
        let done = st.enqueue_local("Done", "finish me").unwrap();
        st.complete_task(&hub_id, &done.id, "ok", vec![], Some("done"))
            .unwrap();
        assert_eq!(st.inbox[0].status, "done");
        assert!(clear_pending_after_complete(None));
        assert!(clear_pending_after_complete(Some(CompleteError::NotFound)));
        assert!(
            !clear_pending_after_complete(Some(CompleteError::Forbidden)),
            "a forbidden complete must keep pending so the claimed row can still finish"
        );
        for i in 0..90 {
            st.enqueue_local("local", &format!("do {i}")).unwrap();
        }
        assert_eq!(st.inbox.len(), 80);
        let mut stuck = HubState::empty();
        let hub = stuck.device_id.clone();
        let mut row = HubTask::enqueue(&hub, "cabin", &hub, "Flash", "flash the pi", 1);
        row.status = "claimed".into();
        stuck.inbox.push(row);
        assert_eq!(stuck.requeue_claimed_for(&hub), 1);
        assert_eq!(stuck.inbox[0].status, "queued");
        assert_eq!(stuck.take_next_queued(&hub).unwrap().status, "claimed");
        assert!(stuck.take_next_queued(&hub).is_none());
    }

    /// A hub-state.json written while phone pairing existed still loads: the phone's
    /// peer row, a phone task with `resultClaimed` and receipts, and stray keys.
    #[test]
    fn old_hub_state_with_phone_rows_still_loads() {
        let dir = std::env::temp_dir().join(format!("grokhub-old-hub-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hub-state.json");
        let old = r#"{
  "deviceId": "d-hub",
  "deviceName": "cabin",
  "sharing": true,
  "port": 18766,
  "pair": null,
  "peers": [
    { "id": "d-phone", "name": "Pixel phone", "token": "tok-phone", "lastSeen": 5 },
    { "id": "d-laptop", "name": "Jeremy's laptop", "token": "tok-laptop", "lastSeen": 6 }
  ],
  "inbox": [
    {
      "id": "task-1",
      "fromId": "d-phone",
      "fromName": "Pixel phone",
      "targetDeviceId": "d-hub",
      "title": "Flash",
      "prompt": "flash the pi",
      "status": "done",
      "createdAt": 1,
      "result": "flashed",
      "receipts": [{ "cmd": "dd if=pi.img", "code": 0 }],
      "resultClaimed": true
    },
    {
      "id": "task-2",
      "fromId": "d-phone",
      "fromName": "Pixel phone",
      "targetDeviceId": "d-hub",
      "title": "Reboot",
      "prompt": "reboot the pi",
      "status": "queued",
      "createdAt": 2
    }
  ],
  "snapshot": null,
  "lastIncomingAt": 0,
  "inhabit": null,
  "consoleApiKey": "",
  "voiceClientSecret": null
}"#;
        std::fs::write(&path, old).unwrap();
        let st = load_hub_state(&path).expect("old hub-state.json must load");
        assert_eq!(st.device_id, "d-hub");
        // The phone row and its queued task go; its token no longer opens anything.
        assert_eq!(st.peers.len(), 1);
        assert_eq!(st.peers[0].name, "Jeremy's laptop");
        assert!(st.peer_for_token("tok-phone").is_none());
        assert_eq!(st.peer_for_token("tok-laptop").map(|p| p.id.as_str()), Some("d-laptop"));
        assert_eq!(st.inbox.len(), 1);
        assert_eq!(st.inbox[0].id, "task-1");
        assert_eq!(st.inbox[0].status, "done");
        assert_eq!(st.inbox[0].receipts[0].cmd, "dd if=pi.img");
        assert!(path.exists(), "a loadable state must not be quarantined");
        save_hub_state(&path, &st).expect("save");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("resultClaimed"), "{raw}");
        assert!(!raw.contains("consoleApiKey"), "{raw}");
        let _ = std::fs::remove_dir_all(&dir);
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
