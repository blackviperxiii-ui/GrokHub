//! Cabin connectivity: Online / Offline / Unknown for cloud vs on-device routing.
//!
//! Windows feeds OS signals from the Network List Manager; every platform can
//! nudge state from classified HTTP transport errors. Debounced transitions and
//! a backoff HEAD re-probe avoid flapping when Wi‑Fi drops for a second.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// How the cabin sees reachability to the configured cloud provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectivityState {
    Unknown = 0,
    Online = 1,
    Offline = 2,
}

impl ConnectivityState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Online,
            2 => Self::Offline,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Online => "online",
            Self::Offline => "offline",
        }
    }
}

/// Cabin UI chip: Online / Reconnecting / Offline (local engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectivityUiPhase {
    Online,
    Reconnecting,
    OfflineLocal,
}

impl ConnectivityUiPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Reconnecting => "Reconnecting",
            Self::OfflineLocal => "Offline (local engine)",
        }
    }
}

/// What a transport failure might mean for connectivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportHint {
    /// TCP/DNS/timeout or a request error with no HTTP status — may be offline.
    OfflineCandidate,
    /// A response arrived (including auth/rate/server errors) — not offline.
    CloudReachable,
    /// Nothing to infer (decode error, cancelled, etc.).
    Inconclusive,
}

/// Debounce window before a pending state becomes live (2–3 s).
pub const DEBOUNCE_MS: u64 = 2_500;
/// First HEAD re-probe delay after going offline-ish.
pub const PROBE_INITIAL_BACKOFF_MS: u64 = 1_000;
/// Cap for exponential re-probe backoff.
pub const PROBE_MAX_BACKOFF_MS: u64 = 30_000;

/// Default HTTP timeouts for provider probes and API calls.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

static LIVE: AtomicU8 = AtomicU8::new(ConnectivityState::Unknown as u8);

/// Process-wide live state (debounced). Cabin reads this; writers use [`Connectivity`].
pub fn state() -> ConnectivityState {
    // Offline only reads as Offline, so the router goes local and nothing probes.
    if crate::offline::enabled() {
        return ConnectivityState::Offline;
    }
    ConnectivityState::from_u8(LIVE.load(Ordering::SeqCst))
}

fn publish(s: ConnectivityState) {
    LIVE.store(s as u8, Ordering::SeqCst);
}

/// Classify a `reqwest` error for connectivity. **401 / 403 / 429 / 5xx never
/// mean offline** — the cloud answered.
pub fn classify_reqwest_error(err: &reqwest::Error) -> TransportHint {
    if err.is_connect() || err.is_timeout() {
        return TransportHint::OfflineCandidate;
    }
    if let Some(status) = err.status() {
        return classify_http_status(status.as_u16());
    }
    if err.is_request() {
        return TransportHint::OfflineCandidate;
    }
    TransportHint::Inconclusive
}

/// Table-driven status handling shared with tests.
pub fn classify_http_status(status: u16) -> TransportHint {
    let _never_offline = matches!(status, 401 | 403 | 429) || status >= 500;
    TransportHint::CloudReachable
}

/// Apply explicit connect and total timeouts on a `reqwest` client builder.
pub fn http_client_with_timeouts(
    builder: reqwest::blocking::ClientBuilder,
    connect_timeout: Duration,
    request_timeout: Duration,
) -> reqwest::blocking::ClientBuilder {
    builder
        .connect_timeout(connect_timeout)
        .timeout(request_timeout)
}

/// Build a client with [`DEFAULT_CONNECT_TIMEOUT`] and [`DEFAULT_REQUEST_TIMEOUT`].
pub fn default_http_client() -> Result<reqwest::blocking::Client, reqwest::Error> {
    http_client_with_timeouts(
        reqwest::blocking::Client::builder(),
        DEFAULT_CONNECT_TIMEOUT,
        DEFAULT_REQUEST_TIMEOUT,
    )
    .build()
}

/// Debounced connectivity engine: OS signals + transport hints + HEAD re-probe.
#[derive(Debug, Clone)]
pub struct Connectivity {
    debounce_ms: u64,
    live: ConnectivityState,
    pending: Option<ConnectivityState>,
    pending_since_ms: u64,
    os_online: Option<bool>,
    probe_url: Option<String>,
    probe_due_ms: u64,
    probe_backoff_ms: u64,
}

impl Default for Connectivity {
    fn default() -> Self {
        Self::new(DEBOUNCE_MS)
    }
}

impl Connectivity {
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            debounce_ms: debounce_ms.max(500),
            live: ConnectivityState::Unknown,
            pending: None,
            pending_since_ms: 0,
            os_online: None,
            probe_url: None,
            probe_due_ms: 0,
            probe_backoff_ms: PROBE_INITIAL_BACKOFF_MS,
        }
    }

    pub fn with_probe_url(mut self, url: impl Into<String>) -> Self {
        self.probe_url = Some(url.into());
        self
    }

    pub fn live(&self) -> ConnectivityState {
        self.live
    }

    /// Chip phase from live + debounced pending state.
    pub fn ui_phase(&self) -> ConnectivityUiPhase {
        if let Some(pending) = self.pending {
            if pending == ConnectivityState::Online && self.live != ConnectivityState::Online {
                return ConnectivityUiPhase::Reconnecting;
            }
            if pending == ConnectivityState::Offline && self.live == ConnectivityState::Online {
                return ConnectivityUiPhase::Online;
            }
        }
        match self.live {
            ConnectivityState::Offline => ConnectivityUiPhase::OfflineLocal,
            ConnectivityState::Online | ConnectivityState::Unknown => ConnectivityUiPhase::Online,
        }
    }

    /// Windows NLM (or any OS hook): `true` when the OS says internet is up.
    pub fn note_os_online(&mut self, online: bool, now_ms: u64) {
        self.os_online = Some(online);
        self.queue_from_os(online, now_ms);
        self.tick(now_ms);
    }

    /// Classified provider / transport failure.
    pub fn note_transport_error(&mut self, err: &reqwest::Error, now_ms: u64) {
        self.note_transport_hint(classify_reqwest_error(err), now_ms);
    }

    /// Classified provider / transport signal (reqwest, ureq, probes).
    pub fn note_transport_hint(&mut self, hint: TransportHint, now_ms: u64) {
        match hint {
            TransportHint::OfflineCandidate => {
                self.queue_pending(ConnectivityState::Offline, now_ms);
            }
            TransportHint::CloudReachable => {
                self.clear_pending_toward(ConnectivityState::Online, now_ms);
            }
            TransportHint::Inconclusive => {}
        }
        self.tick(now_ms);
    }

    /// Any successful HTTP response to the provider (including 401) means reachable.
    pub fn note_transport_success(&mut self, now_ms: u64) {
        self.clear_pending_toward(ConnectivityState::Online, now_ms);
        self.probe_backoff_ms = PROBE_INITIAL_BACKOFF_MS;
        self.probe_due_ms = 0;
        self.tick(now_ms);
    }

    fn queue_from_os(&mut self, online: bool, now_ms: u64) {
        let target = if online {
            ConnectivityState::Online
        } else {
            ConnectivityState::Offline
        };
        self.queue_pending(target, now_ms);
    }

    fn queue_pending(&mut self, target: ConnectivityState, now_ms: u64) {
        if self.pending == Some(target) {
            return;
        }
        self.pending = Some(target);
        self.pending_since_ms = now_ms;
    }

    fn clear_pending_toward(&mut self, target: ConnectivityState, now_ms: u64) {
        if self.live == target {
            self.pending = None;
            return;
        }
        self.queue_pending(target, now_ms);
    }

    /// Apply debounce; returns `Some` when [`live`] changed.
    pub fn tick(&mut self, now_ms: u64) -> Option<ConnectivityState> {
        if let Some(p) = self.pending {
            if now_ms.saturating_sub(self.pending_since_ms) >= self.debounce_ms {
                if self.live != p {
                    self.live = p;
                    publish(self.live);
                    if p == ConnectivityState::Offline {
                        self.schedule_probe(now_ms);
                    }
                    return Some(self.live);
                }
                self.pending = None;
            }
        }
        None
    }

    pub fn probe_due(&self, now_ms: u64) -> bool {
        self.probe_due_ms > 0 && now_ms >= self.probe_due_ms
    }

    fn schedule_probe(&mut self, now_ms: u64) {
        if self.probe_url.is_none() {
            return;
        }
        self.probe_due_ms = now_ms + self.probe_backoff_ms;
    }

    /// Run a short HEAD re-probe when [`probe_due`]. Caller passes a client with
    /// [`http_client_with_timeouts`].
    pub fn run_probe_if_due(
        &mut self,
        client: &reqwest::blocking::Client,
        now_ms: u64,
    ) -> Option<Result<(), reqwest::Error>> {
        if !self.probe_due(now_ms) || crate::offline::enabled() {
            return None;
        }
        let url = self.probe_url.clone().unwrap_or_default();
        self.probe_due_ms = 0;
        let result = client.head(&url).send().map(|_| ());
        match &result {
            Ok(()) => self.note_transport_success(now_ms),
            Err(e) => {
                self.note_transport_error(e, now_ms);
                self.probe_backoff_ms = (self.probe_backoff_ms * 2).min(PROBE_MAX_BACKOFF_MS);
                self.schedule_probe(now_ms);
            }
        }
        Some(result)
    }

    /// Install as the global writer and optionally start the Windows NLM listener.
    pub fn install_global(self) -> Arc<Mutex<Connectivity>> {
        publish(self.live);
        let arc = Arc::new(Mutex::new(self));
        if let Ok(mut slot) = global_slot().lock() {
            *slot = Some(arc.clone());
        }
        #[cfg(windows)]
        windows_nlm::spawn_if_unstarted(arc.clone());
        arc
    }
}

fn global_slot() -> &'static Mutex<Option<Arc<Mutex<Connectivity>>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<Mutex<Connectivity>>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Feed transport errors into the installed global engine, if any.
pub fn note_global_transport_error(err: &reqwest::Error, now_ms: u64) {
    note_global_transport_hint(classify_reqwest_error(err), now_ms);
}

/// Feed a classified transport hint into the installed global engine, if any.
pub fn note_global_transport_hint(hint: TransportHint, now_ms: u64) {
    if hint == TransportHint::Inconclusive {
        return;
    }
    let Ok(slot) = global_slot().lock() else {
        return;
    };
    if let Some(inner) = slot.as_ref() {
        if let Ok(mut g) = inner.lock() {
            g.note_transport_hint(hint, now_ms);
        }
    }
}

/// Any successful HTTP response to the provider means reachable.
pub fn note_global_transport_success(now_ms: u64) {
    let Ok(slot) = global_slot().lock() else {
        return;
    };
    if let Some(inner) = slot.as_ref() {
        if let Ok(mut g) = inner.lock() {
            g.note_transport_success(now_ms);
        }
    }
}

/// UI chip phase from the installed global engine, or debounced live state.
pub fn global_ui_phase() -> ConnectivityUiPhase {
    let Ok(slot) = global_slot().lock() else {
        return ui_phase_from_live(state());
    };
    if let Some(inner) = slot.as_ref() {
        if let Ok(g) = inner.lock() {
            return g.ui_phase();
        }
    }
    ui_phase_from_live(state())
}

fn ui_phase_from_live(live: ConnectivityState) -> ConnectivityUiPhase {
    match live {
        ConnectivityState::Offline => ConnectivityUiPhase::OfflineLocal,
        ConnectivityState::Online | ConnectivityState::Unknown => ConnectivityUiPhase::Online,
    }
}

#[cfg(windows)]
mod windows_nlm {
    use super::Connectivity;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use windows::Win32::Foundation::VARIANT_FALSE;
    use windows::Win32::Networking::NetworkListManager::{INetworkListManager, NetworkListManager};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };

    static STARTED: AtomicBool = AtomicBool::new(false);

    /// Synchronous read: is Windows reporting internet connectivity?
    pub fn is_connected_to_internet() -> Option<bool> {
        read_nlm().ok()
    }

    fn read_nlm() -> windows::core::Result<bool> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let nlm: INetworkListManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL)?;
            let connected = nlm.IsConnectedToInternet()?;
            Ok(connected != VARIANT_FALSE)
        }
    }

    pub fn spawn_if_unstarted(engine: Arc<Mutex<Connectivity>>) {
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        thread::Builder::new()
            .name("grokhub-nlm".into())
            .spawn(move || nlm_poll_loop(engine))
            .ok();
    }

    fn nlm_poll_loop(engine: Arc<Mutex<Connectivity>>) {
        let mut last: Option<bool> = None;
        loop {
            let now_ms = crate::now_ms();
            let online = is_connected_to_internet();
            if online != last {
                last = online;
                if let Some(on) = online {
                    if let Ok(mut g) = engine.lock() {
                        g.note_os_online(on, now_ms);
                    }
                }
            }
            thread::sleep(Duration::from_millis(750));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_table_reqwest_shapes() {
        let cases: &[(TransportHint, bool, bool, bool, Option<u16>)] = &[
            (TransportHint::OfflineCandidate, true, false, false, None),
            (TransportHint::OfflineCandidate, false, true, false, None),
            (TransportHint::OfflineCandidate, false, false, true, None),
            (
                TransportHint::CloudReachable,
                false,
                false,
                false,
                Some(401),
            ),
            (
                TransportHint::CloudReachable,
                false,
                false,
                false,
                Some(403),
            ),
            (
                TransportHint::CloudReachable,
                false,
                false,
                false,
                Some(429),
            ),
            (
                TransportHint::CloudReachable,
                false,
                false,
                false,
                Some(500),
            ),
            (
                TransportHint::CloudReachable,
                false,
                false,
                false,
                Some(502),
            ),
            (
                TransportHint::CloudReachable,
                false,
                false,
                false,
                Some(404),
            ),
        ];
        for (want, is_connect, is_timeout, is_request, status) in cases {
            let got = classify_from_parts(*is_connect, *is_timeout, *is_request, *status);
            assert_eq!(got, *want, "parts connect={is_connect} timeout={is_timeout} request={is_request} status={status:?}");
        }
    }

    fn classify_from_parts(
        is_connect: bool,
        is_timeout: bool,
        is_request: bool,
        status: Option<u16>,
    ) -> TransportHint {
        if is_connect || is_timeout {
            return TransportHint::OfflineCandidate;
        }
        if let Some(code) = status {
            return classify_http_status(code);
        }
        if is_request {
            return TransportHint::OfflineCandidate;
        }
        TransportHint::Inconclusive
    }

    #[test]
    fn state_transitions_debounce_and_probe() {
        let mut c = Connectivity::new(2_000).with_probe_url("https://example.test/");
        assert_eq!(c.live(), ConnectivityState::Unknown);

        c.note_os_online(true, 0);
        assert_eq!(c.live(), ConnectivityState::Unknown);
        assert_eq!(c.tick(1_999), None, "debounce holds before 2s");
        assert_eq!(
            c.tick(2_000),
            Some(ConnectivityState::Online),
            "OS online after debounce"
        );

        let err = synthetic_connect_error();
        c.note_transport_error(&err, 3_000);
        assert_eq!(
            c.live(),
            ConnectivityState::Online,
            "still debouncing offline"
        );
        assert_eq!(c.tick(5_100), Some(ConnectivityState::Offline));

        assert!(!c.probe_due(5_100));
        assert!(c.probe_due(6_100));

        c.note_transport_success(6_000);
        assert_eq!(c.tick(8_100), Some(ConnectivityState::Online));
    }

    #[test]
    fn ui_phase_reconnecting_while_pending_online() {
        let mut c = Connectivity::new(2_000);
        c.live = ConnectivityState::Offline;
        c.pending = Some(ConnectivityState::Online);
        c.pending_since_ms = 0;
        assert_eq!(c.ui_phase(), ConnectivityUiPhase::Reconnecting);
        c.live = ConnectivityState::Online;
        c.pending = None;
        assert_eq!(c.ui_phase(), ConnectivityUiPhase::Online);
        c.live = ConnectivityState::Offline;
        c.pending = None;
        assert_eq!(c.ui_phase(), ConnectivityUiPhase::OfflineLocal);
        assert_eq!(
            ConnectivityUiPhase::OfflineLocal.label(),
            "Offline (local engine)"
        );
    }

    #[test]
    fn auth_errors_do_not_mark_offline() {
        assert_eq!(classify_http_status(401), TransportHint::CloudReachable);
        let mut c = Connectivity::new(500);
        c.live = ConnectivityState::Online;
        c.note_transport_success(1_000);
        c.tick(2_000);
        assert_eq!(c.live(), ConnectivityState::Online);
    }

    fn synthetic_connect_error() -> reqwest::Error {
        reqwest::blocking::Client::builder()
            .build()
            .unwrap()
            .get("http://127.0.0.1:1/")
            .send()
            .unwrap_err()
    }

    #[test]
    fn http_client_timeouts_are_set() {
        let b = http_client_with_timeouts(
            reqwest::blocking::Client::builder(),
            Duration::from_secs(3),
            Duration::from_secs(9),
        );
        let _ = b.build().expect("client builds");
    }
}
