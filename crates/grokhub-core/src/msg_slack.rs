//! Messaging channels, part 7: Slack, without the network. This reads
//! Socket Mode frames and decides each message through [`DmTrust`], as
//! Telegram and Discord do. A direct message from a paired member is handed
//! on; a new member who writes directly gets one pairing code. In a channel
//! the app only reads a message that mentions it, only from a paired member,
//! and answers in that message's thread. A stranger who mentions it in a
//! channel hears nothing, so a pairing code is never posted where others
//! can read it. The socket itself lives in the agent crate.

use serde_json::{json, Value};

use crate::msg_channels::{ChannelKind, DmTrust, Inbound};

/// The Web API base.
pub const API_BASE: &str = "https://slack.com/api";
/// Slack's limit for one message's text is 40,000 characters; replies stay
/// well under it.
pub const MAX_TEXT: usize = 12_000;
/// Event ids remembered, so a retried event is handled once.
pub const SEEN_EVENTS: usize = 256;

/// One message the app may answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    /// The member's Slack user id, as the trust store keeps it.
    pub sender: String,
    /// The conversation to answer in.
    pub channel: String,
    /// The thread to answer in; `None` for a direct message.
    pub thread_ts: Option<String>,
    pub text: String,
}

impl Incoming {
    /// Where the answer goes, as the inbox keeps it: `D…` for a direct
    /// message, `C…:1700000000.000100` for a thread.
    pub fn reply_to(&self) -> String {
        match &self.thread_ts {
            Some(ts) => format!("{}:{ts}", self.channel),
            None => self.channel.clone(),
        }
    }
}

/// `reply_to` read back: the conversation and the thread.
pub fn split_reply_to(reply_to: &str) -> Option<(&str, Option<&str>)> {
    let (channel, ts) = match reply_to.split_once(':') {
        Some((c, ts)) => (c, Some(ts)),
        None => (reply_to, None),
    };
    (slack_id(channel) && ts.is_none_or(valid_ts)).then_some((channel, ts))
}

/// What one Socket Mode frame asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// The socket is ready.
    Hello,
    /// Slack will close this socket; open a new one. `fatal` when the app
    /// was turned off and reconnecting can't help.
    Disconnect { fatal: bool },
    /// An envelope to acknowledge at once, with the event id to dedupe on
    /// and the message in it, if it is one the app reads.
    Event {
        envelope_id: String,
        event_id: Option<String>,
        message: Option<Incoming>,
    },
    /// Anything else.
    Other,
}

/// Slack ids are upper-case letters and digits (`U0…`, `D0…`, `C0…`).
fn slack_id(s: &str) -> bool {
    (2..=24).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// A message timestamp: digits, a dot, digits.
fn valid_ts(s: &str) -> bool {
    s.split_once('.').is_some_and(|(a, b)| {
        !a.is_empty()
            && !b.is_empty()
            && a.len() + b.len() <= 24
            && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit())
    })
}

/// Slack escapes `&`, `<` and `>` in message text.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// The text without any `<@U…>` mention of the app itself.
fn without_mention(text: &str, bot: &str) -> String {
    text.replace(&format!("<@{bot}>"), " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read one frame. `bot` is the app's own user id (from `auth.test`), so a
/// mention of it can be taken out of the text.
pub fn read_frame(frame: &Value, bot: &str) -> Frame {
    match frame.get("type").and_then(Value::as_str) {
        Some("hello") => return Frame::Hello,
        Some("disconnect") => {
            let reason = frame.get("reason").and_then(Value::as_str).unwrap_or("");
            return Frame::Disconnect {
                fatal: reason == "link_disabled",
            };
        }
        _ => {}
    }
    let Some(envelope_id) = frame
        .get("envelope_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 128)
    else {
        return Frame::Other;
    };
    let payload = frame.get("payload");
    let is_event = frame.get("type").and_then(Value::as_str) == Some("events_api");
    Frame::Event {
        envelope_id: envelope_id.to_string(),
        event_id: payload
            .and_then(|p| p.get("event_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        message: is_event
            .then(|| payload.and_then(|p| p.get("event")))
            .flatten()
            .and_then(|e| message(e, bot)),
    }
}

fn message(event: &Value, bot: &str) -> Option<Incoming> {
    // Edits, joins, bot posts and the app's own messages carry a subtype or
    // a bot id; none of them is read.
    if event.get("subtype").is_some() || event.get("bot_id").is_some() {
        return None;
    }
    let sender = event.get("user")?.as_str()?;
    let channel = event.get("channel")?.as_str()?;
    if !slack_id(sender) || !slack_id(channel) || sender == bot {
        return None;
    }
    let raw = unescape(event.get("text")?.as_str()?);
    let thread_ts = match event.get("type")?.as_str()? {
        "message" if event.get("channel_type")?.as_str()? == "im" => None,
        "app_mention" => {
            let ts = event
                .get("thread_ts")
                .or_else(|| event.get("ts"))?
                .as_str()?;
            if !valid_ts(ts) {
                return None;
            }
            Some(ts.to_string())
        }
        _ => return None,
    };
    let text = without_mention(&raw, bot);
    if text.is_empty() {
        return None;
    }
    Some(Incoming {
        sender: sender.to_string(),
        channel: channel.to_string(),
        thread_ts,
        text,
    })
}

/// What to do with one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Forward(Incoming),
    /// Post this in the direct message, and hand nothing on.
    Reply {
        channel: String,
        text: String,
    },
    Drop,
}

/// The decision for `msg`, as [`crate::msg_telegram::step`]. A stranger is
/// only ever answered in a direct message.
pub fn step(trust: &mut DmTrust, msg: Incoming, now_ms: u64) -> Step {
    if msg.thread_ts.is_some() {
        return if trust.is_paired(ChannelKind::Slack, &msg.sender) {
            Step::Forward(msg)
        } else {
            Step::Drop
        };
    }
    match trust.inbound(ChannelKind::Slack, &msg.sender, &msg.sender, now_ms) {
        Inbound::Trusted => Step::Forward(msg),
        Inbound::Pair { code, fresh: true } => Step::Reply {
            channel: msg.channel,
            text: crate::msg_telegram::pairing_text(&code),
        },
        Inbound::Pair { fresh: false, .. } | Inbound::Busy => Step::Drop,
    }
}

/// The ack Slack waits for, within three seconds, before it retries.
pub fn ack(envelope_id: &str) -> Value {
    json!({ "envelope_id": envelope_id })
}

fn token_shape(token: &str, prefix: &str) -> bool {
    token.strip_prefix(prefix).is_some_and(|rest| {
        (20..=256).contains(&rest.len())
            && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// The two tokens a Socket Mode app needs, pasted together in either order
/// with space between: the app-level token (`xapp-…`, `connections:write`)
/// and the bot token (`xoxb-…`). Returns `(app, bot)`.
pub fn split_tokens(pasted: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = pasted.split_whitespace().collect();
    let [a, b] = parts[..] else {
        return None;
    };
    let (app, bot) = if a.starts_with("xapp-") {
        (a, b)
    } else {
        (b, a)
    };
    (token_shape(app, "xapp-") && token_shape(bot, "xoxb-")).then_some((app, bot))
}

/// The `chat.postMessage` body. `&`, `<` and `>` are escaped, so nothing the
/// assistant writes can mention a person or `@channel`.
pub fn post_body(channel: &str, thread_ts: Option<&str>, text: &str) -> Value {
    let text: String = text.chars().take(MAX_TEXT).collect();
    let text = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut body = json!({ "channel": channel, "text": text, "unfurl_links": false });
    if let Some(ts) = thread_ts {
        body["thread_ts"] = json!(ts);
    }
    body
}

/// The bot scopes the app asks for, and nothing more.
pub const BOT_SCOPES: [&str; 3] = ["app_mentions:read", "chat:write", "im:history"];

/// An app manifest to paste into "Create an app → From a manifest".
pub fn manifest(name: &str) -> Value {
    json!({
        "display_information": { "name": name },
        "features": {
            "bot_user": { "display_name": name, "always_online": true },
            "app_home": {
                "messages_tab_enabled": true,
                "messages_tab_read_only_enabled": false
            }
        },
        "oauth_config": { "scopes": { "bot": BOT_SCOPES } },
        "settings": {
            "event_subscriptions": { "bot_events": ["app_mention", "message.im"] },
            "interactivity": { "is_enabled": false },
            "org_deploy_enabled": false,
            "socket_mode_enabled": true,
            "token_rotation_enabled": false
        }
    })
}

/// The id Slack gives an `ok: false` answer, worded for the person.
pub fn api_error(answer: &Value) -> Option<String> {
    if answer.get("ok").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let code = answer
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("unknown_error");
    Some(match code {
        "invalid_auth" | "not_authed" | "account_inactive" | "token_revoked" => {
            "Slack refused the tokens. Copy them again from your Slack app.".into()
        }
        "not_allowed_token_type" => {
            "Slack wants the app token (xapp-) and the bot token (xoxb-), not a user token.".into()
        }
        "missing_scope" => {
            "The Slack app is missing a scope. Create it again from the manifest.".into()
        }
        "channel_not_found" | "not_in_channel" => {
            "Slack says the app isn't in that conversation.".into()
        }
        "ratelimited" => "Slack asked the app to slow down.".into(),
        other => format!(
            "Slack said {}.",
            other
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .take(40)
                .collect::<String>()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_763_550_000_000;
    const BOT: &str = "U0BOT";
    const APP_TOKEN: &str = "xapp-1-A0123456789-0123456789-abcdef0123";
    const BOT_TOKEN: &str = "xoxb-0123456789-0123456789-abcdefABCDEF";

    fn envelope(event: Value) -> Value {
        json!({
            "envelope_id": "env-1",
            "type": "events_api",
            "accepts_response_payload": false,
            "payload": { "type": "event_callback", "event_id": "Ev1", "event": event }
        })
    }

    fn dm(text: &str) -> Value {
        json!({ "type": "message", "channel_type": "im", "channel": "D0DM", "user": "U0ME", "text": text, "ts": "1700000000.000100" })
    }

    fn read(event: Value) -> Option<Incoming> {
        match read_frame(&envelope(event), BOT) {
            Frame::Event {
                envelope_id,
                event_id,
                message,
            } => {
                assert_eq!(envelope_id, "env-1");
                assert_eq!(event_id.as_deref(), Some("Ev1"));
                message
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_direct_message_and_a_mention_are_read_and_nothing_else() {
        assert_eq!(
            read(dm(" a &lt;b&gt; &amp; c ")),
            Some(Incoming {
                sender: "U0ME".into(),
                channel: "D0DM".into(),
                thread_ts: None,
                text: "a <b> & c".into(),
            })
        );
        let mention = read(json!({
            "type": "app_mention", "channel": "C0GEN", "user": "U0ME",
            "text": "<@U0BOT> what's up", "ts": "1700000000.000200"
        }))
        .unwrap();
        assert_eq!(mention.text, "what's up");
        assert_eq!(mention.reply_to(), "C0GEN:1700000000.000200");
        // A mention inside a thread answers in that thread.
        let in_thread = read(json!({
            "type": "app_mention", "channel": "C0GEN", "user": "U0ME",
            "text": "<@U0BOT> again", "ts": "1700000000.000300", "thread_ts": "1700000000.000200"
        }))
        .unwrap();
        assert_eq!(in_thread.reply_to(), "C0GEN:1700000000.000200");

        let mut edited = dm("hi");
        edited["subtype"] = json!("message_changed");
        let mut from_bot = dm("hi");
        from_bot["bot_id"] = json!("B0X");
        let mut from_self = dm("hi");
        from_self["user"] = json!(BOT);
        let mut channel_chatter = dm("hi");
        channel_chatter["channel_type"] = json!("channel");
        for skipped in [
            edited,
            from_bot,
            from_self,
            channel_chatter,
            dm("   "),
            json!({ "type": "app_mention", "channel": "C0GEN", "user": "U0ME", "text": "<@U0BOT>", "ts": "1.2" }),
            json!({ "type": "reaction_added", "user": "U0ME" }),
        ] {
            assert_eq!(read(skipped.clone()), None, "{skipped}");
        }
    }

    #[test]
    fn hello_disconnect_and_other_envelopes_are_told_apart() {
        assert_eq!(read_frame(&json!({ "type": "hello" }), BOT), Frame::Hello);
        assert_eq!(
            read_frame(
                &json!({ "type": "disconnect", "reason": "refresh_requested" }),
                BOT
            ),
            Frame::Disconnect { fatal: false }
        );
        assert_eq!(
            read_frame(
                &json!({ "type": "disconnect", "reason": "link_disabled" }),
                BOT
            ),
            Frame::Disconnect { fatal: true }
        );
        // An envelope that isn't an event is still acknowledged.
        assert_eq!(
            read_frame(
                &json!({ "type": "slash_commands", "envelope_id": "env-9" }),
                BOT
            ),
            Frame::Event {
                envelope_id: "env-9".into(),
                event_id: None,
                message: None
            }
        );
        assert_eq!(
            read_frame(&json!({ "type": "events_api" }), BOT),
            Frame::Other
        );
        assert_eq!(ack("env-1"), json!({ "envelope_id": "env-1" }));
    }

    #[test]
    fn a_stranger_is_paired_only_by_direct_message() {
        let mut trust = DmTrust::default();
        let msg = read(dm("hello")).unwrap();
        let Step::Reply { channel, text } = step(&mut trust, msg.clone(), NOW) else {
            panic!("a stranger's DM gets the code");
        };
        assert_eq!(channel, "D0DM");
        let code = trust.pending[0].code.clone();
        assert_eq!(text, crate::msg_telegram::pairing_text(&code));
        assert_eq!(step(&mut trust, msg.clone(), NOW + 1), Step::Drop);
        // A stranger's mention in a channel hears nothing at all.
        let mention = Incoming {
            sender: "U0OTHER".into(),
            channel: "C0GEN".into(),
            thread_ts: Some("1.2".into()),
            text: "hi".into(),
        };
        assert_eq!(step(&mut trust, mention.clone(), NOW + 2), Step::Drop);
        assert_eq!(trust.pending.len(), 1);
        trust.approve(&code, NOW + 3).unwrap();
        assert_eq!(step(&mut trust, msg.clone(), NOW + 4), Step::Forward(msg));
        let paired_mention = Incoming {
            sender: "U0ME".into(),
            ..mention
        };
        assert_eq!(
            step(&mut trust, paired_mention.clone(), NOW + 5),
            Step::Forward(paired_mention)
        );
    }

    #[test]
    fn tokens_replies_and_the_manifest_have_slacks_shape() {
        assert_eq!(
            split_tokens(&format!(" {BOT_TOKEN}\n{APP_TOKEN} ")),
            Some((APP_TOKEN, BOT_TOKEN))
        );
        assert_eq!(
            split_tokens(&format!("{APP_TOKEN} {BOT_TOKEN}")),
            Some((APP_TOKEN, BOT_TOKEN))
        );
        for bad in [
            APP_TOKEN.to_string(),
            format!("{APP_TOKEN} {APP_TOKEN}"),
            format!("{APP_TOKEN} xoxp-0123456789-0123456789-abcdef"),
            format!("{APP_TOKEN} xoxb-short"),
            format!("{APP_TOKEN} {BOT_TOKEN} extra"),
            format!("{APP_TOKEN} xoxb-0123456789/0123456789-abcdef"),
        ] {
            assert_eq!(split_tokens(&bad), None, "{bad}");
        }
        assert_eq!(
            post_body("D0DM", None, "a <!channel> & b"),
            json!({ "channel": "D0DM", "text": "a &lt;!channel&gt; &amp; b", "unfurl_links": false })
        );
        assert_eq!(
            post_body("C0GEN", Some("1.2"), "x")["thread_ts"],
            json!("1.2")
        );
        assert_eq!(split_reply_to("D0DM"), Some(("D0DM", None)));
        assert_eq!(
            split_reply_to("C0GEN:1700000000.000200"),
            Some(("C0GEN", Some("1700000000.000200")))
        );
        assert_eq!(split_reply_to("C0GEN:abc"), None);
        assert_eq!(split_reply_to("c0gen"), None);

        let m = manifest("Loadout");
        assert_eq!(m["settings"]["socket_mode_enabled"], json!(true));
        assert_eq!(
            m["oauth_config"]["scopes"]["bot"],
            json!(["app_mentions:read", "chat:write", "im:history"])
        );
        assert_eq!(
            m["settings"]["event_subscriptions"]["bot_events"],
            json!(["app_mention", "message.im"])
        );
        assert_eq!(api_error(&json!({ "ok": true })), None);
        assert_eq!(
            api_error(&json!({ "ok": false, "error": "invalid_auth" })).as_deref(),
            Some("Slack refused the tokens. Copy them again from your Slack app.")
        );
        assert_eq!(
            api_error(&json!({ "ok": false, "error": "weird <b>" })).as_deref(),
            Some("Slack said weirdb.")
        );
    }
}
