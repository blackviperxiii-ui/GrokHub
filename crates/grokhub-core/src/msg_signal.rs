//! Messaging channels, part 8: Signal, without the network. The app talks to
//! a signal-cli daemon the person runs on this computer, in its HTTP mode,
//! with signal-cli linked to their own Signal account as an extra device.
//!
//! Because that device speaks as the person's own number, the app never
//! answers anyone else: a pairing code sent from it would reach a stranger
//! from the person's number. So the only conversation it reads is the
//! person's own Note to Self, and its answers go back there. Messages from
//! everyone else, and every group, are dropped without a word.
//!
//! signal-cli's daemon shape, from its `signal-cli-jsonrpc(5)` page: JSON-RPC
//! at `POST /api/v1/rpc`, received messages as server-sent events at
//! `GET /api/v1/events`, and `GET /api/v1/check` answering 200 while it runs.
//! The daemon is started without `-a`, so every call names the account.

use serde_json::{json, Value};

/// Where signal-cli's HTTP daemon listens when the person follows the setup
/// line.
pub const DEFAULT_DAEMON: &str = "http://127.0.0.1:8080";
/// The command the setup line tells the person to run.
pub const DAEMON_COMMAND: &str = "signal-cli daemon --http 127.0.0.1:8080";
/// The label a Note to Self message is queued under.
pub const SELF_LABEL: &str = "Note to Self";
/// Longest answer sent, in characters. signal-cli sends long text as an
/// attachment on its own, so this only keeps one answer reasonable.
pub const MAX_TEXT: usize = 12_000;

/// The account and the daemon it is reached through, as saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The account's number, `+` and digits.
    pub account: String,
    /// `http://127.0.0.1:8080` or another loopback address, no path.
    pub daemon: String,
}

impl Config {
    /// What the keychain keeps: the number, then the daemon when it isn't
    /// the default.
    pub fn saved(&self) -> String {
        if self.daemon == DEFAULT_DAEMON {
            self.account.clone()
        } else {
            format!("{} {}", self.account, self.daemon)
        }
    }

    pub fn rpc_url(&self) -> String {
        format!("{}/api/v1/rpc", self.daemon)
    }

    pub fn events_url(&self) -> String {
        format!("{}/api/v1/events", self.daemon)
    }

    pub fn check_url(&self) -> String {
        format!("{}/api/v1/check", self.daemon)
    }
}

/// The setup box's text read: a phone number in international form, with an
/// optional daemon address after it. Spaces, dashes, dots and brackets in
/// the number are dropped.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let text = text.trim();
    let (number, daemon) = match text.rfind(|c: char| c.is_whitespace()) {
        Some(at) if text[at..].trim_start().starts_with("http") => {
            (&text[..at], daemon_url(text[at..].trim())?)
        }
        _ => (text, DEFAULT_DAEMON.to_string()),
    };
    let account: String = number
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')'))
        .collect();
    if !valid_number(&account) {
        return Err("That isn't a phone number. Type the number your Signal account uses, starting with + and the country code.".into());
    }
    Ok(Config { account, daemon })
}

/// `+` and 7 to 15 digits, the first not 0.
pub fn valid_number(s: &str) -> bool {
    let Some(digits) = s.strip_prefix('+') else {
        return false;
    };
    (7..=15).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0')
}

/// A daemon address on this computer: `http`, a loopback host and a port,
/// with nothing after it. `localhost` becomes `127.0.0.1`, so the name can't
/// resolve somewhere else.
pub fn daemon_url(text: &str) -> Result<String, String> {
    const REFUSED: &str =
        "The Signal daemon must run on this computer: use an address like http://127.0.0.1:8080.";
    let rest = text
        .trim()
        .trim_end_matches('/')
        .strip_prefix("http://")
        .ok_or(REFUSED)?;
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or(REFUSED)?;
        (h, after.strip_prefix(':').ok_or(REFUSED)?)
    } else {
        rest.rsplit_once(':').ok_or(REFUSED)?
    };
    let port: u16 = port.parse().ok().filter(|p| *p != 0).ok_or(REFUSED)?;
    let host = match host {
        "localhost" | "127.0.0.1" => "127.0.0.1".to_string(),
        "::1" => "[::1]".to_string(),
        h => match h.parse::<std::net::Ipv4Addr>() {
            Ok(ip) if ip.is_loopback() => ip.to_string(),
            _ => return Err(REFUSED.into()),
        },
    };
    Ok(format!("http://{host}:{port}"))
}

/// One JSON-RPC request. Every call names the account, as a daemon started
/// without `-a` needs.
pub fn rpc(id: u64, method: &str, account: Option<&str>, mut params: Value) -> Value {
    if let (Some(account), Some(map)) = (account, params.as_object_mut()) {
        map.insert("account".into(), json!(account));
    }
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// The `result` of a JSON-RPC answer, or its error worded for the person.
pub fn rpc_result(answer: &Value, method: &str) -> Result<Value, String> {
    if let Some(err) = answer.get("error") {
        let said = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("no reason given");
        let said: String = said.chars().take(200).collect();
        return Err(format!("signal-cli refused {method}: {said}"));
    }
    answer
        .get("result")
        .cloned()
        .ok_or_else(|| format!("signal-cli's {method} answer has no result."))
}

/// The `send` parameters for an answer to the account's own Note to Self.
pub fn send_params(account: &str, text: &str) -> Value {
    let text: String = text.chars().take(MAX_TEXT).collect();
    json!({"recipient": [account], "message": text})
}

/// The numbers `listAccounts` answered with.
pub fn listed_accounts(result: &Value) -> Vec<String> {
    result
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a.get("number").or(Some(a)).and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// One server-sent event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    pub id: Option<String>,
    pub data: String,
}

/// Reads server-sent events one line at a time. Each line is fed as read,
/// without its line ending; a blank line ends an event.
#[derive(Debug, Default)]
pub struct EventReader {
    id: Option<String>,
    data: Vec<String>,
    /// The last event id, sent back as `Last-Event-ID` on a reconnect so the
    /// daemon replays what was missed.
    pub last_id: Option<String>,
}

/// Longest `data` kept for one event, so a broken stream can't fill memory.
const MAX_EVENT: usize = 1 << 20;

impl EventReader {
    pub fn line(&mut self, line: &str) -> Option<Event> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            if self.data.is_empty() {
                self.id = None;
                return None;
            }
            let event = Event {
                id: self.id.take(),
                data: std::mem::take(&mut self.data).join("\n"),
            };
            if event.id.is_some() {
                self.last_id.clone_from(&event.id);
            }
            return Some(event);
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "data" if self.data.iter().map(String::len).sum::<usize>() < MAX_EVENT => {
                self.data.push(value.to_string());
            }
            "id" if !value.contains('\0') => self.id = Some(value.to_string()),
            _ => {}
        }
        None
    }
}

/// The envelope inside one event's data: a bare envelope wrapper, a
/// `receive` notification, or one in subscribe form.
fn envelope_of(data: &Value) -> Option<(&Value, Option<&str>)> {
    let params = data.get("params").unwrap_or(data);
    let holder = params.get("result").unwrap_or(params);
    let envelope = holder.get("envelope")?;
    let account = holder
        .get("account")
        .or_else(|| params.get("account"))
        .or_else(|| data.get("account"))
        .and_then(Value::as_str);
    Some((envelope, account))
}

/// The text the person wrote to their own Note to Self, from one event's
/// data. Anything else is `None`: other people, groups, reactions, receipts,
/// typing, edits, attachments with no text, and another account's mail.
pub fn note_to_self(data: &Value, account: &str) -> Option<String> {
    let (envelope, said_account) = envelope_of(data)?;
    if said_account.is_some_and(|a| a != account) {
        return None;
    }
    let source = envelope
        .get("sourceNumber")
        .or_else(|| envelope.get("source"))
        .and_then(Value::as_str)?;
    if source != account {
        return None;
    }
    // From a phone or desktop linked to the same account: a sync copy of
    // what it sent. Only one sent to the account itself counts.
    let message = if let Some(sent) = envelope
        .get("syncMessage")
        .and_then(|s| s.get("sentMessage"))
    {
        let to = sent
            .get("destinationNumber")
            .or_else(|| sent.get("destination"))
            .and_then(Value::as_str);
        let to_uuid = sent.get("destinationUuid").and_then(Value::as_str);
        let from_uuid = envelope.get("sourceUuid").and_then(Value::as_str);
        let to_self = to == Some(account) || (to_uuid.is_some() && to_uuid == from_uuid);
        if !to_self {
            return None;
        }
        sent
    } else {
        // A message the account sent itself with no sync wrapper.
        envelope.get("dataMessage")?
    };
    if message.get("groupInfo").is_some() || message.get("groupV2").is_some() {
        return None;
    }
    let text = message.get("message").and_then(Value::as_str)?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// What a `startLink` answer holds: the `sgnl://linkdevice` address the
/// phone scans. Anything else is refused, so the QR can only ever link.
pub fn link_uri(result: &Value) -> Result<String, String> {
    let uri = result
        .get("deviceLinkUri")
        .and_then(Value::as_str)
        .ok_or("signal-cli's startLink answer has no link.")?;
    let ok = uri.starts_with("sgnl://linkdevice?")
        && uri.len() <= 1024
        && uri.bytes().all(|b| b.is_ascii_graphic());
    if ok {
        Ok(uri.to_string())
    } else {
        Err("signal-cli gave a link that isn't a device link.".into())
    }
}

/// The number `finishLink` answered with once the phone confirmed.
pub fn linked_number(result: &Value) -> Result<String, String> {
    result
        .get("number")
        .and_then(Value::as_str)
        .filter(|n| valid_number(n))
        .map(str::to_string)
        .ok_or_else(|| "signal-cli finished linking but gave no phone number.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "+15551234567";

    fn sync_to(dest: &str, text: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "receive",
            "params": {
                "envelope": {
                    "source": ME, "sourceNumber": ME, "sourceUuid": "aci-me",
                    "sourceDevice": 1, "timestamp": 1,
                    "syncMessage": {"sentMessage": {
                        "destination": dest, "destinationNumber": dest,
                        "destinationUuid": if dest == ME { "aci-me" } else { "aci-other" },
                        "timestamp": 1, "message": text
                    }}
                },
                "account": ME
            }
        })
    }

    #[test]
    fn the_setup_box_takes_a_number_and_an_optional_local_daemon() {
        let c = parse_config(" +1 (555) 123-4567 ").unwrap();
        assert_eq!(c.account, ME);
        assert_eq!(c.daemon, DEFAULT_DAEMON);
        assert_eq!(c.saved(), ME);
        let c = parse_config("+15551234567 http://localhost:9000/").unwrap();
        assert_eq!(c.daemon, "http://127.0.0.1:9000");
        assert_eq!(c.saved(), "+15551234567 http://127.0.0.1:9000");
        assert_eq!(parse_config(&c.saved()).unwrap(), c);
        assert_eq!(c.rpc_url(), "http://127.0.0.1:9000/api/v1/rpc");
        for bad in ["5551234567", "+0123456789", "+12", "+1555abc4567", ""] {
            assert!(parse_config(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_a_loopback_daemon_is_taken() {
        assert_eq!(
            daemon_url("http://[::1]:8080").unwrap(),
            "http://[::1]:8080"
        );
        assert_eq!(
            daemon_url("http://127.0.0.2:8080").unwrap(),
            "http://127.0.0.2:8080"
        );
        for bad in [
            "http://192.168.1.5:8080",
            "http://example.com:8080",
            "https://127.0.0.1:8080",
            "http://127.0.0.1",
            "http://127.0.0.1:0",
            "http://127.0.0.1:8080/api",
            "http://localhost.evil.com:8080",
            "http://0.0.0.0:8080",
        ] {
            assert!(daemon_url(bad).is_err(), "{bad}");
        }
        assert!(parse_config("+15551234567 http://10.0.0.1:8080").is_err());
    }

    #[test]
    fn only_the_accounts_own_note_to_self_is_read() {
        assert_eq!(
            note_to_self(&sync_to(ME, " hi there "), ME).as_deref(),
            Some("hi there")
        );
        // The same note, as a bare envelope and in subscribe form.
        let bare = sync_to(ME, "a")["params"].clone();
        assert_eq!(note_to_self(&bare, ME).as_deref(), Some("a"));
        let sub = json!({"method": "receive", "params": {"subscription": 0, "result": bare}});
        assert_eq!(note_to_self(&sub, ME).as_deref(), Some("a"));
        // What the person sent someone else.
        assert_eq!(note_to_self(&sync_to("+15559876543", "x"), ME), None);
        // Another account on the same daemon.
        assert_eq!(note_to_self(&sync_to(ME, "x"), "+15550000000"), None);
        let mut other_account = sync_to(ME, "x");
        other_account["params"]["account"] = json!("+15550000000");
        assert_eq!(note_to_self(&other_account, ME), None);
        // Someone writing to the person.
        let stranger = json!({"params": {"account": ME, "envelope": {
            "source": "+15559876543", "sourceNumber": "+15559876543",
            "dataMessage": {"message": "send me your code"}
        }}});
        assert_eq!(note_to_self(&stranger, ME), None);
        // A group, even one only the person is in.
        let mut group = sync_to(ME, "x");
        group["params"]["envelope"]["syncMessage"]["sentMessage"]["groupInfo"] =
            json!({"groupId": "g"});
        assert_eq!(note_to_self(&group, ME), None);
        // A receipt, typing, and an empty note.
        let receipt =
            json!({"params": {"account": ME, "envelope": {"source": ME, "receiptMessage": {}}}});
        assert_eq!(note_to_self(&receipt, ME), None);
        assert_eq!(note_to_self(&sync_to(ME, "  "), ME), None);
    }

    #[test]
    fn events_are_read_line_by_line_and_keep_the_last_id() {
        let mut r = EventReader::default();
        assert_eq!(r.line(": keep-alive"), None);
        assert_eq!(r.line("event: receive"), None);
        assert_eq!(r.line("id: 7"), None);
        assert_eq!(r.line("data: {\"a\":"), None);
        assert_eq!(r.line("data:1}\r"), None);
        let ev = r.line("").unwrap();
        assert_eq!(
            ev,
            Event {
                id: Some("7".into()),
                data: "{\"a\":\n1}".into()
            }
        );
        assert_eq!(
            serde_json::from_str::<Value>(&ev.data).unwrap(),
            json!({"a": 1})
        );
        assert_eq!(r.last_id.as_deref(), Some("7"));
        // A blank line with nothing before it dispatches nothing.
        assert_eq!(r.line(""), None);
        assert_eq!(r.line("data: x"), None);
        assert_eq!(r.line("").unwrap().id, None);
        assert_eq!(r.last_id.as_deref(), Some("7"));
    }

    #[test]
    fn rpc_calls_name_the_account_and_errors_are_worded() {
        let req = rpc(
            3,
            "send",
            Some(ME),
            send_params(ME, &"é".repeat(MAX_TEXT + 5)),
        );
        assert_eq!(req["method"], "send");
        assert_eq!(req["params"]["account"], ME);
        assert_eq!(req["params"]["recipient"], json!([ME]));
        assert_eq!(
            req["params"]["message"].as_str().unwrap().chars().count(),
            MAX_TEXT
        );
        assert_eq!(rpc(1, "listAccounts", None, json!({}))["params"], json!({}));
        assert_eq!(
            rpc_result(
                &json!({"error": {"code": -1, "message": "Unregistered user"}}),
                "send"
            )
            .unwrap_err(),
            "signal-cli refused send: Unregistered user"
        );
        assert_eq!(
            rpc_result(&json!({"result": {"timestamp": 9}}), "send").unwrap(),
            json!({"timestamp": 9})
        );
        assert!(rpc_result(&json!({}), "send").is_err());
        assert_eq!(
            listed_accounts(&json!([{"number": ME}, {"number": "+15550000000"}])),
            vec![ME.to_string(), "+15550000000".into()]
        );
    }

    #[test]
    fn a_link_is_only_ever_a_device_link() {
        let ok = json!({"deviceLinkUri": "sgnl://linkdevice?uuid=abc&pub_key=def"});
        assert_eq!(
            link_uri(&ok).unwrap(),
            "sgnl://linkdevice?uuid=abc&pub_key=def"
        );
        for bad in [
            json!({"deviceLinkUri": "https://evil.example/linkdevice?uuid=a"}),
            json!({"deviceLinkUri": "sgnl://linkdevice?uuid=a b"}),
            json!({}),
        ] {
            assert!(link_uri(&bad).is_err());
        }
        assert_eq!(
            linked_number(&json!({"number": ME, "aci": "x"})).unwrap(),
            ME
        );
        assert!(linked_number(&json!({"number": "nope"})).is_err());
    }
}
