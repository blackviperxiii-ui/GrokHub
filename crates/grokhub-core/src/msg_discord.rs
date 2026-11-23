//! Messaging channels, part 4: Discord, without the network. This reads
//! a Gateway `MESSAGE_CREATE` event and decides it through [`DmTrust`], as
//! Telegram does: only direct messages (no `guild_id`) from people are
//! read; a paired sender's text is handed on, and a new sender gets one
//! pairing code. [`Gateway`] is the session's state (hello, identify or
//! resume, heartbeats); the socket itself lives in the agent crate.

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

/// The Gateway address, JSON frames, API v10.
pub const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
/// `DIRECT_MESSAGES` only: no server channels, and DM text needs no
/// privileged intent.
pub const INTENTS: u64 = 1 << 12;

/// What the socket should do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Do {
    /// Send this frame.
    Send(Value),
    /// A direct message arrived; decide it with [`step`].
    Message(Incoming),
    /// Close and connect again: to the resume address with a Resume when
    /// `resume`, else afresh with an Identify.
    Reconnect { resume: bool },
}

/// One bot's Gateway session. Not `Debug`: Identify and Resume carry the token.
#[derive(Default)]
pub struct Gateway {
    seq: Option<i64>,
    interval_ms: Option<u64>,
    next_beat_ms: u64,
    acked: bool,
    session_id: Option<String>,
    resume_url: Option<String>,
}

impl Gateway {
    /// Where the next connection goes: the resume address once Ready named one.
    pub fn url(&self) -> String {
        match &self.resume_url {
            Some(base) => format!("{}/?v=10&encoding=json", base.trim_end_matches('/')),
            None => GATEWAY_URL.to_string(),
        }
    }

    /// A new connection is about to open: heartbeats wait for its Hello.
    pub fn connecting(&mut self) {
        self.interval_ms = None;
        self.acked = true;
    }

    fn beat(&self) -> Value {
        json!({ "op": 1, "d": self.seq })
    }

    /// One frame from the Gateway.
    pub fn on_frame(&mut self, frame: &Value, token: &str, now_ms: u64) -> Vec<Do> {
        let op = frame.get("op").and_then(Value::as_i64).unwrap_or(-1);
        match op {
            0 => {
                if let Some(s) = frame.get("s").and_then(Value::as_i64) {
                    self.seq = Some(s);
                }
                if frame.get("t").and_then(Value::as_str) == Some("READY") {
                    let d = &frame["d"];
                    self.session_id = d["session_id"].as_str().map(str::to_string);
                    // Only Discord's own hosts are dialed again.
                    self.resume_url = d["resume_gateway_url"]
                        .as_str()
                        .filter(|u| {
                            u.strip_prefix("wss://").is_some_and(|host| {
                                let host = host.split(['/', '?']).next().unwrap_or("");
                                host == "gateway.discord.gg" || host.ends_with(".discord.gg")
                            })
                        })
                        .map(str::to_string);
                }
                direct_message(frame).map(Do::Message).into_iter().collect()
            }
            1 => vec![Do::Send(self.beat())],
            7 => vec![Do::Reconnect { resume: true }],
            9 => {
                let resumable = frame.get("d").and_then(Value::as_bool).unwrap_or(false);
                if !resumable {
                    self.forget_session();
                }
                vec![Do::Reconnect { resume: resumable }]
            }
            10 => {
                let interval = frame
                    .pointer("/d/heartbeat_interval")
                    .and_then(Value::as_u64)
                    .unwrap_or(41_250)
                    .clamp(1_000, 120_000);
                self.interval_ms = Some(interval);
                self.acked = true;
                // The first beat comes part way into the interval.
                self.next_beat_ms = now_ms + interval / 4;
                let hello = match (&self.session_id, self.seq) {
                    (Some(session), Some(seq)) => json!({
                        "op": 6,
                        "d": { "token": token, "session_id": session, "seq": seq },
                    }),
                    _ => json!({
                        "op": 2,
                        "d": {
                            "token": token,
                            "intents": INTENTS,
                            "properties": { "os": std::env::consts::OS, "browser": "GrokHub", "device": "GrokHub" },
                        },
                    }),
                };
                vec![Do::Send(hello)]
            }
            11 => {
                self.acked = true;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Time passed with no frame. A beat that was never acknowledged means
    /// the connection is dead: reconnect and resume.
    pub fn on_tick(&mut self, now_ms: u64) -> Vec<Do> {
        let Some(interval) = self.interval_ms else {
            return Vec::new();
        };
        if now_ms < self.next_beat_ms {
            return Vec::new();
        }
        if !self.acked {
            return vec![Do::Reconnect { resume: true }];
        }
        self.acked = false;
        self.next_beat_ms = now_ms + interval;
        vec![Do::Send(self.beat())]
    }

    /// The session can't be resumed: the next connection identifies afresh.
    pub fn forget_session(&mut self) {
        self.session_id = None;
        self.resume_url = None;
        self.seq = None;
    }
}

/// What a Gateway close code means: `Err` with the reason when connecting
/// again can't help (a bad token, intents the app may not ask for).
pub fn close_reason(code: u16) -> Result<(), String> {
    match code {
        4004 => Err("Discord refused the bot token. Copy it again from the Bot page.".into()),
        4013 | 4014 => Err(
            "Discord won't let this bot read direct messages. Check its intents on the Bot page."
                .into(),
        ),
        4010..=4012 => Err(format!(
            "Discord closed the connection for good (code {code})."
        )),
        _ => Ok(()),
    }
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

    const TOKEN: &str = "MTIz.abc_DEF.ghi-jkl";

    fn sent(d: Vec<Do>) -> Value {
        match d.as_slice() {
            [Do::Send(v)] => v.clone(),
            other => panic!("expected one frame to send, got {other:?}"),
        }
    }

    #[test]
    fn hello_identifies_then_beats_on_time() {
        let mut gw = Gateway::default();
        gw.connecting();
        assert_eq!(gw.url(), GATEWAY_URL);
        assert_eq!(gw.on_tick(NOW), vec![]);
        let identify = sent(gw.on_frame(
            &json!({ "op": 10, "d": { "heartbeat_interval": 40_000 } }),
            TOKEN,
            NOW,
        ));
        assert_eq!(identify["op"], 2);
        assert_eq!(identify["d"]["token"], TOKEN);
        assert_eq!(identify["d"]["intents"], 4096);
        // The first beat is a quarter in, then one per interval.
        assert_eq!(gw.on_tick(NOW + 9_999), vec![]);
        assert_eq!(
            sent(gw.on_tick(NOW + 10_000)),
            json!({ "op": 1, "d": null })
        );
        assert_eq!(
            gw.on_frame(&json!({ "op": 11 }), TOKEN, NOW + 10_100),
            vec![]
        );
        let ready = json!({
            "op": 0, "t": "READY", "s": 1,
            "d": { "session_id": "abc", "resume_gateway_url": "wss://gateway-us-east1-b.discord.gg" },
        });
        assert_eq!(gw.on_frame(&ready, TOKEN, NOW + 10_200), vec![]);
        assert_eq!(gw.on_tick(NOW + 49_999), vec![]);
        assert_eq!(sent(gw.on_tick(NOW + 50_000)), json!({ "op": 1, "d": 1 }));
        // No ack before the next beat: the connection is a zombie.
        assert_eq!(
            gw.on_tick(NOW + 90_000),
            vec![Do::Reconnect { resume: true }]
        );
    }

    #[test]
    fn a_dropped_connection_resumes_and_an_invalid_session_starts_over() {
        let mut gw = Gateway::default();
        gw.connecting();
        gw.on_frame(
            &json!({ "op": 10, "d": { "heartbeat_interval": 40_000 } }),
            TOKEN,
            NOW,
        );
        gw.on_frame(
            &json!({ "op": 0, "t": "READY", "s": 1,
                "d": { "session_id": "abc", "resume_gateway_url": "wss://gateway-us-east1-b.discord.gg" } }),
            TOKEN,
            NOW,
        );
        let msg = gw.on_frame(&dm("hello"), TOKEN, NOW);
        assert!(matches!(msg.as_slice(), [Do::Message(m)] if m.text == "hello"));
        assert_eq!(
            gw.on_frame(&json!({ "op": 7 }), TOKEN, NOW),
            vec![Do::Reconnect { resume: true }]
        );
        assert_eq!(
            gw.url(),
            "wss://gateway-us-east1-b.discord.gg/?v=10&encoding=json"
        );
        gw.connecting();
        let resume = sent(gw.on_frame(
            &json!({ "op": 10, "d": { "heartbeat_interval": 40_000 } }),
            TOKEN,
            NOW,
        ));
        assert_eq!(
            resume,
            json!({ "op": 6, "d": { "token": TOKEN, "session_id": "abc", "seq": 7 } })
        );
        assert_eq!(
            gw.on_frame(&json!({ "op": 9, "d": false }), TOKEN, NOW),
            vec![Do::Reconnect { resume: false }]
        );
        assert_eq!(gw.url(), GATEWAY_URL);
        gw.connecting();
        let identify = sent(gw.on_frame(
            &json!({ "op": 10, "d": { "heartbeat_interval": 40_000 } }),
            TOKEN,
            NOW,
        ));
        assert_eq!(identify["op"], 2);
    }

    #[test]
    fn a_resume_address_off_discord_is_never_dialed() {
        let mut gw = Gateway::default();
        gw.on_frame(
            &json!({ "op": 0, "t": "READY", "s": 1,
                "d": { "session_id": "abc", "resume_gateway_url": "wss://evil.example/discord.gg" } }),
            TOKEN,
            NOW,
        );
        assert_eq!(gw.url(), GATEWAY_URL);
        assert_eq!(close_reason(1001), Ok(()));
        assert_eq!(close_reason(4000), Ok(()));
        assert_eq!(
            close_reason(4004),
            Err("Discord refused the bot token. Copy it again from the Bot page.".into())
        );
        assert!(close_reason(4014).is_err());
    }
}
