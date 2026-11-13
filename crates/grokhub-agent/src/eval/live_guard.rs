//! Keeps a live eval moving when a model call stalls.
//!
//! The Nov 9 RT-05 run on Beast died this way: calls hung until the client's
//! idle timeout, try after try, until the shell's hard stop killed the
//! process. Two layers stop that:
//!
//! - [`CallDeadline`] gives every call a wall-clock limit. Past it the call is
//!   cut and fails with [`ClientError::IdleTimeout`]. It sits inside
//!   [`super::live::Capped`], so a cut call still counts at its input's price
//!   and the dollar cap holds.
//! - [`Retrying`] sits outside the cap and tries a stalled or transient call
//!   again, a bounded number of times with a growing wait. Each try reserves
//!   its own budget. It never retries once reply text has reached the engine
//!   (a retry would show it twice), and it stops the whole run on a sign-in
//!   or key failure, since every later call would fail the same way.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::client::{
    ClientError, CompactOutput, CompactRequest, ResponsesRequest, StreamEvent, TurnOutput,
};
use crate::{CancelToken, ModelClient};

/// Wall-clock limit on one model call when `--call-timeout-secs` is not given.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(240);
/// Extra tries for a stalled or transient call when `--retries` is not given.
pub const DEFAULT_RETRIES: u32 = 2;
/// The first wait before a retry; each later wait doubles, up to [`MAX_BACKOFF`].
pub const BASE_BACKOFF: Duration = Duration::from_secs(2);
/// No wait between tries runs longer than this, a server's `Retry-After` included.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Cuts any call that runs past `limit` and fails it with
/// [`ClientError::IdleTimeout`].
pub struct CallDeadline {
    inner: Arc<dyn ModelClient + Send + Sync>,
    limit: Duration,
}

impl CallDeadline {
    pub fn new(inner: Arc<dyn ModelClient + Send + Sync>, limit: Duration) -> Self {
        Self { inner, limit }
    }

    /// Runs `call` on a child of `cancel` that a watchdog cancels at the
    /// limit. A call the watchdog cut comes back as `IdleTimeout`; one the
    /// caller cancelled stays `Cancelled`.
    fn guarded<T>(
        &self,
        cancel: &CancelToken,
        call: impl FnOnce(&CancelToken) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let child = CancelToken::child_of(cancel);
        let cut = Arc::new(AtomicBool::new(false));
        let (done, wait) = mpsc::channel::<()>();
        let watchdog = {
            let child = child.clone();
            let cut = cut.clone();
            let limit = self.limit;
            std::thread::Builder::new()
                .name("eval-call-deadline".into())
                .spawn(move || {
                    if let Err(mpsc::RecvTimeoutError::Timeout) = wait.recv_timeout(limit) {
                        cut.store(true, Ordering::SeqCst);
                        child.cancel();
                    }
                })
        };
        let result = call(&child);
        let _ = done.send(());
        if let Ok(handle) = watchdog {
            let _ = handle.join();
        }
        match result {
            Err(_) if cut.load(Ordering::SeqCst) && !cancel.is_cancelled() => {
                Err(ClientError::IdleTimeout)
            }
            other => other,
        }
    }
}

impl ModelClient for CallDeadline {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        self.guarded(cancel, |child| self.inner.stream(req, child, sink))
    }

    fn compacts_server_side(&self) -> bool {
        self.inner.compacts_server_side()
    }

    fn batches(&self) -> bool {
        self.inner.batches()
    }

    fn compact(
        &self,
        req: &CompactRequest,
        cancel: &CancelToken,
    ) -> Option<Result<CompactOutput, ClientError>> {
        let mut skipped = false;
        let result = self.guarded(cancel, |child| match self.inner.compact(req, child) {
            Some(result) => result,
            None => {
                skipped = true;
                Err(ClientError::Cancelled)
            }
        });
        (!skipped).then_some(result)
    }
}

/// How many extra tries a call gets, and the first wait between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub retries: u32,
    pub base: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            retries: DEFAULT_RETRIES,
            base: BASE_BACKOFF,
        }
    }
}

impl RetryPolicy {
    /// The wait before retry number `n` (0 for the first): the server's
    /// `Retry-After` when it sent one, else `base` doubled `n` times, never
    /// past [`MAX_BACKOFF`].
    pub fn wait(&self, n: u32, err: &ClientError) -> Duration {
        let wait = match err.retry_after_secs() {
            Some(secs) => Duration::from_secs(secs),
            None => self.base.saturating_mul(1u32 << n.min(16)),
        };
        wait.min(MAX_BACKOFF)
    }
}

/// A call worth trying again: it stalled, or failed the way a busy or flaky
/// server fails.
fn worth_retry(err: &ClientError) -> bool {
    matches!(err, ClientError::IdleTimeout) || err.is_retryable()
}

/// The key or sign-in no longer works: every later call would fail too.
fn signed_out(err: &ClientError) -> bool {
    matches!(
        err,
        ClientError::Auth { .. }
            | ClientError::KeyOffer { .. }
            | ClientError::PaymentRequired { .. }
    )
}

/// Waits a duration unless cancelled first; false when it was cancelled.
pub type Sleep = Box<dyn Fn(Duration, &CancelToken) -> bool + Send + Sync>;

/// Retries stalled and transient calls under [`RetryPolicy`] and notes a
/// sign-in failure so the run can stop.
pub struct Retrying {
    inner: Arc<dyn ModelClient + Send + Sync>,
    policy: RetryPolicy,
    sleep: Sleep,
    retried: AtomicU32,
    stop: Mutex<Option<String>>,
}

impl Retrying {
    pub fn new(inner: Arc<dyn ModelClient + Send + Sync>, policy: RetryPolicy) -> Self {
        Self::with_sleep(inner, policy, Box::new(sleep_cancellable))
    }

    /// [`Self::new`] with its own sleep, so tests don't wait in real time.
    pub fn with_sleep(
        inner: Arc<dyn ModelClient + Send + Sync>,
        policy: RetryPolicy,
        sleep: Sleep,
    ) -> Self {
        Self {
            inner,
            policy,
            sleep,
            retried: AtomicU32::new(0),
            stop: Mutex::new(None),
        }
    }

    /// Retries made so far in the run.
    pub fn retried(&self) -> u32 {
        self.retried.load(Ordering::SeqCst)
    }

    /// Why the run should stop, once a call failed on its key or sign-in.
    pub fn stop_reason(&self) -> Option<String> {
        self.stop.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn run<T>(
        &self,
        cancel: &CancelToken,
        mut call: impl FnMut() -> Result<T, ClientError>,
        shown: impl Fn() -> bool,
    ) -> Result<T, ClientError> {
        let mut n = 0u32;
        loop {
            let err = match call() {
                Ok(out) => return Ok(out),
                Err(err) => err,
            };
            if signed_out(&err) {
                let mut stop = self.stop.lock().unwrap_or_else(|e| e.into_inner());
                stop.get_or_insert_with(|| format!("the key or sign-in stopped working: {err}"));
                return Err(err);
            }
            if !worth_retry(&err) || n >= self.policy.retries || cancel.is_cancelled() {
                return Err(err);
            }
            if shown() {
                return Err(ClientError::CutOff(err.to_string()));
            }
            let wait = self.policy.wait(n, &err);
            n += 1;
            self.retried.fetch_add(1, Ordering::SeqCst);
            if !(self.sleep)(wait, cancel) {
                return Err(ClientError::Cancelled);
            }
        }
    }
}

impl ModelClient for Retrying {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let shown = AtomicBool::new(false);
        let sink = std::cell::RefCell::new(sink);
        self.run(
            cancel,
            || {
                self.inner.stream(req, cancel, &mut |ev| {
                    if matches!(&ev, StreamEvent::TextDelta(t) if !t.is_empty()) {
                        shown.store(true, Ordering::SeqCst);
                    }
                    (*sink.borrow_mut())(ev)
                })
            },
            || shown.load(Ordering::SeqCst),
        )
    }

    fn compacts_server_side(&self) -> bool {
        self.inner.compacts_server_side()
    }

    fn batches(&self) -> bool {
        self.inner.batches()
    }

    fn compact(
        &self,
        req: &CompactRequest,
        cancel: &CancelToken,
    ) -> Option<Result<CompactOutput, ClientError>> {
        let mut skipped = false;
        let result = self.run(
            cancel,
            || match self.inner.compact(req, cancel) {
                Some(result) => result,
                None => {
                    skipped = true;
                    Err(ClientError::Cancelled)
                }
            },
            || false,
        );
        (!skipped).then_some(result)
    }
}

fn sleep_cancellable(total: Duration, cancel: &CancelToken) -> bool {
    let slice = Duration::from_millis(50);
    let mut left = total;
    while !left.is_zero() {
        if cancel.is_cancelled() {
            return false;
        }
        let step = left.min(slice);
        std::thread::sleep(step);
        left = left.saturating_sub(step);
    }
    !cancel.is_cancelled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Fails with each scripted error in turn, then answers; a `None` step
    /// hangs until it is cancelled, like a stalled stream.
    struct Script {
        steps: Mutex<Vec<Option<ClientError>>>,
        calls: AtomicU32,
        text_first: bool,
    }

    impl Script {
        fn new(steps: Vec<Option<ClientError>>) -> Self {
            Self {
                steps: Mutex::new(steps),
                calls: AtomicU32::new(0),
                text_first: false,
            }
        }
    }

    impl ModelClient for Script {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let step = {
                let mut steps = self.steps.lock().unwrap();
                (!steps.is_empty()).then(|| steps.remove(0))
            };
            match step {
                None => {
                    sink(StreamEvent::TextDelta("ok".into()));
                    Ok(TurnOutput {
                        text: "ok".into(),
                        ..Default::default()
                    })
                }
                Some(Some(err)) => {
                    if self.text_first {
                        sink(StreamEvent::TextDelta("half".into()));
                    }
                    Err(err)
                }
                Some(None) => {
                    while !cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(ClientError::Cancelled)
                }
            }
        }
    }

    fn req() -> ResponsesRequest {
        ResponsesRequest {
            text_format: None,
            model: "grok-4.3".into(),
            effort: Some("low".into()),
            input: Vec::new(),
            conversation_id: "c".into(),
            tools: Vec::new(),
            hosted_search: false,
            call_timeout: None,
        }
    }

    fn no_sleep() -> Sleep {
        Box::new(|_, _| true)
    }

    #[test]
    fn a_stalled_call_is_cut_at_the_limit_as_an_idle_timeout() {
        let inner = Arc::new(Script::new(vec![None]));
        let guard = CallDeadline::new(inner, Duration::from_millis(50));
        let start = Instant::now();
        let out = guard.stream(&req(), &CancelToken::new(), &mut |_| {});
        assert!(matches!(out, Err(ClientError::IdleTimeout)), "{out:?}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_call_the_caller_stops_stays_cancelled() {
        let inner = Arc::new(Script::new(vec![None]));
        let guard = CallDeadline::new(inner, Duration::from_secs(60));
        let cancel = CancelToken::new();
        let stopper = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                cancel.cancel();
            })
        };
        let out = guard.stream(&req(), &cancel, &mut |_| {});
        stopper.join().unwrap();
        assert!(matches!(out, Err(ClientError::Cancelled)), "{out:?}");
    }

    #[test]
    fn a_fast_call_is_left_alone() {
        let inner = Arc::new(Script::new(vec![]));
        let guard = CallDeadline::new(inner, Duration::from_secs(60));
        let out = guard.stream(&req(), &CancelToken::new(), &mut |_| {});
        assert_eq!(out.unwrap().text, "ok");
    }

    #[test]
    fn stalls_are_retried_until_one_answers() {
        let inner = Arc::new(Script::new(vec![None, None]));
        let limited = Arc::new(CallDeadline::new(inner.clone(), Duration::from_millis(30)));
        let waits = Arc::new(Mutex::new(Vec::new()));
        let seen = waits.clone();
        let retrying = Retrying::with_sleep(
            limited,
            RetryPolicy::default(),
            Box::new(move |d, _| {
                seen.lock().unwrap().push(d);
                true
            }),
        );
        let out = retrying.stream(&req(), &CancelToken::new(), &mut |_| {});
        assert_eq!(out.unwrap().text, "ok");
        assert_eq!(inner.calls.load(Ordering::SeqCst), 3);
        assert_eq!(retrying.retried(), 2);
        assert_eq!(
            *waits.lock().unwrap(),
            vec![Duration::from_secs(2), Duration::from_secs(4)]
        );
        assert_eq!(retrying.stop_reason(), None);
    }

    #[test]
    fn retries_are_bounded() {
        let steps = vec![Some(ClientError::IdleTimeout); 5];
        let inner = Arc::new(Script::new(steps));
        let retrying = Retrying::with_sleep(
            inner.clone(),
            RetryPolicy {
                retries: 2,
                base: Duration::from_millis(1),
            },
            no_sleep(),
        );
        let out = retrying.stream(&req(), &CancelToken::new(), &mut |_| {});
        assert!(matches!(out, Err(ClientError::IdleTimeout)), "{out:?}");
        assert_eq!(inner.calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_call_that_already_showed_text_is_not_retried() {
        let mut script = Script::new(vec![Some(ClientError::Disconnect)]);
        script.text_first = true;
        let inner = Arc::new(script);
        let retrying = Retrying::with_sleep(inner.clone(), RetryPolicy::default(), no_sleep());
        let out = retrying.stream(&req(), &CancelToken::new(), &mut |_| {});
        assert!(matches!(out, Err(ClientError::CutOff(_))), "{out:?}");
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_dead_sign_in_stops_the_run_without_a_retry() {
        let inner = Arc::new(Script::new(vec![Some(ClientError::Auth {
            status: 401,
            message: "token expired".into(),
        })]));
        let retrying = Retrying::with_sleep(inner.clone(), RetryPolicy::default(), no_sleep());
        let out = retrying.stream(&req(), &CancelToken::new(), &mut |_| {});
        assert!(matches!(out, Err(ClientError::Auth { .. })), "{out:?}");
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
        assert!(retrying.stop_reason().unwrap().contains("sign-in"));
    }

    #[test]
    fn a_hard_failure_is_not_retried() {
        let inner = Arc::new(Script::new(vec![Some(ClientError::Protocol("bad".into()))]));
        let retrying = Retrying::with_sleep(inner.clone(), RetryPolicy::default(), no_sleep());
        assert!(retrying
            .stream(&req(), &CancelToken::new(), &mut |_| {})
            .is_err());
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(retrying.stop_reason(), None);
    }

    #[test]
    fn the_wait_doubles_takes_retry_after_and_is_capped() {
        let p = RetryPolicy::default();
        let idle = ClientError::IdleTimeout;
        assert_eq!(p.wait(0, &idle), Duration::from_secs(2));
        assert_eq!(p.wait(2, &idle), Duration::from_secs(8));
        assert_eq!(p.wait(10, &idle), MAX_BACKOFF);
        let limited = ClientError::RateLimited {
            retry_after_secs: Some(7),
            message: "slow down".into(),
        };
        assert_eq!(p.wait(0, &limited), Duration::from_secs(7));
        let long = ClientError::RateLimited {
            retry_after_secs: Some(900),
            message: "slow down".into(),
        };
        assert_eq!(p.wait(0, &long), MAX_BACKOFF);
    }
}
