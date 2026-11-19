//! Messaging channels, part 3: Telegram, without the network yet. This turns
//! a `getUpdates` reply into the direct messages it holds and decides each
//! one through [`DmTrust`]: a paired sender's text is handed on, a new
//! sender gets a pairing code, and everything else is dropped. Group chats
//! and other bots are never read.

use serde_json::{json, Value};

use crate::msg_channels::{ChannelKind, DmTrust, Inbound};

/// Telegram's limit for one message's text.
pub const MAX_TEXT: usize = 4096;

/// One direct message from a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub update_id: i64,
    pub chat_id: i64,
    /// The sender's Telegram user id, as the trust store keeps it.
    pub sender: String,
    /// `@username`, else the first name, for the approval row.
    pub label: String,
    pub text: String,
}

/// What to do with one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A paired sender: hand the text on.
    Forward(Incoming),
    /// Send this to the chat, and hand nothing on.
    Reply { chat_id: i64, text: String },
    /// Say nothing and hand nothing on.
    Drop,
}

/// The direct messages in a `getUpdates` reply, and the `offset` for the
/// next call (one past the highest update seen, read or not).
pub fn parse_updates(reply: &Value) -> (Vec<Incoming>, Option<i64>) {
    let Some(updates) = reply
        .get("ok")
        .and_then(Value::as_bool)
        .filter(|ok| *ok)
        .and_then(|_| reply.get("result"))
        .and_then(Value::as_array)
    else {
        return (Vec::new(), None);
    };
    let next = updates
        .iter()
        .filter_map(|u| u.get("update_id").and_then(Value::as_i64))
        .max()
        .map(|id| id + 1);
    let messages = updates.iter().filter_map(direct_message).collect();
    (messages, next)
}

fn direct_message(update: &Value) -> Option<Incoming> {
    let update_id = update.get("update_id")?.as_i64()?;
    let msg = update.get("message")?;
    let chat = msg.get("chat")?;
    if chat.get("type")?.as_str()? != "private" {
        return None;
    }
    let from = msg.get("from")?;
    if from.get("is_bot").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let text = msg.get("text")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let label = from
        .get("username")
        .and_then(Value::as_str)
        .map(|u| format!("@{u}"))
        .or_else(|| {
            from.get("first_name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    Some(Incoming {
        update_id,
        chat_id: chat.get("id")?.as_i64()?,
        sender: from.get("id")?.as_i64()?.to_string(),
        label,
        text: text.to_string(),
    })
}

/// The reply to a sender who isn't paired yet.
pub fn pairing_text(code: &str) -> String {
    format!(
        "This assistant only talks to people its owner paired. To pair this chat, ask the owner to enter the code {code} in the app on their computer. The code lasts an hour."
    )
}

/// The decision for `msg`. A sender who already has a live code hears
/// nothing more, so a stranger can't make the bot repeat itself.
pub fn step(trust: &mut DmTrust, msg: Incoming, now_ms: u64) -> Step {
    match trust.inbound(ChannelKind::Telegram, &msg.sender, &msg.label, now_ms) {
        Inbound::Trusted => Step::Forward(msg),
        Inbound::Pair { code, fresh: true } => Step::Reply {
            chat_id: msg.chat_id,
            text: pairing_text(&code),
        },
        Inbound::Pair { fresh: false, .. } | Inbound::Busy => Step::Drop,
    }
}

/// A bot token as BotFather gives it: `<digits>:<at least 30 of A-Z a-z 0-9 _ ->`.
pub fn valid_token(token: &str) -> bool {
    let Some((id, secret)) = token.split_once(':') else {
        return false;
    };
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && secret.len() >= 30
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The Bot API address for `method`, or `None` for a malformed token.
pub fn api_url(token: &str, method: &str) -> Option<String> {
    valid_token(token).then(|| format!("https://api.telegram.org/bot{token}/{method}"))
}

/// The `sendMessage` body. Long text is cut at [`MAX_TEXT`] characters.
pub fn send_message_body(chat_id: i64, text: &str) -> Value {
    let text: String = text.chars().take(MAX_TEXT).collect();
    json!({ "chat_id": chat_id, "text": text })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_763_550_000_000;
    const TOKEN: &str = "123456789:AAabcdefghijklmnopqrstuvwxyz0123456";

    fn update(id: i64, chat_type: &str, from: Value, text: &str) -> Value {
        json!({
            "update_id": id,
            "message": {
                "message_id": id * 10,
                "chat": { "id": 5001, "type": chat_type },
                "from": from,
                "text": text,
            }
        })
    }

    fn person() -> Value {
        json!({ "id": 777, "is_bot": false, "first_name": "User", "username": "user" })
    }

    #[test]
    fn only_direct_text_from_people_is_read() {
        let reply = json!({
            "ok": true,
            "result": [
                update(41, "private", person(), " hi there "),
                update(42, "group", person(), "group noise"),
                update(43, "private", json!({ "id": 9, "is_bot": true, "first_name": "Bot" }), "bot noise"),
                update(44, "private", person(), "   "),
                { "update_id": 45, "edited_message": { "text": "edit" } },
            ]
        });
        let (messages, next) = parse_updates(&reply);
        assert_eq!(
            messages,
            vec![Incoming {
                update_id: 41,
                chat_id: 5001,
                sender: "777".into(),
                label: "@user".into(),
                text: "hi there".into(),
            }]
        );
        // Past every update, the skipped ones too, so none comes back.
        assert_eq!(next, Some(46));
        assert_eq!(
            parse_updates(&json!({ "ok": false, "description": "Unauthorized" })),
            (vec![], None)
        );
        assert_eq!(
            parse_updates(&json!({ "ok": true, "result": [] })),
            (vec![], None)
        );
    }

    #[test]
    fn a_stranger_gets_one_code_and_a_paired_sender_is_handed_on() {
        let mut trust = DmTrust::default();
        let (mut messages, _) = parse_updates(
            &json!({ "ok": true, "result": [update(1, "private", person(), "hello")] }),
        );
        let msg = messages.remove(0);
        let Step::Reply { chat_id, text } = step(&mut trust, msg.clone(), NOW) else {
            panic!("a stranger gets the pairing text");
        };
        assert_eq!(chat_id, 5001);
        let code = trust.pending[0].code.clone();
        assert_eq!(text, pairing_text(&code));
        assert!(text.contains(&code));
        // Asking again doesn't make it repeat.
        assert_eq!(step(&mut trust, msg.clone(), NOW + 1), Step::Drop);
        trust.approve(&code, NOW + 2).unwrap();
        assert_eq!(step(&mut trust, msg.clone(), NOW + 3), Step::Forward(msg));
        assert_eq!(trust.pending.len(), 0);
    }

    #[test]
    fn tokens_and_bodies_have_the_bot_api_shape() {
        assert_eq!(
            api_url(TOKEN, "getUpdates").as_deref(),
            Some("https://api.telegram.org/bot123456789:AAabcdefghijklmnopqrstuvwxyz0123456/getUpdates")
        );
        for bad in [
            "",
            "123456789",
            "abc:AAabcdefghijklmnopqrstuvwxyz0123456",
            "1:short",
            "1:AAabcdefghijklmnopqrstuvwxyz01234/../x",
        ] {
            assert_eq!(api_url(bad, "getUpdates"), None, "{bad}");
        }
        assert_eq!(
            send_message_body(5001, "hi"),
            json!({ "chat_id": 5001, "text": "hi" })
        );
        let long = "é".repeat(MAX_TEXT + 10);
        assert_eq!(
            send_message_body(1, &long)["text"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            MAX_TEXT
        );
    }
}
