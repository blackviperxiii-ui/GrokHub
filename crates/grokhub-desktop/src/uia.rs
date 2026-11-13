//! The control under a click on Windows, through UI Automation: read-only,
//! one read at a time, capped at [`HIT_CAP`] like the AT-SPI read on Linux.
//! Nothing here sends input or touches a game: reading other apps' controls
//! waits while an anti-cheat runs, as reading their window titles does.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use ::windows::Win32::Foundation::POINT;
use ::windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use ::windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationElement};
use grokhub_core::desktop_mcp::ClickTarget;

/// How long the read may take before the click counts as unknown (Linux's cap).
pub const HIT_CAP: Duration = Duration::from_millis(300);
/// How many parents a hit may climb to reach a control, for text inside a button.
const MAX_CLIMB: usize = 4;

/// A read still running past its cap: the next click is unknown until it ends,
/// so a hung app never piles up reader threads.
static BUSY: AtomicBool = AtomicBool::new(false);

/// The control at a screen point. `None` on a timeout, a COM failure, no
/// control there, or while an anti-cheat runs.
pub fn target_at(x: i32, y: i32) -> Option<ClickTarget> {
    if crate::game_guard::current()
        .blocks(crate::game_guard::Guarded::WindowCheck)
        .is_some()
    {
        return None;
    }
    if BUSY.swap(true, Ordering::AcqRel) {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("uia-hit".into())
        .spawn(move || {
            let found = read(x, y);
            BUSY.store(false, Ordering::Release);
            let _ = tx.send(found);
        });
    if spawned.is_err() {
        BUSY.store(false, Ordering::Release);
        return None;
    }
    rx.recv_timeout(HIT_CAP).ok().flatten()
}

/// COM up, one hit test, COM down, all on this thread.
fn read(x: i32, y: i32) -> Option<ClickTarget> {
    // SAFETY: COM is initialized for this thread only and torn down below, after
    // every interface `hit` made has been dropped.
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return None;
    }
    let found = hit(x, y);
    // SAFETY: pairs with the successful CoInitializeEx above.
    unsafe { CoUninitialize() };
    found
}

/// The nearest control at or above the element under the point, with its name,
/// or the name of the text inside it when the control has none.
fn hit(x: i32, y: i32) -> Option<ClickTarget> {
    // SAFETY: plain read-only UI Automation calls on interfaces created here.
    unsafe {
        let uia: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let walker = uia.ControlViewWalker().ok()?;
        let first = uia.ElementFromPoint(POINT { x, y }).ok()?;
        let inner = name_of(&first);
        let mut el = first;
        for _ in 0..=MAX_CLIMB {
            let kind = el.CurrentControlType().ok()?;
            if let Some(role) = grokhub_core::uia_role(kind.0) {
                let own = name_of(&el);
                let label = if own.is_empty() { inner } else { own };
                return (!label.is_empty()).then(|| ClickTarget {
                    label,
                    role: role.into(),
                });
            }
            el = walker.GetParentElement(&el).ok()?;
        }
        None
    }
}

fn name_of(el: &IUIAutomationElement) -> String {
    // SAFETY: a read-only property of a live element.
    unsafe { el.CurrentName() }
        .map(|name| name.to_string().trim().to_string())
        .unwrap_or_default()
}
