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
//! - [`SlackLink`]: one Socket Mode socket, opened with the app token, and
//!   Web API replies with the bot token. Every envelope is acknowledged
//!   before it is handled, and a retried event is handled once.
//! - [`SignalLink`]: a signal-cli daemon on this computer only, linked to the
//!   person's own account. It reads their Note to Self and answers there;
//!   nobody else is read or written to.

use std::path::{Path, PathBuf};
use std::time::Duration;

use std::net::{TcpStream, ToSocketAddrs};

use grokhub_core::http;
use grokhub_core::msg_channels::{self, ChannelKind, DmTrust, Forwarded};
use grokhub_core::msg_discord::{self, close_reason, Do, Gateway};
use grokhub_core::msg_signal;
use grokhub_core::msg_slack;
use grokhub_core::msg_telegram::{self, Step};
use serde_json::{json, Value};
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
/// The error a refused bot token gives: polling again won't help.
pub const TELEGRAM_TOKEN_REFUSED: &str =
    "Telegram refused the bot token. Paste it again from BotFather.";

/// The keychain account a channel's bot token is saved under.
pub fn token_account(kind: ChannelKind) -> &'static str {
    kind.spec().token_account
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

/// What a kind with no adapter in this build answers.
pub fn coming_next(kind: ChannelKind) -> String {
    format!("{} is coming next. It can't be connected yet.", kind.name())
}

/// Save a bot token in `store`, after checking its shape.
pub fn save_token(store: &dyn Backend, kind: ChannelKind, token: &str) -> Result<(), String> {
    let token = token.trim();
    // Slack's two tokens are kept as one entry, app token first.
    let slack = match kind {
        ChannelKind::Slack => {
            msg_slack::split_tokens(token).map(|(app, bot)| Zeroizing::new(format!("{app} {bot}")))
        }
        _ => None,
    };
    let (shaped, why) = match kind {
        ChannelKind::Telegram => (
            msg_telegram::valid_token(token),
            "That isn't a Telegram bot token. BotFather gives one shaped like 123456789:AA…",
        ),
        ChannelKind::Discord => (
            discord_token_shape(token),
            "That isn't a Discord bot token. Copy it from the Bot page of your Discord application.",
        ),
        ChannelKind::Slack => (
            slack.is_some(),
            "Those aren't Slack's two tokens. Paste the app token (xapp-…) and the bot token (xoxb-…), with a space between.",
        ),
        ChannelKind::Signal => {
            // Kept in its normalized form: the number, then any daemon.
            let config = msg_signal::parse_config(token)?;
            return keyring_blob::set(store, token_account(kind), &config.saved());
        }
        ChannelKind::Email => {
            return Err(coming_next(kind));
        }
    };
    if !shaped {
        return Err(why.into());
    }
    keyring_blob::set(
        store,
        token_account(kind),
        slack.as_deref().map_or(token, String::as_str),
    )
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
            Some(401 | 404) => TELEGRAM_TOKEN_REFUSED.into(),
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

    /// The bot's name, to show the token works.
    pub fn check(&self, config_dir: &Path) -> Result<String, String> {
        let url = format!("{}/users/@me", self.rest);
        guard_quiet(config_dir, &EgressReq::new(&url, &[]))?;
        let me: Value = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bot {}", self.token.as_str()))
            .call()
            .map_err(|e| Self::failed(&e))?
            .into_json()
            .map_err(|_| "Discord's answer isn't JSON.".to_string())?;
        me.get("username")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "Discord's answer has no bot name.".into())
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

    /// TCP, TLS for `wss`, then the WebSocket upgrade. Slack's socket opens
    /// the same way.
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

/// How many event ids [`SlackLink`] remembers.
const SLACK_SEEN: usize = msg_slack::SEEN_EVENTS;

/// One Slack app's Socket Mode session and its Web API replies. Not `Debug`:
/// it holds both tokens.
pub struct SlackLink {
    app: Zeroizing<String>,
    bot: Zeroizing<String>,
    api: String,
    /// The app's own user id, from `auth.test`, so its mention can be cut.
    bot_user: String,
    agent: http::Agent,
    /// Off only in tests, which serve the socket on loopback.
    public_only: bool,
    seen: std::collections::VecDeque<String>,
    last_problem: Option<String>,
}

impl SlackLink {
    /// `tokens` as saved: the app token and the bot token, space between.
    pub fn new(tokens: &str) -> Result<Self, String> {
        Self::at(msg_slack::API_BASE, tokens, true)
    }

    fn at(api: &str, tokens: &str, public_only: bool) -> Result<Self, String> {
        let (app, bot) =
            msg_slack::split_tokens(tokens).ok_or("Those aren't Slack's two tokens.")?;
        Ok(Self {
            app: Zeroizing::new(app.to_string()),
            bot: Zeroizing::new(bot.to_string()),
            api: api.trim_end_matches('/').to_string(),
            bot_user: String::new(),
            agent: http::agent(http::Opts {
                connect: Duration::from_secs(15),
                overall: Some(Duration::from_secs(30)),
                redirects: 0,
                public_only,
                ..Default::default()
            }),
            public_only,
            seen: std::collections::VecDeque::new(),
            last_problem: None,
        })
    }

    /// The last message that couldn't be saved or answered, for the app to show.
    pub fn last_problem(&self) -> Option<&str> {
        self.last_problem.as_deref()
    }

    /// One Web API call with `token`. Slack answers 200 with `ok: false` for
    /// most errors, so both are read; no error carries a token or a URL.
    fn call(
        &self,
        config_dir: &Path,
        method: &str,
        token: &str,
        body: Value,
        data: &[DataClass],
    ) -> Result<Value, String> {
        let url = format!("{}/{method}", self.api);
        guard_quiet(config_dir, &EgressReq::new(&url, data))?;
        let answer: Value = self
            .agent
            .post(&url)
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(body)
            .map_err(|e| match e.status() {
                Some(429) => "Slack asked the app to slow down.".to_string(),
                Some(code) => format!("Slack's {method} failed (status {code})."),
                None => format!("Couldn't reach Slack for {method}."),
            })?
            .into_json()
            .map_err(|_| format!("Slack's {method} answer isn't JSON."))?;
        match msg_slack::api_error(&answer) {
            Some(why) => Err(why),
            None => Ok(answer),
        }
    }

    /// The app's name, to show the bot token works, and its user id.
    pub fn check(&mut self, config_dir: &Path) -> Result<String, String> {
        let me = self.call(config_dir, "auth.test", &self.bot, json!({}), &[])?;
        self.bot_user = me
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        me.get("user")
            .and_then(Value::as_str)
            .map(|u| format!("@{u}"))
            .ok_or_else(|| "Slack's auth.test answer has no app name.".into())
    }

    /// Post `text` where `reply_to` says (a DM, or `channel:thread`); `data`
    /// as [`TelegramLink::send`].
    pub fn send(
        &self,
        config_dir: &Path,
        reply_to: &str,
        text: &str,
        data: &[DataClass],
    ) -> Result<(), String> {
        let (channel, thread) =
            msg_slack::split_reply_to(reply_to).ok_or("That isn't a Slack conversation.")?;
        self.call(
            config_dir,
            "chat.postMessage",
            &self.bot,
            msg_slack::post_body(channel, thread, text),
            data,
        )
        .map(|_| ())
    }

    /// A fresh Socket Mode address. It holds a one-time ticket, so the guard
    /// sees only its host.
    fn open_url(&self, config_dir: &Path) -> Result<String, String> {
        let answer = self.call(
            config_dir,
            "apps.connections.open",
            &self.app,
            json!({}),
            &[],
        )?;
        let url = answer
            .get("url")
            .and_then(Value::as_str)
            .ok_or("Slack's apps.connections.open answer has no address.")?;
        if !self.public_only && url.starts_with("ws://127.0.0.1:") {
            return Ok(url.to_string());
        }
        let host = url
            .strip_prefix("wss://")
            .and_then(|rest| rest.split(['/', '?']).next())
            .filter(|h| *h == "slack.com" || h.ends_with(".slack.com"))
            .ok_or("Slack gave a socket address that isn't Slack's.")?;
        guard_quiet(
            config_dir,
            &EgressReq::new(&format!("wss://{host}/link/"), &[]),
        )?;
        Ok(url.to_string())
    }

    /// Connect and stay connected until `stop` says so, opening a new socket
    /// whenever Slack drops or refreshes one. `Err` when the tokens are
    /// refused, the app is turned off, or the egress guard refuses.
    pub fn run(&mut self, config_dir: &Path, stop: &dyn Fn() -> bool) -> Result<(), String> {
        if self.bot_user.is_empty() {
            self.check(config_dir)?;
        }
        let mut backoff = Duration::from_secs(1);
        while !stop() {
            let url = match self.open_url(config_dir) {
                Ok(url) => url,
                Err(why) if !Self::passing(&why) => return Err(why),
                Err(why) => {
                    self.last_problem = Some(why);
                    backoff = (backoff * 2).min(DISCORD_MAX_BACKOFF);
                    Self::nap(stop, backoff);
                    continue;
                }
            };
            match DiscordLink::connect(&url) {
                Ok(mut socket) => {
                    let ended = self.session(config_dir, &mut socket, stop);
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    match ended {
                        Ended::Stopped => return Ok(()),
                        Ended::Fatal(why) => return Err(why),
                        Ended::Again { .. } => backoff = Duration::from_secs(1),
                    }
                }
                Err(_) => backoff = (backoff * 2).min(DISCORD_MAX_BACKOFF),
            }
            Self::nap(stop, backoff);
        }
        Ok(())
    }

    /// True for a problem that trying again later can fix: Slack out of
    /// reach, busy or asking for a pause. Anything else needs the person.
    fn passing(why: &str) -> bool {
        why.starts_with("Couldn't reach") || why.contains("slow down") || why.contains("(status 5")
    }

    fn nap(stop: &dyn Fn() -> bool, total: Duration) {
        let until = std::time::Instant::now() + total;
        while !stop() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// True the first time `event_id` is seen.
    fn first_time(&mut self, event_id: &str) -> bool {
        if self.seen.iter().any(|id| id == event_id) {
            return false;
        }
        if self.seen.len() >= SLACK_SEEN {
            self.seen.pop_front();
        }
        self.seen.push_back(event_id.to_string());
        true
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
            let frame = match socket.read() {
                Ok(Message::Text(text)) => match serde_json::from_str::<Value>(text.as_str()) {
                    Ok(frame) => msg_slack::read_frame(&frame, &self.bot_user),
                    Err(_) => continue,
                },
                Ok(Message::Close(_)) => return Ended::Again { resume: false },
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(_) => return Ended::Again { resume: false },
            };
            match frame {
                msg_slack::Frame::Hello | msg_slack::Frame::Other => {}
                msg_slack::Frame::Disconnect { fatal: true } => {
                    return Ended::Fatal("Slack turned the app's socket off. Turn Socket Mode back on in the Slack app.".into());
                }
                msg_slack::Frame::Disconnect { fatal: false } => {
                    return Ended::Again { resume: false };
                }
                msg_slack::Frame::Event {
                    envelope_id,
                    event_id,
                    message,
                } => {
                    // Acknowledged first: Slack retries anything not acked
                    // within three seconds.
                    let ack = msg_slack::ack(&envelope_id).to_string();
                    if socket.send(Message::text(ack)).is_err() {
                        return Ended::Again { resume: false };
                    }
                    let fresh = event_id.as_deref().is_none_or(|id| self.first_time(id));
                    if let (true, Some(msg)) = (fresh, message) {
                        if let Err(why) = self.handle(config_dir, msg, grokhub_core::now_ms()) {
                            self.last_problem = Some(why);
                        }
                    }
                }
            }
        }
    }

    fn handle(
        &self,
        config_dir: &Path,
        msg: msg_slack::Incoming,
        now_ms: u64,
    ) -> Result<(), String> {
        let mut trust = DmTrust::load(config_dir);
        match msg_slack::step(&mut trust, msg, now_ms) {
            msg_slack::Step::Forward(m) => {
                msg_channels::queue_message(
                    config_dir,
                    &Forwarded {
                        channel: ChannelKind::Slack,
                        sender: &m.sender,
                        label: &m.sender,
                        reply_to: &m.reply_to(),
                        text: &m.text,
                    },
                    now_ms,
                )?;
                Ok(())
            }
            msg_slack::Step::Reply { channel, text } => {
                // Saved before it's sent, as for Telegram.
                trust.save(config_dir)?;
                self.send(config_dir, &channel, &text, &[])
            }
            msg_slack::Step::Drop => trust.save(config_dir),
        }
    }
}

/// How long a call to the local signal-cli daemon may take. `finishLink`
/// waits for the phone, so it has its own limit.
const SIGNAL_CALL: Duration = Duration::from_secs(30);
/// How long linking waits for the phone to confirm.
pub const SIGNAL_LINK_WAIT: Duration = Duration::from_secs(180);
/// A quiet event stream is opened again after this, so a daemon that hung is
/// noticed.
const SIGNAL_QUIET: Duration = Duration::from_secs(300);

/// The person's own Signal account, reached through a signal-cli daemon on
/// this computer. It reads their Note to Self and answers there; nobody else
/// is ever read or written to (see [`msg_signal`]).
pub struct SignalLink {
    config: msg_signal::Config,
    /// Short calls: the check and JSON-RPC.
    calls: http::Agent,
    /// The event stream and `finishLink`, which wait.
    waits: http::Agent,
    events: msg_signal::EventReader,
    next_id: std::sync::atomic::AtomicU64,
}

impl SignalLink {
    /// `saved` as the keychain keeps it: the number, then the daemon when it
    /// isn't the default.
    pub fn new(saved: &str) -> Result<Self, String> {
        Ok(Self::with(msg_signal::parse_config(saved)?))
    }

    /// A link to the daemon alone, before any account is linked.
    pub fn daemon(daemon: &str) -> Result<Self, String> {
        Ok(Self::with(msg_signal::Config {
            account: String::new(),
            daemon: msg_signal::daemon_url(daemon)?,
        }))
    }

    fn with(config: msg_signal::Config) -> Self {
        // Loopback only, whatever the address resolves to.
        let opts = http::Opts {
            connect: Duration::from_secs(3),
            redirects: 0,
            resolve: Some(http::loopback_only_resolve),
            ..Default::default()
        };
        Self {
            config,
            calls: http::agent(http::Opts {
                overall: Some(SIGNAL_CALL),
                ..opts
            }),
            waits: http::agent(http::Opts {
                read: Some(SIGNAL_QUIET),
                ..opts
            }),
            events: msg_signal::EventReader::default(),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    pub fn config(&self) -> &msg_signal::Config {
        &self.config
    }

    fn unreachable(&self) -> String {
        format!(
            "signal-cli isn't answering at {}. Start it with: {}",
            self.config.daemon,
            msg_signal::DAEMON_COMMAND
        )
    }

    fn rpc_on(
        &self,
        agent: &http::Agent,
        config_dir: &Path,
        method: &str,
        params: Value,
        data: &[DataClass],
    ) -> Result<Value, String> {
        let url = self.config.rpc_url();
        guard_quiet(config_dir, &EgressReq::new(&url, data))?;
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let account = (!self.config.account.is_empty()).then_some(self.config.account.as_str());
        let answer: Value = agent
            .post(&url)
            .send_json(msg_signal::rpc(id, method, account, params))
            .map_err(|e| match e.status() {
                Some(code) => format!("signal-cli's {method} failed (status {code})."),
                None => self.unreachable(),
            })?
            .into_json()
            .map_err(|_| format!("signal-cli's {method} answer isn't JSON."))?;
        msg_signal::rpc_result(&answer, method)
    }

    fn rpc(
        &self,
        config_dir: &Path,
        method: &str,
        params: Value,
        data: &[DataClass],
    ) -> Result<Value, String> {
        self.rpc_on(&self.calls, config_dir, method, params, data)
    }

    /// The daemon runs and knows the account. Answers the number.
    pub fn check(&self, config_dir: &Path) -> Result<String, String> {
        let url = self.config.check_url();
        guard_quiet(config_dir, &EgressReq::new(&url, &[]))?;
        self.calls.get(&url).call().map_err(|e| match e.status() {
            Some(code) => format!(
                "Something at {} answered, but not as signal-cli's HTTP daemon (status {code}).",
                self.config.daemon
            ),
            None => self.unreachable(),
        })?;
        let accounts =
            msg_signal::listed_accounts(&self.rpc(config_dir, "listAccounts", json!({}), &[])?);
        if !accounts.contains(&self.config.account) {
            return Err(format!(
                "signal-cli isn't linked to {}. Link it first, then connect again.",
                self.config.account
            ));
        }
        Ok(self.config.account.clone())
    }

    /// Answer in the account's own Note to Self. `reply_to` must be that
    /// account: there is nobody else to answer.
    pub fn send(
        &self,
        config_dir: &Path,
        reply_to: &str,
        text: &str,
        data: &[DataClass],
    ) -> Result<(), String> {
        if reply_to != self.config.account {
            return Err("Signal answers go only to your own Note to Self.".into());
        }
        self.rpc(
            config_dir,
            "send",
            msg_signal::send_params(&self.config.account, text),
            data,
        )
        .map(|_| ())
    }

    /// Start linking: the `sgnl://linkdevice` address the phone scans.
    pub fn start_link(&self, config_dir: &Path) -> Result<String, String> {
        msg_signal::link_uri(&self.rpc(config_dir, "startLink", json!({}), &[])?)
    }

    /// Wait for the phone to confirm `uri`, up to [`SIGNAL_LINK_WAIT`].
    /// Answers the linked number.
    pub fn finish_link(
        &self,
        config_dir: &Path,
        uri: &str,
        device: &str,
    ) -> Result<String, String> {
        let wait = http::agent(http::Opts {
            connect: Duration::from_secs(3),
            overall: Some(SIGNAL_LINK_WAIT),
            redirects: 0,
            resolve: Some(http::loopback_only_resolve),
            ..Default::default()
        });
        let result = self.rpc_on(
            &wait,
            config_dir,
            "finishLink",
            json!({"deviceLinkUri": uri, "deviceName": device}),
            &[],
        )?;
        msg_signal::linked_number(&result)
    }

    /// Read events until `stop` says so, opening the stream again (with
    /// `Last-Event-ID`, so the daemon replays what was missed) whenever it
    /// ends. `report` gets the current problem, or `None` once events flow.
    /// `Err` only when the egress guard refuses.
    pub fn run(
        &mut self,
        config_dir: &Path,
        stop: &(dyn Fn() -> bool + Sync),
        report: &dyn Fn(Option<&str>),
    ) -> Result<(), String> {
        let mut backoff = Duration::from_secs(1);
        while !stop() {
            match self.stream(config_dir, stop, report) {
                Ok(()) => backoff = Duration::from_secs(1),
                Err(Some(why)) => {
                    report(Some(&why));
                    backoff = (backoff * 2).min(DISCORD_MAX_BACKOFF);
                }
                Err(None) => return Err("The egress guard refused the Signal daemon.".into()),
            }
            SlackLink::nap(stop, backoff);
        }
        Ok(())
    }

    /// One open stream. `Err(None)` for a guard refusal; `Err(Some)` for
    /// anything trying again can fix.
    fn stream(
        &mut self,
        config_dir: &Path,
        stop: &(dyn Fn() -> bool + Sync),
        report: &dyn Fn(Option<&str>),
    ) -> Result<(), Option<String>> {
        let url = self.config.events_url();
        guard_quiet(config_dir, &EgressReq::new(&url, &[])).map_err(|_| None)?;
        // Pressed by the watcher below once `stop` reads true.
        let pressed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let switch = {
            let pressed = pressed.clone();
            http::StopSwitch::new(move || pressed.load(std::sync::atomic::Ordering::SeqCst))
        };
        let mut req = self.waits.get(&url).header("Accept", "text/event-stream");
        if let Some(id) = &self.events.last_id {
            req = req.header("Last-Event-ID", id);
        }
        // The body is taken inside, so its reads keep the switch.
        let reader = http::with_stop(&switch, || req.call().map(http::Response::into_reader))
            .map_err(|e| {
                Some(match e.status() {
                    Some(code) => format!("signal-cli's event stream failed (status {code})."),
                    None => self.unreachable(),
                })
            })?;
        report(None);
        let mut lines = std::io::BufReader::new(reader);
        let mut line = String::new();
        std::thread::scope(|s| {
            // Presses the switch once `stop` reads true, so a quiet stream
            // doesn't hold the worker.
            let watcher = s.spawn(|| {
                while !pressed.load(std::sync::atomic::Ordering::SeqCst) {
                    if stop() {
                        pressed.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            let out = loop {
                line.clear();
                match std::io::BufRead::read_line(&mut lines, &mut line) {
                    Ok(0) => break Err(Some("signal-cli closed its event stream.".to_string())),
                    Ok(_) => {}
                    Err(e) if http::is_stopped(&e) => break Ok(()),
                    Err(_) => break Err(Some("signal-cli's event stream broke off.".to_string())),
                }
                let Some(event) = self.events.line(line.trim_end_matches('\n')) else {
                    continue;
                };
                if let Err(why) = self.handle(config_dir, &event.data, grokhub_core::now_ms()) {
                    report(Some(&why));
                }
            };
            pressed.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = watcher.join();
            out
        })
    }

    /// One event's data: a Note to Self is queued for the app; anything else
    /// is dropped without a word.
    fn handle(&self, config_dir: &Path, data: &str, now_ms: u64) -> Result<(), String> {
        let Ok(data) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        let Some(text) = msg_signal::note_to_self(&data, &self.config.account) else {
            return Ok(());
        };
        let account = &self.config.account;
        // Paired when connected; checked again, so unpairing in the app stops
        // the queue at once.
        if !DmTrust::load(config_dir).is_paired(ChannelKind::Signal, account) {
            return Ok(());
        }
        msg_channels::queue_message(
            config_dir,
            &Forwarded {
                channel: ChannelKind::Signal,
                sender: account,
                label: msg_signal::SELF_LABEL,
                reply_to: account,
                text: &text,
            },
            now_ms,
        )
        .map(|_| ())
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
        assert!(save_token(&store, ChannelKind::Slack, TOKEN).is_err());
        let (app, bot) = SLACK_TOKENS.split_once(' ').unwrap();
        save_token(&store, ChannelKind::Slack, &format!("{bot}\n{app} ")).unwrap();
        assert_eq!(
            load_token(&store, ChannelKind::Slack)
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some(SLACK_TOKENS)
        );
        assert!(save_token(&store, ChannelKind::Signal, TOKEN).is_err());
        save_token(
            &store,
            ChannelKind::Signal,
            "+1 555 123 4567 http://localhost:8080",
        )
        .unwrap();
        assert_eq!(
            load_token(&store, ChannelKind::Signal)
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("+15551234567")
        );
        assert_eq!(
            save_token(&store, ChannelKind::Email, TOKEN).unwrap_err(),
            coming_next(ChannelKind::Email)
        );
        assert_eq!(load_token(&store, ChannelKind::Email).unwrap(), None);
        save_token(&store, ChannelKind::Discord, "MTIz.abc_DEF.ghi-jkl").unwrap();
        assert_eq!(
            store.0.lock().unwrap().keys().cloned().collect::<Vec<_>>(),
            vec![
                "channel-discord".to_string(),
                "channel-signal".to_string(),
                "channel-slack".to_string(),
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
        let (rest, me) = fake_api(vec![
            (200, json!({ "id": "1", "username": "my-app" })),
            (401, json!({ "message": "401: Unauthorized" })),
        ]);
        let link = DiscordLink::at(None, &rest, DISCORD_TOKEN, false).unwrap();
        assert_eq!(link.check(&dir).unwrap(), "my-app");
        assert_eq!(
            link.check(&dir).unwrap_err(),
            "Discord refused the bot token. Copy it again from the Bot page."
        );
        assert_eq!(
            me.join().unwrap()[0],
            format!("GET /users/@me HTTP/1.1 [auth: Bot {DISCORD_TOKEN}]\n")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    const SLACK_TOKENS: &str =
        "xapp-1-A0123456789-0123456789-abcdef0123 xoxb-0123456789-0123456789-abcdefABCDEF";

    /// A fake Socket Mode server: says hello, sends one stranger's DM twice
    /// (Slack's retry, same event id), collects both acks, then asks for a
    /// refresh so the link opens a new socket.
    fn fake_socket(listener: TcpListener) -> JoinHandle<Vec<Value>> {
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            ws.send(Message::text(json!({ "type": "hello" }).to_string()))
                .unwrap();
            let dm = |envelope: &str| {
                json!({ "type": "events_api", "envelope_id": envelope, "payload": {
                    "event_id": "Ev01",
                    "event": { "type": "message", "channel_type": "im", "channel": "D0DM",
                               "user": "U0STRANGER", "text": "hello", "ts": "1700000000.000100" },
                }})
            };
            let mut acks = Vec::new();
            for envelope in ["env-1", "env-2"] {
                ws.send(Message::text(dm(envelope).to_string())).unwrap();
                acks.push(loop {
                    if let Message::Text(t) = ws.read().unwrap() {
                        break serde_json::from_str(t.as_str()).unwrap();
                    }
                });
            }
            let refresh = json!({ "type": "disconnect", "reason": "refresh_requested" });
            ws.send(Message::text(refresh.to_string())).unwrap();
            let _ = ws.read();
            acks
        })
    }

    #[test]
    fn a_slack_stranger_gets_one_code_for_a_retried_event_and_refused_tokens_stop_the_run() {
        let dir = temp_dir("slack");
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let socket_url = format!("ws://{}/link/?ticket=t1", socket.local_addr().unwrap());
        let socket_server = fake_socket(socket);
        let (api, api_server) = fake_api(vec![
            (
                200,
                json!({ "ok": true, "user": "loadout", "user_id": "U0BOT" }),
            ),
            (200, json!({ "ok": true, "url": socket_url })),
            (200, json!({ "ok": true, "ts": "1700000000.000200" })),
            (200, json!({ "ok": false, "error": "invalid_auth" })),
        ]);
        let mut link = SlackLink::at(&api, SLACK_TOKENS, false).unwrap();
        let result = link.run(&dir, &|| false);
        let acks = socket_server.join().unwrap();
        let seen = api_server.join().unwrap();

        assert_eq!(
            acks,
            vec![
                json!({ "envelope_id": "env-1" }),
                json!({ "envelope_id": "env-2" })
            ]
        );
        let lines: Vec<&str> = seen.iter().map(|s| s.split_once('\n').unwrap().0).collect();
        assert_eq!(
            lines,
            vec![
                "POST /auth.test HTTP/1.1 [auth: Bearer xoxb-0123456789-0123456789-abcdefABCDEF]",
                "POST /apps.connections.open HTTP/1.1 [auth: Bearer xapp-1-A0123456789-0123456789-abcdef0123]",
                "POST /chat.postMessage HTTP/1.1 [auth: Bearer xoxb-0123456789-0123456789-abcdefABCDEF]",
                "POST /apps.connections.open HTTP/1.1 [auth: Bearer xapp-1-A0123456789-0123456789-abcdef0123]",
            ]
        );
        let code = DmTrust::load(&dir).pending[0].code.clone();
        let posted: Value = serde_json::from_str(seen[2].split_once('\n').unwrap().1).unwrap();
        assert_eq!(posted["channel"], "D0DM");
        assert_eq!(posted["text"], msg_telegram::pairing_text(&code));
        assert_eq!(link.last_problem(), None);
        assert_eq!(
            result,
            Err("Slack refused the tokens. Copy them again from your Slack app.".into())
        );

        // Only Slack's own host may hold the socket, and a lone token isn't enough.
        let (api, opened) = fake_api(vec![(
            200,
            json!({ "ok": true, "url": "wss://evil.example/link/" }),
        )]);
        let link = SlackLink::at(&api, SLACK_TOKENS, false).unwrap();
        assert_eq!(
            link.open_url(&dir).unwrap_err(),
            "Slack gave a socket address that isn't Slack's."
        );
        opened.join().unwrap();
        assert!(SlackLink::new("xoxb-0123456789-0123456789-abcdefABCDEF").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    const ME: &str = "+15551234567";

    /// One request the fake daemon saw: its line, `Last-Event-ID` and body.
    type Seen = (String, Option<String>, String);

    /// A fake signal-cli daemon on loopback. `answers` is served one per
    /// connection: `rpc` answers carry a JSON body, `events` a raw event
    /// stream. Hands back each request's line, its `Last-Event-ID` and its
    /// body, and sets `done` after the last one.
    fn fake_daemon(
        answers: Vec<(u16, String)>,
        done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> (String, JoinHandle<Vec<Seen>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let (mut length, mut last_id) = (0, None);
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
                        if name.eq_ignore_ascii_case("last-event-id") {
                            last_id = Some(value.trim().to_string());
                        }
                    }
                }
                let mut sent = vec![0; length];
                reader.read_exact(&mut sent).unwrap();
                seen.push((
                    line.trim_end().to_string(),
                    last_id,
                    String::from_utf8(sent).unwrap(),
                ));
                let kind = if line.contains("/events") {
                    "text/event-stream"
                } else {
                    "application/json"
                };
                let mut stream = reader.into_inner();
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            done.store(true, std::sync::atomic::Ordering::SeqCst);
            seen
        });
        (base, handle)
    }

    fn rpc_answer(result: Value) -> (u16, String) {
        (
            200,
            json!({"jsonrpc": "2.0", "id": 1, "result": result}).to_string(),
        )
    }

    fn event(id: u32, params: Value) -> String {
        let data = json!({"jsonrpc": "2.0", "method": "receive", "params": params});
        format!("event:receive\nid:{id}\ndata:{data}\n\n")
    }

    #[test]
    fn signal_reads_only_the_note_to_self_and_picks_up_where_the_stream_broke() {
        let dir = temp_dir("signal");
        let mut trust = DmTrust::default();
        trust.pair(ChannelKind::Signal, ME);
        trust.save(&dir).unwrap();
        let stream = [
            event(1, json!({"account": ME, "envelope": {
                "source": "+15559876543", "sourceNumber": "+15559876543",
                "dataMessage": {"message": "what's your pairing code?"}
            }})),
            ": keep-alive\n\n".to_string(),
            event(2, json!({"account": ME, "envelope": {
                "source": ME, "sourceNumber": ME, "sourceUuid": "aci-me",
                "syncMessage": {"sentMessage": {"destinationNumber": ME, "message": "plan my week"}}
            }})),
        ]
        .concat();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (base, daemon) = fake_daemon(vec![(200, stream), (503, String::new())], done.clone());
        let mut link = SignalLink::new(&format!("{ME} {base}")).unwrap();
        let problems = Mutex::new(Vec::new());
        let result = link.run(
            &dir,
            &|| done.load(std::sync::atomic::Ordering::SeqCst),
            &|p| problems.lock().unwrap().push(p.map(str::to_string)),
        );
        let seen = daemon.join().unwrap();

        assert_eq!(result, Ok(()));
        assert_eq!(seen[0].0, "GET /api/v1/events HTTP/1.1");
        assert_eq!(seen[0].1, None);
        // The second stream asks the daemon to replay from the last event.
        assert_eq!(seen[1].1.as_deref(), Some("2"));
        let queued: Vec<_> = std::fs::read_dir(msg_channels::inbox_dir(&dir))
            .unwrap()
            .collect();
        assert_eq!(queued.len(), 1, "only the Note to Self is queued");
        let saved = std::fs::read_to_string(queued[0].as_ref().unwrap().path()).unwrap();
        assert!(
            saved.contains("plan my week") && !saved.contains("pairing code"),
            "{saved}"
        );
        // Nothing was ever sent, and the stranger got no code.
        assert!(seen.iter().all(|s| !s.0.starts_with("POST")));
        assert!(DmTrust::load(&dir).pending.is_empty());
        let problems = problems.into_inner().unwrap();
        assert_eq!(problems.first(), Some(&None));
        assert!(problems.contains(&Some("signal-cli closed its event stream.".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signal_checks_the_daemon_knows_the_account_and_answers_only_itself() {
        let dir = temp_dir("signal-send");
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (base, daemon) = fake_daemon(
            vec![
                (200, String::new()),
                rpc_answer(json!([{"number": ME}])),
                (200, String::new()),
                rpc_answer(json!([{"number": "+15550000000"}])),
                rpc_answer(json!({"timestamp": 9})),
            ],
            done,
        );
        let link = SignalLink::new(&format!("{ME} {base}")).unwrap();
        assert_eq!(link.check(&dir).unwrap(), ME);
        let other = SignalLink::new(&format!("+15551112222 {base}")).unwrap();
        assert_eq!(
            other.check(&dir).unwrap_err(),
            "signal-cli isn't linked to +15551112222. Link it first, then connect again."
        );
        assert_eq!(
            link.send(&dir, "+15559876543", "hi", &[]).unwrap_err(),
            "Signal answers go only to your own Note to Self."
        );
        link.send(&dir, ME, "done", &[DataClass::Chat]).unwrap();
        let seen = daemon.join().unwrap();
        let lines: Vec<&str> = seen.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(
            lines,
            [
                "GET /api/v1/check HTTP/1.1",
                "POST /api/v1/rpc HTTP/1.1",
                "GET /api/v1/check HTTP/1.1",
                "POST /api/v1/rpc HTTP/1.1",
                "POST /api/v1/rpc HTTP/1.1",
            ]
        );
        let listed: Value = serde_json::from_str(&seen[1].2).unwrap();
        assert_eq!(listed["method"], "listAccounts");
        assert_eq!(listed["params"]["account"], ME);
        let sent: Value = serde_json::from_str(&seen[4].2).unwrap();
        assert_eq!(sent["method"], "send");
        assert_eq!(
            sent["params"],
            json!({"recipient": [ME], "message": "done", "account": ME})
        );

        // A daemon that isn't running, and one that isn't on this computer.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let gone = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let err = SignalLink::new(&format!("{ME} {gone}"))
            .unwrap()
            .check(&dir)
            .unwrap_err();
        assert!(
            err.starts_with("signal-cli isn't answering at http://127.0.0.1:"),
            "{err}"
        );
        assert!(err.ends_with(msg_signal::DAEMON_COMMAND), "{err}");
        assert!(SignalLink::new(&format!("{ME} http://192.168.1.4:8080")).is_err());
        assert!(SignalLink::daemon("http://example.com:8080").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
