//! RemoteDesktop portal + libei input. Portal calls stay on one worker thread.
//! Tests use [`FakePortal`] and never open a session bus.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use grokhub_core::desktop_mcp::{CastStream, EisRegion};

const SERVICE: &str = "GrokHub";
const ACCOUNT: &str = "desktop-portal-restore";
const TOKEN_RETRY: &str = "KDE asked again because the monitors changed.";

type RdPortal = ashpd::desktop::remote_desktop::RemoteDesktop;
type RdSession = ashpd::desktop::Session<RdPortal>;

/// Devices and regions from one successful `Start` + EIS bind.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PortalStart {
    pub restore_token: Option<String>,
    pub regions: Vec<EisRegion>,
    pub text: bool,
    pub keyboard: bool,
    /// ScreenCast streams from the same `Start`. Notify* uses these.
    pub streams: Vec<CastStream>,
    /// ConnectToEIS failed and this session stays open for Notify*.
    pub notify: bool,
}

/// What [`open_with_restore`] hands the Wayland backend.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OpenedPortal {
    pub regions: Vec<EisRegion>,
    pub text: bool,
    pub keyboard: bool,
    pub streams: Vec<CastStream>,
    pub notify: bool,
    /// Set when a stored token was rejected and a fresh dialog succeeded.
    pub notice: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PortalFail {
    /// User cancel, portal error, or halt. The caller may use ydotool.
    Denied(String),
    /// The restore token was refused. Retry `Start` once without it.
    TokenRejected(String),
    /// ConnectToEIS or the absolute device is unavailable.
    Fallback(String),
}

impl PortalFail {
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::Denied(text) | Self::TokenRejected(text) | Self::Fallback(text) => text,
        }
    }
}

pub(crate) trait RestoreStore: Send {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, token: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

pub(crate) trait RemoteDesktopPortal {
    fn start(&mut self, restore_token: Option<String>) -> Result<PortalStart, PortalFail>;
    fn close(&mut self);
}

pub(crate) trait EisInput {
    fn pointer_absolute(&mut self, x: f32, y: f32) -> Result<(), String>;
    fn button(&mut self, code: u32, down: bool) -> Result<(), String>;
    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String>;
    fn key(&mut self, code: u32, down: bool) -> Result<(), String>;
    fn text(&mut self, text: &str) -> Result<(), String>;
}

pub(crate) trait NotifyInput: Send {
    fn notify_pointer(&mut self, stream: u32, x: f64, y: f64) -> Result<(), String>;
    fn notify_button(&mut self, button: i32, down: bool) -> Result<(), String>;
    fn notify_axis(&mut self, horizontal: bool, steps: i32) -> Result<(), String>;
    fn notify_keysym(&mut self, keysym: i32, down: bool) -> Result<(), String>;
}

pub(crate) trait PortalDevice: RemoteDesktopPortal + EisInput + NotifyInput + Send {}
impl<T> PortalDevice for T where T: RemoteDesktopPortal + EisInput + NotifyInput + Send {}

/// Load the single-use token, `Start`, and replace it with the token from this Start.
/// A rejected token starts one fresh session and says why.
pub(crate) fn open_with_restore(
    portal: &mut dyn RemoteDesktopPortal,
    store: &dyn RestoreStore,
) -> Result<OpenedPortal, PortalFail> {
    let saved = match store.load() {
        Ok(token) => token.filter(|value| !value.trim().is_empty()),
        Err(err) => {
            eprintln!("desktop-mcp: restore token: {err}");
            None
        }
    };
    let had = saved.is_some();
    match portal.start(saved) {
        Ok(start) => {
            remember(store, start.restore_token.as_deref(), had);
            Ok(opened(start, None))
        }
        Err(PortalFail::TokenRejected(why)) if had => {
            eprintln!("desktop-mcp: restore token was rejected ({why}).");
            portal.close();
            match portal.start(None) {
                Ok(start) => {
                    remember(store, start.restore_token.as_deref(), true);
                    Ok(opened(start, Some(TOKEN_RETRY.to_string())))
                }
                Err(PortalFail::TokenRejected(text)) => Err(PortalFail::Denied(text)),
                Err(err) => Err(err),
            }
        }
        Err(PortalFail::TokenRejected(text)) => Err(PortalFail::Denied(text)),
        Err(err) => Err(err),
    }
}

fn opened(start: PortalStart, notice: Option<String>) -> OpenedPortal {
    OpenedPortal {
        regions: start.regions,
        text: start.text,
        keyboard: start.keyboard,
        streams: start.streams,
        notify: start.notify,
        notice,
    }
}

fn remember(store: &dyn RestoreStore, token: Option<&str>, used_saved: bool) {
    match token.map(str::trim).filter(|token| !token.is_empty()) {
        Some(token) => {
            if let Err(err) = store.save(token) {
                eprintln!("desktop-mcp: could not store the restore token: {err}");
            }
        }
        None if used_saved => {
            if let Err(err) = store.delete() {
                eprintln!("desktop-mcp: could not clear the restore token: {err}");
            }
        }
        None => {}
    }
}

fn keychain_error(err: &str) -> String {
    format!("Could not use the keychain for the desktop restore token ({err}).")
}

pub(crate) struct KeychainRestoreStore;

impl RestoreStore for KeychainRestoreStore {
    fn load(&self) -> Result<Option<String>, String> {
        let entry = keyring_entry()?;
        match entry.get_password() {
            Ok(raw) => {
                let token = raw.trim().to_string();
                if token.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(token))
                }
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(keychain_error(&err.to_string())),
        }
    }

    fn save(&self, token: &str) -> Result<(), String> {
        keyring_entry()?
            .set_password(token)
            .map_err(|err| keychain_error(&err.to_string()))
    }

    fn delete(&self) -> Result<(), String> {
        match keyring_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(keychain_error(&err.to_string())),
        }
    }
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|err| keychain_error(&err.to_string()))
}

enum Cmd {
    Start {
        token: Option<String>,
        reply: Sender<Result<PortalStart, PortalFail>>,
    },
    Close {
        reply: Sender<()>,
    },
    Pointer {
        x: f32,
        y: f32,
        reply: Sender<Result<(), String>>,
    },
    Button {
        code: u32,
        down: bool,
        reply: Sender<Result<(), String>>,
    },
    Scroll {
        dx: i32,
        dy: i32,
        reply: Sender<Result<(), String>>,
    },
    Key {
        code: u32,
        down: bool,
        reply: Sender<Result<(), String>>,
    },
    Text {
        text: String,
        reply: Sender<Result<(), String>>,
    },
    NotifyPointer {
        stream: u32,
        x: f64,
        y: f64,
        reply: Sender<Result<(), String>>,
    },
    NotifyButton {
        button: i32,
        down: bool,
        reply: Sender<Result<(), String>>,
    },
    NotifyAxis {
        horizontal: bool,
        steps: i32,
        reply: Sender<Result<(), String>>,
    },
    NotifyKeysym {
        keysym: i32,
        down: bool,
        reply: Sender<Result<(), String>>,
    },
}

/// Real RemoteDesktop + libei client. The worker thread starts on the first call.
pub(crate) struct AshpdPortal {
    tx: Mutex<Option<Sender<Cmd>>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    cancel: Arc<AtomicBool>,
}

impl AshpdPortal {
    pub(crate) fn new() -> Self {
        Self {
            tx: Mutex::new(None),
            thread: Mutex::new(None),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn sender(&mut self) -> Result<Sender<Cmd>, PortalFail> {
        let mut slot = self.tx.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(tx) = slot.as_ref() {
            return Ok(tx.clone());
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::clone(&self.cancel);
        match std::thread::Builder::new()
            .name("grokhub-portal".into())
            .spawn(move || worker(rx, cancel))
        {
            Ok(handle) => {
                *self.thread.lock().unwrap_or_else(|err| err.into_inner()) = Some(handle);
                *slot = Some(tx.clone());
                Ok(tx)
            }
            Err(err) => Err(PortalFail::Fallback(format!(
                "Could not start the portal worker: {err}"
            ))),
        }
    }

    fn roundtrip(
        &mut self,
        build: impl FnOnce(Sender<Result<(), String>>) -> Cmd,
    ) -> Result<(), String> {
        let tx = self.sender().map_err(|err| err.text().to_string())?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(build(reply_tx))
            .map_err(|_| "The portal worker stopped.".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "The portal worker timed out.".to_string())?
    }
}

impl Drop for AshpdPortal {
    fn drop(&mut self) {
        self.close();
        drop(self.tx.lock().unwrap_or_else(|err| err.into_inner()).take());
        if let Some(handle) = self.thread.lock().unwrap_or_else(|err| err.into_inner()).take() {
            let _ = handle.join();
        }
    }
}

impl RemoteDesktopPortal for AshpdPortal {
    fn start(&mut self, restore_token: Option<String>) -> Result<PortalStart, PortalFail> {
        let tx = self.sender()?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(Cmd::Start {
            token: restore_token,
            reply: reply_tx,
        })
        .map_err(|_| PortalFail::Fallback("The portal worker stopped.".into()))?;
        reply_rx
            .recv_timeout(Duration::from_secs(200))
            .map_err(|_| PortalFail::Denied("The remote-control dialog timed out.".into()))?
    }

    fn close(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        let tx = self
            .tx
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let Some(tx) = tx else {
            return;
        };
        let (reply_tx, reply_rx) = mpsc::channel();
        if tx.send(Cmd::Close { reply: reply_tx }).is_ok() {
            let _ = reply_rx.recv_timeout(Duration::from_secs(3));
        }
    }
}

impl EisInput for AshpdPortal {
    fn pointer_absolute(&mut self, x: f32, y: f32) -> Result<(), String> {
        self.roundtrip(|reply| Cmd::Pointer { x, y, reply })
    }

    fn button(&mut self, code: u32, down: bool) -> Result<(), String> {
        self.roundtrip(|reply| Cmd::Button { code, down, reply })
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        self.roundtrip(|reply| Cmd::Scroll { dx, dy, reply })
    }

    fn key(&mut self, code: u32, down: bool) -> Result<(), String> {
        self.roundtrip(|reply| Cmd::Key { code, down, reply })
    }

    fn text(&mut self, text: &str) -> Result<(), String> {
        let text = text.to_string();
        self.roundtrip(|reply| Cmd::Text { text, reply })
    }
}

impl NotifyInput for AshpdPortal {
    fn notify_pointer(&mut self, stream: u32, x: f64, y: f64) -> Result<(), String> {
        self.roundtrip(|reply| Cmd::NotifyPointer { stream, x, y, reply })
    }

    fn notify_button(&mut self, button: i32, down: bool) -> Result<(), String> {
        self.roundtrip(|reply| Cmd::NotifyButton { button, down, reply })
    }

    fn notify_axis(&mut self, horizontal: bool, steps: i32) -> Result<(), String> {
        self.roundtrip(|reply| Cmd::NotifyAxis {
            horizontal,
            steps,
            reply,
        })
    }

    fn notify_keysym(&mut self, keysym: i32, down: bool) -> Result<(), String> {
        self.roundtrip(|reply| Cmd::NotifyKeysym { keysym, down, reply })
    }
}

fn worker(rx: Receiver<Cmd>, cancel: Arc<AtomicBool>) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            let msg = format!("tokio: {err}");
            while let Ok(cmd) = rx.recv() {
                fail_cmd(cmd, &msg);
            }
            return;
        }
    };
    rt.block_on(run_worker(rx, cancel));
}

fn fail_cmd(cmd: Cmd, msg: &str) {
    match cmd {
        Cmd::Start { reply, .. } => {
            let _ = reply.send(Err(PortalFail::Fallback(msg.to_string())));
        }
        Cmd::Close { reply } => {
            let _ = reply.send(());
        }
        Cmd::Pointer { reply, .. }
        | Cmd::Button { reply, .. }
        | Cmd::Scroll { reply, .. }
        | Cmd::Key { reply, .. }
        | Cmd::Text { reply, .. }
        | Cmd::NotifyPointer { reply, .. }
        | Cmd::NotifyButton { reply, .. }
        | Cmd::NotifyAxis { reply, .. }
        | Cmd::NotifyKeysym { reply, .. } => {
            let _ = reply.send(Err(msg.to_string()));
        }
    }
}

async fn run_worker(rx: Receiver<Cmd>, cancel: Arc<AtomicBool>) {
    let mut live: Option<Live> = None;
    loop {
        match rx.try_recv() {
            Ok(cmd) => dispatch(&mut live, cmd, &cancel).await,
            Err(TryRecvError::Empty) => {
                if let Some(session) = live.as_mut() {
                    if let Err(err) = session.drain().await {
                        eprintln!("desktop-mcp: {err}");
                        live = None;
                    }
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            Err(TryRecvError::Disconnected) => break,
        }
    }
    if let Some(old) = live.take() {
        old.shutdown().await;
    }
}

async fn dispatch(live: &mut Option<Live>, cmd: Cmd, cancel: &AtomicBool) {
    match cmd {
        Cmd::Start { token, reply } => {
            if let Some(old) = live.take() {
                old.shutdown().await;
            }
            cancel.store(false, Ordering::SeqCst);
            match open_eis(cancel, token).await {
                Ok(session) => {
                    let summary = session.summary();
                    *live = Some(session);
                    let _ = reply.send(Ok(summary));
                }
                Err(err) => {
                    let _ = reply.send(Err(err));
                }
            }
        }
        Cmd::Close { reply } => {
            cancel.store(false, Ordering::SeqCst);
            if let Some(old) = live.take() {
                old.shutdown().await;
            }
            let _ = reply.send(());
        }
        Cmd::Pointer { x, y, reply } => {
            let result = match live.as_mut() {
                Some(Live::Eis(session)) => session.pointer(x, y).await,
                Some(Live::Notify(_)) => Err("The portal session is using Notify, not libei.".into()),
                None => Err("The portal session is closed.".into()),
            };
            let _ = reply.send(result);
        }
        Cmd::Button { code, down, reply } => {
            let result = match live.as_mut() {
                Some(Live::Eis(session)) => session.button(code, down).await,
                Some(Live::Notify(_)) => Err("The portal session is using Notify, not libei.".into()),
                None => Err("The portal session is closed.".into()),
            };
            let _ = reply.send(result);
        }
        Cmd::Scroll { dx, dy, reply } => {
            let result = match live.as_mut() {
                Some(Live::Eis(session)) => session.scroll(dx, dy).await,
                Some(Live::Notify(_)) => Err("The portal session is using Notify, not libei.".into()),
                None => Err("The portal session is closed.".into()),
            };
            let _ = reply.send(result);
        }
        Cmd::Key { code, down, reply } => {
            let result = match live.as_mut() {
                Some(Live::Eis(session)) => session.key(code, down).await,
                Some(Live::Notify(_)) => Err("The portal session is using Notify, not libei.".into()),
                None => Err("The portal session is closed.".into()),
            };
            let _ = reply.send(result);
        }
        Cmd::Text { text, reply } => {
            let result = match live.as_mut() {
                Some(Live::Eis(session)) => session.text(&text).await,
                Some(Live::Notify(_)) => Err("The portal session is using Notify, not libei.".into()),
                None => Err("The portal session is closed.".into()),
            };
            let _ = reply.send(result);
        }
        Cmd::NotifyPointer { stream, x, y, reply } => {
            let result = match live.as_mut() {
                Some(Live::Notify(session)) => session.pointer(stream, x, y).await,
                _ => Err("The portal session is not using Notify.".into()),
            };
            let _ = reply.send(result);
        }
        Cmd::NotifyButton { button, down, reply } => {
            let result = match live.as_mut() {
                Some(Live::Notify(session)) => session.button(button, down).await,
                _ => Err("The portal session is not using Notify.".into()),
            };
            let _ = reply.send(result);
        }
        Cmd::NotifyAxis { horizontal, steps, reply } => {
            let result = match live.as_mut() {
                Some(Live::Notify(session)) => session.axis(horizontal, steps).await,
                _ => Err("The portal session is not using Notify.".into()),
            };
            let _ = reply.send(result);
        }
        Cmd::NotifyKeysym { keysym, down, reply } => {
            let result = match live.as_mut() {
                Some(Live::Notify(session)) => session.keysym(keysym, down).await,
                _ => Err("The portal session is not using Notify.".into()),
            };
            let _ = reply.send(result);
        }
    }
}

fn classify(err: ashpd::Error, had_token: bool) -> PortalFail {
    match &err {
        ashpd::Error::RequiresVersion(_, _) => PortalFail::Fallback(format!(
            "ConnectToEIS is not available on this portal ({err})."
        )),
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)
        | ashpd::Error::Portal(ashpd::PortalError::Cancelled(_)) => PortalFail::Denied(format!(
            "The remote-control dialog was cancelled ({err})."
        )),
        ashpd::Error::Response(ashpd::desktop::ResponseError::Other)
        | ashpd::Error::Portal(ashpd::PortalError::Failed(_))
        | ashpd::Error::Portal(ashpd::PortalError::InvalidArgument(_))
        | ashpd::Error::Portal(ashpd::PortalError::NotAllowed(_))
            if had_token =>
        {
            PortalFail::TokenRejected(err.to_string())
        }
        _ => PortalFail::Denied(err.to_string()),
    }
}

async fn drive<T>(
    cancel: &AtomicBool,
    fut: impl std::future::Future<Output = T>,
    limit: Duration,
) -> Result<T, PortalFail> {
    let mut fut = std::pin::pin!(fut);
    let deadline = Instant::now() + limit;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(PortalFail::Denied(
                "Desktop control was halted. The portal session is closed.".into(),
            ));
        }
        if Instant::now() >= deadline {
            return Err(PortalFail::Denied(
                "The remote-control dialog timed out.".into(),
            ));
        }
        match tokio::time::timeout(Duration::from_millis(200), fut.as_mut()).await {
            Ok(value) => return Ok(value),
            Err(_) => continue,
        }
    }
}

async fn open_eis(cancel: &AtomicBool, token: Option<String>) -> Result<Live, PortalFail> {
    let had_token = token.as_ref().is_some_and(|value| !value.trim().is_empty());
    let portal = drive(cancel, RdPortal::new(), Duration::from_secs(20))
        .await?
        .map_err(|err| classify(err, had_token))?;
    let session = drive(
        cancel,
        portal.create_session(ashpd::desktop::CreateSessionOptions::default()),
        Duration::from_secs(30),
    )
    .await?
    .map_err(|err| classify(err, had_token))?;
    match start_session(portal, session, cancel, token.as_deref(), had_token).await {
        Ok(live) => Ok(live),
        Err((session, err)) => {
            let _ = session.close().await;
            Err(err)
        }
    }
}

async fn start_session(
    portal: RdPortal,
    session: RdSession,
    cancel: &AtomicBool,
    token: Option<&str>,
    had_token: bool,
) -> Result<Live, (RdSession, PortalFail)> {
    use ashpd::desktop::remote_desktop::{
        ConnectToEISOptions, DeviceType, SelectDevicesOptions, StartOptions,
    };
    use ashpd::desktop::PersistMode;

    let options = SelectDevicesOptions::default()
        .set_devices(DeviceType::Keyboard | DeviceType::Pointer)
        .set_persist_mode(PersistMode::ExplicitlyRevoked)
        .set_restore_token(token.filter(|value| !value.trim().is_empty()));
    let selected = match drive(
        cancel,
        portal.select_devices(&session, options),
        Duration::from_secs(30),
    )
    .await
    {
        Ok(Ok(value)) => value,
        Ok(Err(err)) => return Err((session, classify(err, had_token))),
        Err(err) => return Err((session, err)),
    };
    if let Err(err) = selected.response().map_err(|err| classify(err, had_token)) {
        return Err((session, err));
    }
    // Same session, still one Start consent. A failed SelectSources leaves EIS-only.
    let _ = select_monitor_sources(&session, cancel).await;

    let started = match drive(
        cancel,
        portal.start(&session, None, StartOptions::default()),
        Duration::from_secs(180),
    )
    .await
    {
        Ok(result) => result,
        Err(err) => return Err((session, err)),
    };
    let started = match started {
        Ok(value) => value,
        Err(err) => return Err((session, classify(err, had_token))),
    };
    let selected = match started.response() {
        Ok(value) => value,
        Err(err) => return Err((session, classify(err, had_token))),
    };
    let restore_token = selected.restore_token().map(str::to_string);
    let streams = cast_streams(&selected);

    let connected = match drive(
        cancel,
        portal.connect_to_eis(&session, ConnectToEISOptions::default()),
        Duration::from_secs(10),
    )
    .await
    {
        Ok(result) => result,
        Err(err) if keep_for_notify(&err, &streams) => {
            return Ok(Live::Notify(LiveNotify {
                portal,
                session,
                streams,
                restore_token,
            }));
        }
        Err(err) => return Err((session, err)),
    };
    let fd = match connected {
        Ok(fd) => fd,
        Err(err) => {
            let fail = classify(err, had_token);
            if keep_for_notify(&fail, &streams) {
                return Ok(Live::Notify(LiveNotify {
                    portal,
                    session,
                    streams,
                    restore_token,
                }));
            }
            return Err((session, fail));
        }
    };
    let context = match reis::ei::Context::new(std::os::unix::net::UnixStream::from(fd)) {
        Ok(context) => context,
        Err(err) => {
            let fail = PortalFail::Fallback(format!("EIS socket: {err}"));
            if keep_for_notify(&fail, &streams) {
                return Ok(Live::Notify(LiveNotify {
                    portal,
                    session,
                    streams,
                    restore_token,
                }));
            }
            return Err((session, fail));
        }
    };
    let mut handshaker =
        reis::handshake::EiHandshaker::new("grokhub", reis::ei::handshake::ContextType::Sender);
    let resp = match handshake(&context, &mut handshaker, cancel).await {
        Ok(resp) => resp,
        Err(err) if keep_for_notify(&err, &streams) => {
            return Ok(Live::Notify(LiveNotify {
                portal,
                session,
                streams,
                restore_token,
            }));
        }
        Err(err) => return Err((session, err)),
    };
    let mut converter = reis::event::EiEventConverter::new(&context, resp);
    let bound = match wait_absolute(&context, &mut converter, cancel).await {
        Ok(bound) => bound,
        Err(err) if keep_for_notify(&err, &streams) => {
            return Ok(Live::Notify(LiveNotify {
                portal,
                session,
                streams,
                restore_token,
            }));
        }
        Err(err) => return Err((session, err)),
    };
    Ok(Live::Eis(Box::new(LiveEis {
        session,
        context,
        converter,
        device: bound.device,
        serial: bound.serial,
        sequence: 2,
        emulating: true,
        regions: bound.regions,
        text: bound.text,
        keyboard: bound.keyboard,
        restore_token,
    })))
}

async fn select_monitor_sources(session: &RdSession, cancel: &AtomicBool) -> bool {
    use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
    use ashpd::desktop::PersistMode;

    let cast = match Screencast::new().await {
        Ok(cast) => cast,
        Err(err) => {
            eprintln!("desktop-mcp: screencast portal: {err}");
            return false;
        }
    };
    let options = SelectSourcesOptions::default()
        .set_sources(ashpd::enumflags2::BitFlags::from_flag(SourceType::Monitor))
        .set_multiple(true)
        .set_cursor_mode(CursorMode::Hidden)
        .set_persist_mode(PersistMode::DoNot);
    match drive(cancel, cast.select_sources(session, options), Duration::from_secs(30)).await {
        Ok(Ok(request)) => match request.response() {
            Ok(()) => true,
            Err(err) => {
                eprintln!("desktop-mcp: screencast sources: {err}");
                false
            }
        },
        Ok(Err(err)) => {
            eprintln!("desktop-mcp: screencast sources: {err}");
            false
        }
        Err(err) => {
            eprintln!("desktop-mcp: screencast sources: {}", err.text());
            false
        }
    }
}

fn cast_streams(selected: &ashpd::desktop::remote_desktop::SelectedDevices) -> Vec<CastStream> {
    selected
        .streams()
        .iter()
        .filter_map(|stream| {
            let (width, height) = stream.size()?;
            if width <= 0 || height <= 0 {
                return None;
            }
            let (x, y) = stream.position().unwrap_or((0, 0));
            Some(CastStream {
                node: stream.pipe_wire_node_id(),
                x,
                y,
                width: u32::try_from(width).unwrap_or(0),
                height: u32::try_from(height).unwrap_or(0),
            })
        })
        .filter(|stream| stream.width > 0 && stream.height > 0)
        .collect()
}

fn keep_for_notify(err: &PortalFail, streams: &[CastStream]) -> bool {
    if streams.is_empty() {
        return false;
    }
    !err.text().contains("halted")
}

enum Live {
    Eis(Box<LiveEis>),
    Notify(LiveNotify),
}

struct LiveNotify {
    portal: RdPortal,
    session: RdSession,
    streams: Vec<CastStream>,
    restore_token: Option<String>,
}

impl Live {
    fn summary(&self) -> PortalStart {
        match self {
            Self::Eis(session) => session.summary(),
            Self::Notify(session) => PortalStart {
                restore_token: session.restore_token.clone(),
                regions: Vec::new(),
                text: false,
                keyboard: true,
                streams: session.streams.clone(),
                notify: true,
            },
        }
    }

    async fn shutdown(self) {
        match self {
            Self::Eis(session) => session.shutdown().await,
            Self::Notify(session) => {
                let _ = session.session.close().await;
            }
        }
    }

    async fn drain(&mut self) -> Result<(), String> {
        match self {
            Self::Eis(session) => session.drain().await,
            Self::Notify(_) => Ok(()),
        }
    }
}

impl LiveNotify {
    async fn pointer(&self, stream: u32, x: f64, y: f64) -> Result<(), String> {
        use ashpd::desktop::remote_desktop::NotifyPointerMotionAbsoluteOptions;
        self.portal
            .notify_pointer_motion_absolute(
                &self.session,
                stream,
                x,
                y,
                NotifyPointerMotionAbsoluteOptions::default(),
            )
            .await
            .map_err(|err| format!("NotifyPointerMotionAbsolute: {err}"))
    }

    async fn button(&self, button: i32, down: bool) -> Result<(), String> {
        use ashpd::desktop::remote_desktop::{KeyState, NotifyPointerButtonOptions};
        let state = if down { KeyState::Pressed } else { KeyState::Released };
        self.portal
            .notify_pointer_button(&self.session, button, state, NotifyPointerButtonOptions::default())
            .await
            .map_err(|err| format!("NotifyPointerButton: {err}"))
    }

    async fn axis(&self, horizontal: bool, steps: i32) -> Result<(), String> {
        use ashpd::desktop::remote_desktop::{Axis, NotifyPointerAxisDiscreteOptions};
        let axis = if horizontal { Axis::Horizontal } else { Axis::Vertical };
        self.portal
            .notify_pointer_axis_discrete(
                &self.session,
                axis,
                steps,
                NotifyPointerAxisDiscreteOptions::default(),
            )
            .await
            .map_err(|err| format!("NotifyPointerAxisDiscrete: {err}"))
    }

    async fn keysym(&self, keysym: i32, down: bool) -> Result<(), String> {
        use ashpd::desktop::remote_desktop::{KeyState, NotifyKeyboardKeysymOptions};
        let state = if down { KeyState::Pressed } else { KeyState::Released };
        self.portal
            .notify_keyboard_keysym(
                &self.session,
                keysym,
                state,
                NotifyKeyboardKeysymOptions::default(),
            )
            .await
            .map_err(|err| format!("NotifyKeyboardKeysym: {err}"))
    }
}

struct BoundDevice {
    device: reis::event::Device,
    serial: u32,
    regions: Vec<EisRegion>,
    text: bool,
    keyboard: bool,
}

struct LiveEis {
    session: RdSession,
    context: reis::ei::Context,
    converter: reis::event::EiEventConverter,
    device: reis::event::Device,
    serial: u32,
    sequence: u32,
    emulating: bool,
    regions: Vec<EisRegion>,
    text: bool,
    keyboard: bool,
    restore_token: Option<String>,
}

impl LiveEis {
    fn summary(&self) -> PortalStart {
        PortalStart {
            restore_token: self.restore_token.clone(),
            regions: self.regions.clone(),
            text: self.text,
            keyboard: self.keyboard,
            streams: Vec::new(),
            notify: false,
        }
    }

    async fn shutdown(self) {
        if self.emulating {
            self.device.device().stop_emulating(self.serial);
            let _ = self.context.flush();
        }
        let _ = self.session.close().await;
    }

    async fn pointer(&mut self, x: f32, y: f32) -> Result<(), String> {
        self.drain().await?;
        self.ensure_emulating()?;
        let pointer = self
            .device
            .interface::<reis::ei::PointerAbsolute>()
            .ok_or_else(|| "The EIS device has no absolute pointer.".to_string())?;
        pointer.motion_absolute(x, y);
        self.frame()
    }

    async fn button(&mut self, code: u32, down: bool) -> Result<(), String> {
        self.drain().await?;
        self.ensure_emulating()?;
        let button = self
            .device
            .interface::<reis::ei::Button>()
            .ok_or_else(|| "The EIS device has no button capability.".to_string())?;
        let state = if down {
            reis::ei::button::ButtonState::Press
        } else {
            reis::ei::button::ButtonState::Released
        };
        button.button(code, state);
        self.frame()
    }

    async fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        self.drain().await?;
        self.ensure_emulating()?;
        let scroll = self
            .device
            .interface::<reis::ei::Scroll>()
            .ok_or_else(|| "The EIS device has no scroll capability.".to_string())?;
        // libei counts one wheel detent as 120. The sign is checked on a live desktop.
        scroll.scroll_discrete(dx.saturating_mul(120), dy.saturating_mul(120));
        self.frame()
    }

    async fn key(&mut self, code: u32, down: bool) -> Result<(), String> {
        self.drain().await?;
        self.ensure_emulating()?;
        let keyboard = self
            .device
            .interface::<reis::ei::Keyboard>()
            .ok_or_else(|| "The EIS device has no keyboard capability.".to_string())?;
        let state = if down {
            reis::ei::keyboard::KeyState::Press
        } else {
            reis::ei::keyboard::KeyState::Released
        };
        keyboard.key(code, state);
        self.frame()
    }

    async fn text(&mut self, text: &str) -> Result<(), String> {
        self.drain().await?;
        self.ensure_emulating()?;
        let text_if = self
            .device
            .interface::<reis::ei::Text>()
            .ok_or_else(|| "The EIS device has no text capability.".to_string())?;
        text_if.utf8(text);
        self.frame()
    }

    fn ensure_emulating(&self) -> Result<(), String> {
        if self.emulating {
            Ok(())
        } else {
            Err("The EIS device is paused.".into())
        }
    }

    fn frame(&self) -> Result<(), String> {
        let serial = self.converter.connection().serial().max(self.serial);
        self.device.device().frame(serial, mono_micros());
        self.context
            .flush()
            .map_err(|err| format!("EIS flush: {err}"))
    }

    async fn drain(&mut self) -> Result<(), String> {
        loop {
            if !poll_ready(&self.context, Duration::ZERO)? {
                break;
            }
            self.context
                .read()
                .map_err(|err| format!("EIS read: {err}"))?;
            pump(&self.context, &mut self.converter).map_err(|err| err.text().to_string())?;
        }
        self.dispatch()
    }

    fn dispatch(&mut self) -> Result<(), String> {
        while let Some(event) = self.converter.next_event() {
            match event {
                reis::event::EiEvent::DeviceResumed(resumed) => {
                    self.serial = resumed.serial;
                    if !self.emulating {
                        self.sequence = self.sequence.wrapping_add(1).max(1);
                        resumed
                            .device
                            .device()
                            .start_emulating(resumed.serial, self.sequence);
                        self.context
                            .flush()
                            .map_err(|err| format!("EIS flush: {err}"))?;
                        self.emulating = true;
                    }
                }
                reis::event::EiEvent::DevicePaused(paused) => {
                    self.serial = paused.serial;
                    self.emulating = false;
                }
                reis::event::EiEvent::Disconnected(gone) => {
                    let why = gone.explanation.unwrap_or_else(|| "disconnected".into());
                    return Err(format!("EIS disconnected: {why}"));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

async fn handshake(
    context: &reis::ei::Context,
    handshaker: &mut reis::handshake::EiHandshaker<'_>,
    cancel: &AtomicBool,
) -> Result<reis::handshake::HandshakeResp, PortalFail> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(halted());
        }
        while let Some(result) = context.pending_event() {
            let event = take_event(result)?;
            if let Some(resp) = handshaker
                .handle_event(event)
                .map_err(|err| PortalFail::Fallback(err.to_string()))?
            {
                return Ok(resp);
            }
        }
        if Instant::now() >= deadline {
            return Err(PortalFail::Fallback("The EIS handshake timed out.".into()));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if read_ready(context, left.min(Duration::from_millis(50))).await? {
            context
                .read()
                .map_err(|err| PortalFail::Fallback(format!("EIS read: {err}")))?;
        }
    }
}

async fn wait_absolute(
    context: &reis::ei::Context,
    converter: &mut reis::event::EiEventConverter,
    cancel: &AtomicBool,
) -> Result<BoundDevice, PortalFail> {
    use reis::event::{DeviceCapability, EiEvent};
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(halted());
        }
        pump(context, converter)?;
        while let Some(event) = converter.next_event() {
            match event {
                EiEvent::SeatAdded(added) => {
                    added.seat.bind_capabilities(wanted_caps());
                    context
                        .flush()
                        .map_err(|err| PortalFail::Fallback(format!("EIS flush: {err}")))?;
                }
                EiEvent::DeviceAdded(added) => {
                    if added.device.device().version() >= 3 {
                        added.device.device().ready();
                        context
                            .flush()
                            .map_err(|err| PortalFail::Fallback(format!("EIS flush: {err}")))?;
                    }
                }
                EiEvent::DeviceResumed(resumed) => {
                    if !resumed
                        .device
                        .has_capability(DeviceCapability::PointerAbsolute)
                    {
                        continue;
                    }
                    resumed.device.device().start_emulating(resumed.serial, 1);
                    context
                        .flush()
                        .map_err(|err| PortalFail::Fallback(format!("EIS flush: {err}")))?;
                    let device = resumed.device;
                    return Ok(BoundDevice {
                        regions: regions_of(&device),
                        text: device.has_capability(DeviceCapability::Text),
                        keyboard: device.has_capability(DeviceCapability::Keyboard),
                        serial: resumed.serial,
                        device,
                    });
                }
                EiEvent::Disconnected(gone) => {
                    let why = gone.explanation.unwrap_or_else(|| "disconnected".into());
                    return Err(PortalFail::Fallback(format!("EIS disconnected: {why}")));
                }
                _ => {}
            }
        }
        if Instant::now() >= deadline {
            return Err(PortalFail::Fallback(
                "The portal EIS connection has no absolute pointer device. Pointer input is imprecise.".into(),
            ));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if read_ready(context, left.min(Duration::from_millis(50))).await? {
            context
                .read()
                .map_err(|err| PortalFail::Fallback(format!("EIS read: {err}")))?;
        }
    }
}

fn wanted_caps() -> reis::enumflags2::BitFlags<reis::event::DeviceCapability> {
    use reis::event::DeviceCapability;
    DeviceCapability::PointerAbsolute
        | DeviceCapability::Button
        | DeviceCapability::Scroll
        | DeviceCapability::Keyboard
        | DeviceCapability::Text
}

fn regions_of(device: &reis::event::Device) -> Vec<EisRegion> {
    device
        .regions()
        .iter()
        .enumerate()
        .map(|(index, region)| EisRegion {
            name: region
                .mapping_id
                .clone()
                .unwrap_or_else(|| format!("region-{index}")),
            x: i32::try_from(region.x).unwrap_or(i32::MAX),
            y: i32::try_from(region.y).unwrap_or(i32::MAX),
            width: region.width,
            height: region.height,
        })
        .collect()
}

fn pump(
    context: &reis::ei::Context,
    converter: &mut reis::event::EiEventConverter,
) -> Result<(), PortalFail> {
    while let Some(result) = context.pending_event() {
        let event = take_event(result)?;
        converter
            .handle_event(event)
            .map_err(|err| PortalFail::Fallback(err.to_string()))?;
    }
    Ok(())
}

fn take_event(
    result: reis::PendingRequestResult<reis::ei::Event>,
) -> Result<reis::ei::Event, PortalFail> {
    match result {
        reis::PendingRequestResult::Request(event) => Ok(event),
        reis::PendingRequestResult::ParseError(err) => {
            Err(PortalFail::Fallback(format!("EIS parse error: {err}")))
        }
        reis::PendingRequestResult::InvalidObject(id) => {
            Err(PortalFail::Fallback(format!("EIS invalid object {id}.")))
        }
    }
}

async fn read_ready(context: &reis::ei::Context, timeout: Duration) -> Result<bool, PortalFail> {
    let ready = poll_ready(context, timeout).map_err(PortalFail::Fallback)?;
    tokio::task::yield_now().await;
    Ok(ready)
}

fn poll_ready(fd: &impl std::os::fd::AsFd, timeout: Duration) -> Result<bool, String> {
    let ts = rustix::event::Timespec {
        tv_sec: i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: timeout.subsec_nanos() as _,
    };
    loop {
        match rustix::event::poll(
            &mut [rustix::event::PollFd::new(fd, rustix::event::PollFlags::IN)],
            Some(&ts),
        ) {
            Ok(0) => return Ok(false),
            Ok(_) => return Ok(true),
            Err(rustix::io::Errno::INTR) => continue,
            Err(err) => return Err(format!("EIS poll: {err}")),
        }
    }
}

fn mono_micros() -> u64 {
    let ts = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    (ts.tv_sec.max(0) as u64).saturating_mul(1_000_000) + (ts.tv_nsec.max(0) as u64) / 1_000
}

fn halted() -> PortalFail {
    PortalFail::Denied("Desktop control was halted. The portal session is closed.".into())
}

#[cfg(test)]
pub(crate) mod doubles {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Clone)]
    pub(crate) struct MemoryRestoreStore {
        inner: Arc<Mutex<Option<String>>>,
    }

    impl MemoryRestoreStore {
        pub(crate) fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl RestoreStore for MemoryRestoreStore {
        fn load(&self) -> Result<Option<String>, String> {
            Ok(self
                .inner
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone())
        }

        fn save(&self, token: &str) -> Result<(), String> {
            *self.inner.lock().unwrap_or_else(|err| err.into_inner()) = Some(token.to_string());
            Ok(())
        }

        fn delete(&self) -> Result<(), String> {
            *self.inner.lock().unwrap_or_else(|err| err.into_inner()) = None;
            Ok(())
        }
    }

    pub(crate) enum FakeReply {
        Token(String),
        Reject,
        Deny(String),
    }

    struct FakeInner {
        script: VecDeque<FakeReply>,
        seen: Vec<Option<String>>,
        closed: u32,
        pointers: Vec<(f32, f32)>,
        regions: Vec<EisRegion>,
    }

    #[derive(Clone)]
    pub(crate) struct FakePortal {
        inner: Arc<Mutex<FakeInner>>,
    }

    impl FakePortal {
        pub(crate) fn script(steps: Vec<FakeReply>) -> Self {
            Self {
                inner: Arc::new(Mutex::new(FakeInner {
                    script: VecDeque::from(steps),
                    seen: Vec::new(),
                    closed: 0,
                    pointers: Vec::new(),
                    regions: vec![EisRegion {
                        name: "HDMI-A-2".into(),
                        x: 0,
                        y: 0,
                        width: 1920,
                        height: 1080,
                    }],
                })),
            }
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, FakeInner> {
            self.inner.lock().unwrap_or_else(|err| err.into_inner())
        }

        pub(crate) fn seen(&self) -> Vec<Option<String>> {
            self.lock().seen.clone()
        }

        pub(crate) fn start_count(&self) -> usize {
            self.lock().seen.len()
        }

        pub(crate) fn closed_count(&self) -> u32 {
            self.lock().closed
        }

        pub(crate) fn pointers(&self) -> Vec<(f32, f32)> {
            self.lock().pointers.clone()
        }
    }

    impl RemoteDesktopPortal for FakePortal {
        fn start(&mut self, restore_token: Option<String>) -> Result<PortalStart, PortalFail> {
            let mut inner = self.lock();
            inner.seen.push(restore_token);
            match inner.script.pop_front() {
                Some(FakeReply::Token(token)) => Ok(PortalStart {
                    restore_token: Some(token),
                    regions: inner.regions.clone(),
                    text: true,
                    keyboard: true,
                    streams: Vec::new(),
                    notify: false,
                }),
                Some(FakeReply::Reject) => {
                    Err(PortalFail::TokenRejected("restore token rejected".into()))
                }
                Some(FakeReply::Deny(text)) => Err(PortalFail::Denied(text)),
                None => Err(PortalFail::Denied("no scripted portal response".into())),
            }
        }

        fn close(&mut self) {
            let mut inner = self.lock();
            inner.closed = inner.closed.saturating_add(1);
        }
    }

    impl EisInput for FakePortal {
        fn pointer_absolute(&mut self, x: f32, y: f32) -> Result<(), String> {
            self.lock().pointers.push((x, y));
            Ok(())
        }

        fn button(&mut self, _code: u32, _down: bool) -> Result<(), String> {
            Ok(())
        }

        fn scroll(&mut self, _dx: i32, _dy: i32) -> Result<(), String> {
            Ok(())
        }

        fn key(&mut self, _code: u32, _down: bool) -> Result<(), String> {
            Ok(())
        }

        fn text(&mut self, _text: &str) -> Result<(), String> {
            Ok(())
        }
    }

    impl NotifyInput for FakePortal {
        fn notify_pointer(&mut self, _stream: u32, _x: f64, _y: f64) -> Result<(), String> {
            Ok(())
        }
        fn notify_button(&mut self, _button: i32, _down: bool) -> Result<(), String> {
            Ok(())
        }
        fn notify_axis(&mut self, _horizontal: bool, _steps: i32) -> Result<(), String> {
            Ok(())
        }
        fn notify_keysym(&mut self, _keysym: i32, _down: bool) -> Result<(), String> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::doubles::{FakePortal, FakeReply, MemoryRestoreStore};
    use super::*;

    #[test]
    fn portal_restore_token_is_saved_and_reused() {
        let store = MemoryRestoreStore::new();
        let mut portal = FakePortal::script(vec![
            FakeReply::Token("alpha".into()),
            FakeReply::Token("beta".into()),
        ]);
        let first = open_with_restore(&mut portal, &store).unwrap();
        assert!(first.notice.is_none());
        assert_eq!(store.load().unwrap().as_deref(), Some("alpha"));
        assert_eq!(portal.seen(), vec![None]);

        let second = open_with_restore(&mut portal, &store).unwrap();
        assert!(second.notice.is_none());
        assert_eq!(store.load().unwrap().as_deref(), Some("beta"));
        assert_eq!(portal.seen(), vec![None, Some("alpha".into())]);

        let store = MemoryRestoreStore::new();
        store.save("stale").unwrap();
        let mut portal = FakePortal::script(vec![
            FakeReply::Reject,
            FakeReply::Token("fresh".into()),
        ]);
        let opened = open_with_restore(&mut portal, &store).unwrap();
        assert_eq!(portal.seen(), vec![Some("stale".into()), None]);
        assert_eq!(store.load().unwrap().as_deref(), Some("fresh"));
        let notice = opened.notice.expect("retry notice");
        assert!(notice.contains("monitors changed"), "{notice}");
    }
}
