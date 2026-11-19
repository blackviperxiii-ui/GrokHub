//! Messaging channels, part 4: Discord, without the network yet. This reads
//! a Gateway `MESSAGE_CREATE` event and decides it through [`DmTrust`], as
//! Telegram does: only direct messages (no `guild_id`) from people are
//! read; a paired sender's text is handed on, and a new sender gets one
//! pairing code.

use serde_json::{json, Value};

use crate::msg_channels::{ChannelKind, DmTrust, Inbound};

/// Discord's limit for one message's content.
pub const MAX_CONTENT: usize = 2000;
/// The REST base the bot posts replies to.
pub const API_BASE: &str = "https://discord.com/api/v10";

/// One direct message from a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    /// The DM channel to answer in.
    pub channel_id: String,
    /// The sender's Discord user id, as the trust store keeps it.
    pub sender: String,
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Forward(Incoming),
    Reply { channel_id: String, text: String },
    Drop,
}

fn snowflake(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    (!s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.to_string())
}

/// The direct message in one Gateway dispatch, if it is one from a person.
pub fn direct_message(event: &Value) -> Option<Incoming> {
    if event.get("op")?.as_i64()? != 0 || event.get("t")?.as_str()? != "MESSAGE_CREATE" {
        return None;
    }
    let d = event.get("d")?;
    // A message in a server channel carries its guild; a DM doesn't.
    if d.get("guild_id").is_some_and(|g| !g.is_null()) {
        return None;
    }
    let author = d.get("author")?;
    if author.get("bot").and_then(Value::as_bool).unwrap_or(false)
        || author
            .get("system")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return None;
    }
    let text = d.get("content")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    Some(Incoming {
        channel_id: snowflake(d.get("channel_id"))?,
        sender: snowflake(author.get("id"))?,
        label: author
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text: text.to_string(),
    })
}

/// The decision for `msg`, as [`crate::msg_telegram::step`].
pub fn step(trust: &mut DmTrust, msg: Incoming, now_ms: u64) -> Step {
    match trust.inbound(ChannelKind::Discord, &msg.sender, &msg.label, now_ms) {
        Inbound::Trusted => Step::Forward(msg),
        Inbound::Pair { code, fresh: true } => Step::Reply {
            channel_id: msg.channel_id,
            text: crate::msg_telegram::pairing_text(&code),
        },
        Inbound::Pair { fresh: false, .. } | Inbound::Busy => Step::Drop,
    }
}

/// `POST {API_BASE}/channels/{id}/messages`, or `None` for a bad id.
pub fn messages_url(channel_id: &str) -> Option<String> {
    snowflake(Some(&Value::String(channel_id.to_string())))
        .map(|id| format!("{API_BASE}/channels/{id}/messages"))
}

/// The message body. Long text is cut at [`MAX_CONTENT`] characters, and no
/// mention in it can ping anyone.
pub fn message_body(text: &str) -> Value {
    let content: String = text.chars().take(MAX_CONTENT).collect();
    json!({ "content": content, "allowed_mentions": { "parse": [] } })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_763_550_000_000;

    fn event(d: Value) -> Value {
        json!({ "op": 0, "t": "MESSAGE_CREATE", "s": 7, "d": d })
    }

    fn dm(content: &str) -> Value {
        event(json!({
            "id": "1100000000000000001",
            "channel_id": "1200000000000000002",
            "author": { "id": "1300000000000000003", "username": "user" },
            "content": content,
        }))
    }

    #[test]
    fn only_direct_messages_from_people_are_read() {
        assert_eq!(
            direct_message(&dm(" hi ")),
            Some(Incoming {
                channel_id: "1200000000000000002".into(),
                sender: "1300000000000000003".into(),
                label: "user".into(),
                text: "hi".into(),
            })
        );
        let mut in_server = dm("server noise");
        in_server["d"]["guild_id"] = json!("1400000000000000004");
        assert_eq!(direct_message(&in_server), None);
        let mut from_bot = dm("bot noise");
        from_bot["d"]["author"]["bot"] = json!(true);
        assert_eq!(direct_message(&from_bot), None);
        assert_eq!(direct_message(&dm("  ")), None);
        let mut other = dm("typing");
        other["t"] = json!("TYPING_START");
        assert_eq!(direct_message(&other), None);
        let mut heartbeat = dm("x");
        heartbeat["op"] = json!(11);
        assert_eq!(direct_message(&heartbeat), None);
        let mut odd_id = dm("x");
        odd_id["d"]["channel_id"] = json!("../../users/@me");
        assert_eq!(direct_message(&odd_id), None);
    }

    #[test]
    fn a_stranger_gets_one_code_and_a_paired_sender_is_handed_on() {
        let mut trust = DmTrust::default();
        let msg = direct_message(&dm("hello")).unwrap();
        let Step::Reply { channel_id, text } = step(&mut trust, msg.clone(), NOW) else {
            panic!("a stranger gets the pairing text");
        };
        assert_eq!(channel_id, "1200000000000000002");
        let code = trust.pending[0].code.clone();
        assert_eq!(text, crate::msg_telegram::pairing_text(&code));
        assert_eq!(step(&mut trust, msg.clone(), NOW + 1), Step::Drop);
        trust.approve(&code, NOW + 2).unwrap();
        assert_eq!(step(&mut trust, msg.clone(), NOW + 3), Step::Forward(msg));
        // Paired on Discord is not paired on Telegram.
        assert!(!trust.is_paired(ChannelKind::Telegram, "1300000000000000003"));
    }

    #[test]
    fn replies_go_to_the_dm_channel_and_never_ping() {
        assert_eq!(
            messages_url("1200000000000000002").as_deref(),
            Some("https://discord.com/api/v10/channels/1200000000000000002/messages")
        );
        assert_eq!(messages_url("12/../34"), None);
        assert_eq!(
            message_body("@everyone hi"),
            json!({ "content": "@everyone hi", "allowed_mentions": { "parse": [] } })
        );
        let long = "x".repeat(MAX_CONTENT + 5);
        assert_eq!(
            message_body(&long)["content"].as_str().unwrap().len(),
            MAX_CONTENT
        );
    }
}
