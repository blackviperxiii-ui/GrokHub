//! Messaging channels, part 5: Telegram and Discord over the network. Bot
//! tokens live in the OS keychain, never in a file. Every call runs the egress
//! guard first; a stranger only ever gets the pairing text, and a paired
//! sender's message is queued in `channels/inbox/` for the app. Nothing here
//! calls a model.
//!
//! - [`TelegramLink`]: Bot API long polling. The token is part of every Bot
//!   API path, so no error from here carries a URL: it words its own.
//! - [`DiscordLink`]: one Gateway socket ([`Gateway`] keeps the session) and
//!   REST replies with the token in a header.

use std::path::{Path, PathBuf};
use std::time::Duration;

use std::net::{TcpStream, ToSocketAddrs};

use grokhub_core::http;
use grokhub_core::msg_channels::{self, ChannelKind, DmTrust, Forwarded};
use grokhub_core::msg_discord::{self, close_reason, Do, Gateway};
use grokhub_core::msg_telegram::{self, Step};
use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};
use zeroize::Zeroizing;

use crate::harness::{guard_quiet, DataClass, EgressReq};
use crate::keyring_blob::{self, Backend};

/// The Bot API host.
pub const TELEGRAM_API: &str = "https://api.telegram.org";
/// How long one `getUpdates` call waits for a message before it answers empty.
pub const POLL_WAIT_S: u64 = 25;

/// The keychain account a channel's bot token is saved under.
pub fn token_account(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::Telegram => "channel-telegram",
        ChannelKind::Discord => "channel-discord",
    }
}

fn keychain_message(err: keyring::Error) -> String {
    format!("the system keychain: {err}")
}

/// The OS keychain, under the app's own service name.
pub fn os_keychain() -> keyring_blob::OsBackend {
    keyring_blob::OsBackend::new(grokhub_core::brand::INTERNAL_ID, keychain_message)
}

/// A Discord bot token: three base64url parts joined by dots.
fn discord_token_shape(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    parts.len() == 3
        && token.len() <= 200
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

/// Save a bot token in `store`, after checking its shape.
pub fn save_token(store: &dyn Backend, kind: ChannelKind, token: &str) -> Result<(), String> {
    let token = token.trim();
    let shaped = match kind {
        ChannelKind::Telegram => msg_telegram::valid_token(token),
        ChannelKind::Discord => discord_token_shape(token),
    };
    if !shaped {
        return Err(match kind {
            ChannelKind::Telegram => {
                "That isn't a Telegram bot token. BotFather gives one shaped like 123456789:AA…"
            }
            ChannelKind::Discord => {
                "That isn't a Discord bot token. Copy it from the Bot page of your Discord application."
            }
        }
        .into());
    }
    keyring_blob::set(store, token_account(kind), token)
}

pub fn load_token(
    store: &dyn Backend,
    kind: ChannelKind,
) -> Result<Option<Zeroizing<String>>, String> {
    Ok(keyring_blob::get(store, token_account(kind))?.map(Zeroizing::new))
}

pub fn forget_token(store: &dyn Backend, kind: ChannelKind) -> Result<(), String> {
    keyring_blob::delete(store, token_account(kind))
}

/// What one poll did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Polled {
    /// Paired senders' messages, saved for the app.
    pub queued: Vec<PathBuf>,
    /// Strangers sent the pairing text.
    pub replied: usize,
    /// Messages answered with silence (a repeat stranger, a full queue).
    pub dropped: usize,
    /// Pairing texts that didn't go out, with why.
    pub unsent: Vec<String>,
}

/// One bot's connection to the Bot API. Not `Debug`: it holds the token.
pub struct TelegramLink {
    base: String,
    token: Zeroizing<String>,
    offset: Option<i64>,
    agent: http::Agent,
}

impl TelegramLink {
    pub fn new(token: &str) -> Result<Self, String> {
        Self::at(TELEGRAM_API, token, true)
    }

    fn at(base: &str, token: &str, public_only: bool) -> Result<Self, String> {
        let token = token.trim();
        if !msg_telegram::valid_token(token) {
            return Err("That isn't a Telegram bot token.".into());
        }
        let agent = http::agent(http::Opts {
            connect: Duration::from_secs(15),
            read: Some(Duration::from_secs(POLL_WAIT_S + 15)),
            overall: Some(Duration::from_secs(POLL_WAIT_S + 30)),
            redirects: 0,
            public_only,
            ..Default::default()
        });
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            token: Zeroizing::new(token.to_string()),
            offset: None,
            agent,
        })
    }

    fn method_url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.base, self.token.as_str())
    }

    /// The guard sees the method's address without the token in it.
    fn guard(&self, config_dir: &Path, method: &str, data: &[DataClass]) -> Result<(), String> {
        guard_quiet(
            config_dir,
            &EgressReq::new(&format!("{}/{method}", self.base), data),
        )
    }

    /// A failed call, worded with no URL in it.
    fn failed(method: &str, err: &http::Error) -> String {
        match err.status() {
            Some(401 | 404) => {
                "Telegram refused the bot token. Paste it again from BotFather.".into()
            }
            Some(409) => "Another program is reading this bot's messages (a webhook, or a second copy of the app). Stop it and try again.".into(),
            Some(429) => "Telegram asked the app to slow down.".into(),
            Some(code) => format!("Telegram's {method} failed (status {code})."),
            None => format!("Couldn't reach Telegram for {method}."),
        }
    }

    fn call(&self, config_dir: &Path, method: &str, query: &str) -> Result<Value, String> {
        self.guard(config_dir, method, &[])?;
        self.agent
            .get(&format!("{}{query}", self.method_url(method)))
            .call()
            .map_err(|e| Self::failed(method, &e))?
            .into_json()
            .map_err(|_| format!("Telegram's {method} answer isn't JSON."))
    }

    /// The bot's `@username`, to show the token works.
    pub fn check(&self, config_dir: &Path) -> Result<String, String> {
        let me = self.call(config_dir, "getMe", "")?;
        me.pointer("/result/username")
            .and_then(Value::as_str)
            .map(|u| format!("@{u}"))
            .ok_or_else(|| "Telegram's getMe answer has no bot name.".into())
    }

    /// Send `text` to a chat. `data` names what the text carries: nothing for
    /// the pairing text, chat for an answer the assistant wrote.
    pub fn send(
        &self,
        config_dir: &Path,
        chat_id: i64,
        text: &str,
        data: &[DataClass],
    ) -> Result<(), String> {
        self.guard(config_dir, "sendMessage", data)?;
        self.agent
            .post(&self.method_url("sendMessage"))
            .send_json(msg_telegram::send_message_body(chat_id, text))
            .map_err(|e| Self::failed("sendMessage", &e))?;
        Ok(())
    }

    /// One long poll of up to `wait_s` seconds. The next poll starts past
    /// every update this one read, but only once they are all saved.
    pub fn poll(&mut self, config_dir: &Path, wait_s: u64, now_ms: u64) -> Result<Polled, String> {
        let mut query = format!("?timeout={wait_s}&allowed_updates=%5B%22message%22%5D");
        if let Some(offset) = self.offset {
            query.push_str(&format!("&offset={offset}"));
        }
        let reply = self.call(config_dir, "getUpdates", &query)?;
        let (messages, next) = msg_telegram::parse_updates(&reply);
        let mut polled = Polled::default();
        if !messages.is_empty() {
            let mut trust = DmTrust::load(config_dir);
            let mut replies = Vec::new();
            for msg in messages {
                match msg_telegram::step(&mut trust, msg, now_ms) {
                    Step::Forward(m) => polled.queued.push(msg_channels::queue_message(
                        config_dir,
                        &Forwarded {
                            channel: ChannelKind::Telegram,
                            sender: &m.sender,
                            label: &m.label,
                            reply_to: &m.chat_id.to_string(),
                            text: &m.text,
                        },
                        now_ms,
                    )?),
                    Step::Reply { chat_id, text } => replies.push((chat_id, text)),
                    Step::Drop => polled.dropped += 1,
                }
            }
            // The codes are saved before they're sent, so one the user is
            // read out can always be approved.
            trust.save(config_dir)?;
            for (chat_id, text) in replies {
                match self.send(config_dir, chat_id, &text, &[]) {
                    Ok(()) => polled.replied += 1,
                    Err(why) => polled.unsent.push(why),
                }
            }
        }
        if next.is_some() {
            self.offset = next;
        }
        Ok(polled)
    }
}

/// How long the Gateway socket opens and its handshake may take.
const DISCORD_CONNECT: Duration = Duration::from_secs(15);
/// How long one socket read waits before the session's clock is checked.
const DISCORD_READ_POLL: Duration = Duration::from_millis(500);
/// The longest wait between two connection attempts.
const DISCORD_MAX_BACKOFF: Duration = Duration::from_secs(60);

type GatewaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

/// How one Gateway connection ended.
enum Ended {
    Stopped,
    /// Connecting again can't help (a bad token, refused intents).
    Fatal(String),
    Again {
        resume: bool,
    },
}

/// One bot's Gateway session and its REST replies. Not `Debug`: it holds the token.
pub struct DiscordLink {
    token: Zeroizing<String>,
    rest: String,
    gateway: Gateway,
    /// Tests dial a loopback gateway instead of Discord's.
    gateway_at: Option<String>,
    agent: http::Agent,
    last_problem: Option<String>,
}

impl DiscordLink {
    pub fn new(token: &str) -> Result<Self, String> {
        Self::at(None, msg_discord::API_BASE, token, true)
    }

    fn at(
        gateway_at: Option<&str>,
        rest: &str,
        token: &str,
        public_only: bool,
    ) -> Result<Self, String> {
        let token = token.trim();
        if !discord_token_shape(token) {
            return Err("That isn't a Discord bot token.".into());
        }
        Ok(Self {
            token: Zeroizing::new(token.to_string()),
            rest: rest.trim_end_matches('/').to_string(),
            gateway: Gateway::default(),
            gateway_at: gateway_at.map(str::to_string),
            agent: http::agent(http::Opts {
                connect: Duration::from_secs(15),
                overall: Some(Duration::from_secs(30)),
                redirects: 0,
                public_only,
                ..Default::default()
            }),
            last_problem: None,
        })
    }

    /// The last message that couldn't be saved or answered, for the app to show.
    pub fn last_problem(&self) -> Option<&str> {
        self.last_problem.as_deref()
    }

    fn failed(err: &http::Error) -> String {
        match err.status() {
            Some(401) => "Discord refused the bot token. Copy it again from the Bot page.".into(),
            Some(403) => "Discord won't let the bot message that person.".into(),
            Some(429) => "Discord asked the app to slow down.".into(),
            Some(code) => format!("Discord's send failed (status {code})."),
            None => "Couldn't reach Discord to send a reply.".into(),
        }
    }

    /// Send `text` to a DM channel; `data` as [`TelegramLink::send`].
    pub fn send(
        &self,
        config_dir: &Path,
        channel_id: &str,
        text: &str,
        data: &[DataClass],
    ) -> Result<(), String> {
        let path = msg_discord::messages_url(channel_id)
            .ok_or("That isn't a Discord channel id.")?
            .split_off(msg_discord::API_BASE.len());
        let url = format!("{}{path}", self.rest);
        guard_quiet(config_dir, &EgressReq::new(&url, data))?;
        self.agent
            .post(&url)
            .header("Authorization", &format!("Bot {}", self.token.as_str()))
            .send_json(msg_discord::message_body(text))
            .map_err(|e| Self::failed(&e))?;
        Ok(())
    }

    /// TCP, TLS for `wss`, then the WebSocket upgrade.
    fn connect(url: &str) -> Result<GatewaySocket, String> {
        let request = url.into_client_request().map_err(|e| e.to_string())?;
        let uri = request.uri();
        let host = uri.host().ok_or("no host")?.to_string();
        let tls = uri.scheme_str() == Some("wss");
        let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
        let mut last = String::from("no address");
        let mut stream = None;
        for addr in (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .take(4)
        {
            match TcpStream::connect_timeout(&addr, DISCORD_CONNECT) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last = e.to_string(),
            }
        }
        let stream = stream.ok_or(last)?;
        stream
            .set_read_timeout(Some(DISCORD_CONNECT))
            .and_then(|()| stream.set_write_timeout(Some(DISCORD_CONNECT)))
            .map_err(|e| e.to_string())?;
        let connector = if tls {
            Some(tungstenite::Connector::Rustls(crate::ws_mode::tls_config()?))
        } else {
            None
        };
        let (socket, _) = tungstenite::client_tls_with_config(request, stream, None, connector)
            .map_err(|e| e.to_string())?;
        let tcp = match socket.get_ref() {
            MaybeTlsStream::Plain(s) => s,
            MaybeTlsStream::Rustls(s) => s.get_ref(),
            _ => return Err("unknown stream".into()),
        };
        tcp.set_read_timeout(Some(DISCORD_READ_POLL))
            .map_err(|e| e.to_string())?;
        Ok(socket)
    }

    /// Connect and stay connected until `stop` says so, resuming after a drop
    /// and waiting longer after each failed attempt. `Err` only when Discord
    /// says connecting again can't help, or the egress guard refuses.
    pub fn run(&mut self, config_dir: &Path, stop: &dyn Fn() -> bool) -> Result<(), String> {
        let mut backoff = Duration::from_secs(1);
        while !stop() {
            let url = self
                .gateway_at
                .clone()
                .unwrap_or_else(|| self.gateway.url());
            guard_quiet(config_dir, &EgressReq::new(&url, &[]))?;
            self.gateway.connecting();
            match Self::connect(&url) {
                Ok(mut socket) => {
                    let ended = self.session(config_dir, &mut socket, stop);
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    match ended {
                        Ended::Stopped => return Ok(()),
                        Ended::Fatal(why) => return Err(why),
                        Ended::Again { resume } => {
                            if !resume {
                                self.gateway.forget_session();
                            }
                            backoff = Duration::from_secs(1);
                        }
                    }
                }
                Err(_) => backoff = (backoff * 2).min(DISCORD_MAX_BACKOFF),
            }
            let until = std::time::Instant::now() + backoff;
            while !stop() && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        Ok(())
    }

    fn session(
        &mut self,
        config_dir: &Path,
        socket: &mut GatewaySocket,
        stop: &dyn Fn() -> bool,
    ) -> Ended {
        loop {
            if stop() {
                return Ended::Stopped;
            }
            let now = grokhub_core::now_ms();
            let mut todo = match socket.read() {
                Ok(Message::Text(text)) => match serde_json::from_str::<Value>(text.as_str()) {
                    Ok(frame) => self.gateway.on_frame(&frame, &self.token, now),
                    Err(_) => Vec::new(),
                },
                Ok(Message::Close(frame)) => {
                    let code = frame.map_or(1000, |f| u16::from(f.code));
                    return match close_reason(code) {
                        Err(why) => Ended::Fatal(why),
                        Ok(()) => Ended::Again { resume: true },
                    };
                }
                Ok(_) => Vec::new(),
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    Vec::new()
                }
                Err(_) => return Ended::Again { resume: true },
            };
            todo.extend(self.gateway.on_tick(now));
            for next in todo {
                match next {
                    Do::Send(frame) => {
                        if socket.send(Message::text(frame.to_string())).is_err() {
                            return Ended::Again { resume: true };
                        }
                    }
                    Do::Message(msg) => {
                        if let Err(why) = self.handle(config_dir, msg, now) {
                            self.last_problem = Some(why);
                        }
                    }
                    Do::Reconnect { resume } => return Ended::Again { resume },
                }
            }
        }
    }

    fn handle(
        &self,
        config_dir: &Path,
        msg: msg_discord::Incoming,
        now_ms: u64,
    ) -> Result<(), String> {
        let mut trust = DmTrust::load(config_dir);
        match msg_discord::step(&mut trust, msg, now_ms) {
            msg_discord::Step::Forward(m) => {
                msg_channels::queue_message(
                    config_dir,
                    &Forwarded {
                        channel: ChannelKind::Discord,
                        sender: &m.sender,
                        label: &m.label,
                        reply_to: &m.channel_id,
                        text: &m.text,
                    },
                    now_ms,
                )?;
                Ok(())
            }
            msg_discord::Step::Reply { channel_id, text } => {
                // Saved before it's sent, as for Telegram.
                trust.save(config_dir)?;
                self.send(config_dir, &channel_id, &text, &[])
            }
            msg_discord::Step::Drop => trust.save(config_dir),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::thread::JoinHandle;

    use serde_json::json;

    const TOKEN: &str = "123456789:AAabcdefghijklmnopqrstuvwxyz0123456";
    const NOW: u64 = 1_763_550_000_000;

    #[derive(Default)]
    struct MemoryStore(Mutex<BTreeMap<String, String>>);

    impl Backend for MemoryStore {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(account.into(), value.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    /// A fake HTTP API on loopback: answers each request with the next
    /// `(status, body)` and hands back `"{request line}\n{body}"` for each,
    /// with ` [auth: …]` after the line when it sent an Authorization header.
    fn fake_api(answers: Vec<(u16, Value)>) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut line = line.trim_end().to_string();
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap();
                        }
                        if name.eq_ignore_ascii_case("authorization") {
                            line.push_str(&format!(" [auth: {}]", value.trim()));
                        }
                    }
                }
                let mut sent = vec![0; length];
                reader.read_exact(&mut sent).unwrap();
                seen.push(format!("{line}\n{}", String::from_utf8(sent).unwrap()));
                let text = body.to_string();
                let mut stream = reader.into_inner();
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                )
                .unwrap();
            }
            seen
        });
        (base, handle)
    }

    fn update(id: i64, text: &str) -> Value {
        json!({
            "update_id": id,
            "message": {
                "message_id": id,
                "chat": { "id": 5001, "type": "private" },
                "from": { "id": 777, "is_bot": false, "first_name": "User", "username": "user" },
                "text": text,
            }
        })
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grokhub-chnet-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_stranger_is_sent_a_code_and_a_paired_sender_is_queued() {
        let dir = temp_dir("pair");
        let (base, server) = fake_api(vec![
            (200, json!({ "ok": true, "result": [update(41, "hello")] })),
            (200, json!({ "ok": true, "result": { "message_id": 1 } })),
            (
                200,
                json!({ "ok": true, "result": [update(42, "status?")] }),
            ),
        ]);
        let mut link = TelegramLink::at(&base, TOKEN, false).unwrap();

        let first = link.poll(&dir, 0, NOW).unwrap();
        assert_eq!(
            first,
            Polled {
                replied: 1,
                ..Default::default()
            }
        );
        let mut trust = DmTrust::load(&dir);
        let code = trust.pending[0].code.clone();
        trust.approve(&code, NOW + 1).unwrap();
        trust.save(&dir).unwrap();

        let second = link.poll(&dir, 0, NOW + 2).unwrap();
        assert_eq!(second.queued.len(), 1);
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&second.queued[0]).unwrap()).unwrap();
        assert_eq!(
            saved,
            json!({
                "channel": "telegram",
                "sender": "777",
                "label": "@user",
                "replyTo": "5001",
                "text": "status?",
                "receivedMs": NOW + 2,
            })
        );

        let seen = server.join().unwrap();
        assert_eq!(
            seen[0],
            format!("GET /bot{TOKEN}/getUpdates?timeout=0&allowed_updates=%5B%22message%22%5D HTTP/1.1\n")
        );
        let (line, body) = seen[1].split_once('\n').unwrap();
        assert_eq!(line, format!("POST /bot{TOKEN}/sendMessage HTTP/1.1"));
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap(),
            json!({ "chat_id": 5001, "text": msg_telegram::pairing_text(&code) })
        );
        // The second poll starts past the first update.
        assert_eq!(
            seen[2],
            format!("GET /bot{TOKEN}/getUpdates?timeout=0&allowed_updates=%5B%22message%22%5D&offset=42 HTTP/1.1\n")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn errors_never_show_the_token() {
        let dir = temp_dir("errors");
        let (base, server) = fake_api(vec![
            (
                401,
                json!({ "ok": false, "error_code": 401, "description": "Unauthorized" }),
            ),
            (
                409,
                json!({ "ok": false, "error_code": 409, "description": "Conflict" }),
            ),
            (
                200,
                json!({ "ok": true, "result": { "username": "my_app_bot" } }),
            ),
        ]);
        let mut link = TelegramLink::at(&base, TOKEN, false).unwrap();
        assert_eq!(
            link.poll(&dir, 0, NOW).unwrap_err(),
            "Telegram refused the bot token. Paste it again from BotFather."
        );
        let conflict = link.poll(&dir, 0, NOW).unwrap_err();
        assert!(
            conflict.starts_with("Another program is reading"),
            "{conflict}"
        );
        assert!(!conflict.contains(TOKEN));
        assert_eq!(link.check(&dir).unwrap(), "@my_app_bot");
        server.join().unwrap();
        // Nothing reached the trust store or the inbox.
        assert!(!msg_channels::inbox_dir(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tokens_live_in_the_keychain_and_are_checked_first() {
        let store = MemoryStore::default();
        assert_eq!(load_token(&store, ChannelKind::Telegram).unwrap(), None);
        assert_eq!(
            save_token(&store, ChannelKind::Telegram, "not-a-token").unwrap_err(),
            "That isn't a Telegram bot token. BotFather gives one shaped like 123456789:AA…"
        );
        save_token(&store, ChannelKind::Telegram, &format!(" {TOKEN}\n")).unwrap();
        assert_eq!(
            load_token(&store, ChannelKind::Telegram)
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some(TOKEN)
        );
        assert!(save_token(&store, ChannelKind::Discord, TOKEN).is_err());
        save_token(&store, ChannelKind::Discord, "MTIz.abc_DEF.ghi-jkl").unwrap();
        assert_eq!(
            store.0.lock().unwrap().keys().cloned().collect::<Vec<_>>(),
            vec![
                "channel-discord".to_string(),
                "channel-telegram".to_string()
            ]
        );
        forget_token(&store, ChannelKind::Telegram).unwrap();
        assert_eq!(load_token(&store, ChannelKind::Telegram).unwrap(), None);
        assert!(TelegramLink::new("123:short").is_err());
    }

    const DISCORD_TOKEN: &str = "MTIz.abc_DEF.ghi-jkl";

    /// A fake Gateway on loopback: says Hello, records the Identify, sends
    /// Ready and one stranger's DM, waits for `replied`, then closes with 4004.
    fn fake_gateway(replied: std::sync::mpsc::Receiver<()>) -> (String, JoinHandle<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "ws://{}/?v=10&encoding=json",
            listener.local_addr().unwrap()
        );
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let hello = json!({ "op": 10, "d": { "heartbeat_interval": 60_000 } });
            ws.send(Message::text(hello.to_string())).unwrap();
            let identify: Value = loop {
                if let Message::Text(t) = ws.read().unwrap() {
                    break serde_json::from_str(t.as_str()).unwrap();
                }
            };
            let ready = json!({ "op": 0, "t": "READY", "s": 1, "d": { "session_id": "abc" } });
            ws.send(Message::text(ready.to_string())).unwrap();
            let dm = json!({ "op": 0, "t": "MESSAGE_CREATE", "s": 2, "d": {
                "id": "1100000000000000001",
                "channel_id": "1200000000000000002",
                "author": { "id": "1300000000000000003", "username": "user" },
                "content": "hello",
            }});
            ws.send(Message::text(dm.to_string())).unwrap();
            replied.recv_timeout(Duration::from_secs(10)).unwrap();
            ws.close(Some(tungstenite::protocol::CloseFrame {
                code: 4004.into(),
                reason: "Authentication failed".into(),
            }))
            .unwrap();
            // Let the close go out before the socket drops.
            let _ = ws.read();
            identify
        });
        (url, handle)
    }

    #[test]
    fn a_discord_stranger_gets_a_code_over_rest_and_a_bad_token_stops_the_run() {
        let dir = temp_dir("discord");
        let (rest, rest_server) = fake_api(vec![(200, json!({ "id": "1" }))]);
        let (tx, rx) = std::sync::mpsc::channel();
        let (gateway, gateway_server) = fake_gateway(rx);
        let mut link = DiscordLink::at(Some(&gateway), &rest, DISCORD_TOKEN, false).unwrap();
        let runner = std::thread::spawn(move || {
            let result = link.run(&dir, &|| false);
            (result, link.last_problem().map(str::to_string), dir)
        });
        let seen = rest_server.join().unwrap();
        tx.send(()).unwrap();
        let identify = gateway_server.join().unwrap();
        let (result, problem, dir) = runner.join().unwrap();

        assert_eq!(identify["op"], 2);
        assert_eq!(identify["d"]["token"], DISCORD_TOKEN);
        assert_eq!(identify["d"]["intents"], 4096);
        let (line, body) = seen[0].split_once('\n').unwrap();
        assert_eq!(
            line,
            format!(
                "POST /channels/1200000000000000002/messages HTTP/1.1 [auth: Bot {DISCORD_TOKEN}]"
            )
        );
        let code = DmTrust::load(&dir).pending[0].code.clone();
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap(),
            json!({
                "content": msg_telegram::pairing_text(&code),
                "allowed_mentions": { "parse": [] },
            })
        );
        assert_eq!(problem, None);
        assert_eq!(
            result,
            Err("Discord refused the bot token. Copy it again from the Bot page.".into())
        );
        assert!(DiscordLink::new("not a token").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
