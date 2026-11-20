//! Messaging channels, part 6d: approval asks through paired chats.
//!
//! The user picks, in the app, which paired senders get asks. Each waiting
//! decision from the decision inbox goes to them with a short code, and
//! they answer "yes CODE" or "no CODE". Answers go through the same path as
//! the inbox row (`answer_inbox`), so a chat can't do more than the row:
//! no "Always", and a hard stop (a hard class or a harness park) can only
//! be denied from a chat. Allowing one stays on the computer.
//!
//! Plan, question and form asks need their chat open, so they stay off chats.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use grokhub_core::msg_channels::{ChannelKind, DmTrust};

use crate::channel_inbox::ReplyTo;
use crate::inbox::Decision;

/// Code letters: no 0/O or 1/I, so a code reads back right.
const CODE_LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 4;

fn approvers_path(config_dir: &Path) -> PathBuf {
    config_dir.join("channels").join("approvers.json")
}

fn reach_path(config_dir: &Path) -> PathBuf {
    config_dir.join("channels").join("reach.json")
}

fn kind_key(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::Telegram => "telegram",
        ChannelKind::Discord => "discord",
    }
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(path, &text).map_err(|e| e.to_string())
}

/// Senders the user turned asks on for, per channel.
type Approvers = BTreeMap<String, BTreeSet<String>>;
/// The chat each sender last wrote from, per channel.
type Reach = BTreeMap<String, BTreeMap<String, String>>;

pub fn is_approver(config_dir: &Path, kind: ChannelKind, sender: &str) -> bool {
    let approvers: Approvers = read_json(&approvers_path(config_dir));
    approvers
        .get(kind_key(kind))
        .is_some_and(|s| s.contains(sender))
}

/// Turn asks on or off for one sender.
pub fn set_approver(
    config_dir: &Path,
    kind: ChannelKind,
    sender: &str,
    on: bool,
) -> Result<(), String> {
    let path = approvers_path(config_dir);
    let mut approvers: Approvers = read_json(&path);
    let set = approvers.entry(kind_key(kind).to_string()).or_default();
    if on {
        set.insert(sender.to_string());
    } else {
        set.remove(sender);
    }
    write_json(&path, &approvers)
}

/// Note the chat a sender wrote from, so asks can reach them.
pub fn remember_reach(config_dir: &Path, to: &ReplyTo) {
    let path = reach_path(config_dir);
    let mut reach: Reach = read_json(&path);
    let chats = reach.entry(kind_key(to.channel).to_string()).or_default();
    if chats.get(&to.sender) != Some(&to.to) {
        chats.insert(to.sender.clone(), to.to.clone());
        let _ = write_json(&path, &reach);
    }
}

/// True when asks have a chat to go to for this sender.
pub fn reachable(config_dir: &Path, kind: ChannelKind, sender: &str) -> bool {
    let reach: Reach = read_json(&reach_path(config_dir));
    reach
        .get(kind_key(kind))
        .is_some_and(|c| c.contains_key(sender))
}

/// Where asks go now: senders with asks on, still paired, who have written.
pub fn targets(config_dir: &Path) -> Vec<ReplyTo> {
    let approvers: Approvers = read_json(&approvers_path(config_dir));
    let reach: Reach = read_json(&reach_path(config_dir));
    let trust = DmTrust::load(config_dir);
    let mut out = Vec::new();
    for kind in [ChannelKind::Telegram, ChannelKind::Discord] {
        let key = kind_key(kind);
        for sender in approvers.get(key).into_iter().flatten() {
            if !trust.is_paired(kind, sender) {
                continue;
            }
            if let Some(to) = reach.get(key).and_then(|c| c.get(sender)) {
                out.push(ReplyTo {
                    channel: kind,
                    sender: sender.clone(),
                    to: to.clone(),
                });
            }
        }
    }
    out
}

/// One decision sent to chats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub id: String,
    /// "permission" or "park", as `answer_inbox` reads it.
    pub kind: String,
    pub chat_id: String,
    pub what: String,
    pub hard: bool,
    pub code: String,
}

/// What a chat reply comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Answer this ask through `answer_inbox`.
    Answer { ask: Ask, approve: bool },
    /// Nothing to answer: say this back.
    Said(String),
}

/// The asks out on chats, while they wait.
#[derive(Debug, Default)]
pub struct RemoteAsks {
    sent: Vec<Ask>,
}

fn ask_text(ask: &Ask) -> String {
    let app = grokhub_core::brand::product_name();
    let code = &ask.code;
    if ask.hard {
        format!(
            "{app} is asking: {}\nThis is a hard stop, so only the computer can allow it. Reply \"no {code}\" to deny it.",
            ask.what
        )
    } else {
        format!(
            "{app} is asking: {}\nReply \"yes {code}\" to allow it once, or \"no {code}\" to deny it.",
            ask.what
        )
    }
}

/// A fresh code from `random`, a source of random bytes.
pub fn new_code(random: &mut dyn FnMut(&mut [u8])) -> String {
    let mut bytes = [0u8; CODE_LEN];
    random(&mut bytes);
    bytes
        .iter()
        .map(|b| CODE_LETTERS[*b as usize % CODE_LETTERS.len()] as char)
        .collect()
}

impl RemoteAsks {
    /// Match what waits now: asks answered or gone elsewhere are dropped,
    /// and each new one gets a code. Returns the texts to send.
    pub fn sync(&mut self, waiting: &[Decision], random: &mut dyn FnMut(&mut [u8])) -> Vec<String> {
        self.sent
            .retain(|ask| waiting.iter().any(|d| d.id == ask.id && d.kind == ask.kind));
        let mut texts = Vec::new();
        for d in waiting {
            if d.kind == "elicit" || self.sent.iter().any(|a| a.id == d.id && a.kind == d.kind) {
                continue;
            }
            let code = loop {
                let code = new_code(random);
                if !self.sent.iter().any(|a| a.code == code) {
                    break code;
                }
            };
            let ask = Ask {
                id: d.id.clone(),
                kind: d.kind.clone(),
                chat_id: d.chat_id.clone(),
                what: d.what.clone(),
                hard: d.hard || d.kind == "park",
                code,
            };
            texts.push(ask_text(&ask));
            self.sent.push(ask);
        }
        texts
    }

    /// Read a chat message as an answer. `None` when it isn't shaped like
    /// one ("yes CODE" / "no CODE"), so it goes on to the assistant.
    pub fn reply(&mut self, text: &str) -> Option<Reply> {
        let mut words = text.split_whitespace();
        let (verb, code, None) = (words.next()?, words.next()?, words.next()) else {
            return None;
        };
        let approve = match verb.to_ascii_lowercase().as_str() {
            "yes" | "allow" | "approve" => true,
            "no" | "deny" => false,
            _ => return None,
        };
        let code = code.to_ascii_uppercase();
        if code.len() != CODE_LEN || !code.bytes().all(|b| CODE_LETTERS.contains(&b)) {
            return None;
        }
        let Some(at) = self.sent.iter().position(|a| a.code == code) else {
            return Some(Reply::Said(format!(
                "No ask is waiting with code {code}. It may have been answered already."
            )));
        };
        if approve && self.sent[at].hard {
            return Some(Reply::Said(format!(
                "That one is a hard stop, so only the computer can allow it. Reply \"no {code}\" to deny it."
            )));
        }
        let ask = self.sent.remove(at);
        Some(Reply::Answer { ask, approve })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(id: &str, kind: &str, hard: bool) -> Decision {
        Decision {
            id: id.into(),
            kind: kind.into(),
            chat_id: if kind == "park" {
                String::new()
            } else {
                "chat-1".into()
            },
            what: format!("run step {id}"),
            hard,
        }
    }

    /// Bytes that pick letters A, B, C, D, then E, F, G, H, and so on.
    fn counting() -> impl FnMut(&mut [u8]) {
        let mut next = 0u8;
        move |buf: &mut [u8]| {
            for b in buf {
                *b = next;
                next = next.wrapping_add(1);
            }
        }
    }

    #[test]
    fn each_waiting_ask_goes_out_once_with_its_own_code() {
        let app = grokhub_core::brand::product_name();
        let mut asks = RemoteAsks::default();
        let mut random = counting();
        let waiting = vec![
            decision("call-1", "permission", false),
            decision("park-1", "park", true),
            decision("form-1", "elicit", false),
        ];
        assert_eq!(
            asks.sync(&waiting, &mut random),
            vec![
                format!(
                    "{app} is asking: run step call-1\nReply \"yes ABCD\" to allow it once, or \"no ABCD\" to deny it."
                ),
                format!(
                    "{app} is asking: run step park-1\nThis is a hard stop, so only the computer can allow it. Reply \"no EFGH\" to deny it."
                ),
            ]
        );
        assert_eq!(asks.sync(&waiting, &mut random), Vec::<String>::new());
    }

    #[test]
    fn yes_allows_a_soft_ask_once_and_its_code_is_spent() {
        let mut asks = RemoteAsks::default();
        asks.sync(&[decision("call-1", "permission", false)], &mut counting());
        assert_eq!(
            asks.reply("Yes abcd"),
            Some(Reply::Answer {
                ask: Ask {
                    id: "call-1".into(),
                    kind: "permission".into(),
                    chat_id: "chat-1".into(),
                    what: "run step call-1".into(),
                    hard: false,
                    code: "ABCD".into(),
                },
                approve: true,
            })
        );
        assert_eq!(
            asks.reply("yes ABCD"),
            Some(Reply::Said(
                "No ask is waiting with code ABCD. It may have been answered already.".into()
            ))
        );
    }

    #[test]
    fn a_hard_stop_can_be_denied_from_a_chat_but_never_allowed() {
        let mut asks = RemoteAsks::default();
        asks.sync(&[decision("call-9", "permission", true)], &mut counting());
        assert_eq!(
            asks.reply("allow ABCD"),
            Some(Reply::Said(
                "That one is a hard stop, so only the computer can allow it. Reply \"no ABCD\" to deny it.".into()
            ))
        );
        let Some(Reply::Answer { ask, approve }) = asks.reply("no abcd") else {
            panic!("the deny should answer the ask");
        };
        assert_eq!(
            (ask.id.as_str(), ask.hard, approve),
            ("call-9", true, false)
        );
    }

    #[test]
    fn other_messages_go_on_to_the_assistant() {
        let mut asks = RemoteAsks::default();
        asks.sync(&[decision("call-1", "permission", false)], &mut counting());
        assert_eq!(asks.reply("yes please tell me the weather"), None);
        assert_eq!(asks.reply("no worries"), None);
        assert_eq!(asks.reply("is the build done?"), None);
    }

    #[test]
    fn an_ask_answered_on_the_computer_is_dropped() {
        let mut asks = RemoteAsks::default();
        asks.sync(&[decision("call-1", "permission", false)], &mut counting());
        asks.sync(&[], &mut counting());
        assert_eq!(
            asks.reply("yes ABCD"),
            Some(Reply::Said(
                "No ask is waiting with code ABCD. It may have been answered already.".into()
            ))
        );
    }

    #[test]
    fn asks_go_only_to_chosen_still_paired_senders_who_have_written() {
        let dir = std::env::temp_dir().join(format!("grokhub-asks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut trust = DmTrust::default();
        let paired = trust.paired.entry(ChannelKind::Telegram).or_default();
        paired.insert("777".into());
        paired.insert("888".into());
        trust.save(&dir).unwrap();
        let reply_to = |sender: &str, to: &str| ReplyTo {
            channel: ChannelKind::Telegram,
            sender: sender.into(),
            to: to.into(),
        };
        set_approver(&dir, ChannelKind::Telegram, "777", true).unwrap();
        set_approver(&dir, ChannelKind::Telegram, "999", true).unwrap();
        assert_eq!(targets(&dir), vec![]);
        assert!(!reachable(&dir, ChannelKind::Telegram, "777"));

        remember_reach(&dir, &reply_to("777", "5001"));
        remember_reach(&dir, &reply_to("888", "5002"));
        remember_reach(&dir, &reply_to("999", "5003"));
        assert!(reachable(&dir, ChannelKind::Telegram, "777"));
        assert!(is_approver(&dir, ChannelKind::Telegram, "777"));
        assert!(!is_approver(&dir, ChannelKind::Telegram, "888"));
        // 888 never had asks turned on; 999 isn't paired.
        assert_eq!(targets(&dir), vec![reply_to("777", "5001")]);

        set_approver(&dir, ChannelKind::Telegram, "777", false).unwrap();
        assert_eq!(targets(&dir), vec![]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
