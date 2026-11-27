//! The email channel's network side: a small IMAP reader and an SMTP sender,
//! both on TLS from the first byte (993 and 465), signed in with the
//! mailbox's app password from the keychain.
//!
//! Every connection runs the egress guard first, dials only public
//! addresses, and is checked against the webpki roots. The mailbox is opened
//! read-only (`EXAMINE`) and messages are fetched with `BODY.PEEK[]`, so
//! nothing is marked read or changed. What a message must pass before it is
//! queued is in [`msg_email`]; answers go only to a sender still allowed.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_core::msg_channels::{self, ChannelKind, DmTrust, Forwarded};
use grokhub_core::msg_email::{self, Config};
use zeroize::Zeroizing;

use crate::harness::{guard_quiet, DataClass, EgressReq};

/// How often the mailbox is read.
pub const POLL_EVERY: Duration = Duration::from_secs(60);
const CONNECT: Duration = Duration::from_secs(15);
/// One read or write on an open connection.
const IO: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(900);
/// Messages read in one pass; the rest wait for the next.
const MAX_FETCH: usize = 20;
/// A bigger message is skipped. Attachments aren't read in this version.
const MAX_SIZE: usize = 2 * 1024 * 1024;
/// The longest line a server may send outside a literal.
const MAX_LINE: usize = 64 * 1024;

/// The person's own mailbox.
pub struct EmailLink {
    config: Config,
    password: Zeroizing<String>,
}

impl EmailLink {
    /// `saved` as the keychain keeps it, or as typed: the address, any
    /// servers, then the app password.
    pub fn new(saved: &str) -> Result<Self, String> {
        let (config, password) = msg_email::parse_setup(saved)?;
        Ok(Self {
            config,
            password: Zeroizing::new(password),
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// What the keychain keeps.
    pub fn saved(&self) -> Zeroizing<String> {
        Zeroizing::new(msg_email::saved_setup(&self.config, &self.password))
    }

    /// Sign in over IMAP and open the inbox. Answers the address, and sets
    /// the place reading starts from to now, so older mail isn't answered.
    pub fn check(&self, config_dir: &Path) -> Result<String, String> {
        let mut imap = self.imap(config_dir)?;
        let mailbox = imap.open_inbox(&self.config.address, &self.password)?;
        Cursor::start(mailbox).save(config_dir)?;
        imap.logout();
        Ok(self.config.address.clone())
    }

    /// Read what arrived since the last pass and queue what passes
    /// [`msg_email::incoming`]. Answers how many were queued.
    pub fn poll(&self, config_dir: &Path, now_ms: u64) -> Result<usize, String> {
        let mut imap = self.imap(config_dir)?;
        let queued = self.read_new(config_dir, &mut imap, now_ms);
        imap.logout();
        queued
    }

    fn read_new<S: Read + Write>(
        &self,
        config_dir: &Path,
        imap: &mut Imap<S>,
        now_ms: u64,
    ) -> Result<usize, String> {
        let mailbox = imap.open_inbox(&self.config.address, &self.password)?;
        let mut cursor = match Cursor::load(config_dir) {
            Some(c) if c.validity == mailbox.validity => c,
            // A first pass, or the server renumbered the inbox: start from now.
            _ => return Cursor::start(mailbox).save(config_dir).map(|()| 0),
        };
        if mailbox.next <= cursor.last + 1 {
            return Ok(0);
        }
        let mut sizes = imap.sizes_after(cursor.last)?;
        sizes.sort_unstable();
        let mut queued = 0;
        for (uid, size) in sizes.into_iter().take(MAX_FETCH) {
            if size <= MAX_SIZE {
                if let Some(raw) = imap.body(uid)? {
                    queued += usize::from(self.handle(config_dir, &raw, now_ms)?);
                }
            }
            cursor.last = uid;
            cursor.save(config_dir)?;
        }
        Ok(queued)
    }

    /// One message: queued when its sender is still allowed and their
    /// server vouched for them; dropped without a word otherwise.
    fn handle(&self, config_dir: &Path, raw: &[u8], now_ms: u64) -> Result<bool, String> {
        let trust = DmTrust::load(config_dir);
        let allowed = |sender: &str| trust.is_paired(ChannelKind::Email, sender);
        let Some(msg) = msg_email::incoming(raw, &self.config, allowed) else {
            return Ok(false);
        };
        msg_channels::queue_message(
            config_dir,
            &Forwarded {
                channel: ChannelKind::Email,
                sender: &msg.sender,
                label: &msg.sender,
                reply_to: &msg.thread(),
                text: &msg.prompt_text(),
            },
            now_ms,
        )
        .map(|_| true)
    }

    /// Answer `to` under the thread a queued message kept. `to` must be an
    /// allowed sender; the caller checks it is still paired.
    pub fn send(
        &self,
        config_dir: &Path,
        to: &str,
        thread: &str,
        text: &str,
        data: &[DataClass],
    ) -> Result<(), String> {
        if !msg_email::valid_address(to) {
            return Err("That isn't an email address.".into());
        }
        let thread = msg_email::parse_thread(thread);
        let mut random = [0u8; 16];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut random)
            .map_err(|_| "This computer couldn't make a message id.".to_string())?;
        let unix = grokhub_core::now_ms() / 1000;
        let mail = msg_email::Reply {
            from: &self.config.address,
            to,
            subject: &thread.subject,
            in_reply_to: &thread.in_reply_to,
            references: &thread.references,
            text,
            message_id: &msg_email::message_id(&random, self.config.domain()),
            date: &msg_email::rfc5322_date(unix),
        }
        .render();
        let stream = self.dial(
            config_dir,
            "smtps",
            &self.config.smtp_host,
            msg_email::SMTP_PORT,
            data,
        )?;
        Smtp::new(stream).send(&self.config.address, &self.password, to, &mail)
    }

    /// Read the mailbox every [`POLL_EVERY`] until `stop` says so, waiting
    /// longer after each failed pass. `report` gets the current problem, or
    /// `None` after a pass that worked. `Err` only when the egress guard
    /// refuses.
    pub fn run(
        &self,
        config_dir: &Path,
        stop: &(dyn Fn() -> bool + Sync),
        report: &dyn Fn(Option<&str>),
    ) -> Result<(), String> {
        let mut wait = POLL_EVERY;
        while !stop() {
            match self.poll(config_dir, grokhub_core::now_ms()) {
                Ok(_) => {
                    report(None);
                    wait = POLL_EVERY;
                }
                Err(why) if why.starts_with(GUARD) => return Err(why),
                Err(why) => {
                    report(Some(&why));
                    wait = (wait * 2).min(MAX_BACKOFF);
                }
            }
            nap(stop, wait);
        }
        Ok(())
    }

    fn imap(&self, config_dir: &Path) -> Result<Imap<Tls>, String> {
        let stream = self.dial(
            config_dir,
            "imaps",
            &self.config.imap_host,
            msg_email::IMAP_PORT,
            &[],
        )?;
        let mut imap = Imap::new(stream);
        imap.greeting()?;
        Ok(imap)
    }

    /// The guard, then TCP to a public address, then TLS checked against the
    /// webpki roots for `host`.
    fn dial(
        &self,
        config_dir: &Path,
        scheme: &str,
        host: &str,
        port: u16,
        data: &[DataClass],
    ) -> Result<Tls, String> {
        guard_quiet(
            config_dir,
            &EgressReq::new(&format!("{scheme}://{host}"), data),
        )
        .map_err(|why| format!("{GUARD}{why}"))?;
        let unreachable = format!("{host} isn't answering on port {port}.");
        let mut tcp = None;
        for addr in (host, port)
            .to_socket_addrs()
            .map_err(|_| format!("{host} wasn't found."))?
            .filter(|a| public(a.ip()))
            .take(4)
        {
            if let Ok(s) = TcpStream::connect_timeout(&addr, CONNECT) {
                tcp = Some(s);
                break;
            }
        }
        let tcp = tcp.ok_or(unreachable)?;
        tcp.set_read_timeout(Some(IO))
            .and_then(|()| tcp.set_write_timeout(Some(IO)))
            .map_err(|e| e.to_string())?;
        let name = rustls::pki_types::ServerName::try_from(host.to_string())
            .map_err(|_| format!("{host} isn't a server name."))?;
        let conn = rustls::ClientConnection::new(crate::ws_mode::tls_config()?, name)
            .map_err(|e| e.to_string())?;
        Ok(rustls::StreamOwned::new(conn, tcp))
    }
}

type Tls = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

/// The start of an error that means the egress guard refused.
const GUARD: &str = "The egress guard refused: ";

/// A public unicast address: mail servers never sit on this computer or the
/// local network.
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                // Carrier-grade NAT, 100.64.0.0/10.
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64))
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| !public(IpAddr::V4(v4))))
        }
    }
}

fn nap(stop: &(dyn Fn() -> bool + Sync), total: Duration) {
    let until = std::time::Instant::now() + total;
    while !stop() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Where reading starts: the inbox's `UIDVALIDITY`, and the last UID read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cursor {
    validity: u64,
    last: u64,
}

impl Cursor {
    /// Everything already in the inbox counts as read.
    fn start(mailbox: Mailbox) -> Self {
        Self {
            validity: mailbox.validity,
            last: mailbox.next.saturating_sub(1),
        }
    }

    fn path(config_dir: &Path) -> PathBuf {
        config_dir.join("channels").join("email-cursor.json")
    }

    fn load(config_dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(Self::path(config_dir)).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        Some(Self {
            validity: v.get("validity")?.as_u64()?,
            last: v.get("last")?.as_u64()?,
        })
    }

    fn save(self, config_dir: &Path) -> Result<(), String> {
        let path = Self::path(config_dir);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::json!({"validity": self.validity, "last": self.last}).to_string();
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text)
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| e.to_string())
    }
}

/// The inbox as `EXAMINE` describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mailbox {
    validity: u64,
    next: u64,
}

/// One server response: its text with each literal left as `{n}`, and the
/// literals' bytes in order.
#[derive(Debug, Default, PartialEq, Eq)]
struct Response {
    text: String,
    literals: Vec<Vec<u8>>,
}

/// The IMAP commands this channel uses, over any stream (TLS in the app,
/// a canned transcript in tests).
struct Imap<S: Read + Write> {
    io: BufReader<S>,
    tag: u32,
}

impl<S: Read + Write> Imap<S> {
    fn new(stream: S) -> Self {
        Self {
            io: BufReader::new(stream),
            tag: 0,
        }
    }

    fn greeting(&mut self) -> Result<(), String> {
        let line = self.response()?;
        if line.text.starts_with("* OK") || line.text.starts_with("* PREAUTH") {
            Ok(())
        } else {
            Err("The IMAP server turned the connection away.".into())
        }
    }

    /// One line, with any literals it announces read in full.
    fn response(&mut self) -> Result<Response, String> {
        let mut out = Response::default();
        loop {
            let mut line = Vec::new();
            let n = (&mut self.io)
                .take(MAX_LINE as u64)
                .read_until(b'\n', &mut line)
                .map_err(|_| "The IMAP server stopped answering.".to_string())?;
            if n == 0 || !line.ends_with(b"\n") {
                return Err("The IMAP server closed the connection.".into());
            }
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            out.text.push_str(line);
            let Some(size) = literal_size(line) else {
                return Ok(out);
            };
            if size > MAX_SIZE + MAX_LINE {
                return Err("The IMAP server sent more than this channel reads.".into());
            }
            let mut bytes = vec![0; size];
            self.io
                .read_exact(&mut bytes)
                .map_err(|_| "The IMAP server stopped mid-message.".to_string())?;
            out.literals.push(bytes);
        }
    }

    /// Send one command and gather its untagged responses. A `NO` or `BAD`
    /// comes back as the error.
    fn command(&mut self, command: &str) -> Result<Vec<Response>, String> {
        self.tag += 1;
        let tag = format!("a{}", self.tag);
        let stream = self.io.get_mut();
        stream
            .write_all(format!("{tag} {command}\r\n").as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|_| "The IMAP server stopped answering.".to_string())?;
        let mut untagged = Vec::new();
        loop {
            let response = self.response()?;
            let Some(rest) = response.text.strip_prefix(&format!("{tag} ")) else {
                untagged.push(response);
                continue;
            };
            let verb = command.split_whitespace().next().unwrap_or("");
            return match rest.split_whitespace().next() {
                Some("OK") => Ok(untagged),
                _ => Err(format!(
                    "The IMAP server refused {verb}: {}",
                    rest.split_once(' ').map_or("", |(_, why)| why).trim()
                )),
            };
        }
    }

    /// `LOGIN`, then `EXAMINE INBOX` (read-only).
    fn open_inbox(&mut self, user: &str, password: &str) -> Result<Mailbox, String> {
        let login = format!("LOGIN {} {}", quoted(user)?, quoted(password)?);
        self.command(&login).map_err(|why| {
            format!("The mail server didn't accept the address and app password. {why}")
        })?;
        let answer = self.command("EXAMINE INBOX")?;
        let code = |name: &str| answer.iter().find_map(|r| response_code(&r.text, name));
        let validity = code("UIDVALIDITY").ok_or("The IMAP server didn't say UIDVALIDITY.")?;
        let next = match code("UIDNEXT") {
            Some(next) => next,
            None => self.highest_uid()? + 1,
        };
        Ok(Mailbox { validity, next })
    }

    /// The highest UID in the mailbox, for a server that sends no UIDNEXT.
    fn highest_uid(&mut self) -> Result<u64, String> {
        Ok(self
            .command("UID FETCH * (UID)")
            .unwrap_or_default()
            .iter()
            .filter_map(|r| fetch_number(&r.text, "UID"))
            .max()
            .unwrap_or(0))
    }

    /// The UID and size of every message after `last`.
    fn sizes_after(&mut self, last: u64) -> Result<Vec<(u64, usize)>, String> {
        let answer = self.command(&format!("UID FETCH {}:* (UID RFC822.SIZE)", last + 1))?;
        Ok(answer
            .iter()
            .filter_map(|r| {
                let uid = fetch_number(&r.text, "UID")?;
                let size = fetch_number(&r.text, "RFC822.SIZE")?;
                // `n:*` always names the newest message, even one before `n`.
                (uid > last).then(|| (uid, usize::try_from(size).unwrap_or(usize::MAX)))
            })
            .collect())
    }

    /// One message's bytes, leaving it unread.
    fn body(&mut self, uid: u64) -> Result<Option<Vec<u8>>, String> {
        let answer = self.command(&format!("UID FETCH {uid} (BODY.PEEK[])"))?;
        Ok(answer
            .into_iter()
            .find(|r| fetch_number(&r.text, "UID") == Some(uid) && r.text.contains("BODY[]"))
            .and_then(|r| r.literals.into_iter().next()))
    }

    fn logout(&mut self) {
        let _ = self.command("LOGOUT");
    }
}

/// `{123}` or `{123+}` at a line's end: the literal that follows.
fn literal_size(line: &str) -> Option<usize> {
    let open = line.strip_suffix('}')?.rfind('{')?;
    line[open + 1..line.len() - 1]
        .trim_end_matches('+')
        .parse()
        .ok()
}

/// A number in a response code, like `[UIDVALIDITY 3857529045]`.
fn response_code(text: &str, name: &str) -> Option<u64> {
    let at = text.find(&format!("[{name} "))?;
    text[at + name.len() + 2..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// A number after an item name in a `FETCH` response, like `UID 41`.
fn fetch_number(text: &str, name: &str) -> Option<u64> {
    let list = text.split_once(" FETCH (")?.1;
    let mut words = list.split([' ', '(', ')']);
    while let Some(word) = words.next() {
        if word.eq_ignore_ascii_case(name) {
            return words.next()?.parse().ok();
        }
    }
    None
}

/// An IMAP quoted string. CR, LF and anything outside ASCII can't be sent
/// in one, so they are refused rather than mangled.
fn quoted(text: &str) -> Result<String, String> {
    if !text.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        return Err("The address or app password has characters IMAP can't send.".into());
    }
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// SMTP submission over any stream.
struct Smtp<S: Read + Write> {
    io: BufReader<S>,
}

impl<S: Read + Write> Smtp<S> {
    fn new(stream: S) -> Self {
        Self {
            io: BufReader::new(stream),
        }
    }

    /// One reply, joining `250-` continuation lines: its code and its text.
    fn reply(&mut self) -> Result<(u16, String), String> {
        let mut text = String::new();
        loop {
            let mut line = Vec::new();
            let n = (&mut self.io)
                .take(MAX_LINE as u64)
                .read_until(b'\n', &mut line)
                .map_err(|_| "The SMTP server stopped answering.".to_string())?;
            if n == 0 {
                return Err("The SMTP server closed the connection.".into());
            }
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            let code: u16 = line
                .get(..3)
                .and_then(|c| c.parse().ok())
                .ok_or("The SMTP server answered in a way this channel can't read.")?;
            text.push_str(line.get(4..).unwrap_or(""));
            if line.as_bytes().get(3) != Some(&b'-') {
                return Ok((code, text));
            }
            text.push(' ');
        }
    }

    fn step(&mut self, line: Option<&str>, ok: &[u16], what: &str) -> Result<(), String> {
        if let Some(line) = line {
            let stream = self.io.get_mut();
            stream
                .write_all(line.as_bytes())
                .and_then(|()| stream.write_all(b"\r\n"))
                .and_then(|()| stream.flush())
                .map_err(|_| "The SMTP server stopped answering.".to_string())?;
        }
        let (code, text) = self.reply()?;
        if ok.contains(&code) {
            Ok(())
        } else {
            Err(format!("The SMTP server refused {what} ({code}): {text}"))
        }
    }

    fn send(mut self, from: &str, password: &str, to: &str, mail: &str) -> Result<(), String> {
        let plain = Zeroizing::new(format!("\0{from}\0{password}"));
        let auth = Zeroizing::new(format!(
            "AUTH PLAIN {}",
            msg_email::base64_encode(plain.as_bytes())
        ));
        self.step(None, &[220], "the connection")?;
        self.step(Some("EHLO localhost"), &[250], "EHLO")?;
        self.step(Some(&auth), &[235], "the address and app password")?;
        self.step(Some(&format!("MAIL FROM:<{from}>")), &[250], "the sender")?;
        self.step(
            Some(&format!("RCPT TO:<{to}>")),
            &[250, 251],
            "the recipient",
        )?;
        self.step(Some("DATA"), &[354], "the message")?;
        // A line that starts with a dot gets a second one (RFC 5321 4.5.2).
        let body: Vec<String> = mail
            .trim_end_matches("\r\n")
            .split("\r\n")
            .map(|l| {
                if l.starts_with('.') {
                    format!(".{l}")
                } else {
                    l.to_string()
                }
            })
            .collect();
        self.step(
            Some(&format!("{}\r\n.", body.join("\r\n"))),
            &[250],
            "the message",
        )?;
        let _ = self.step(Some("QUIT"), &[221], "QUIT");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// A server that answers each line the client writes with the next
    /// scripted reply, and keeps what was written. Inside SMTP's DATA only
    /// the closing dot is answered.
    struct Script {
        replies: VecDeque<Vec<u8>>,
        pending: VecDeque<u8>,
        written: Arc<Mutex<Vec<u8>>>,
        partial: Vec<u8>,
        in_data: bool,
    }

    impl Script {
        fn new(greeting: &[u8], replies: &[&[u8]]) -> (Self, Arc<Mutex<Vec<u8>>>) {
            let written = Arc::new(Mutex::new(Vec::new()));
            let script = Self {
                replies: replies.iter().map(|r| r.to_vec()).collect(),
                pending: greeting.iter().copied().collect(),
                written: written.clone(),
                partial: Vec::new(),
                in_data: false,
            };
            (script, written)
        }
    }

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.pending.len());
            for b in buf.iter_mut().take(n) {
                *b = self.pending.pop_front().unwrap_or(0);
            }
            Ok(n)
        }
    }

    impl Write for Script {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.lock().unwrap().extend_from_slice(buf);
            self.partial.extend_from_slice(buf);
            while let Some(at) = self.partial.windows(2).position(|w| w == b"\r\n") {
                let line: Vec<u8> = self.partial.drain(..at + 2).collect();
                let answer = if self.in_data {
                    self.in_data = line != b".\r\n";
                    !self.in_data
                } else {
                    self.in_data = line == b"DATA\r\n";
                    true
                };
                if answer && let Some(reply) = self.replies.pop_front() {
                    self.pending.extend(reply);
                }
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn written(w: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(w.lock().unwrap().clone()).unwrap()
    }

    const PASS: &str =
        "mx.example.com; dkim=pass header.i=@example.com; dmarc=pass header.from=example.com";

    fn mail(from: &str, text: &str) -> String {
        format!(
            "Authentication-Results: {PASS}\r\nFrom: {from}\r\nSubject: Plans\r\nMessage-ID: <m7@example.com>\r\n\r\n{text}\r\n"
        )
    }

    fn examine(next: u64) -> Vec<u8> {
        format!("* 3 EXISTS\r\n* OK [UIDVALIDITY 77] ok\r\n* OK [UIDNEXT {next}] ok\r\na2 OK [READ-ONLY] done\r\n").into_bytes()
    }

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("grokhub-email-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn link() -> EmailLink {
        EmailLink::new(
            "me@example.com imap.example.com smtp.example.com mx.example.com abcd efgh ijkl mnop",
        )
        .unwrap()
    }

    #[test]
    fn the_first_pass_starts_from_now_and_later_ones_read_only_allowed_mail() {
        let config_dir = dir("poll");
        let link = link();
        // First pass: nothing is read, the place is set.
        let (s, out) = Script::new(b"* OK ready\r\n", &[b"a1 OK hi\r\n", &examine(10)]);
        let mut imap = Imap::new(s);
        imap.greeting().unwrap();
        assert_eq!(link.read_new(&config_dir, &mut imap, 1).unwrap(), 0);
        assert!(written(&out)
            .contains("a1 LOGIN \"me@example.com\" \"abcdefghijklmnop\"\r\na2 EXAMINE INBOX\r\n"));
        assert_eq!(
            Cursor::load(&config_dir),
            Some(Cursor {
                validity: 77,
                last: 9
            })
        );

        let mut trust = DmTrust::load(&config_dir);
        trust.pair(ChannelKind::Email, "jane@example.com");
        trust.save(&config_dir).unwrap();
        let ok = mail("Jane <jane@example.com>", "Noon works?");
        let stranger = mail("eve@example.com", "Hi");
        let fetch_ok = format!(
            "* 1 FETCH (UID 10 BODY[] {{{}}}\r\n{ok})\r\na4 OK done\r\n",
            ok.len()
        );
        let fetch_stranger = format!(
            "* 2 FETCH (BODY[] {{{}}}\r\n{stranger} UID 12)\r\na5 OK done\r\n",
            stranger.len()
        );
        let (s, out) = Script::new(
            b"* OK ready\r\n",
            &[
                b"a1 OK hi\r\n",
                &examine(14),
                // 13 is too big, and 9 is the newest-message echo of `10:*`.
                b"* 1 FETCH (UID 10 RFC822.SIZE 300)\r\n* 2 FETCH (UID 12 RFC822.SIZE 200)\r\n* 3 FETCH (UID 13 RFC822.SIZE 9999999)\r\n* 0 FETCH (UID 9 RFC822.SIZE 5)\r\na3 OK done\r\n",
                fetch_ok.as_bytes(),
                fetch_stranger.as_bytes(),
            ],
        );
        let mut imap = Imap::new(s);
        imap.greeting().unwrap();
        assert_eq!(link.read_new(&config_dir, &mut imap, 2).unwrap(), 1);
        let sent = written(&out);
        assert!(sent.contains("a3 UID FETCH 10:* (UID RFC822.SIZE)\r\n"));
        assert!(sent.contains("a4 UID FETCH 10 (BODY.PEEK[])\r\na5 UID FETCH 12 (BODY.PEEK[])\r\n"));
        assert!(
            !sent.contains("FETCH 13 "),
            "a message over the cap isn't fetched"
        );
        assert_eq!(
            Cursor::load(&config_dir),
            Some(Cursor {
                validity: 77,
                last: 13
            })
        );
        let queued: Vec<_> = std::fs::read_dir(msg_channels::inbox_dir(&config_dir))
            .unwrap()
            .filter_map(|e| std::fs::read_to_string(e.ok()?.path()).ok())
            .collect();
        assert_eq!(queued.len(), 1);
        let record: serde_json::Value = serde_json::from_str(&queued[0]).unwrap();
        assert_eq!(record["sender"], "jane@example.com");
        assert_eq!(record["text"], "Subject: Plans\n\nNoon works?");
        assert_eq!(
            record["replyTo"],
            "<m7@example.com>\n<m7@example.com>\nPlans"
        );
    }

    #[test]
    fn a_renumbered_inbox_starts_again_from_now() {
        let config_dir = dir("validity");
        Cursor {
            validity: 5,
            last: 400,
        }
        .save(&config_dir)
        .unwrap();
        let (s, _) = Script::new(b"* OK ready\r\n", &[b"a1 OK hi\r\n", &examine(3)]);
        let mut imap = Imap::new(s);
        imap.greeting().unwrap();
        assert_eq!(link().read_new(&config_dir, &mut imap, 1).unwrap(), 0);
        assert_eq!(
            Cursor::load(&config_dir),
            Some(Cursor {
                validity: 77,
                last: 2
            })
        );
    }

    #[test]
    fn a_refused_sign_in_says_so_and_reads_nothing() {
        let config_dir = dir("refused");
        let (s, out) = Script::new(
            b"* OK ready\r\n",
            &[b"a1 NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)\r\n"],
        );
        let mut imap = Imap::new(s);
        imap.greeting().unwrap();
        let why = link().read_new(&config_dir, &mut imap, 1).unwrap_err();
        assert!(
            why.starts_with("The mail server didn't accept the address and app password."),
            "{why}"
        );
        assert!(!written(&out).contains("EXAMINE"));
        assert!(quoted("pa\"ss\\").unwrap() == "\"pa\\\"ss\\\\\"");
        assert!(quoted("line\r\nbreak").is_err());
    }

    #[test]
    fn smtp_signs_in_threads_the_reply_and_stuffs_dots() {
        let (s, out) = Script::new(
            b"220 smtp.gmail.com ESMTP ready\r\n",
            &[
                b"250-smtp.gmail.com at your service\r\n250-AUTH LOGIN PLAIN\r\n250 SMTPUTF8\r\n",
                b"235 2.7.0 Accepted\r\n",
                b"250 2.1.0 OK\r\n",
                b"250 2.1.5 OK\r\n",
                b"354 Go ahead\r\n",
                b"250 2.0.0 OK queued\r\n",
                b"221 2.0.0 closing\r\n",
            ],
        );
        let mail = "From: me@example.com\r\nSubject: Re: Plans\r\n\r\n.hidden line\r\nok\r\n";
        Smtp::new(s)
            .send("me@example.com", "pw", "jane@example.com", mail)
            .unwrap();
        let sent = written(&out);
        let auth = msg_email::base64_encode(b"\0me@example.com\0pw");
        assert!(sent.starts_with(&format!("EHLO localhost\r\nAUTH PLAIN {auth}\r\nMAIL FROM:<me@example.com>\r\nRCPT TO:<jane@example.com>\r\nDATA\r\n")));
        assert!(sent.contains("\r\n\r\n..hidden line\r\nok\r\n.\r\nQUIT\r\n"));
    }

    #[test]
    fn smtp_refusals_carry_the_servers_words() {
        let (s, _) = Script::new(
            b"220 ready\r\n",
            &[
                b"250 hi\r\n",
                b"535-5.7.8 Username and Password not accepted.\r\n535 5.7.8 Learn more\r\n",
            ],
        );
        let why = Smtp::new(s)
            .send("me@example.com", "pw", "jane@example.com", "x")
            .unwrap_err();
        assert_eq!(
            why,
            "The SMTP server refused the address and app password (535): 5.7.8 Username and Password not accepted. 5.7.8 Learn more"
        );
    }

    #[test]
    fn parses_literals_response_codes_and_fetch_items() {
        assert_eq!(literal_size("* 1 FETCH (UID 4 BODY[] {12}"), Some(12));
        assert_eq!(literal_size("* 1 FETCH (UID 4 BODY[] {12+}"), Some(12));
        assert_eq!(literal_size("* OK done"), None);
        assert_eq!(
            response_code("* OK [UIDVALIDITY 3857529045] UIDs valid", "UIDVALIDITY"),
            Some(3_857_529_045)
        );
        assert_eq!(
            fetch_number("* 9 FETCH (RFC822.SIZE 44 UID 7)", "UID"),
            Some(7)
        );
        assert_eq!(
            fetch_number("* 9 FETCH (FLAGS (\\Seen) UID 7)", "UID"),
            Some(7)
        );
        assert_eq!(fetch_number("* 9 EXISTS", "UID"), None);
    }

    #[test]
    fn only_public_addresses_are_dialed() {
        for local in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.9",
            "169.254.1.1",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!public(local.parse().unwrap()), "{local}");
        }
        for remote in ["142.250.1.108", "2607:f8b0:4004::6c"] {
            assert!(public(remote.parse().unwrap()), "{remote}");
        }
    }

    #[test]
    fn an_answer_goes_only_to_a_valid_address() {
        let config_dir = dir("send");
        let why = link()
            .send(
                &config_dir,
                "jane@example.com\r\nRCPT TO:<eve@evil.test>",
                "",
                "hi",
                &[],
            )
            .unwrap_err();
        assert_eq!(why, "That isn't an email address.");
    }
}
