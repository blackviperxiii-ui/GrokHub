//! `/sync` and `/inhabit`: what this computer's hub hands to the computers
//! paired with it. Both carry your chats or memory off this computer, so both
//! pass the EgressGuard first. A standing hub grant sends at once and logs
//! the send; with none, the chat shows a card, and only that card's Send (its
//! own call, which no typed line reaches) sends, logged as approved once.
//! While no computer is paired or sharing is off, nothing is asked, sent or
//! logged.
//!
//! The hub serves what is handed to it only while sharing is on, and only
//! until the app quits (see [`crate::hub_share`]).

use std::path::Path;

use grokhub_agent::harness as hx;
use grokhub_core::{now_ms, redact_secret_words, state::Peer, HubMemoryFile, InhabitBundle};
use serde_json::{json, Value};

use crate::hub_share::{HubPeers, HubShare};
use crate::slash::{Effect, HubSend};

pub const NO_PEERS: &str = "Nothing is paired yet. Pair a computer on Devices first.";
pub const SHARE_OFF: &str = "Sharing is off. Start share on Devices first.";
pub const SYNC_ASK: &str = "Share your chats and memory with your paired computers?";
pub const SYNC_NOTE: &str = "They can fetch your chats, memory files, skills, automations and board while sharing stays on. Send shares them once; Cancel sends nothing.";
pub const NOTHING_TO_HAND: &str =
    "There's nothing to hand off yet: SOUL.md, your skills and the goal pin are all empty.";
const PHONE: &str = "A phone can't pick up a hand-off.";

/// What `/inhabit` carries: SOUL.md is memory, and skill names and the goal
/// pin say what you work on. No chat goes with it.
const INHABIT_DATA: &[hx::DataClass] = &[hx::DataClass::Personal];

/// How a send was allowed: your standing hub grant, or Send on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Basis {
    Grant,
    Once,
}

fn data(send: &HubSend) -> &'static [hx::DataClass] {
    match send {
        HubSend::Sync => hx::HUB_SYNC_DATA,
        HubSend::Inhabit { .. } => INHABIT_DATA,
    }
}

pub fn inhabit_ask(name: &str) -> String {
    format!("Hand off to {name}?")
}

pub fn inhabit_note(name: &str) -> String {
    format!(
        "{name} can pick up your SOUL.md, skill names and goal pin once, while sharing stays on. Send hands them over; Cancel sends nothing."
    )
}

fn synced(peers: usize) -> String {
    match peers {
        1 => "Shared chats and memory with your paired computer.".into(),
        n => format!("Shared chats and memory with your {n} paired computers."),
    }
}

fn handed(name: &str) -> String {
    format!("Handed off to {name}. It can pick this up once while sharing is on.")
}

/// The paired computer `/inhabit` names: its device id first, else its name,
/// either ignoring case.
fn find_peer<'a>(peers: &'a [Peer], want: &str) -> Option<&'a Peer> {
    let want = want.trim();
    peers
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(want))
        .or_else(|| peers.iter().find(|p| p.name.eq_ignore_ascii_case(want)))
}

/// Someone to send to: the computer `/inhabit` names (none for `/sync`), or
/// the line that says why there is nobody.
fn target(hub: &HubPeers, send: &HubSend) -> Result<Option<Peer>, String> {
    if hub.peers.is_empty() {
        return Err(NO_PEERS.into());
    }
    if !hub.sharing {
        return Err(SHARE_OFF.into());
    }
    let HubSend::Inhabit { peer } = send else {
        return Ok(None);
    };
    let Some(found) = find_peer(&hub.peers, peer) else {
        return Err(format!("No paired computer is named {}.", peer.trim()));
    };
    if grokhub_core::is_phone_name(&found.name) {
        return Err(PHONE.into());
    }
    Ok(Some(found.clone()))
}

/// A send must be logged to go, and the log is sealed with the private-data
/// key: while that key can't be had, nothing is asked or sent.
fn unlocked(config_dir: &Path, ledger: &hx::ConsentLedger) -> Result<(), String> {
    let why = match ledger.locked() {
        Some(why) => why.message(),
        None => match hx::read_key(config_dir, true) {
            Some(Err(why)) => why.message(),
            _ => return Ok(()),
        },
    };
    Err(format!("Nothing was sent. {why}"))
}

/// The slash line: sends at once under a standing grant, else asks on a card.
pub fn check(config_dir: &Path, share: &HubShare, send: HubSend) -> Result<Effect, String> {
    let hub = share.peers();
    let peer = target(&hub, &send)?;
    let ledger = hx::ConsentLedger::load(config_dir);
    unlocked(config_dir, &ledger)?;
    let step = hx::Step::Egress {
        dest: hx::HUB_DEST,
        data: data(&send),
        ledger: &ledger,
    };
    match hx::decide(step) {
        hx::GateOutcome::Allow => Ok(Effect::Notice {
            text: send_now(config_dir, share, &hub, &send, peer.as_ref(), Basis::Grant)?,
        }),
        hx::GateOutcome::Park { hard: Some(_), .. } => Ok(match peer {
            // The card holds the computer's id, so Send goes to the one
            // asked about even if another one pairs under its name meanwhile.
            Some(peer) => Effect::HubAsk {
                send: HubSend::Inhabit {
                    peer: peer.id.clone(),
                },
                prompt: inhabit_ask(&peer.name),
                note: inhabit_note(&peer.name),
            },
            None => Effect::HubAsk {
                send,
                prompt: SYNC_ASK.into(),
                note: SYNC_NOTE.into(),
            },
        }),
        hx::GateOutcome::Park { reason, .. } | hx::GateOutcome::Refuse { reason } => {
            Err(format!("Nothing was sent: {reason}"))
        }
    }
}

/// Send on the card. Checks again who is paired and whether sharing is on,
/// since the card may have waited.
pub fn approve(config_dir: &Path, share: &HubShare, send: HubSend) -> Result<String, String> {
    let hub = share.peers();
    let peer = target(&hub, &send)?;
    unlocked(config_dir, &hx::ConsentLedger::load(config_dir))?;
    send_now(config_dir, share, &hub, &send, peer.as_ref(), Basis::Once)
}

/// Builds what goes, logs the send (or sends nothing), then hands it to the
/// hub. Building comes first, so a file that can't be read logs no send.
fn send_now(
    config_dir: &Path,
    share: &HubShare,
    hub: &HubPeers,
    send: &HubSend,
    peer: Option<&Peer>,
    basis: Basis,
) -> Result<String, String> {
    let req = hx::EgressReq::new(hx::HUB_DEST, data(send));
    match peer {
        Some(peer) => {
            let bundle = hand_off(config_dir, hub, peer)?;
            log(config_dir, &req, basis)?;
            share.stage(bundle);
            Ok(handed(&peer.name))
        }
        None => {
            let snap = snapshot(config_dir, hub)?;
            log(config_dir, &req, basis)?;
            share.publish(snap);
            Ok(synced(hub.peers.len()))
        }
    }
}

/// One `egress.jsonl` line, or nothing goes: a grant revoked since the check
/// stops here, and a log that can't be written sends nothing.
fn log(config_dir: &Path, req: &hx::EgressReq<'_>, basis: Basis) -> Result<(), String> {
    match basis {
        Basis::Grant => match hx::guard_egress(config_dir, req) {
            hx::GateOutcome::Allow => Ok(()),
            hx::GateOutcome::Park { reason, .. } | hx::GateOutcome::Refuse { reason } => {
                Err(format!("Nothing was sent: {reason}"))
            }
        },
        Basis::Once => hx::record_approved_once(config_dir, req)
            .map_err(|why| format!("Nothing was sent: {why}")),
    }
}

/// Every string in `v` with its key- and password-shaped words masked.
fn scrub(v: &mut Value) {
    match v {
        Value::String(s) => *s = redact_secret_words(s),
        Value::Array(items) => items.iter_mut().for_each(scrub),
        Value::Object(map) => map.values_mut().for_each(scrub),
        _ => {}
    }
}

/// The memory tabs, the skills in this app's folder that are on, and the
/// goal pin, read as the config folder has them.
fn memory_files(config_dir: &Path) -> Result<Vec<(&'static str, String, u64)>, String> {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    crate::memory::tab_files()
}

/// `/sync`: the saved chats (newest first, scratch chats left out, tool
/// steps left out), the memory tabs, skills, automations and the board,
/// with key- and password-shaped words masked.
fn snapshot(config_dir: &Path, hub: &HubPeers) -> Result<grokhub_core::HubSnapshot, String> {
    let mut threads: Vec<Value> = crate::history::list(config_dir)
        .into_iter()
        .filter(|row| !grokhub_agent::scratch::is_scratch(&row.id))
        .take(40)
        .filter_map(|row| {
            let info = crate::history::saved(config_dir, &row.id).ok()?;
            let messages: Vec<Value> = crate::history::lines(&info)
                .into_iter()
                .filter_map(|line| {
                    let role = match line.role {
                        crate::history::Role::User => "user",
                        crate::history::Role::Assistant => "assistant",
                        crate::history::Role::Tools => return None,
                    };
                    Some(json!({ "role": role, "content": line.text }))
                })
                .collect();
            Some(json!({
                "id": row.id,
                "title": row.title,
                "updatedAt": row.updated,
                "messages": messages,
            }))
        })
        .collect();
    threads.iter_mut().for_each(scrub);
    let memory = memory_files(config_dir)?
        .into_iter()
        .map(|(name, body, at)| HubMemoryFile {
            name: name.into(),
            content: redact_secret_words(&body),
            updated_at: at,
        })
        .collect();
    let skills = crate::skills::shared(config_dir)
        .into_iter()
        .map(|(name, at)| json!({ "id": name, "name": name, "updatedAt": at }))
        .collect();
    let mut automations: Vec<Value> = crate::automations::load(config_dir)?
        .iter()
        .filter_map(|a| serde_json::to_value(a).ok())
        .collect();
    automations.iter_mut().for_each(scrub);
    let cards = grokhub_core::work_file::read_board(&crate::workboard::path(config_dir))?;
    let mut board = json!({ "items": cards });
    scrub(&mut board);
    Ok(grokhub_core::build_hub_snapshot(
        &hub.device_id,
        &hub.device_name,
        now_ms(),
        threads,
        board,
        skills,
        automations,
        memory,
    ))
}

/// `/inhabit`: SOUL.md, the names of your skills that are on, and the goal
/// pin, addressed to `peer` by its device id.
fn hand_off(config_dir: &Path, hub: &HubPeers, peer: &Peer) -> Result<InhabitBundle, String> {
    let soul = memory_files(config_dir)?
        .into_iter()
        .find(|(name, _, _)| *name == "SOUL.md")
        .map(|(_, body, _)| redact_secret_words(&body))
        .unwrap_or_default();
    let goal = crate::slash_ops::goal_pin(config_dir);
    let bundle = InhabitBundle {
        soul,
        skill_ids: crate::skills::shared(config_dir)
            .into_iter()
            .map(|(name, _)| name)
            .collect(),
        goal: (!goal.trim().is_empty()).then_some(goal),
        project_snapshot_id: None,
        from_id: Some(hub.device_id.clone()),
        from_name: Some(hub.device_name.clone()),
        to_id: Some(peer.id.clone()),
        to_name: Some(peer.name.clone()),
        at: Some(now_ms()),
    };
    if !grokhub_core::inhabit_bundle_usable(&bundle) {
        return Err(NOTHING_TO_HAND.into());
    }
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::harness::{use_key_store_for, MemoryKeyStore};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::path::PathBuf;
    use std::sync::Arc;

    const KEY: &str = "sk-abcdefghijklmnopqrstuv";

    struct Hub {
        cfg: PathBuf,
        share: HubShare,
        keys: Arc<MemoryKeyStore>,
    }

    impl Drop for Hub {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.cfg);
        }
    }

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, body).expect("write");
    }

    fn say(role: &str, text: &str) -> grokhub_agent::InputItem {
        grokhub_agent::InputItem::Message {
            role: role.into(),
            content: vec![grokhub_agent::ContentPart::InputText(text.into())],
        }
    }

    fn chat(cfg: &Path, id: &str, user: &str, reply: &str) {
        let _guard = grokhub_agent::perm::ConfigGuard::set(cfg);
        grokhub_agent::record_turn(
            id,
            "/home/user/my-app",
            "grok-test",
            &[say("user", user), say("assistant", reply)],
            &grokhub_agent::Usage::default(),
            "",
            user,
        )
        .expect("chat");
    }

    /// A config folder with a chat, a scratch chat, the memory tabs, a skill,
    /// an automation, a board card and a goal pin; its hub not started.
    fn hub(name: &str) -> Hub {
        let cfg = std::env::temp_dir().join(format!(
            "grokhub-shell-hub-send-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).expect("cfg");
        let keys = Arc::new(MemoryKeyStore::new());
        use_key_store_for(&cfg, keys.clone());
        chat(
            &cfg,
            "shell-harbor",
            "plan the harbor trip",
            "Tuesday at nine.",
        );
        chat(
            &cfg,
            "shell-scratch-x",
            "a scratch question",
            "A scratch answer.",
        );
        write(&cfg.join("memory/SOUL.md"), "Be brief.");
        write(&cfg.join("memory/USER.md"), "Likes tea.");
        write(&cfg.join("memory/MEMORY.md"), &format!("deploy key {KEY}"));
        write(
            &cfg.join("skills/tidy/SKILL.md"),
            "---\nname: tidy\ndescription: Tidy the repo\n---\n",
        );
        write(
            &cfg.join("app.json"),
            r#"{"goalPin":"ship the harbor app"}"#,
        );
        let share = HubShare::on_loopback(&cfg);
        Hub { cfg, share, keys }
    }

    fn call(port: u16, method: &str, path: &str, token: &str, body: &str) -> (u16, Value) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        write!(
            s,
            "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .expect("send");
        let mut out = String::new();
        s.read_to_string(&mut out).expect("read");
        let status = out
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let json = out.split("\r\n\r\n").nth(1).unwrap_or("");
        (status, serde_json::from_str(json).unwrap_or(Value::Null))
    }

    /// Start share and pair one computer the way another app does; its port
    /// and token.
    fn pair(hub: &Hub, id: &str, name: &str) -> (u16, String) {
        let on = hub.share.start().expect("start");
        let body = format!(
            r#"{{"code":"{}","deviceId":"{id}","deviceName":"{name}"}}"#,
            on.code
        );
        let (status, v) = call(on.port, "POST", "/v1/pair", "", &body);
        assert_eq!(status, 200, "{v}");
        (on.port, v["token"].as_str().expect("token").to_string())
    }

    fn sent(cfg: &Path) -> Vec<(String, String)> {
        hx::read_egress(cfg)
            .into_iter()
            .map(|l| (l.dest, l.grant_id))
            .collect()
    }

    #[test]
    fn nobody_paired_or_sharing_off_asks_and_sends_nothing() {
        let hub = hub("nobody");
        for send in [
            HubSend::Sync,
            HubSend::Inhabit {
                peer: "desk".into(),
            },
        ] {
            assert_eq!(
                check(&hub.cfg, &hub.share, send.clone()),
                Err(NO_PEERS.into())
            );
            assert_eq!(approve(&hub.cfg, &hub.share, send), Err(NO_PEERS.into()));
        }
        let (port, token) = pair(&hub, "d-desk", "Desk");
        hub.share.stop();
        for send in [
            HubSend::Sync,
            HubSend::Inhabit {
                peer: "desk".into(),
            },
        ] {
            assert_eq!(
                check(&hub.cfg, &hub.share, send.clone()),
                Err(SHARE_OFF.into())
            );
            assert_eq!(approve(&hub.cfg, &hub.share, send), Err(SHARE_OFF.into()));
        }
        assert_eq!(sent(&hub.cfg), Vec::<(String, String)>::new());
        hub.share.start().expect("start");
        assert_eq!(
            call(port, "GET", "/v1/snapshot", &token, ""),
            (200, json!({ "ok": true, "snapshot": null }))
        );
    }

    #[test]
    fn sync_asks_first_and_send_shares_chats_and_memory_once() {
        let hub = hub("sync");
        let (port, token) = pair(&hub, "d-desk", "Desk");
        assert_eq!(
            check(&hub.cfg, &hub.share, HubSend::Sync),
            Ok(Effect::HubAsk {
                send: HubSend::Sync,
                prompt: SYNC_ASK.into(),
                note: SYNC_NOTE.into(),
            })
        );
        // The card is up: nothing has gone and nothing is logged.
        assert_eq!(sent(&hub.cfg), Vec::<(String, String)>::new());
        assert_eq!(
            call(port, "GET", "/v1/snapshot", &token, "").1["snapshot"],
            Value::Null
        );

        assert_eq!(
            approve(&hub.cfg, &hub.share, HubSend::Sync),
            Ok("Shared chats and memory with your paired computer.".into())
        );
        assert_eq!(
            sent(&hub.cfg),
            vec![("hub".to_string(), "approved-once".to_string())]
        );

        let (status, got) = call(port, "GET", "/v1/snapshot", &token, "");
        assert_eq!(status, 200);
        let snap = &got["snapshot"];
        assert_eq!(snap["kind"], grokhub_core::HUB_KIND);
        let threads = snap["threads"].as_array().expect("threads");
        assert_eq!(threads.len(), 1, "the scratch chat stays here: {threads:?}");
        assert_eq!(threads[0]["id"], "shell-harbor");
        assert_eq!(threads[0]["title"], "plan the harbor trip");
        assert_eq!(
            threads[0]["messages"],
            json!([
                { "role": "user", "content": "plan the harbor trip" },
                { "role": "assistant", "content": "Tuesday at nine." },
            ])
        );
        let files: Vec<(&str, &str)> = snap["memoryFiles"]
            .as_array()
            .expect("memory")
            .iter()
            .map(|f| (f["name"].as_str().unwrap(), f["content"].as_str().unwrap()))
            .collect();
        assert_eq!(
            files,
            vec![
                ("SOUL.md", "Be brief."),
                ("USER.md", "Likes tea."),
                ("MEMORY.md", "deploy key [redacted]"),
            ]
        );
        assert_eq!(snap["skills"][0]["name"], "tidy");
        assert!(!got.to_string().contains(KEY), "a key left this computer");

        // Stop share keeps the listener: the paired computer gets nothing.
        hub.share.stop();
        assert_eq!(
            call(port, "GET", "/v1/snapshot", &token, ""),
            (
                403,
                json!({ "ok": false, "error": "Sharing is off on this computer." })
            )
        );
    }

    #[test]
    fn a_standing_hub_grant_sends_without_a_card_and_logs_it() {
        let hub = hub("grant");
        let (port, token) = pair(&hub, "d-desk", "Desk");
        let grant = hx::grant_destination(
            &hub.cfg,
            hx::HUB_DEST,
            hx::HUB_SYNC_DATA,
            hx::UserClick::from_click(),
        )
        .expect("grant");
        assert_eq!(
            check(&hub.cfg, &hub.share, HubSend::Sync),
            Ok(Effect::Notice {
                text: "Shared chats and memory with your paired computer.".into()
            })
        );
        assert_eq!(sent(&hub.cfg), vec![("hub".to_string(), grant.id)]);
        let snap = call(port, "GET", "/v1/snapshot", &token, "").1;
        assert_eq!(snap["snapshot"]["threads"][0]["id"], "shell-harbor");
    }

    #[test]
    fn a_locked_ledger_sends_nothing_and_says_why() {
        let hub = hub("locked");
        pair(&hub, "d-desk", "Desk");
        // One send seals the log with this computer's key.
        approve(&hub.cfg, &hub.share, HubSend::Sync).expect("first send");
        assert_eq!(sent(&hub.cfg).len(), 1);
        let log = std::fs::read(hx::egress_path(&hub.cfg)).expect("log");
        hub.keys.forget();
        // A fresh look at the keyring, as after the app's recheck.
        use_key_store_for(&hub.cfg, hub.keys.clone());
        for send in [
            HubSend::Sync,
            HubSend::Inhabit {
                peer: "d-desk".into(),
            },
        ] {
            let refused = check(&hub.cfg, &hub.share, send.clone()).expect_err("locked");
            assert!(refused.starts_with("Nothing was sent. "), "{refused}");
            let refused = approve(&hub.cfg, &hub.share, send).expect_err("locked");
            assert!(refused.starts_with("Nothing was sent. "), "{refused}");
        }
        assert_eq!(std::fs::read(hx::egress_path(&hub.cfg)).expect("log"), log);
    }

    #[test]
    fn inhabit_hands_soul_skills_and_goal_to_the_computer_named_once() {
        let hub = hub("inhabit");
        let (port, desk) = pair(&hub, "d-desk", "Desk PC");
        assert_eq!(
            check(
                &hub.cfg,
                &hub.share,
                HubSend::Inhabit {
                    peer: "nope".into()
                }
            ),
            Err("No paired computer is named nope.".into())
        );
        // By name, ignoring case; the card holds the computer's id.
        assert_eq!(
            check(
                &hub.cfg,
                &hub.share,
                HubSend::Inhabit {
                    peer: "desk pc".into()
                }
            ),
            Ok(Effect::HubAsk {
                send: HubSend::Inhabit {
                    peer: "d-desk".into()
                },
                prompt: "Hand off to Desk PC?".into(),
                note: inhabit_note("Desk PC"),
            })
        );
        assert_eq!(sent(&hub.cfg), Vec::<(String, String)>::new());
        assert_eq!(
            approve(
                &hub.cfg,
                &hub.share,
                HubSend::Inhabit {
                    peer: "d-desk".into()
                }
            ),
            Ok("Handed off to Desk PC. It can pick this up once while sharing is on.".into())
        );
        assert_eq!(
            sent(&hub.cfg),
            vec![("hub".to_string(), "approved-once".to_string())]
        );
        let (status, got) = call(port, "GET", "/v1/inhabit", &desk, "");
        assert_eq!(status, 200);
        let bundle = &got["bundle"];
        assert_eq!(bundle["soul"], "Be brief.");
        assert_eq!(bundle["skillIds"], json!(["tidy"]));
        assert_eq!(bundle["goal"], "ship the harbor app");
        assert_eq!(bundle["toId"], "d-desk");
        assert_eq!(bundle["toName"], "Desk PC");
        // Claimed once.
        assert_eq!(
            call(port, "GET", "/v1/inhabit", &desk, "").1["bundle"],
            Value::Null
        );
    }

    #[test]
    fn inhabit_with_nothing_to_hand_logs_no_send() {
        let hub = hub("empty");
        pair(&hub, "d-desk", "Desk");
        std::fs::write(hub.cfg.join("memory/SOUL.md"), "").expect("soul");
        std::fs::remove_dir_all(hub.cfg.join("skills")).expect("skills");
        std::fs::write(hub.cfg.join("app.json"), "{}").expect("app");
        assert_eq!(
            approve(
                &hub.cfg,
                &hub.share,
                HubSend::Inhabit {
                    peer: "d-desk".into()
                }
            ),
            Err(NOTHING_TO_HAND.into())
        );
        assert_eq!(sent(&hub.cfg), Vec::<(String, String)>::new());
    }
}
