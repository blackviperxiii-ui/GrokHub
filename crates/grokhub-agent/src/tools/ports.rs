//! Per-thread fetch and Imagine transports.
//! The engine installs them for one turn. Tests pass a fake into the tool
//! instead of installing a client that dials.

use std::cell::RefCell;
use std::sync::Arc;

use super::media::ImagineApi;
use super::web_fetch::PageFetch;

thread_local! {
    static PORTS: RefCell<Ports> = RefCell::new(Ports::default());
}

#[derive(Clone, Default)]
pub(crate) struct Ports {
    pub fetch: Option<Arc<dyn PageFetch>>,
    pub imagine: Option<Arc<dyn ImagineApi>>,
    /// Session media folder. Downloaded Imagine results land here and nowhere else.
    pub media_dir: Option<std::path::PathBuf>,
}

pub(crate) struct Guard {
    prev: Ports,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let prev = std::mem::take(&mut self.prev);
        PORTS.with(|slot| {
            *slot.borrow_mut() = prev;
        });
    }
}

pub(crate) fn current() -> Ports {
    PORTS.with(|slot| slot.borrow().clone())
}

pub(crate) fn enter(next: Ports) -> Guard {
    let prev = PORTS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let prev = slot.clone();
        *slot = next;
        prev
    });
    Guard { prev }
}
